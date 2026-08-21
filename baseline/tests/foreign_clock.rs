//! A producer timestamp crossing the wire.
//!
//! `Artifact::modified` was a foreign clock *inside a value*:
//! `ObservedWire::receive` re-bases the envelope and passes values
//! through untouched, so it crossed raw and every duration computed
//! from it on the receiving machine was a cross-machine clock
//! comparison — the thing this platform forbids everywhere else.
//!
//! It now crosses as `ArtifactWire`, which carries an **age at
//! observation** rather than a timestamp. A duration has no clock in it
//! and nothing to compare.
//!
//! The failure these tests lock out was quiet. It did not produce an
//! absurd number a reader would question; it produced a plausible one,
//! wrong by exactly the skew.

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

fn artifact(modified: Option<SystemTime>) -> Artifact {
    Artifact {
        path: PathBuf::from("/home/pi/jepa/results/run7/loss.jsonl"),
        kind: ArtifactKind::Metrics,
        modified,
        detail: ArtifactDetail::Metrics {
            series: vec![Series::ordered("loss", vec![2.7, 2.1, 1.6], "step")],
        },
    }
}

/// One project carrying one artifact feed, observed at `observed_at` on
/// the observing machine's own clock.
fn scanned(modified: Option<SystemTime>, observed_at: SystemTime) -> ProjectState {
    let mut state = ProjectState {
        name: "jepa".into(),
        ..Default::default()
    };
    state
        .artifacts
        .push(Observed::watched(vec![artifact(modified)], observed_at));
    state
}

/// Sends a project from a peer whose clock reads `probe_now`, and
/// receives it here at `at(0)`.
fn across(state: ProjectState, probe_now: SystemTime) -> ProjectState {
    ProjectWire::send(state).receive(probe_now, at(0), Duration::from_secs(30))
}

/// The age a pane computes. `None` when nobody could say.
fn producer_age(artifact: &Artifact, now: SystemTime) -> Option<Duration> {
    artifact
        .modified
        .and_then(|modified| now.duration_since(modified).ok())
}

/// The control: the observation's own age crosses correctly, and always
/// did. It is here so a failure below is unambiguously about the
/// timestamp *inside* the value rather than about `receive` in general.
#[test]
fn the_observations_own_age_crosses_correctly() {
    let received = across(scanned(Some(at(SKEW)), at(SKEW)), at(SKEW));
    assert_eq!(received.artifacts[0].age(at(0)), Duration::ZERO);
}

/// The bug this slice exists for, now fixed. A peer four minutes fast
/// used to turn a run silent for three minutes into `0s` —
/// indistinguishable on screen from one still writing, because
/// `SystemTime` subtraction saturates rather than erroring.
#[test]
fn a_stalled_run_on_a_fast_peer_reports_the_time_it_was_actually_silent() {
    let probe_now = at(SKEW);
    let received = across(
        scanned(
            Some(probe_now - Duration::from_secs(STALLED_FOR)),
            probe_now,
        ),
        probe_now,
    );

    assert_eq!(
        producer_age(&received.artifacts[0].value[0], at(0)),
        Some(Duration::from_secs(STALLED_FOR)),
        "silent for {STALLED_FOR}s on the only clock that measured it"
    );
}

/// The same skew the other way. A peer four minutes behind used to
/// report a file written 60s ago as 300s old.
#[test]
fn a_slow_peer_no_longer_overstates_the_same_age() {
    let probe_now = at(0) - Duration::from_secs(SKEW);
    let received = across(
        scanned(Some(probe_now - Duration::from_secs(60)), probe_now),
        probe_now,
    );

    assert_eq!(
        producer_age(&received.artifacts[0].value[0], at(0)),
        Some(Duration::from_secs(60)),
    );
}

