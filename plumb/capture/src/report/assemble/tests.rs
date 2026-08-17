//! Tests for `assemble`: split into its own file to keep
//! `report/assemble.rs` under the line-count ceiling, matching the
//! `mod.rs`/`tests.rs` split already used by `prompt`, `verdict`,
//! `rulings`, and `cli::merge`.

use super::*;
use crate::finding::{ClampRecord, Confidence, ParsedFindings, Severity};
use crate::manifest;
use crate::report::render_report;
use base64::Engine as _;

/// Writes `<scenario>.png` (a solid `w`x`h` image) and
/// `<scenario>.manifest.json` naming it, the minimum a scenario
/// needs before any lens evidence is added.
fn write_manifest_and_sheet(run: &Path, scenario: &str, frame_count: usize, w: u32, h: u32) {
    let sheet = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
    let file_name = format!("{scenario}.png");
    sheet.save(run.join(&file_name)).expect("save sheet");

    let m = RunManifest {
        run_id: "test-run".into(),
        scenario: scenario.into(),
        adapter: "command".into(),
        image: PathBuf::from(file_name),
        animation: None,
        frame_count,
        size: None,
        intent: None,
        expects: vec![],
        caveats: vec![],
    };
    manifest::write_manifest(&m, run).expect("write manifest");
}

fn finding(claim: &str, region: &str) -> Finding {
    Finding {
        lens: Lens::Breakage,
        scenario: "s".into(),
        severity: Severity::Major,
        region: region.into(),
        claim: claim.into(),
        evidence: "e".into(),
        confidence: Confidence::High,
    }
}

/// Stages a run with one finding dropped (no region), one clamped
/// to its lens's ceiling, one suppressed by a ruling after merge —
/// the three discards the load-bearing test proves keep their text.
fn stage_run_with_discards(run: &Path) {
    write_manifest_and_sheet(run, "s", 1, 40, 30);

    let dropped = finding("DROPPED-CLAIM-TEXT", "");
    let clamped_finding = finding("CLAMPED-CLAIM-TEXT", "frame 1");
    let suppressed_finding = finding("SUPPRESSED-CLAIM-TEXT", "frame 1");
    let kept_finding = finding("kept and visible", "frame 1");

    let parsed = ParsedFindings {
        kept: vec![
            clamped_finding.clone(),
            suppressed_finding.clone(),
            kept_finding,
        ],
        dropped_no_region: 1,
        clamped: 1,
        dropped: vec![dropped],
        clamped_records: vec![ClampRecord {
            finding: clamped_finding,
            from: Severity::Blocker,
        }],
    };
    evidence::write_findings(run, Lens::Breakage, "s", &parsed).expect("write findings");
    evidence::write_prompt(run, Lens::Breakage, "s", "PROMPT BODY").expect("write prompt");
    evidence::write_reply(run, Lens::Breakage, "s", 1, "[]").expect("write reply");

    let fp = fingerprint(
        &suppressed_finding.scenario,
        &suppressed_finding.region,
        &suppressed_finding.claim,
    );
    let merged = MergedFinding {
        finding: suppressed_finding,
        also_raised_by: vec![],
        fingerprint: fp,
    };
    let merge_dir = evidence::merge_dir(run);
    std::fs::create_dir_all(&merge_dir).expect("mkdir merge");
    std::fs::write(
        merge_dir.join("suppressed.json"),
        serde_json::to_string(&vec![merged]).expect("serialize suppressed"),
    )
    .expect("write suppressed.json");
}

/// Stages only a manifest and sheet — no lens ever reported.
fn stage_run_without_evidence(run: &Path) {
    write_manifest_and_sheet(run, "s", 1, 40, 30);
}

#[test]
fn the_report_contains_every_discarded_findings_text() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    stage_run_with_discards(run);

    let html = render_report(&build_run_report(run));

    assert!(
        html.contains("DROPPED-CLAIM-TEXT"),
        "a dropped finding's text must survive into the report"
    );
    assert!(
        html.contains("CLAMPED-CLAIM-TEXT"),
        "a clamped finding's text must survive into the report"
    );
    assert!(
        html.contains("SUPPRESSED-CLAIM-TEXT"),
        "a suppressed finding's text must survive into the report"
    );
    assert!(
        html.contains("kept and visible"),
        "a genuine survivor must still render as a live finding"
    );
}

