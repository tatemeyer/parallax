use super::*;
use crate::finding::{Confidence, Finding, Lens, Severity};
use crate::merge::{merge, MergedFinding};
use crate::prompt::Skip;

fn reported(lens: Lens) -> LensReport {
    LensReport {
        scenario: "dial".into(),
        lens,
        outcome: LensOutcome::Reported,
    }
}

fn finding(lens: Lens, sev: Severity) -> Vec<MergedFinding> {
    merge(vec![Finding {
        lens,
        scenario: "dial".into(),
        severity: sev,
        region: "upper right".into(),
        claim: "the border does not close".into(),
        evidence: "e".into(),
        confidence: Confidence::High,
    }])
}

#[test]
fn no_findings_is_a_go() {
    let v = aggregate(&[reported(Lens::Breakage), reported(Lens::Intent)], &[]);
    assert_eq!(v, Verdict::Go);
}

#[test]
fn advisory_findings_only_is_still_a_go() {
    let v = aggregate(
        &[reported(Lens::Design)],
        &finding(Lens::Design, Severity::Major),
    );
    assert_eq!(
        v,
        Verdict::Go,
        "advisory findings are reported and never block"
    );
}

#[test]
fn a_blocker_from_a_blocker_capable_lens_is_a_no_go() {
    let v = aggregate(
        &[reported(Lens::Breakage)],
        &finding(Lens::Breakage, Severity::Blocker),
    );
    assert_eq!(v, Verdict::NoGo);
}

#[test]
fn a_single_no_go_holds_the_run_however_many_lenses_reported_clean() {
    let reports = vec![
        reported(Lens::Breakage),
        reported(Lens::Intent),
        reported(Lens::Design),
        reported(Lens::Motion),
    ];
    let v = aggregate(&reports, &finding(Lens::Intent, Severity::Blocker));
    assert_eq!(v, Verdict::NoGo, "one console's no-go holds the launch");
}

#[test]
fn a_held_lens_is_never_upgraded_to_a_go() {
    // The single most important gate rule.
    let reports = vec![
        reported(Lens::Breakage),
        LensReport {
            scenario: "dial".into(),
            lens: Lens::Intent,
            outcome: LensOutcome::Held("unparseable output twice".into()),
        },
    ];
    assert_eq!(aggregate(&reports, &[]), Verdict::Hold);
}

#[test]
fn a_skipped_lens_does_not_hold_the_run() {
    // Skipped is a checked non-applicability, not an unknown.
    let reports = vec![
        reported(Lens::Breakage),
        LensReport {
            scenario: "dial".into(),
            lens: Lens::Design,
            outcome: LensOutcome::Skipped(Skip::NoTasteProfile),
        },
    ];
    assert_eq!(aggregate(&reports, &[]), Verdict::Go);
}

#[test]
fn a_no_go_outranks_a_hold() {
    let reports = vec![
        reported(Lens::Breakage),
        LensReport {
            scenario: "dial".into(),
            lens: Lens::Motion,
            outcome: LensOutcome::Held("x".into()),
        },
    ];
    assert_eq!(
        aggregate(&reports, &finding(Lens::Breakage, Severity::Blocker)),
        Verdict::NoGo
    );
}

#[test]
fn exit_codes_are_zero_one_two() {
    assert_eq!(Verdict::Go.exit_code(), 0);
    assert_eq!(Verdict::NoGo.exit_code(), 1);
    assert_eq!(Verdict::Hold.exit_code(), 2);
}

// --- rendering ------------------------------------------------------

fn input() -> VerdictInput {
    VerdictInput {
        run_id: "20260814T101500Z".into(),
        reports: vec![reported(Lens::Breakage)],
        findings: finding(Lens::Breakage, Severity::Blocker),
        suppressed: Vec::new(),
        stale_rulings: Vec::new(),
        dropped_no_region: 0,
        deferred: Vec::new(),
        capture_failures: Vec::new(),
    }
}

