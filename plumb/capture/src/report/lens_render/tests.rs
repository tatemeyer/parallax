//! Tests for `lens_render`: split into its own file to keep
//! `report/lens_render.rs` under the line-count ceiling, matching the
//! `mod.rs`/`tests.rs` split already used by `prompt`, `verdict`,
//! `rulings`, and `cli::merge`.

use super::*;

fn sample_finding(claim: &str) -> Finding {
    Finding {
        lens: Lens::Breakage,
        scenario: "s".into(),
        severity: Severity::Major,
        region: "top row, first frame".into(),
        claim: claim.into(),
        evidence: "e".into(),
        confidence: Confidence::High,
    }
}

fn empty_lens(lens: Lens) -> LensReport {
    LensReport {
        lens,
        parsed: Evidence::Present(()),
        findings: vec![],
        dropped: Evidence::Missing,
        clamped: Evidence::Missing,
        suppressed: Evidence::Missing,
        prompt: Evidence::Missing,
        replies: vec![],
    }
}

#[test]
fn a_lens_with_no_persisted_reply_says_so() {
    let html = render_lens(&empty_lens(Lens::Breakage));
    assert!(html.contains("no reply persisted"));
}

#[test]
fn dropped_and_clamped_findings_keep_their_claim_text() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.dropped = Evidence::Present(vec![sample_finding("DROPPED-CLAIM-TEXT")]);
    lens.clamped = Evidence::Present(vec![ClampRecord {
        finding: sample_finding("CLAMPED-CLAIM-TEXT"),
        from: Severity::Blocker,
    }]);
    let html = render_lens(&lens);
    assert!(html.contains("DROPPED-CLAIM-TEXT"));
    assert!(html.contains("CLAMPED-CLAIM-TEXT"));
}

#[test]
fn a_suppressed_findings_claim_text_survives_into_the_report() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.suppressed = Evidence::Present(vec![sample_finding("SUPPRESSED-CLAIM-TEXT")]);
    let html = render_lens(&lens);
    assert!(html.contains("SUPPRESSED-CLAIM-TEXT"));
}

#[test]
fn a_corrupt_parsed_json_is_marked_unparseable_not_zero_findings() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.parsed = Evidence::Unparseable("not json at all".into());
    let html = render_lens(&lens);
    assert!(html.contains("findings: present but unparseable"));
    assert!(html.contains("not json at all"));
}

#[test]
fn the_attrition_chain_accounts_for_every_finding() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.dropped = Evidence::Present(vec![sample_finding("d")]);
    lens.clamped = Evidence::Present(vec![ClampRecord {
        finding: sample_finding("c"),
        from: Severity::Blocker,
    }]);
    lens.suppressed = Evidence::Present(vec![sample_finding("s")]);
    lens.findings = vec![RenderedFinding {
        finding: sample_finding("kept"),
        crop_uri: None,
    }];
    let html = render_lens(&lens);
    assert!(
        html.contains("3 raw \u{2192} 1 dropped \u{2192} 1 clamped \u{2192} 1 suppressed \u{2192} 1 survivors"),
        "attrition chain must read 3 raw -> 1 dropped -> 1 clamped -> 1 suppressed -> 1 survivors: {html}"
    );
}

#[test]
fn a_finding_with_a_resolved_crop_embeds_it() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.findings = vec![RenderedFinding {
        finding: sample_finding("kept with crop"),
        crop_uri: Some("data:image/png;base64,AAAA".into()),
    }];
    let html = render_lens(&lens);
    assert!(html.contains("data:image/png;base64,AAAA"));
    assert!(html.contains("kept with crop"));
}

#[test]
fn a_crop_uri_is_escaped_like_every_other_interpolated_string() {
    // Safe today (a crop URI is always our own generated base64),
    // but the escaping must not depend on that fact — route it
    // through html_escape for consistency with the sheet URI and
    // every other interpolated string, so a future non-base64
    // source can never slip an unescaped quote or tag into `src`.
    let mut lens = empty_lens(Lens::Breakage);
    lens.findings = vec![RenderedFinding {
        finding: sample_finding("kept with a hostile crop uri"),
        crop_uri: Some("data:image/png;base64,AAA\"><script>alert(1)</script>".into()),
    }];
    let html = render_lens(&lens);
    assert!(
        !html.contains("\"><script>alert(1)</script>"),
        "the crop uri must be escaped, not interpolated raw: {html}"
    );
    assert!(html.contains("&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;"));
}

#[test]
fn the_attrition_chain_shows_unknown_rather_than_zero_when_dropped_is_missing() {
    // dropped.json never persisted: the true count is unknown, and
    // an unknown count must never render as 0 — that would make an
    // unreadable artifact look like a healthy "found nothing"
    // result, the exact failure this whole task exists to prevent.
    let mut lens = empty_lens(Lens::Breakage);
    lens.dropped = Evidence::Missing;
    lens.clamped = Evidence::Present(vec![]);
    lens.suppressed = Evidence::Present(vec![]);
    lens.findings = vec![RenderedFinding {
        finding: sample_finding("kept"),
        crop_uri: None,
    }];
    let html = render_lens(&lens);
    assert!(
        html.contains("? raw \u{2192} ? dropped \u{2192} 0 clamped \u{2192} 0 suppressed \u{2192} 1 survivors"),
        "an unknown dropped count must show ?, and must make raw unknown too: {html}"
    );
}