#[test]
fn a_lens_with_no_persisted_reply_says_so() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    stage_run_without_evidence(run);

    let html = render_report(&build_run_report(run));
    assert!(
        html.contains("no reply persisted"),
        "absence is marked, never rendered as zero findings"
    );
}

#[test]
fn a_run_with_two_scenarios_renders_a_section_for_each() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    write_manifest_and_sheet(run, "alpha", 1, 20, 20);
    write_manifest_and_sheet(run, "beta", 1, 20, 20);

    let report = build_run_report(run);
    assert_eq!(report.scenarios.len(), 2);
    let html = render_report(&report);
    assert!(html.contains("alpha"));
    assert!(html.contains("beta"));
}

#[test]
fn a_present_run_json_supplies_the_run_id_and_contract_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    evidence::write_run_json(run, "20260815T000000Z").expect("write run.json");

    let report = build_run_report(run);
    assert_eq!(report.run_id, "20260815T000000Z");
    assert_eq!(report.contract_version, Some(evidence::CONTRACT_VERSION));
}

#[test]
fn a_run_with_no_run_json_falls_back_to_the_directory_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path().join("fallback-run-id");
    std::fs::create_dir_all(&run).expect("mkdir");

    let report = build_run_report(&run);
    assert_eq!(report.run_id, "fallback-run-id");
    assert_eq!(report.contract_version, None);
}

#[test]
fn a_persisted_verdict_md_is_embedded_in_the_report() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    std::fs::write(run.join("verdict.md"), "# Plumb verdict: NO-GO (run x)\n")
        .expect("write verdict.md");

    let html = render_report(&build_run_report(run));
    assert!(html.contains("Plumb verdict: NO-GO (run x)"));
}

/// End-to-end region anchoring: a real tiled contact sheet (built via
/// `contact::tile_frames`, the same function production uses) with
/// eight distinguishable panes, and a finding naming "top row, third
/// (rightmost) frame" — the exact phrasing a real breakage-lens
/// finding used — must crop out exactly that frame's pixels, not
/// merely produce *some* crop. A solid-colour fixture (the previous
/// version of this test) cannot catch a wrong frame index or a gutter
/// off-by-one, since every crop would be byte-identical regardless;
/// distinguishable panes can, following `geometry.rs`'s and
/// `contact.rs`'s own pixel-matching pattern.
#[test]
fn a_finding_whose_region_resolves_crops_the_correct_named_frame() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    let frame_count = 8usize;
    let pane = 10u32;

    let colors: [image::Rgba<u8>; 8] = [
        image::Rgba([255, 0, 0, 255]),   // 0: top-left
        image::Rgba([0, 255, 0, 255]),   // 1: top-middle
        image::Rgba([0, 0, 255, 255]),   // 2: top-right ("top row, third frame")
        image::Rgba([255, 255, 0, 255]), // 3
        image::Rgba([255, 0, 255, 255]), // 4
        image::Rgba([0, 255, 255, 255]), // 5
        image::Rgba([128, 64, 0, 255]),  // 6
        image::Rgba([0, 128, 64, 255]),  // 7
    ];
    let frames: Vec<image::RgbaImage> = colors
        .iter()
        .map(|c| image::RgbaImage::from_pixel(pane, pane, *c))
        .collect();
    let sheet = crate::contact::tile_frames(&frames);
    let file_name = "omni.png";
    sheet.save(run.join(file_name)).expect("save sheet");

    let m = RunManifest {
        run_id: "test-run".into(),
        scenario: "omni".into(),
        adapter: "command".into(),
        image: PathBuf::from(file_name),
        animation: None,
        frame_count,
        size: None,
        intent: None,
        expects: vec![],
        caveats: vec![],
    };
    manifest::write_manifest(&m, run).expect("write manifest");

    let f = finding(
        "solid fill",
        "top row, third (rightmost) frame of the contact sheet",
    );
    let parsed = ParsedFindings {
        kept: vec![f],
        dropped_no_region: 0,
        clamped: 0,
        dropped: vec![],
        clamped_records: vec![],
    };
    evidence::write_findings(run, Lens::Breakage, "omni", &parsed).expect("write findings");

    let report = build_run_report(run);
    let crop_uri = report.scenarios[0].lenses[0].findings[0]
        .crop_uri
        .as_ref()
        .expect("region should resolve to a crop");

    let b64 = crop_uri
        .strip_prefix("data:image/png;base64,")
        .expect("data uri carries the expected PNG prefix");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("valid base64");
    let cropped = image::load_from_memory(&bytes)
        .expect("valid PNG bytes")
        .to_rgba8();

    assert_eq!((cropped.width(), cropped.height()), (pane, pane));
    for pixel in cropped.pixels() {
        assert_eq!(
            *pixel, colors[2],
            "crop must contain only frame 2's colour — a wrong frame index or \
             gutter off-by-one would crop a different pane"
        );
    }
}

