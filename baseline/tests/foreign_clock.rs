//! A producer timestamp crossing the wire.
//!
//! `Artifact::modified` is a foreign clock *inside a value*.
//! `ObservedWire::receive` re-bases `observed_at` and passes
//! `self.value` through untouched, so every duration computed from
//! `modified` on the receiving machine is the cross-machine clock
//! comparison this platform forbids everywhere else.
//!
//! The failure is quiet. It does not produce an absurd number a reader
//! would question; it produces a plausible one that is wrong by exactly
//! the skew.

use parallax_baseline::adapters::artifact::{Artifact, ArtifactDetail, Series};
use parallax_baseline::freshness::Observed;
use parallax_baseline::manifest::ArtifactKind;
use parallax_baseline::state::ProjectState;
use parallax_baseline::wire::ProjectWire;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// The receiving machine's clock. Everything is expressed against it.
const BASE: u64 = 1_700_000_000;

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(BASE + secs)
}

/// The peer's clock reads four minutes ahead of ours.
const SKEW: u64 = 240;
/// The run stopped writing three minutes before the probe scanned it.
const STALLED_FOR: u64 = 180;

/// A project carrying one metrics artifact, as the probe would have
/// scanned it.
///
/// `observed_at` and `modified` are both on the **probe's** clock,
/// which is what makes their difference a true age no matter how wrong
/// that clock is.
fn as_the_probe_saw_it() -> ProjectState {
    let probe_now = at(SKEW);
    let artifact = Artifact {
        path: PathBuf::from("/home/pi/jepa/results/run7/loss.jsonl"),
        kind: ArtifactKind::Metrics,
        modified: probe_now - Duration::from_secs(STALLED_FOR),
        detail: ArtifactDetail::Metrics {
            series: vec![Series::ordered("loss", vec![2.7, 2.1, 1.6], "step")],
        },
    };
    let mut state = ProjectState {
        name: "jepa".into(),
        ..Default::default()
    };
    state
        .artifacts
        .push(Observed::watched(vec![artifact], probe_now));
    state
}

/// Receives that project onto this machine's clock.
fn received() -> ProjectState {
    ProjectWire::send(as_the_probe_saw_it()).receive(
        at(SKEW), // probe_now — the peer's clock
        at(0),    // client_now — ours
        Duration::from_secs(30),
    )
}

/// The age a pane would compute, written the way a renderer naturally
/// would: saturating, because `SystemTime` subtraction can fail and
/// there is nothing else sensible to do with the error.
fn producer_age(artifact: &Artifact, now: SystemTime) -> Duration {
    now.duration_since(artifact.modified)
        .unwrap_or(Duration::ZERO)
}

/// The observation's own age survives the crossing, because that is
/// what `receive` was built to do. This is the control: it proves the
/// failure below is specific to the timestamp *inside* the value.
#[test]
fn the_observations_own_age_crosses_correctly() {
    let state = received();
    assert_eq!(state.artifacts[0].age(at(0)), Duration::ZERO);
}

/// The bug. A run that stopped three minutes ago renders as `0s` —
/// indistinguishable on screen from one still writing.
///
/// Four minutes of skew swallows three minutes of silence, and the
/// subtraction saturates rather than erroring, so nothing anywhere
/// reports a problem.
#[test]
fn a_stalled_run_on_a_fast_peer_renders_as_the_freshest_thing_on_screen() {
    let state = received();
    let artifact = &state.artifacts[0].value[0];

    assert_eq!(
        producer_age(artifact, at(0)),
        Duration::ZERO,
        "a run silent for {STALLED_FOR}s reports as freshly written"
    );
}

/// The same skew in the other direction, so the shape of the bug is on
/// record rather than one instance of it. A peer four minutes *behind*
/// reports a file written 60s ago as 300s old — plausible, unremarkable,
/// and wrong by exactly the skew. Nothing about either number invites a
/// second look, which is why this needed a test rather than an eye.
#[test]
fn a_slow_peer_overstates_the_same_age_by_the_same_skew() {
    let probe_now = at(0) - Duration::from_secs(SKEW);
    let artifact = Artifact {
        path: PathBuf::from("/home/pi/jepa/results/run7/loss.jsonl"),
        kind: ArtifactKind::Metrics,
        modified: probe_now - Duration::from_secs(60),
        detail: ArtifactDetail::Metrics { series: Vec::new() },
    };
    let mut state = ProjectState {
        name: "jepa".into(),
        ..Default::default()
    };
    state
        .artifacts
        .push(Observed::watched(vec![artifact], probe_now));

    let received = ProjectWire::send(state).receive(probe_now, at(0), Duration::from_secs(30));

    assert_eq!(
        producer_age(&received.artifacts[0].value[0], at(0)),
        Duration::from_secs(60 + SKEW),
        "written 60s ago, reported as {}s",
        60 + SKEW
    );
}
