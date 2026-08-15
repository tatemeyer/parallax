//! Tests for `rulings`: the suppression filter's matching rules
//! (scope, staleness, severity ceiling), the `taste_hash`/JSONL
//! round trip, and the structural blinding guarantee — split into
//! its own file to keep `rulings/mod.rs` under the project's
//! soft line-count ceiling.

use super::*;
use crate::finding::{Confidence, Finding, Lens, Severity};
use crate::merge::{fingerprint, merge, MergedFinding};

fn f(scenario: &str, region: &str, claim: &str) -> Vec<MergedFinding> {
    merge(vec![Finding {
        lens: Lens::Design,
        scenario: scenario.into(),
        severity: Severity::Major,
        region: region.into(),
        claim: claim.into(),
        evidence: "e".into(),
        confidence: Confidence::Medium,
    }])
}

/// The Step 1 brief's own `ruling()` helper omits `severity` from
/// the struct literal, which does not compile against a struct
/// with a mandatory `severity` field (see task-17-report.md's
/// disclosed deviations). Every `ruling()` caller in this test
/// module suppresses a finding built by `f()`, which is always
/// `Severity::Major`, so `Severity::Major` is the natural default
/// here; tests that need a different ceiling call
/// `ruling_with_severity` directly.
fn ruling(scenario: &str, region: &str, claim: &str, hash: &str, scope: Scope) -> Ruling {
    ruling_with_severity(scenario, region, claim, Severity::Major, hash, scope)
}

fn ruling_with_severity(
    scenario: &str,
    region: &str,
    claim: &str,
    severity: Severity,
    hash: &str,
    scope: Scope,
) -> Ruling {
    Ruling {
        fingerprint: fingerprint(scenario, region, claim),
        lens: Lens::Design,
        severity,
        scenario: scenario.into(),
        region: region.into(),
        claim: claim.into(),
        reason: "density is the point here".into(),
        date: "2026-08-14".into(),
        taste_hash: hash.into(),
        scope,
    }
}

#[test]
fn a_matching_ruling_suppresses_its_finding() {
    let s = suppress(
        f("dial", "left column", "too dense"),
        &[ruling(
            "dial",
            "left column",
            "too dense",
            "H",
            Scope::Scenario,
        )],
        "H",
    );
    assert!(s.kept.is_empty());
    assert_eq!(s.suppressed.len(), 1);
}

#[test]
fn suppression_is_scoped_to_its_scenario_by_default() {
    // Overruling one screen's density must not mute density everywhere.
    let s = suppress(
        f("tardis", "left column", "too dense"),
        &[ruling(
            "dial",
            "left column",
            "too dense",
            "H",
            Scope::Scenario,
        )],
        "H",
    );
    assert_eq!(s.kept.len(), 1, "another scenario's ruling must not apply");
}

#[test]
fn project_wide_scope_is_opt_in_and_crosses_scenarios() {
    let r = ruling("dial", "left column", "too dense", "H", Scope::ProjectWide);
    let s = suppress(f("tardis", "left column", "too dense"), &[r], "H");
    assert_eq!(s.suppressed.len(), 1);
}

#[test]
fn a_ruling_made_under_a_different_taste_hash_is_stale_and_does_not_suppress() {
    // Your aesthetic moving is precisely when old rejections stop
    // being valid, so a stale ruling surfaces rather than applying.
    let s = suppress(
        f("dial", "left column", "too dense"),
        &[ruling(
            "dial",
            "left column",
            "too dense",
            "OLD",
            Scope::Scenario,
        )],
        "NEW",
    );
    assert_eq!(s.kept.len(), 1, "the finding reappears");
    assert!(s.suppressed.is_empty());
    assert_eq!(s.stale.len(), 1);
}

#[test]
fn a_non_matching_ruling_leaves_a_finding_alone() {
    let s = suppress(
        f("dial", "left column", "too dense"),
        &[ruling(
            "dial",
            "right column",
            "too dense",
            "H",
            Scope::Scenario,
        )],
        "H",
    );
    assert_eq!(s.kept.len(), 1);
}

#[test]
fn a_ruling_applies_across_lenses_because_the_fingerprint_excludes_the_lens() {
    let mut findings = f("dial", "left column", "too dense");
    findings[0].finding.lens = Lens::Motion;
    let s = suppress(
        findings,
        &[ruling(
            "dial",
            "left column",
            "too dense",
            "H",
            Scope::Scenario,
        )],
        "H",
    );
    assert_eq!(s.suppressed.len(), 1);
}

