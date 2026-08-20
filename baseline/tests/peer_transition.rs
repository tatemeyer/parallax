//! A machine that was there, and then was not.
//!
//! Every other unreachable-peer test starts with a peer that never
//! answered. That is a cold start, and it is the easier half: with
//! nothing remembered there is nothing to lose. The case the freshness
//! model exists for is the other one — a machine that answered, was
//! rendered, and then went away — because that is when a row can quietly
//! keep showing values nobody is refreshing any more.
//!
//! Observed for real on a Pi whose probe was stopped while its cockpit
//! was pointed at it. This is that, on a socket, so it keeps being true.

use parallax_baseline::adapters::http::UreqTransport;
use parallax_baseline::adapters::verification::{VerificationOutcome, VerificationStatus};
use parallax_baseline::freshness::{Freshness, Observed};
use parallax_baseline::peers::PeerClient;
use parallax_baseline::state::ProjectState;
use parallax_baseline::wire::StateEnvelope;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant, SystemTime};

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// What a probe on `peer` would serve for one project.
fn envelope(now: SystemTime) -> String {
    let project = ProjectState {
        name: "sesh".into(),
        // Watched on the probe — the observation that must not arrive
        // claiming to be live, and must not go on claiming anything at
        // all once the machine is gone.
        verification: vec![Observed::watched(
            VerificationStatus {
                kind: "tests".into(),
                outcome: VerificationOutcome::Pass,
                detail: None,
            },
            now,
        )],
        ..Default::default()
    };
    serde_json::to_string(&StateEnvelope::send("tatepi", now, vec![project])).unwrap()
}

/// A socket that answers exactly once and is then taken away.
///
/// A `tiny_http` server cannot be stopped from outside without reaching
/// into it, and what is being tested is the client, so this speaks just
/// enough HTTP to be answered once. When the thread returns the listener
/// drops, the port closes, and the machine is gone.
fn answers_once_then_disappears(body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds a loopback port");
    let addr = listener.local_addr().expect("has an address");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut scratch = [0u8; 2048];
            let _ = stream.read(&mut scratch);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    format!("http://{addr}")
}

/// Retries until the peer stops answering, so the test does not race the
/// listener's drop. Fails loudly rather than hanging.
fn fetch_until_it_fails(
    peer: &mut PeerClient<UreqTransport>,
) -> parallax_baseline::peers::PeerFailure {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Err(failure) = peer.fetch(at(60)) {
            return failure;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the peer kept answering after its socket was closed");
}

#[test]
fn a_machine_that_answered_and_then_vanished_keeps_its_rows_and_says_why() {
    let mut peer = PeerClient::new(
        UreqTransport::new(),
        answers_once_then_disappears(envelope(at(0))),
    );

    // While it is there.
    let projects = peer.fetch(at(0)).expect("the peer answers once");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "sesh");
    assert_ne!(
        projects[0].verification[0].freshness(at(0)),
        Freshness::Live,
        "a value fetched from another machine reported Live"
    );

    // And then it is not.
    let failure = fetch_until_it_fails(&mut peer);
    let rows = peer.unavailable(&failure);

    assert_eq!(
        rows.len(),
        1,
        "the machine's project vanished from the rail instead of going unavailable"
    );
    assert_eq!(
        rows[0].name, "sesh",
        "it fell back to a bare machine row and forgot what it had been serving"
    );
    match &rows[0].sources(at(60))[0].freshness {
        Freshness::Unavailable { reason, .. } => {
            assert!(
                !reason.is_empty(),
                "unavailable with no reason teaches nothing"
            )
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

/// The row it leaves behind must not be able to claim freshness. This is
/// the failure that would be invisible: a pane still showing yesterday's
/// answer with nothing on screen admitting it.
#[test]
fn nothing_left_behind_by_a_vanished_machine_reports_live_or_fresh() {
    let mut peer = PeerClient::new(
        UreqTransport::new(),
        answers_once_then_disappears(envelope(at(0))),
    );
    peer.fetch(at(0)).expect("the peer answers once");

    let failure = fetch_until_it_fails(&mut peer);
    for row in peer.unavailable(&failure) {
        for source in row.sources(at(60)) {
            assert!(
                matches!(source.freshness, Freshness::Unavailable { .. }),
                "{} survived its machine as {:?}",
                source.label,
                source.freshness
            );
        }
    }
}

/// It remembers what that machine served, so the rail keeps the same
/// rows rather than collapsing to one line naming the host.
#[test]
fn the_rows_it_keeps_are_the_ones_that_machine_served() {
    let mut peer = PeerClient::new(
        UreqTransport::new(),
        answers_once_then_disappears(envelope(at(0))),
    );
    let before: Vec<String> = peer
        .fetch(at(0))
        .expect("the peer answers once")
        .iter()
        .map(ProjectState::qualified_name)
        .collect();

    let failure = fetch_until_it_fails(&mut peer);
    let after: Vec<String> = peer
        .unavailable(&failure)
        .iter()
        .map(ProjectState::qualified_name)
        .collect();

    assert_eq!(
        before, after,
        "the rail changed shape when the machine left"
    );
}
