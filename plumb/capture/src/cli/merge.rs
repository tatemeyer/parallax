//! `plumb merge`: ingests each lens's raw report text through
//! `finding::parse_findings`, merges duplicate findings, writes
//! `verdict.md` into the run directory, and hands back the run's
//! overall verdict so `dispatch` can exit with its code. A report that
//! fails to parse is not a usage error here — the lens is recorded as
//! `Held` with the parse failure as its reason, since an unparseable
//! agent response must never silently read as a clean pass.

use super::IoFailure;
use parallax_plumb::finding::{self, Lens};
use parallax_plumb::manifest;
use parallax_plumb::merge;
use parallax_plumb::verdict::{self, LensOutcome, LensReport, Verdict, VerdictInput};
use std::path::{Path, PathBuf};

/// One `--report lens:scenario:file` argument, parsed.
#[derive(Debug)]
struct ReportArg {
    lens: Lens,
    scenario: String,
    path: PathBuf,
}

/// Failure running `merge`.
#[derive(Debug)]
pub(super) enum MergeCliError {
    /// A `--report` argument was not `lens:scenario:file`, or named an
    /// unknown lens.
    Usage(String),
    /// Reading a report file, or writing `verdict.md`, failed.
    Io(IoFailure),
}

impl std::fmt::Display for MergeCliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeCliError::Usage(m) => write!(f, "{m}"),
            MergeCliError::Io(e) => write!(f, "{e}"),
        }
    }
}

fn parse_lens(s: &str) -> Option<Lens> {
    match s {
        "breakage" => Some(Lens::Breakage),
        "intent" => Some(Lens::Intent),
        "design" => Some(Lens::Design),
        "motion" => Some(Lens::Motion),
        _ => None,
    }
}

fn parse_report_arg(raw: &str) -> Result<ReportArg, MergeCliError> {
    let mut parts = raw.splitn(3, ':');
    let (Some(lens_s), Some(scenario), Some(path)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(MergeCliError::Usage(format!(
            "--report {raw:?} is not `lens:scenario:file`"
        )));
    };
    let lens = parse_lens(lens_s).ok_or_else(|| {
        MergeCliError::Usage(format!("--report {raw:?} names an unknown lens {lens_s:?}"))
    })?;
    Ok(ReportArg {
        lens,
        scenario: scenario.to_string(),
        path: PathBuf::from(path),
    })
}

/// One `--expected lens:scenario` argument, parsed: a lens the run
/// dispatched and is therefore owed a report from.
struct ExpectedArg {
    lens: Lens,
    scenario: String,
}

fn parse_expected_arg(raw: &str) -> Result<ExpectedArg, MergeCliError> {
    let mut parts = raw.splitn(2, ':');
    let (Some(lens_s), Some(scenario)) = (parts.next(), parts.next()) else {
        return Err(MergeCliError::Usage(format!(
            "--expected {raw:?} is not `lens:scenario`"
        )));
    };
    let lens = parse_lens(lens_s).ok_or_else(|| {
        MergeCliError::Usage(format!(
            "--expected {raw:?} names an unknown lens {lens_s:?}"
        ))
    })?;
    Ok(ExpectedArg {
        lens,
        scenario: scenario.to_string(),
    })
}

/// One `--capture-failure scenario:reason` argument, parsed. `reason`
/// is everything after the first `:`, so a reason string may itself
/// contain colons (a Windows path, a URL) without truncating.
fn parse_capture_failure_arg(raw: &str) -> Result<(String, String), MergeCliError> {
    let mut parts = raw.splitn(2, ':');
    let (Some(scenario), Some(reason)) = (parts.next(), parts.next()) else {
        return Err(MergeCliError::Usage(format!(
            "--capture-failure {raw:?} is not `scenario:reason`"
        )));
    };
    Ok((scenario.to_string(), reason.to_string()))
}

