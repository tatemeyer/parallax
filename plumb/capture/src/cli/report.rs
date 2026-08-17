//! `plumb report`: renders a run's already-captured evidence to a
//! human-facing HTML report. A viewer, not a stage — it runs no
//! capture, dispatches no agents, makes no network call, and never
//! writes into the run directory's *evidence*. The report file it
//! does write is the run's own artifact, not evidence, so writing it
//! at the default `<run-dir>/report.html` — inside the very directory
//! this subcommand must not disturb — is the intended behaviour, not
//! a violation of it.

use super::IoFailure;
use parallax_plumb::report::{build_run_report, render_report};
use std::path::{Path, PathBuf};

/// Failure producing a report: the run directory could not be read at
/// all, or the rendered HTML could not be written to its output path.
#[derive(Debug)]
pub(super) enum ReportCliError {
    /// `run_dir` does not exist or is not a readable directory. The
    /// only failure this subcommand's contract names explicitly.
    UnreadableRunDir(IoFailure),
    /// The rendered report could not be written to `--out` (or the
    /// default `<run_dir>/report.html`).
    Io(IoFailure),
}

impl std::fmt::Display for ReportCliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReportCliError::UnreadableRunDir(e) => write!(f, "{e}"),
            ReportCliError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Where a report lands when `--out` is not given: inside the run
/// directory itself, as the run's own artifact.
fn default_out(run_dir: &Path) -> PathBuf {
    run_dir.join("report.html")
}

/// Confirms `run_dir` is a readable directory before touching
/// anything else — `build_run_report` itself never fails (a missing
/// artifact renders as an explicit absence marker), so without this
/// check a wholly missing or bogus run directory would silently
/// render an near-empty report instead of the exit-1 this
/// subcommand's contract promises for "I could not read the
/// directory at all".
fn check_run_dir_readable(run_dir: &Path) -> Result<(), ReportCliError> {
    let is_dir = std::fs::metadata(run_dir)
        .map_err(|source| {
            ReportCliError::UnreadableRunDir(IoFailure {
                path: run_dir.to_path_buf(),
                source,
            })
        })?
        .is_dir();
    if is_dir {
        Ok(())
    } else {
        Err(ReportCliError::UnreadableRunDir(IoFailure {
            path: run_dir.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::NotADirectory, "not a directory"),
        }))
    }
}

/// Builds `run_dir`'s report and writes it to `out` (or
/// `<run_dir>/report.html` when `out` is `None`), returning the path
/// written. Read-only over the run's evidence: `build_run_report`/
/// `render_report` only ever read `run_dir`, and this function's own
/// single write lands at the output path, never at any evidence file.
fn write_report(run_dir: &Path, out: Option<&Path>) -> Result<PathBuf, ReportCliError> {
    check_run_dir_readable(run_dir)?;

    let run = build_run_report(run_dir);
    let html = render_report(&run);
    let out_path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_out(run_dir));
    std::fs::write(&out_path, html).map_err(|source| {
        ReportCliError::Io(IoFailure {
            path: out_path.clone(),
            source,
        })
    })?;
    Ok(out_path)
}

