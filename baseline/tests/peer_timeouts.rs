//! A peer that stops answering must fail, not hang.
//!
//! Every other unreachable-peer test uses a transport that returns an
//! error immediately, which is what a *refused* connection looks like.
//! That is the easy failure. The one that matters is a machine that
//! accepts the connection and then says nothing — a laptop that went to
//! sleep mid-conversation, or a tailnet route that stopped forwarding —
//! because with no read timeout that wait is bounded only by the
//! operating system.
//!
//! It matters more than it looks. Peers are fetched one after another on
//! the refresh thread, so a single blackholed machine would stall every
//! peer behind it *and* never be reported unavailable itself, since the
//! fetch it is stuck inside is the thing that would report it.

use parallax_baseline::adapters::http::UreqTransport;
use parallax_baseline::peers::PeerClient;
use std::net::TcpListener;
use std::time::{Duration, Instant, SystemTime};

/// A socket that accepts and then goes quiet, holding every connection
/// open so the client waits on a read rather than seeing a close.
fn blackhole() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds a loopback port");
    let addr = listener.local_addr().expect("has an address");
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming().flatten() {
            // Held, deliberately. Dropping it would send a FIN and the
            // client would fail fast for the wrong reason, which would
            // make this test pass without testing anything.
            held.push(stream);
        }
    });
    format!("http://{addr}")
}

#[test]
fn a_peer_that_accepts_and_never_answers_fails_instead_of_hanging() {
    let mut peer = PeerClient::new(UreqTransport::new(), blackhole());

    let started = Instant::now();
    let failure = peer
        .fetch(SystemTime::UNIX_EPOCH)
        .expect_err("a silent peer must not look like a successful fetch");
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(25),
        "waited {elapsed:?} on a silent peer — the read timeout is not being applied, \
         and one sleeping machine will stall every peer behind it"
    );
    assert!(
        elapsed >= Duration::from_secs(1),
        "returned in {elapsed:?}, which is too fast to have waited on anything — \
         the connection probably failed for a different reason and this test proved nothing"
    );
    assert!(
        !failure.reason.is_empty(),
        "a peer that timed out must say so"
    );
}

/// And having failed, it is reportable: the cockpit gets rows to mark
/// unavailable rather than a thread that never came back.
#[test]
fn a_timed_out_peer_still_produces_a_row_to_mark_unavailable() {
    let mut peer = PeerClient::new(UreqTransport::new(), blackhole());
    let failure = peer.fetch(SystemTime::UNIX_EPOCH).unwrap_err();

    let rows = peer.unavailable(&failure);
    assert_eq!(
        rows.len(),
        1,
        "a configured machine with no rows is invisible"
    );
    assert_eq!(rows[0].peer.as_deref(), Some("127.0.0.1"));
    assert_eq!(rows[0].degradations.len(), 1);
}
