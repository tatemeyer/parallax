//! The cockpit across machines.
//!
//! Everything here runs against `FixtureTransport`, so a cockpit showing
//! three machines is tested with no network and no second machine —
//! which is the whole reason a peer is an HTTP source rather than a
//! remote filesystem.

use panopticon::refresh::{BoxedPeer, Clock, Refresher, Request, Update};
use parallax_baseline::adapters::http::{FixtureTransport, HttpTransport};
use parallax_baseline::adapters::verification::{VerificationOutcome, VerificationStatus};
use parallax_baseline::adapters::AdapterError;
use parallax_baseline::freshness::{Freshness, Observed};
use parallax_baseline::peers::{PeerClient, STATE_PATH};
use parallax_baseline::state::ProjectState;
use parallax_baseline::wire::StateEnvelope;
use std::time::{Duration, SystemTime};

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

fn status(kind: &str) -> VerificationStatus {
    VerificationStatus {
        kind: kind.into(),
        outcome: VerificationOutcome::Pass,
        detail: None,
    }
}

/// What a probe on `peer` would have served for `projects`.
fn envelope(peer: &str, projects: &[&str], now: SystemTime) -> String {
    let states: Vec<ProjectState> = projects
        .iter()
        .map(|name| ProjectState {
            name: (*name).into(),
            // Watched on the probe: the case that would render Live if
            // it crossed the wire unchanged.
            verification: vec![Observed::watched(status("tests"), now)],
            ..Default::default()
        })
        .collect();
    serde_json::to_string(&StateEnvelope::send(peer, now, states)).unwrap()
}

/// A peer that answers with the given body.
fn peer(name: &str, body: String) -> BoxedPeer {
    let url = format!("https://{name}");
    let mut transport = FixtureTransport::new();
    transport.insert(format!("{url}{STATE_PATH}"), body, None);
    let transport: Box<dyn HttpTransport + Send> = Box::new(transport);
    PeerClient::new(transport, url)
}

/// A peer that cannot be reached.
fn unreachable(name: &str) -> BoxedPeer {
    let mut transport = FixtureTransport::new();
    transport.fail_next(AdapterError::Timeout("connection refused".into()));
    let transport: Box<dyn HttpTransport + Send> = Box::new(transport);
    PeerClient::new(transport, format!("https://{name}"))
}

/// Drives one refresh and collects everything it produced.
fn refresh(peers: Vec<BoxedPeer>) -> Vec<Update> {
    let refresher = Refresher::spawn_with_peers(Vec::new(), peers, Clock::Frozen(at(0)));
    refresher.request(Request::RefreshReads);

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut updates = Vec::new();
    while std::time::Instant::now() < deadline {
        updates.extend(refresher.drain());
        if !updates.is_empty() {
            // Give the rest of the cycle a moment to land.
            std::thread::sleep(Duration::from_millis(50));
            updates.extend(refresher.drain());
            break;
        }
    }
    refresher.stop();
    updates
}

#[test]
fn a_peer_that_answers_contributes_its_projects() {
    let updates = refresh(vec![peer("pi5", envelope("pi5", &["sesh"], at(0)))]);

    let states: Vec<_> = updates
        .iter()
        .filter_map(|u| match u {
            Update::PeerState { peer, projects } => Some((peer, projects)),
            _ => None,
        })
        .collect();

    assert_eq!(states.len(), 1, "got {updates:?}");
    assert_eq!(states[0].0, "pi5");
    assert_eq!(states[0].1.len(), 1);
    assert_eq!(states[0].1[0].qualified_name(), "sesh@pi5");
}

/// The design's central claim, at the outermost layer that can still
/// observe it: what the cockpit receives must not claim to be live.
#[test]
fn nothing_a_peer_sends_arrives_claiming_to_be_live() {
    let updates = refresh(vec![peer("pi5", envelope("pi5", &["sesh"], at(0)))]);

    for update in &updates {
        if let Update::PeerState { projects, .. } = update {
            for project in projects {
                for source in project.sources(at(0)) {
                    assert_ne!(
                        source.freshness,
                        Freshness::Live,
                        "{} claimed Live over a network",
                        source.label
                    );
                }
            }
        }
    }
}

