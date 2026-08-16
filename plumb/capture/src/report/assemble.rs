//! Walks a run directory and turns what's actually on disk —
//! manifests, prompts, replies, kept/dropped/clamped findings, and
//! whatever a ruling later suppressed after merge — into a
//! [`RunReport`]. Split out of `mod.rs` on line-count grounds (soft
//! ceiling 500 lines/file): this is the one function-heavy concern in
//! the `report` module that isn't itself a rendering step, so it
//! stands apart from `render`/`lens_render` (HTML) and
//! `geometry`/`region` (frame math) cleanly. Read-only: nothing here
//! ever writes into `run_dir`.

use super::render::{RunReport, ScenarioReport};
use super::{
    crop_png_data_uri, frame_rect, png_data_uri, resolve_frame, LensReport, RenderedFinding,
};
use crate::contact::grid_dims;
use crate::evidence::{self, Evidence};
use crate::finding::{Finding, Lens};
use crate::manifest::{self, RunManifest};
use crate::merge::{fingerprint, MergedFinding};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Builds a full evidence assembly for `run_dir`: every scenario's
/// manifest and contact sheet, and each of the four lenses' prompt,
/// replies, and findings — including everything discarded along the
/// way (dropped for naming no region, clamped to a lens's severity
/// ceiling, or suppressed by a ruling after merge) — so
/// `render::render_report` shows a human the whole chain, not just the
/// verdict word. Never fails: an unreadable artifact renders as an
/// explicit [`Evidence`] marker rather than aborting the report.
pub fn build_run_report(run_dir: &Path) -> RunReport {
    let (run_id, contract_version) = read_run_identity(run_dir);
    let verdict = read_text_evidence(&run_dir.join("verdict.md"));

    let suppressed_all = read_merge_suppressed(run_dir);
    let suppressed_fingerprints: HashSet<String> = match &suppressed_all {
        Evidence::Present(items) => items.iter().map(|m| m.fingerprint.clone()).collect(),
        Evidence::Missing | Evidence::Unparseable(_) => HashSet::new(),
    };

    let mut scenarios: Vec<ScenarioReport> = manifest_paths(run_dir)
        .into_iter()
        .filter_map(|p| manifest::read_manifest(&p).ok())
        .map(|m| build_scenario_report(run_dir, &m, &suppressed_all, &suppressed_fingerprints))
        .collect();
    // A run directory holds one manifest per scenario with no inherent
    // order (directory enumeration order is platform-dependent); sort
    // so the report is deterministic across two runs of the same
    // capture.
    scenarios.sort_by(|a, b| a.scenario.cmp(&b.scenario));

    RunReport {
        run_id,
        contract_version,
        verdict,
        scenarios,
    }
}

/// Reads `<run_dir>/run.json` for the run's id and evidence-contract
/// version. Falls back to the run directory's own name (and an unknown
/// version) when `run.json` is missing or unreadable — the same
/// directory-name fallback `cli::merge::run_merge` itself uses when no
/// run id was otherwise supplied.
fn read_run_identity(run_dir: &Path) -> (String, Option<u32>) {
    let fallback_id = run_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match std::fs::read_to_string(run_dir.join("run.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<evidence::RunJson>(&text).ok())
    {
        Some(j) => (j.run_id, Some(j.contract_version)),
        None => (fallback_id, None),
    }
}

/// Reads a text file into the two states a plain (non-JSON) artifact
/// can be in: [`Evidence::Missing`] when it could not be read,
/// [`Evidence::Present`] with its content otherwise. Mirrors
/// `evidence::read_evidence_text`'s behaviour exactly (lossy UTF-8,
/// `Missing` only on read failure) since `evidence` does not expose
/// that helper and this module reads one further whole-run artifact
/// (`verdict.md`) it does not cover.
fn read_text_evidence(path: &Path) -> Evidence<String> {
    match std::fs::read(path) {
        Ok(bytes) => Evidence::Present(String::from_utf8_lossy(&bytes).into_owned()),
        Err(_) => Evidence::Missing,
    }
}

/// Reads and parses `<run_dir>/merge/suppressed.json` — a whole-run
/// artifact covering every scenario and lens, written by
/// `rulings::suppress` via `cli::merge::run_merge`. This is the one
/// deliberate reach beyond `evidence::read_lens_evidence`: a ruling's
/// suppression happens after `merge::merge` collapses duplicate
/// findings across lenses, so it is not scoped to a single lens the
/// way prompt/reply/dropped/clamped evidence is.
fn read_merge_suppressed(run_dir: &Path) -> Evidence<Vec<MergedFinding>> {
    let path = evidence::merge_dir(run_dir).join("suppressed.json");
    match std::fs::read_to_string(&path) {
        Err(_) => Evidence::Missing,
        Ok(text) => match serde_json::from_str(&text) {
            Ok(items) => Evidence::Present(items),
            Err(_) => Evidence::Unparseable(text),
        },
    }
}

/// Every `*.manifest.json` file directly inside `run_dir` — one per
/// scenario the run captured. A run directory that cannot even be
/// listed yields no scenarios rather than an error, consistent with
/// this whole function never failing.
fn manifest_paths(run_dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(run_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".manifest.json"))
        })
        .collect()
}