#[test]
fn the_verdict_states_go_no_go_or_hold_in_those_exact_words() {
    assert!(render_verdict(&input()).contains("NO-GO"));
}

#[test]
fn a_capture_failure_is_reported_as_hold_and_is_never_a_go() {
    let mut i = input();
    i.findings = Vec::new();
    i.capture_failures = vec![("tardis-idle".into(), "unmapped glyph U+2726".into())];
    let text = render_verdict(&i);
    assert!(text.contains("tardis-idle"));
    assert!(text.contains("U+2726"));
    assert!(text.contains("HOLD"));
}

#[test]
fn suppressed_findings_appear_as_a_collapsed_previously_overruled_line() {
    let mut i = input();
    i.suppressed = finding(Lens::Design, Severity::Major);
    assert!(render_verdict(&i).contains("previously overruled (1)"));
}

#[test]
fn dropped_regionless_findings_are_counted_not_hidden() {
    let mut i = input();
    i.dropped_no_region = 2;
    assert!(render_verdict(&i).contains("2 finding(s) dropped for naming no region"));
}

#[test]
fn deferred_scenarios_are_named_rather_than_silently_omitted() {
    // A review that quietly covered half its scenarios reads as a
    // pass it did not earn.
    let mut i = input();
    i.deferred = vec!["smash-crabs-explosion".into()];
    let text = render_verdict(&i);
    assert!(text.contains("deferred"));
    assert!(text.contains("smash-crabs-explosion"));
}

#[test]
fn a_skipped_lens_is_named_with_its_reason() {
    let mut i = input();
    i.reports.push(LensReport {
        scenario: "dial".into(),
        lens: Lens::Design,
        outcome: LensOutcome::Skipped(Skip::NoTasteProfile),
    });
    let text = render_verdict(&i);
    assert!(text.contains("design"));
    assert!(text.contains("no taste.md"));
}

#[test]
fn a_held_lens_is_named_with_why_it_could_not_report() {
    let mut i = input();
    i.reports.push(LensReport {
        scenario: "dial".into(),
        lens: Lens::Motion,
        outcome: LensOutcome::Held("unparseable output twice".into()),
    });
    assert!(render_verdict(&i).contains("unparseable output twice"));
}

#[test]
fn stale_rulings_are_surfaced_for_revalidation() {
    let mut i = input();
    i.stale_rulings = vec!["a1b2c3d4e5f60718".into()];
    let text = render_verdict(&i);
    assert!(text.contains("stale"));
    assert!(text.contains("a1b2c3d4e5f60718"));
}

// --- verdict_for / write_verdict (this task's own extension beyond the
// brief's literal `aggregate`, needed because `aggregate` alone cannot
// see `capture_failures`) --------------------------------------------

#[test]
fn capture_failure_alone_forces_hold_never_go() {
    let mut i = input();
    i.findings = Vec::new(); // no blocker, so aggregate() alone would be Go
    i.capture_failures = vec![("tardis-idle".into(), "boom".into())];
    assert_eq!(verdict_for(&i), Verdict::Hold);
}

#[test]
fn capture_failure_does_not_downgrade_an_existing_no_go() {
    // findings already carries a Breakage blocker (from input()).
    let mut i = input();
    i.capture_failures = vec![("other-scenario".into(), "boom".into())];
    assert_eq!(
        verdict_for(&i),
        Verdict::NoGo,
        "a NO-GO elsewhere in the run must not be softened to a HOLD"
    );
}

#[test]
fn no_capture_failures_defers_to_the_poll_aggregate() {
    let mut i = input();
    i.findings = Vec::new();
    assert_eq!(verdict_for(&i), Verdict::Go);
}

#[test]
fn write_verdict_writes_verdict_md_into_the_run_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let i = input();
    let path = write_verdict(&i, tmp.path()).unwrap();
    assert_eq!(path, tmp.path().join("verdict.md"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, render_verdict(&i));
}
