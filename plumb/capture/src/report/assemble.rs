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
use crate::manifest::RunManifest;
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
        .map(|p| match read_manifest_evidence(&p) {
            Evidence::Present(m) => {
                build_scenario_report(run_dir, &m, &suppressed_all, &suppressed_fingerprints)
            }
            Evidence::Missing => unreadable_scenario_report(&p, Evidence::Missing),
            Evidence::Unparseable(raw) => {
                unreadable_scenario_report(&p, Evidence::Unparseable(raw))
            }
        })
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

/// Reads and parses one `*.manifest.json` file into the same
/// three-state marker every other artifact in this module uses:
/// [`Evidence::Missing`] when the file itself could not be read,
/// [`Evidence::Unparseable`] (carrying its raw text) when it was read
/// but did not parse as [`RunManifest`], [`Evidence::Present`] on
/// success. Deliberately bypasses `manifest::read_manifest` (which
/// collapses both failure modes into one opaque `ManifestError`) since
/// a corrupt manifest must render as a visible marker with its raw
/// text still shown, not silently drop its whole scenario from the
/// report the way a `.ok()`-filtered read would.
fn read_manifest_evidence(path: &Path) -> Evidence<RunManifest> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return Evidence::Missing,
    };
    match serde_json::from_str(&text) {
        Ok(m) => Evidence::Present(m),
        Err(_) => Evidence::Unparseable(text),
    }
}

/// The scenario name a manifest path implies, for the one case its
/// content cannot supply it: `<scenario>.manifest.json`'s stem, or the
/// literal filename when it doesn't even match that shape.
fn manifest_scenario_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.strip_suffix(".manifest.json").unwrap_or(n).to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Builds a stand-in scenario report for a manifest that could not be
/// read: no sheet, no frames, no lenses (there is nothing to derive
/// them from), but a visible, named section carrying the manifest's
/// own [`Evidence`] marker — so an unreadable manifest still shows up
/// in the report's scenario count and in the document itself, rather
/// than silently disappearing while the run looks unremarkable.
fn unreadable_scenario_report(path: &Path, manifest: Evidence<()>) -> ScenarioReport {
    ScenarioReport {
        scenario: manifest_scenario_name(path),
        manifest,
        sheet_uri: None,
        frame_count: 0,
        lenses: vec![],
    }
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
        manifest: Evidence::Present(()),
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
mod tests;