/// The sleeping-laptop case. One machine being unreachable must not
/// cost the others their rows — the rule the registry and `aggregate`
/// already share, now across a network.
#[test]
fn an_unreachable_peer_degrades_only_itself() {
    let updates = refresh(vec![
        peer("pi5", envelope("pi5", &["sesh"], at(0))),
        unreachable("laptop"),
        peer("desktop", envelope("desktop", &["parallax"], at(0))),
    ]);

    let answered: Vec<&str> = updates
        .iter()
        .filter_map(|u| match u {
            Update::PeerState { peer, .. } => Some(peer.as_str()),
            _ => None,
        })
        .collect();
    let failed: Vec<(&str, &str)> = updates
        .iter()
        .filter_map(|u| match u {
            Update::PeerFailed { peer, reason } => Some((peer.as_str(), reason.as_str())),
            _ => None,
        })
        .collect();

    assert_eq!(answered, vec!["pi5", "desktop"], "got {updates:?}");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, "laptop");
    assert!(failed[0].1.contains("connection refused"));
}

/// A probe that answers with something that is not an envelope is a
/// failure with a reason, not a panic on the refresh thread and not a
/// silently empty machine.
#[test]
fn a_peer_answering_with_nonsense_fails_with_a_reason() {
    let updates = refresh(vec![peer("pi5", "<html>gateway timeout</html>".into())]);

    let failed: Vec<&str> = updates
        .iter()
        .filter_map(|u| match u {
            Update::PeerFailed { reason, .. } => Some(reason.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(failed.len(), 1, "got {updates:?}");
    assert!(
        failed[0].contains("not a state envelope"),
        "got {}",
        failed[0]
    );
}

/// **Every** recorded machine answers, not just the first one.
///
/// A fixture peer whose envelope is malformed does not fail loudly — it
/// renders as a machine that did not answer, which is a real and
/// expected state, so the screen looks plausible and the fixture is
/// silently broken. Asserted here because that is exactly how the
/// second recorded machine was added with a bad `source` shape.
#[test]
fn every_recorded_machine_in_the_shipped_set_answers() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut set = panopticon::fixtures::load(&dir).expect("the fixture set loads");
    let now = set.now;
    assert!(!set.peers.is_empty(), "the fixture set lost its peers");

    for peer in &mut set.peers {
        let name = peer.name().to_string();
        let projects = peer
            .fetch(now)
            .unwrap_or_else(|e| panic!("recorded machine `{name}` did not answer: {}", e.reason));
        assert!(
            !projects.is_empty(),
            "recorded machine `{name}` answered with no projects"
        );
    }
}

/// The shipped fixture set carries a recorded machine, and it renders
/// the same way twice. Without this, remote hosts would be the one part
/// of the cockpit Plumb could never judge — a NO-GO on a screen holding
/// a live peer would mean "the laptop answered differently", not "the
/// layout is wrong".
#[test]
fn the_shipped_fixture_set_holds_a_peer_that_loads_identically_twice() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");

    // Peers are sorted by file name, so `pi5` precedes `tates-laptop`
    // and a row's position is a property of the directory rather than of
    // whichever order the filesystem happened to hand back.
    let load = || {
        let mut set = panopticon::fixtures::load(&dir).expect("the fixture set loads");
        assert_eq!(set.peers.len(), 2, "the fixture set lost a peer");
        let now = set.now;
        let names: Vec<String> = set.peers.iter().map(|p| p.name().to_string()).collect();
        let projects = set.peers[1].fetch(now).expect("the recorded peer answers");
        (names, projects, now)
    };

    let (names_a, first, now) = load();
    let (names_b, second, _) = load();

    assert_eq!(names_a, ["pi5", "tates-laptop"]);
    assert_eq!(names_b, names_a);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].qualified_name(), "ttui@tates-laptop");

    // Identical run to run: same rows, same ages, same standings.
    let render = |projects: &[ProjectState]| {
        projects
            .iter()
            .map(|p| format!("{}|{:?}", p.qualified_name(), p.sources(now)))
            .collect::<Vec<_>>()
    };
    assert_eq!(render(&first), render(&second));

    // And the recorded machine is subject to the same rule as a live
    // one: it was `Watched` on the probe and must not arrive as `Live`.
    for source in first[0].sources(now) {
        assert_ne!(source.freshness, Freshness::Live, "{}", source.label);
    }
}

/// A probe built against a version this cockpit cannot read is refused
/// by name rather than parsed optimistically.
#[test]
fn a_peer_speaking_a_future_version_is_refused_by_name() {
    let body = envelope("pi5", &["sesh"], at(0)).replace("parallax/v1", "parallax/v2");
    let updates = refresh(vec![peer("pi5", body)]);

    let failed: Vec<&str> = updates
        .iter()
        .filter_map(|u| match u {
            Update::PeerFailed { reason, .. } => Some(reason.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(failed.len(), 1, "got {updates:?}");
    assert!(failed[0].contains("parallax/v2"), "got {}", failed[0]);
}
