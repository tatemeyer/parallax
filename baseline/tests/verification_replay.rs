//! The verification adapters, replayed against sample Plumb verdicts.
//! Nothing here links `parallax-plumb`: the platform consumes Plumb's
//! output as text on disk.

use parallax_baseline::adapters::verification::{
    parse_verdict, PlumbVerificationAdapter, VerificationAdapter, VerificationOutcome,
};
use parallax_baseline::adapters::ProjectContext;
use parallax_baseline::freshness::Freshness;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/plumb")
        .join(name)
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// Builds a runs directory containing the given verdict files, each in
/// its own run subdirectory named for its run id.
fn runs_dir(verdicts: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (run_id, fixture_name) in verdicts {
        let run = dir.path().join(run_id);
        std::fs::create_dir_all(&run).unwrap();
        std::fs::copy(fixture(fixture_name), run.join("verdict.md")).unwrap();
    }
    dir
}

#[test]
fn the_three_verdict_states_parse_from_the_header_line() {
    for (name, expected) in [
        ("verdict-go.md", VerificationOutcome::Pass),
        ("verdict-no-go.md", VerificationOutcome::Fail),
        ("verdict-hold.md", VerificationOutcome::Hold),
    ] {
        let text = std::fs::read_to_string(fixture(name)).unwrap();
        assert_eq!(parse_verdict(&text), Some(expected), "{name}");
    }
}

/// `NO-GO` contains `GO`. Getting this backwards would turn every
/// blocked review into a pass, which is the worst possible direction to
/// be wrong in.
#[test]
fn no_go_is_never_mistaken_for_go() {
    assert_eq!(
        parse_verdict("# run 1 — NO-GO"),
        Some(VerificationOutcome::Fail)
    );
    assert_eq!(
        parse_verdict("# run 1 — GO"),
        Some(VerificationOutcome::Pass)
    );
}

/// The words recur in per-lens rows further down; only the header counts.
#[test]
fn only_the_first_line_carrying_a_verdict_word_counts() {
    let text = "# Plumb verdict — run 1 — NO-GO\n\n| a | breakage | GO |\n";
    assert_eq!(parse_verdict(text), Some(VerificationOutcome::Fail));
}

#[test]
fn a_verdict_file_naming_no_state_parses_to_none() {
    assert_eq!(
        parse_verdict("# Plumb verdict — run 1\n\nnothing here\n"),
        None
    );
}

#[test]
fn the_adapter_reads_the_most_recent_run_by_directory_name() {
    let dir = runs_dir(&[
        ("20260814T101500Z", "verdict-go.md"),
        ("20260814T112200Z", "verdict-no-go.md"),
    ]);
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path());
    let status = a
        .check(&ProjectContext::new("ttui", dir.path()), at(0))
        .unwrap()
        .value;
    assert_eq!(
        status.outcome,
        VerificationOutcome::Fail,
        "the later run id wins"
    );
    assert_eq!(status.detail.as_deref(), Some("20260814T112200Z"));
}

/// The spec's precedent, carried through: a Hold is never upgraded.
#[test]
fn a_hold_is_reported_as_a_hold_and_never_as_a_pass() {
    let dir = runs_dir(&[("20260814T120000Z", "verdict-hold.md")]);
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path());
    let status = a
        .check(&ProjectContext::new("ttui", dir.path()), at(0))
        .unwrap()
        .value;
    assert_eq!(status.outcome, VerificationOutcome::Hold);
}

#[test]
fn a_project_that_has_never_run_plumb_reports_not_run_rather_than_erroring() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path().join("runs"));
    let status = a
        .check(&ProjectContext::new("ttui", dir.path()), at(0))
        .unwrap()
        .value;
    assert_eq!(status.outcome, VerificationOutcome::NotRun);
}

#[test]
fn a_run_directory_with_no_verdict_file_is_skipped_rather_than_failing_the_check() {
    let dir = runs_dir(&[("20260814T101500Z", "verdict-go.md")]);
    std::fs::create_dir_all(dir.path().join("20260814T130000Z")).unwrap();
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path());
    let status = a
        .check(&ProjectContext::new("ttui", dir.path()), at(0))
        .unwrap()
        .value;
    assert_eq!(
        status.outcome,
        VerificationOutcome::Pass,
        "the in-progress run is ignored"
    );
}

#[test]
fn a_verdict_read_from_disk_is_live_not_polled() {
    let dir = runs_dir(&[("20260814T101500Z", "verdict-go.md")]);
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path());
    let observed = a
        .check(&ProjectContext::new("ttui", dir.path()), at(0))
        .unwrap();
    assert_eq!(observed.freshness(at(9999)), Freshness::Live);
}