#[test]
fn a_missing_clamped_count_reads_as_unknown_without_making_raw_unknown() {
    // clamped is a subset of survivors, not a term in the raw sum
    // (dropped + survivors + suppressed) — its own unreadability
    // must still show as ?, but must not spuriously blank out an
    // otherwise-known raw count.
    let mut lens = empty_lens(Lens::Breakage);
    lens.dropped = Evidence::Present(vec![]);
    lens.clamped = Evidence::Missing;
    lens.suppressed = Evidence::Present(vec![]);
    lens.findings = vec![RenderedFinding {
        finding: sample_finding("kept"),
        crop_uri: None,
    }];
    let html = render_lens(&lens);
    assert!(
        html.contains("1 raw \u{2192} 0 dropped \u{2192} ? clamped \u{2192} 0 suppressed \u{2192} 1 survivors"),
        "a missing clamped count shows ? without corrupting raw: {html}"
    );
}

#[test]
fn dropped_being_unparseable_shows_the_raw_text() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.dropped = Evidence::Unparseable("not json at all".into());
    let html = render_lens(&lens);
    assert!(html.contains("Dropped (no region named): present but unparseable"));
    assert!(html.contains("not json at all"));
}

#[test]
fn clamped_being_unparseable_shows_the_raw_text() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.clamped = Evidence::Unparseable("also not json".into());
    let html = render_lens(&lens);
    assert!(html.contains("Clamped: present but unparseable"));
    assert!(html.contains("also not json"));
}

#[test]
fn suppressed_being_unparseable_shows_the_raw_text() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.suppressed = Evidence::Unparseable("still not json".into());
    let html = render_lens(&lens);
    assert!(html.contains("Suppressed (overruled by a prior ruling): present but unparseable"));
    assert!(html.contains("still not json"));
}

#[test]
fn a_missing_parsed_json_says_not_persisted() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.parsed = Evidence::Missing;
    let html = render_lens(&lens);
    assert!(html.contains("findings: not persisted"));
}

#[test]
fn a_lens_reporting_zero_findings_is_distinct_from_no_reply() {
    // Present(vec![]) — the lens genuinely dropped nothing — must
    // not read the same as an absent reply.
    let mut lens = empty_lens(Lens::Breakage);
    lens.replies = vec![(1, "[]".into())];
    let html = render_lens(&lens);
    assert!(!html.contains("no reply persisted"));
    assert!(html.contains("no findings"));
}

// --- Priority 2: guard the artifact that proves a critic was blinded.
// Deleting the `render_text_evidence("Prompt", &lens.prompt)` call
// from `render_lens` left all pre-existing tests green — none of them
// actually asserted a lens's prompt text reaches the document. These
// pin that, plus the severity/confidence labels and the clamp
// from->to arrow, so a mislabeling (e.g. `Severity::Blocker` mapped
// to the string `"nit"`) fails loudly instead of shipping quietly. ---

#[test]
fn a_lens_prompt_reaches_the_rendered_html() {
    let mut lens = empty_lens(Lens::Breakage);
    lens.prompt = Evidence::Present("PROMPT-REACHES-HTML-MARKER".into());
    let html = render_lens(&lens);
    assert!(
        html.contains("PROMPT-REACHES-HTML-MARKER"),
        "the prompt is the artifact that proves a critic was blinded — it must reach the document: {html}"
    );
}

#[test]
fn a_findings_severity_and_confidence_labels_render_exactly() {
    let mut lens = empty_lens(Lens::Breakage);
    let mut f = sample_finding("kept");
    f.severity = Severity::Minor;
    f.confidence = Confidence::Low;
    lens.findings = vec![RenderedFinding {
        finding: f,
        crop_uri: None,
    }];
    let html = render_lens(&lens);
    assert!(
        html.contains("[minor]"),
        "the finding's own severity must render as its own label, not a stand-in: {html}"
    );
    assert!(
        html.contains("confidence: low"),
        "the finding's own confidence must render as its own label, not a stand-in: {html}"
    );
}

#[test]
fn a_clamp_records_from_and_to_severities_both_render_correctly() {
    // from = Major, the surviving finding's own severity = Minor — two
    // different, specific values, so a hardcoded or swapped mapping
    // (e.g. always rendering "nit" as the reviewer found) would fail
    // this assertion where a looser "does some clamp text appear"
    // check could not.
    let mut lens = empty_lens(Lens::Breakage);
    let mut clamped_finding = sample_finding("CLAMP-ARROW-CLAIM");
    clamped_finding.severity = Severity::Minor;
    lens.clamped = Evidence::Present(vec![ClampRecord {
        finding: clamped_finding,
        from: Severity::Major,
    }]);
    let html = render_lens(&lens);
    assert!(
        html.contains("[major \u{2192} minor]"),
        "the clamp record's from -> to severities must both render, exactly: {html}"
    );
}

#[test]
fn an_unresolved_region_renders_a_declined_marker_not_nothing() {
    // Before this fix, `render_findings` rendered nothing at all for
    // an unresolved region (a bare `if let Some(uri) = ...` with no
    // `else`) — a reader could not tell "declined, ambiguous" from
    // "this finding carries no visual evidence at all".
    let mut lens = empty_lens(Lens::Breakage);
    lens.findings = vec![RenderedFinding {
        finding: sample_finding("vague claim with no resolved crop"),
        crop_uri: None,
    }];
    let html = render_lens(&lens);
    assert!(
        html.contains("region not resolved"),
        "an unresolved region must render an explicit declined marker: {html}"
    );
}