#[test]
fn an_unresolved_region_falls_back_to_the_full_sheet_with_no_crop() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    write_manifest_and_sheet(run, "s", 1, 40, 30);

    let f = finding("vague claim", "upper-right quadrant");
    let parsed = ParsedFindings {
        kept: vec![f],
        dropped_no_region: 0,
        clamped: 0,
        dropped: vec![],
        clamped_records: vec![],
    };
    evidence::write_findings(run, Lens::Breakage, "s", &parsed).expect("write findings");

    let html = render_report(&build_run_report(run));
    assert!(
        html.contains("vague claim"),
        "the finding itself still renders"
    );
    assert!(
        !html.contains("class=\"crop\""),
        "an unresolved region must not fabricate a crop"
    );
}

/// The corrupt-manifest defect (Priority 1b): a scenario whose
/// manifest cannot be parsed must still appear in the report, visibly
/// marked, rather than silently vanishing while the header's scenario
/// count and the readable scenarios beside it look unremarkable.
#[test]
fn a_corrupt_manifest_renders_a_marker_instead_of_vanishing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    write_manifest_and_sheet(run, "good", 1, 20, 20);
    std::fs::write(run.join("bad.manifest.json"), "not json at all")
        .expect("write corrupt manifest");

    let report = build_run_report(run);
    assert_eq!(
        report.scenarios.len(),
        2,
        "the corrupt manifest's scenario must still appear, not vanish"
    );

    let html = render_report(&report);
    assert!(html.contains("good"), "the readable scenario still renders");
    assert!(
        html.contains("bad"),
        "the corrupt scenario's name (from its filename) must appear"
    );
    assert!(html.contains("manifest: present but unparseable"));
    assert!(html.contains("not json at all"));
}

/// The survivors-attrition defect (Priority 1a): when `parsed.json`
/// is corrupt but `dropped.json`/`clamped.json`/`merge/suppressed.json`
/// are all present-and-empty, the pre-fix chain reads "0 raw -> 0
/// dropped -> 0 clamped -> 0 suppressed -> 0 survivors" — self
/// consistent arithmetic, no `?` anywhere, indistinguishable from a
/// genuinely clean run even though a blocker sits unread inside the
/// corrupt `parsed.json`. `raw` and `survivors` must both read `?`
/// instead.
#[test]
fn a_corrupt_parsed_json_shows_unknown_survivors_and_unknown_raw() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    write_manifest_and_sheet(run, "s", 1, 40, 30);

    // dropped.json/clamped.json start present-and-empty (a lens that
    // genuinely reported nothing discarded), and merge/suppressed.json
    // likewise — every OTHER term in the chain is known-zero, so only
    // parsed.json's own corruption can be the thing that saves
    // survivors/raw from misreading as a clean 0.
    let empty_parsed = ParsedFindings {
        kept: vec![],
        dropped_no_region: 0,
        clamped: 0,
        dropped: vec![],
        clamped_records: vec![],
    };
    evidence::write_findings(run, Lens::Breakage, "s", &empty_parsed).expect("write findings");
    let merge_dir = evidence::merge_dir(run);
    std::fs::create_dir_all(&merge_dir).expect("mkdir merge");
    std::fs::write(
        merge_dir.join("suppressed.json"),
        serde_json::to_string(&Vec::<MergedFinding>::new()).expect("serialize"),
    )
    .expect("write empty suppressed.json");

    // Now corrupt parsed.json in place — the one artifact this test is
    // actually about.
    let dir = evidence::lens_dir(run, Lens::Breakage, "s");
    std::fs::write(dir.join("parsed.json"), "not json at all").expect("write corrupt parsed.json");

    let html = render_report(&build_run_report(run));
    assert!(
        html.contains("? raw \u{2192} 0 dropped \u{2192} 0 clamped \u{2192} 0 suppressed \u{2192} ? survivors"),
        "a corrupt parsed.json must make survivors (and therefore raw) read ?, \
         not a falsely-known 0 that reads as a clean pass: {html}"
    );
}