/// The Pi 5 case. Its clock has no RTC and reads 1970 until NTP lands,
/// so both its numbers are absurd — and their *difference* is still
/// exactly right, which is the whole reason an age travels instead of a
/// timestamp.
#[test]
fn a_peer_whose_clock_is_decades_wrong_still_reports_a_true_age() {
    let probe_now = SystemTime::UNIX_EPOCH + Duration::from_secs(60 * 60);
    let received = across(
        scanned(
            Some(probe_now - Duration::from_secs(STALLED_FOR)),
            probe_now,
        ),
        probe_now,
    );

    assert_eq!(
        producer_age(&received.artifacts[0].value[0], at(0)),
        Some(Duration::from_secs(STALLED_FOR)),
        "a wrong clock subtracted from itself is still a right duration"
    );
}

/// A producer that could not say stays unable to say. The alternative —
/// a fallback timestamp — is a sentence about a missing value that a
/// renderer cannot tell from a measurement.
#[test]
fn an_artifact_whose_producer_could_not_say_arrives_saying_so() {
    let received = across(scanned(None, at(SKEW)), at(SKEW));
    assert_eq!(received.artifacts[0].value[0].modified, None);
    assert_eq!(producer_age(&received.artifacts[0].value[0], at(0)), None);
}

/// A modification time *after* the scan that found it is not an age on
/// any clock — both numbers came from the same machine, so the file
/// cannot have been written after it was read. Reporting a saturated
/// zero would be the same silent lie in a smaller coat.
#[test]
fn a_modification_time_after_the_scan_that_found_it_is_unknown_not_zero() {
    let probe_now = at(SKEW);
    let received = across(
        scanned(Some(probe_now + Duration::from_secs(30)), probe_now),
        probe_now,
    );

    assert_eq!(
        received.artifacts[0].value[0].modified, None,
        "not an age, so not reported as one"
    );
}

/// Nothing is lost when there is no skew at all, which is the case that
/// would hide a sign error.
#[test]
fn a_peer_whose_clock_agrees_with_ours_round_trips_unchanged() {
    let received = across(
        scanned(Some(at(0) - Duration::from_secs(STALLED_FOR)), at(0)),
        at(0),
    );

    assert_eq!(
        received.artifacts[0].value[0].modified,
        Some(at(0) - Duration::from_secs(STALLED_FOR))
    );
}

/// The value beside the timestamp is genuinely inert and must survive
/// untouched — including Slice 1's ordering claim, which a renderer
/// reads to decide whether it may draw a curve.
#[test]
fn the_rest_of_the_artifact_crosses_unchanged() {
    let received = across(scanned(Some(at(SKEW)), at(SKEW)), at(SKEW));
    let crossed = &received.artifacts[0].value[0];

    assert_eq!(
        crossed.path,
        PathBuf::from("/home/pi/jepa/results/run7/loss.jsonl")
    );
    assert_eq!(crossed.kind, ArtifactKind::Metrics);
    match &crossed.detail {
        ArtifactDetail::Metrics { series } => {
            assert_eq!(series[0].points, vec![2.7, 2.1, 1.6]);
            assert_eq!(
                *series[0].order(),
                parallax_baseline::adapters::artifact::SeriesOrder::By("step".into())
            );
        }
        other => panic!("expected metrics, got {other:?}"),
    }
}

/// A timestamp must not appear anywhere in the serialized envelope's
/// artifacts. This is the structural half of the guarantee: the tests
/// above check the arithmetic, and this checks that the raw material
/// for getting it wrong is not on the wire at all.
#[test]
fn no_producer_timestamp_is_serialized_at_all() {
    let probe_now = at(SKEW);
    let wire = ProjectWire::send(scanned(
        Some(probe_now - Duration::from_secs(STALLED_FOR)),
        probe_now,
    ));
    let json = serde_json::to_string(&wire.artifacts).unwrap();

    assert!(json.contains("modifiedAgeSecs"), "an age travels: {json}");
    assert!(
        json.contains(&format!("\"modifiedAgeSecs\":{STALLED_FOR}")),
        "and it is the age the producer measured: {json}"
    );
    assert!(
        !json.contains("\"modified\":"),
        "no producer timestamp on the wire: {json}"
    );
}