#[test]
fn an_overruled_cosmetic_finding_does_not_silence_a_blocker_in_the_same_region() {
    let mut findings = f("dial", "left column", "the label overlaps the frame");
    findings[0].finding.lens = Lens::Breakage;
    findings[0].finding.severity = Severity::Blocker;
    // The ruling was made against a `minor` design finding — same
    // scenario, region and claim, so the fingerprint matches.
    let r = ruling_with_severity(
        "dial",
        "left column",
        "the label overlaps the frame",
        Severity::Minor,
        "H",
        Scope::Scenario,
    );
    let s = suppress(findings, &[r], "H");
    assert_eq!(
        s.suppressed.len(),
        0,
        "a waived cosmetic complaint must not suppress a blocker"
    );
    assert_eq!(s.kept.len(), 1);
}

#[test]
fn a_ruling_suppresses_at_or_below_its_own_severity() {
    let mut findings = f("dial", "left column", "too dense");
    findings[0].finding.severity = Severity::Nit;
    let r = ruling_with_severity(
        "dial",
        "left column",
        "too dense",
        Severity::Minor,
        "H",
        Scope::Scenario,
    );
    let s = suppress(findings, &[r], "H");
    assert_eq!(s.suppressed.len(), 1);
}

#[test]
fn taste_hash_is_stable_and_distinguishes_an_edit() {
    assert_eq!(taste_hash(Some("abc")), taste_hash(Some("abc")));
    assert_ne!(taste_hash(Some("abc")), taste_hash(Some("abd")));
}

#[test]
fn an_absent_taste_profile_hashes_to_a_stable_sentinel() {
    assert_eq!(taste_hash(None), taste_hash(None));
    assert_ne!(taste_hash(None), taste_hash(Some("")));
}

#[test]
fn rulings_round_trip_through_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rulings.jsonl");
    append_ruling(&path, &ruling("a", "r", "c", "H", Scope::Scenario)).unwrap();
    append_ruling(&path, &ruling("b", "r", "c", "H", Scope::ProjectWide)).unwrap();
    let back = load_rulings(&path).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(back[1].scope, Scope::ProjectWide);
}

#[test]
fn a_missing_rulings_file_loads_as_an_empty_list() {
    assert!(load_rulings(std::path::Path::new("/does/not/exist.jsonl"))
        .unwrap()
        .is_empty());
}

// --- the severity ceiling, named in this task's own title -------

#[test]
fn a_ruling_at_the_finding_s_exact_severity_still_suppresses() {
    // The ceiling is inclusive: "at or below its own severity."
    let mut findings = f("dial", "left column", "too dense");
    findings[0].finding.severity = Severity::Blocker;
    let r = ruling_with_severity(
        "dial",
        "left column",
        "too dense",
        Severity::Blocker,
        "H",
        Scope::Scenario,
    );
    let s = suppress(findings, &[r], "H");
    assert_eq!(s.suppressed.len(), 1);
}

// --- the blinding guarantee: structurally enforced, pinned here -

/// The single most load-bearing test in this module. `suppress`
/// runs strictly after `merge` — every lens has already reported
/// by the time a `Ruling` exists to consult. This test pins the
/// structural half of that guarantee: `prompt::build_prompt` and
/// `prompt::LensInputs` (the only way a value reaches a lens
/// agent's prompt) are defined entirely in `prompt/mod.rs` and
/// `prompt/text.rs`, and neither file may ever import or mention
/// this module. If either grew a `use crate::rulings` or a
/// `Ruling`/`Suppression`/`suppress` reference, this test fails
/// before a lens ever sees a prior ruling — the same "assert on
/// the generated/consulted text" technique Task 8's own blinding
/// tests already use, aimed at the source instead of the output.
#[test]
fn prompt_construction_never_references_rulings_or_suppression() {
    for (name, src) in [
        ("prompt/mod.rs", include_str!("../prompt/mod.rs")),
        ("prompt/text.rs", include_str!("../prompt/text.rs")),
    ] {
        for forbidden in ["rulings", "Ruling", "Suppression", "suppress"] {
            assert!(
                !src.contains(forbidden),
                "{name} must never reference {forbidden:?} — a ruling must never reach a lens prompt"
            );
        }
    }
}
