//! The probe over a real socket.
//!
//! Everything else about the probe is tested against values. This binds
//! an ephemeral loopback port, serves from it, and fetches with the same
//! `PeerClient` a cockpit uses — so the bytes, the JSON, and the
//! re-stamping are all exercised by the code that ships rather than by a
//! stand-in for it.
//!
//! Ephemeral ports throughout: a fixed one would make two of these tests
//! collide when the harness runs them in parallel, and would fail on a
//! machine already running a probe — which, on the machines this was
//! written for, is all of them.

use parallax_baseline::adapters::factory::AdapterConfig;
use parallax_baseline::adapters::http::UreqTransport;
use parallax_baseline::freshness::Freshness;
use parallax_baseline::peers::PeerClient;
use parallax_baseline::registry::Registry;
use parallax_probe::server::{Probe, Serving};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// A manifest declaring a check that would take minutes and fail, so
/// "it reported NotRun" can only mean it was never spawned.
const WITH_TESTS: &str = "project:\n  name: sesh\n  language: rust\nverification:\n  - kind: tests\n    adapter: command\n    command: cargo test --workspace\n";

fn project(dir: &Path, name: &str, manifest: &str) {
    let root = dir.join(name);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("parallax.yaml"), manifest).unwrap();
}

/// Starts a probe on a free loopback port and returns its base URL.
///
/// No sleep and no retry: `Probe::bind` has already bound the socket by
/// the time it returns, so a connection made before the thread reaches
/// `accept` waits in the backlog rather than being refused.
fn spawn(root: PathBuf, peer: &'static str) -> String {
    let probe = Probe::bind(0).expect("binds an ephemeral loopback port");
    let url = probe.url();
    std::thread::spawn(move || {
        let registry = Registry::scan(&root);
        probe.serve(&Serving {
            registry: &registry,
            config: &AdapterConfig::default(),
            peer,
            // These are the read tests, and this is the default probe:
            // no control, so nothing here can cause the machine to act.
            control: None,
        });
    });
    url
}

fn client(url: String) -> PeerClient<UreqTransport> {
    PeerClient::new(UreqTransport::new(), url)
}

#[test]
fn a_probe_serves_over_a_socket_and_a_client_reads_it_back() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "sesh", WITH_TESTS);

    let mut peer = client(spawn(dir.path().to_path_buf(), "pi5"));
    let projects = peer.fetch(at(0)).expect("the probe answers");

    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "sesh");
    assert_eq!(projects[0].language.as_deref(), Some("rust"));
}

/// The probe binds loopback, so it is named by the URL the client was
/// configured with — an address, which stays whole rather than being cut
/// at its first dot.
#[test]
fn a_loopback_peer_keeps_its_whole_address_as_its_name() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "sesh", WITH_TESTS);

    let mut peer = client(spawn(dir.path().to_path_buf(), "pi5"));
    let projects = peer.fetch(at(0)).unwrap();

    assert_eq!(peer.name(), "127.0.0.1");
    assert_eq!(projects[0].qualified_name(), "sesh@127.0.0.1");
}

/// The claim the whole design rests on, over a real socket this time.
#[test]
fn nothing_that_crossed_the_socket_claims_to_be_live() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "sesh", WITH_TESTS);

    let mut peer = client(spawn(dir.path().to_path_buf(), "pi5"));
    let projects = peer.fetch(at(0)).unwrap();

    for source in projects[0].sources(at(0)) {
        assert_ne!(
            source.freshness,
            Freshness::Live,
            "{} came off a socket claiming to be live",
            source.label
        );
    }
}

/// `cargo test --workspace` inside a temp directory would take minutes
/// and then fail. It reported `NotRun` in milliseconds, which is only
/// possible if the probe never spawned it.
#[test]
fn serving_state_over_http_still_runs_no_build() {
    use parallax_baseline::adapters::verification::VerificationOutcome;

    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "sesh", WITH_TESTS);

    let mut peer = client(spawn(dir.path().to_path_buf(), "pi5"));
    let started = std::time::Instant::now();
    let projects = peer.fetch(at(0)).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(projects[0].verification.len(), 1);
    assert_eq!(
        projects[0].verification[0].value.outcome,
        VerificationOutcome::NotRun
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "took {elapsed:?}, which is long enough to have built something"
    );
}

/// Two machines, simulated by two probes. This is the shape the cockpit
/// runs in, and it is reachable on one machine because the port is
/// ephemeral.
#[test]
fn two_probes_are_two_distinct_peers() {
    let a = tempfile::tempdir().unwrap();
    project(a.path(), "sesh", WITH_TESTS);
    let b = tempfile::tempdir().unwrap();
    project(
        b.path(),
        "ttui",
        "project:\n  name: ttui\n  language: rust\n",
    );

    let mut first = client(spawn(a.path().to_path_buf(), "pi5"));
    let mut second = client(spawn(b.path().to_path_buf(), "laptop"));

    let from_a = first.fetch(at(0)).expect("the first probe answers");
    let from_b = second.fetch(at(0)).expect("the second probe answers");

    assert_eq!(from_a[0].name, "sesh");
    assert_eq!(from_b[0].name, "ttui");
    assert_ne!(
        first.url(),
        second.url(),
        "both probes took the same port, so this proved nothing"
    );
}

/// A machine with nothing registered answers, rather than refusing to
/// start or timing out. Indistinguishable-from-unreachable is the one
/// thing an empty machine must not be.
#[test]
fn an_empty_machine_answers_with_an_empty_list() {
    let dir = tempfile::tempdir().unwrap();

    let mut peer = client(spawn(dir.path().to_path_buf(), "fresh"));
    let projects = peer.fetch(at(0)).expect("an empty probe still answers");

    assert!(projects.is_empty());
}