/// Builds one scenario's report: its contact sheet, embedded whole, and
/// each of the four lenses' evidence.
fn build_scenario_report(
    run_dir: &Path,
    m: &RunManifest,
    suppressed_all: &Evidence<Vec<MergedFinding>>,
    suppressed_fingerprints: &HashSet<String>,
) -> ScenarioReport {
    let sheet_path = run_dir.join(&m.image);
    let sheet_uri = png_data_uri(&sheet_path).ok();
    // A lightweight header read, not a full decode — needed only to
    // convert a resolved frame index into pixel coordinates via
    // `frame_rect`; the crop itself is decoded separately, on demand,
    // per finding.
    let sheet_dims = image::image_dimensions(&sheet_path).ok();
    let (cols, _rows) = grid_dims(m.frame_count);

    let lenses = [Lens::Breakage, Lens::Intent, Lens::Design, Lens::Motion]
        .into_iter()
        .map(|lens| {
            build_lens_report(
                run_dir,
                lens,
                m,
                &sheet_path,
                sheet_dims,
                cols,
                suppressed_all,
                suppressed_fingerprints,
            )
        })
        .collect();

    ScenarioReport {
        scenario: m.scenario.clone(),
        sheet_uri,
        frame_count: m.frame_count,
        lenses,
    }
}

/// Builds one lens's report for one scenario: prompt and replies from
/// `evidence::read_lens_evidence`, dropped/clamped findings likewise,
/// and kept findings split against the whole-run suppression set — a
/// finding whose fingerprint (scenario + region + claim, the identity
/// `rulings::suppress` matches against) appears there moves into
/// `suppressed` instead of `findings`, mirroring what
/// `verdict::VerdictInput.findings` was actually judged on.
#[allow(clippy::too_many_arguments)]
fn build_lens_report(
    run_dir: &Path,
    lens: Lens,
    m: &RunManifest,
    sheet_path: &Path,
    sheet_dims: Option<(u32, u32)>,
    cols: u32,
    suppressed_all: &Evidence<Vec<MergedFinding>>,
    suppressed_fingerprints: &HashSet<String>,
) -> LensReport {
    let ev = evidence::read_lens_evidence(run_dir, lens, &m.scenario);

    let (parsed, findings, suppressed_here) = match ev.parsed {
        Evidence::Present(items) => {
            let mut kept = Vec::new();
            let mut suppressed_items = Vec::new();
            for f in items {
                let fp = fingerprint(&f.scenario, &f.region, &f.claim);
                if suppressed_fingerprints.contains(&fp) {
                    suppressed_items.push(f);
                } else {
                    kept.push(f);
                }
            }
            let rendered = kept
                .into_iter()
                .map(|f| render_finding(sheet_path, sheet_dims, m.frame_count, cols, f))
                .collect();
            (Evidence::Present(()), rendered, suppressed_items)
        }
        Evidence::Missing => (Evidence::Missing, Vec::new(), Vec::new()),
        Evidence::Unparseable(raw) => (Evidence::Unparseable(raw), Vec::new(), Vec::new()),
    };

    // The suppressed section's own three-state marker follows
    // `merge/suppressed.json`'s state, not `parsed.json`'s — they are
    // separate artifacts and either can be readable while the other
    // is not.
    let suppressed = match suppressed_all {
        Evidence::Missing => Evidence::Missing,
        Evidence::Unparseable(raw) => Evidence::Unparseable(raw.clone()),
        Evidence::Present(_) => Evidence::Present(suppressed_here),
    };

    LensReport {
        lens,
        parsed,
        findings,
        dropped: ev.dropped,
        clamped: ev.clamped,
        suppressed,
        prompt: ev.prompt,
        replies: ev.replies,
    }
}

/// Resolves `finding`'s region to a frame and attaches a crop of it
/// when the match is unambiguous (`region::resolve_frame`), leaving
/// `crop_uri` empty otherwise so the report falls back to the full
/// sheet already rendered once for the scenario. A crop that fails to
/// decode or encode (a corrupt or missing sheet) is treated the same
/// as an unresolved region: no crop, not an aborted report.
fn render_finding(
    sheet_path: &Path,
    sheet_dims: Option<(u32, u32)>,
    frame_count: usize,
    cols: u32,
    finding: Finding,
) -> RenderedFinding {
    let crop_uri = sheet_dims.and_then(|(w, h)| {
        resolve_frame(&finding.region, frame_count, cols)
            .and_then(|index| frame_rect(index, frame_count, w, h))
            .and_then(|rect| crop_png_data_uri(sheet_path, rect).ok())
    });
    RenderedFinding { finding, crop_uri }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{ClampRecord, Confidence, ParsedFindings, Severity};
    use crate::report::render_report;

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

    /// End-to-end region anchoring: a 3x3-grid sheet laid out with the
    /// real gutter constant, and a finding naming "top row, third
    /// (rightmost) frame" — the exact phrasing a real breakage-lens
    /// finding used — must resolve to a crop, not the full sheet.
    #[test]
    fn a_finding_whose_region_resolves_gets_a_crop_of_the_named_frame() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let run = tmp.path();
        let frame_count = 8usize;
        let (cols, rows) = grid_dims(frame_count);
        let pane = 10u32;
        let gutter = crate::contact::GUTTER_PX;
        let w = cols * pane + (cols + 1) * gutter;
        let h = rows * pane + (rows + 1) * gutter;
        write_manifest_and_sheet(run, "omni", frame_count, w, h);

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

        let html = render_report(&build_run_report(run));
        assert!(
            html.contains("class=\"crop\""),
            "a resolved region must embed a crop image: {html}"
        );
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
}