/// CLI entry point for `plumb report`: writes the run's HTML report
/// and returns the process exit code directly — 0 on success, 1 when
/// the run directory could not be read or the report could not be
/// written. Unlike `dispatch`'s other subcommands, this one has no
/// verdict-shaped outcome space to route through a match arm: its
/// only failure mode is plain I/O, so folding exit-code selection
/// into this single function (rather than adding a `ReportCliError`
/// arm to `dispatch`) keeps that one fact in one place.
pub(super) fn run_report(run_dir: &Path, out: Option<&Path>) -> i32 {
    match write_report(run_dir, out) {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_plumb::evidence::{self};
    use parallax_plumb::finding::{
        ClampRecord, Confidence, Finding, Lens, ParsedFindings, Severity,
    };
    use parallax_plumb::manifest::{self, RunManifest};
    use parallax_plumb::merge::{fingerprint, MergedFinding};

    /// A run directory's contents as sorted (relative path, content
    /// hash) pairs, walked recursively. Excludes `.html` files:
    /// `report` is the only thing ever known to write one into a run
    /// directory (the report itself, which is this run's own
    /// artifact, not evidence) — filtering by that one extension is
    /// what lets this same helper serve as both the "before" and
    /// "after" snapshot even when `--out` names a path that lands
    /// inside the run directory it reads. The assertion this buys is
    /// "no evidence artifact changed", not "the directory is
    /// byte-identical" — the latter would be false by construction
    /// whenever the default output path is used.
    ///
    /// Hashes actual content rather than comparing byte length: a
    /// same-size in-place rewrite (e.g. `parsed.json` swapped for a
    /// different-but-equal-length payload) changed nothing a
    /// length-only comparison could detect, which would have let a
    /// stray write through this guard undetected.
    fn dir_fingerprint(dir: &Path) -> Vec<(String, u64)> {
        use std::hash::{Hash, Hasher};

        fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, u64)>) {
            for entry in std::fs::read_dir(dir).expect("read_dir") {
                let entry = entry.expect("dir entry");
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("html"))
                {
                    continue;
                } else {
                    let rel = path
                        .strip_prefix(base)
                        .expect("strip_prefix")
                        .to_string_lossy()
                        .into_owned();
                    let bytes = std::fs::read(&path).expect("read file");
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    bytes.hash(&mut hasher);
                    out.push((rel, hasher.finish()));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    /// Writes `<scenario>.png` (a solid 40x30 image) and its manifest
    /// — the minimum a scenario needs before any lens evidence is
    /// added.
    fn write_manifest_and_sheet(run: &Path, scenario: &str) {
        let sheet = image::RgbaImage::from_pixel(40, 30, image::Rgba([10, 20, 30, 255]));
        let file_name = format!("{scenario}.png");
        sheet.save(run.join(&file_name)).expect("save sheet");

        let m = RunManifest {
            run_id: "test-run".into(),
            scenario: scenario.into(),
            adapter: "command".into(),
            image: PathBuf::from(file_name),
            animation: None,
            frame_count: 1,
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
    /// to its lens's ceiling, and one suppressed by a ruling after
    /// merge — the sort of evidence-laden run `report` is meant to
    /// render, and exactly the shape a stray write would be easiest
    /// to hide inside.
    fn stage_run_with_discards(run: &Path) {
        write_manifest_and_sheet(run, "s");

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

    /// Priority 3b: `dir_fingerprint` must catch a same-size in-place
    /// content rewrite, not just a length change — the exact gap a
    /// byte-length comparison could not close (`report_writes_a_
    /// file_and_never_touches_the_run` would have passed even if
    /// `report` silently rewrote `parsed.json` with different content
    /// of the same length).
    #[test]
    fn dir_fingerprint_detects_a_same_size_content_change() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let run = tmp.path();
        std::fs::write(run.join("parsed.json"), b"AAAA").expect("write");
        let before = dir_fingerprint(run);

        std::fs::write(run.join("parsed.json"), b"BBBB").expect("rewrite same length");
        let after = dir_fingerprint(run);

        assert_ne!(
            before, after,
            "a same-size in-place content change must change the fingerprint"
        );
    }

    #[test]
    fn report_writes_a_file_and_never_touches_the_run() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let run = tmp.path();
        stage_run_with_discards(run);
        let before = dir_fingerprint(run);

        let out = tmp.path().join("r.html");
        assert_eq!(run_report(run, Some(&out)), 0);
        assert!(out.exists());
        assert_eq!(
            dir_fingerprint(run),
            before,
            "report is read-only with respect to the run directory"
        );
    }

    #[test]
    fn report_defaults_its_output_path_inside_the_run_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let run = tmp.path();
        stage_run_with_discards(run);

        assert_eq!(run_report(run, None), 0);

        assert!(run.join("report.html").exists());
    }

    #[test]
    fn report_exits_1_on_an_unreadable_run_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("does-not-exist");

        assert_eq!(run_report(&missing, None), 1);
    }

    #[test]
    fn report_renders_the_html_body_into_the_output_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let run = tmp.path();
        stage_run_with_discards(run);
        let out = tmp.path().join("report.html");

        assert_eq!(run_report(run, Some(&out)), 0);

        let html = std::fs::read_to_string(&out).expect("read report");
        assert!(html.contains("DROPPED-CLAIM-TEXT"));
    }
}