/// Reads every `--report`'s raw text, parses it, merges duplicate
/// findings, writes `verdict.md` into `run_dir`, and returns the run's
/// overall verdict alongside the path written.
pub(super) fn run_merge(
    run_dir: &Path,
    reports: &[String],
    expected: &[String],
    capture_failures: &[String],
) -> Result<(Verdict, PathBuf), MergeCliError> {
    let mut lens_reports = Vec::new();
    let mut all_findings = Vec::new();
    let mut dropped_no_region = 0;
    let mut received: Vec<(Lens, String)> = Vec::new();

    for raw in reports {
        let arg = parse_report_arg(raw)?;
        let text = std::fs::read_to_string(&arg.path).map_err(|source| {
            MergeCliError::Io(IoFailure {
                path: arg.path.clone(),
                source,
            })
        })?;
        received.push((arg.lens, arg.scenario.clone()));
        match finding::parse_findings(arg.lens, &arg.scenario, &text) {
            Ok(parsed) => {
                dropped_no_region += parsed.dropped_no_region;
                all_findings.extend(parsed.kept);
                lens_reports.push(LensReport {
                    scenario: arg.scenario,
                    lens: arg.lens,
                    outcome: LensOutcome::Reported,
                });
            }
            Err(e) => {
                lens_reports.push(LensReport {
                    scenario: arg.scenario,
                    lens: arg.lens,
                    outcome: LensOutcome::Held(e.to_string()),
                });
            }
        }
    }

    // A lens the run dispatched (per --expected) but that never
    // produced a --report holds the run rather than silently vanishing
    // from the poll — the same "silence must never read as consent"
    // rule the capture-failure fix below also serves.
    for raw in expected {
        let arg = parse_expected_arg(raw)?;
        if received
            .iter()
            .any(|(lens, scenario)| *lens == arg.lens && *scenario == arg.scenario)
        {
            continue;
        }
        lens_reports.push(LensReport {
            scenario: arg.scenario,
            lens: arg.lens,
            outcome: LensOutcome::Held(
                "expected to report but no --report was received for it".into(),
            ),
        });
    }

    let mut capture_failure_pairs = Vec::with_capacity(capture_failures.len());
    for raw in capture_failures {
        capture_failure_pairs.push(parse_capture_failure_arg(raw)?);
    }

    let run_id = run_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .unwrap_or_else(manifest::new_run_id);

    let input = VerdictInput {
        run_id,
        reports: lens_reports,
        findings: merge::merge(all_findings),
        suppressed: Vec::new(),
        stale_rulings: Vec::new(),
        dropped_no_region,
        deferred: Vec::new(),
        capture_failures: capture_failure_pairs,
    };

    let verdict = verdict::verdict_for(&input);
    let path = verdict::write_verdict(&input, run_dir).map_err(|source| {
        MergeCliError::Io(IoFailure {
            path: run_dir.join("verdict.md"),
            source,
        })
    })?;

    Ok((verdict, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_report(dir: &Path, name: &str, json: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, json).unwrap();
        path
    }

    const ONE_BLOCKER: &str = r#"[{"lens":"breakage","scenario":"dial","severity":"blocker",
      "region":"upper-right","claim":"the border does not close",
      "evidence":"e","confidence":"high"}]"#;

    #[test]
    fn merge_rejects_a_malformed_report_spec() {
        let err = parse_report_arg("breakage-dial-file.json").unwrap_err();
        assert!(matches!(err, MergeCliError::Usage(_)));
    }

    #[test]
    fn merge_rejects_an_unknown_lens_name() {
        let err = parse_report_arg("nonsense:dial:file.json").unwrap_err();
        assert!(matches!(err, MergeCliError::Usage(_)));
    }

    #[test]
    fn a_clean_report_produces_a_go_and_writes_verdict_md() {
        let tmp = tempfile::tempdir().unwrap();
        let report = write_report(tmp.path(), "breakage.json", "[]");
        let spec = format!("breakage:dial:{}", report.display());

        let (verdict, path) = run_merge(tmp.path(), &[spec], &[], &[]).unwrap();

        assert_eq!(verdict, Verdict::Go);
        assert!(path.is_file());
    }

    #[test]
    fn a_blocker_report_produces_a_no_go() {
        let tmp = tempfile::tempdir().unwrap();
        let report = write_report(tmp.path(), "breakage.json", ONE_BLOCKER);
        let spec = format!("breakage:dial:{}", report.display());

        let (verdict, _) = run_merge(tmp.path(), &[spec], &[], &[]).unwrap();

        assert_eq!(verdict, Verdict::NoGo);
    }

    #[test]
    fn unparseable_report_text_holds_rather_than_erroring_the_whole_merge() {
        let tmp = tempfile::tempdir().unwrap();
        let report = write_report(tmp.path(), "intent.json", "not json at all");
        let spec = format!("intent:dial:{}", report.display());

        let (verdict, _) = run_merge(tmp.path(), &[spec], &[], &[]).unwrap();

        assert_eq!(
            verdict,
            Verdict::Hold,
            "an unparseable lens report must hold, never silently pass as GO"
        );
    }

    #[test]
    fn a_missing_report_file_is_an_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = format!(
            "breakage:dial:{}",
            tmp.path().join("missing.json").display()
        );

        let err = run_merge(tmp.path(), &[spec], &[], &[]).unwrap_err();

        assert!(matches!(err, MergeCliError::Io(_)));
    }

    // --- review Finding 1: a capture failure must actually be able to
    // reach verdict.md from the CLI, not just from a hand-built
    // VerdictInput in a unit test. ----------------------------------

    #[test]
    fn a_capture_failure_flag_holds_the_run_with_no_report_at_all() {
        let tmp = tempfile::tempdir().unwrap();
        let cf = "tardis-idle:unmapped glyph U+2726".to_string();

        let (verdict, path) = run_merge(tmp.path(), &[], &[], &[cf]).unwrap();

        assert_eq!(
            verdict,
            Verdict::Hold,
            "a capture failure supplied via the CLI must reach the verdict, not just a hand-built VerdictInput"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("tardis-idle"));
        assert!(text.contains("U+2726"));
    }

    #[test]
    fn a_capture_failure_flag_does_not_soften_an_existing_no_go() {
        let tmp = tempfile::tempdir().unwrap();
        let report = write_report(tmp.path(), "breakage.json", ONE_BLOCKER);
        let spec = format!("breakage:dial:{}", report.display());
        let cf = "other-scenario:boom".to_string();

        let (verdict, _) = run_merge(tmp.path(), &[spec], &[], &[cf]).unwrap();

        assert_eq!(verdict, Verdict::NoGo);
    }

    #[test]
    fn merge_rejects_a_malformed_capture_failure_arg() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run_merge(tmp.path(), &[], &[], &["no-colon-here".into()]).unwrap_err();
        assert!(matches!(err, MergeCliError::Usage(_)));
    }

    // --- review Finding 2: a lens that was dispatched but never
    // returned a report must hold, not silently vanish from the poll.

    #[test]
    fn an_expected_lens_with_no_report_holds_the_run() {
        let tmp = tempfile::tempdir().unwrap();
        let report = write_report(tmp.path(), "breakage.json", "[]");
        let spec = format!("breakage:dial:{}", report.display());

        let (verdict, path) =
            run_merge(tmp.path(), &[spec], &["motion:dial".to_string()], &[]).unwrap();

        assert_eq!(
            verdict,
            Verdict::Hold,
            "a lens that was expected to report but did not must hold, not vanish"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("motion"));
    }

    #[test]
    fn an_expected_lens_that_did_report_is_not_double_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let report = write_report(tmp.path(), "breakage.json", "[]");
        let spec = format!("breakage:dial:{}", report.display());

        let (verdict, _) =
            run_merge(tmp.path(), &[spec], &["breakage:dial".to_string()], &[]).unwrap();

        assert_eq!(
            verdict,
            Verdict::Go,
            "a lens that both was expected and did report must not read as missing"
        );
    }

    #[test]
    fn merge_rejects_a_malformed_expected_arg() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run_merge(tmp.path(), &[], &["no-colon-here".into()], &[]).unwrap_err();
        assert!(matches!(err, MergeCliError::Usage(_)));
    }
}
