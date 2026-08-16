//! `plumb merge`: ingests each lens's raw report text through
//! `finding::parse_findings`, merges duplicate findings, writes
//! `verdict.md` into the run directory, and hands back the run's
//! overall verdict so `dispatch` can exit with its code. A report that
//! fails to parse is not a usage error here — the lens is recorded as
//! `Held` with the parse failure as its reason, since an unparseable
//! agent response must never silently read as a clean pass.

#[cfg(test)]
mod tests;

use super::IoFailure;
use parallax_plumb::evidence::{self, EvidenceError};
use parallax_plumb::finding::{self, Lens};
use parallax_plumb::manifest;
use parallax_plumb::merge;
use parallax_plumb::rulings::{self, RulingError};
use parallax_plumb::verdict::{self, LensOutcome, LensReport, Verdict, VerdictInput};
use std::path::{Path, PathBuf};

/// One `--report lens:scenario:file` (or `lens:scenario:file:attempt`)
/// argument, parsed.
#[derive(Debug)]
struct ReportArg {
    lens: Lens,
    scenario: String,
    path: PathBuf,
    /// The retry attempt this reply belongs to; `1` when no fourth
    /// field was given.
    attempt: u32,
}

/// A JSON encoding failure writing a merge-stage artifact
/// (`merge/suppressed.json`/`merge/survivors.json`), together with the
/// path that caused it — mirrors `evidence::JsonFailure`.
#[derive(Debug)]
pub(super) struct JsonFailure {
    path: PathBuf,
    source: serde_json::Error,
}

impl std::fmt::Display for JsonFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "encoding {}: {}", self.path.display(), self.source)
    }
}

/// Failure running `merge`.
#[derive(Debug)]
pub(super) enum MergeCliError {
    /// A `--report` argument was not `lens:scenario:file[:attempt]`, or
    /// named an unknown lens.
    Usage(String),
    /// Reading a report file, `--taste`, or writing `verdict.md`, failed.
    Io(IoFailure),
    /// Encoding `merge/suppressed.json` or `merge/survivors.json` failed.
    Json(JsonFailure),
    /// `--rulings` named a file that exists but could not be read as
    /// ruling history (Arc 4). A missing `--rulings` file is not this
    /// — see `rulings::load_rulings` — only a malformed one is.
    Ruling(RulingError),
    /// A reply, a lens's parsed findings, or `run.json` could not be
    /// persisted as evidence.
    Evidence(EvidenceError),
}

impl std::fmt::Display for MergeCliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeCliError::Usage(m) => write!(f, "{m}"),
            MergeCliError::Io(e) => write!(f, "{e}"),
            MergeCliError::Json(e) => write!(f, "{e}"),
            MergeCliError::Ruling(e) => write!(f, "{e}"),
            MergeCliError::Evidence(e) => write!(f, "{e}"),
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

/// Splits the field after `lens:scenario:` into a path and an optional
/// trailing `:attempt`. Deliberately NOT `raw.splitn(4, ':')` — see the
/// module-level hazard this guards against: a Windows absolute path's
/// own drive-letter colon (`C:\tmp\rep.json`) would be split apart,
/// turning the path into `"C"`. Instead the attempt is peeled off the
/// *right* with `rsplit_once`, and only when the suffix actually parses
/// as a `u32` and a non-empty prefix remains — a bare drive-letter
/// split (`"C"`, `"\tmp\rep.json"`) fails that numeric check and falls
/// through to "the whole remainder is the path, attempt 1", which is
/// exactly the pre-existing behavior for a path with no attempt field.
fn split_path_and_attempt(remainder: &str) -> (PathBuf, u32) {
    if let Some((prefix, suffix)) = remainder.rsplit_once(':') {
        if !prefix.is_empty() {
            if let Ok(attempt) = suffix.parse::<u32>() {
                return (PathBuf::from(prefix), attempt);
            }
        }
    }
    (PathBuf::from(remainder), 1)
}

fn parse_report_arg(raw: &str) -> Result<ReportArg, MergeCliError> {
    let mut parts = raw.splitn(3, ':');
    let (Some(lens_s), Some(scenario), Some(remainder)) =
        (parts.next(), parts.next(), parts.next())
    else {
        return Err(MergeCliError::Usage(format!(
            "--report {raw:?} is not `lens:scenario:file`"
        )));
    };
    let lens = parse_lens(lens_s).ok_or_else(|| {
        MergeCliError::Usage(format!("--report {raw:?} names an unknown lens {lens_s:?}"))
    })?;
    let (path, attempt) = split_path_and_attempt(remainder);
    Ok(ReportArg {
        lens,
        scenario: scenario.to_string(),
        path,
        attempt,
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

/// Serializes `value` as pretty JSON and writes it to `path`, creating
/// any missing parent directories first — the merge-stage counterpart
/// to `evidence`'s own internal `write_json`, kept local here since
/// `merge/suppressed.json` and `merge/survivors.json` are whole-run
/// artifacts this module owns, not per-lens ones `evidence` writes on
/// this task's behalf.
fn write_merge_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), MergeCliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| {
            MergeCliError::Io(IoFailure {
                path: parent.to_path_buf(),
                source,
            })
        })?;
    }
    let json = serde_json::to_string_pretty(value).map_err(|source| {
        MergeCliError::Json(JsonFailure {
            path: path.to_path_buf(),
            source,
        })
    })?;
    std::fs::write(path, json).map_err(|source| {
        MergeCliError::Io(IoFailure {
            path: path.to_path_buf(),
            source,
        })
    })
}

/// Reads every `--report`'s raw text, parses it, merges duplicate
/// findings, runs the result through `rulings::suppress` (Arc 4) —
/// against `rulings_path`'s history, if given, hashed against
/// `taste_path`'s current content — writes `verdict.md` into
/// `run_dir`, and returns the run's overall verdict alongside the path
/// written. Neither `rulings_path` nor `taste_path` ever reaches
/// `finding::parse_findings` or anything upstream of this point: every
/// lens has already reported by the time a ruling is even loaded.
pub(super) fn run_merge(
    run_dir: &Path,
    reports: &[String],
    expected: &[String],
    capture_failures: &[String],
    rulings_path: Option<&Path>,
    taste_path: Option<&Path>,
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
        // The raw reply is persisted before parsing even touches it —
        // a reply that turns out unparseable must still leave a trace
        // of exactly what the lens returned.
        evidence::write_reply(run_dir, arg.lens, &arg.scenario, arg.attempt, &text)
            .map_err(MergeCliError::Evidence)?;
        received.push((arg.lens, arg.scenario.clone()));
        match finding::parse_findings(arg.lens, &arg.scenario, &text) {
            Ok(parsed) => {
                evidence::write_findings(run_dir, arg.lens, &arg.scenario, &parsed)
                    .map_err(MergeCliError::Evidence)?;
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
    evidence::write_run_json(run_dir, &run_id).map_err(MergeCliError::Evidence)?;

    let history = match rulings_path {
        Some(p) => rulings::load_rulings(p).map_err(MergeCliError::Ruling)?,
        None => Vec::new(),
    };
    let taste_text = taste_path
        .map(|p| {
            std::fs::read_to_string(p).map_err(|source| {
                MergeCliError::Io(IoFailure {
                    path: p.to_path_buf(),
                    source,
                })
            })
        })
        .transpose()?;
    let current_taste_hash = rulings::taste_hash(taste_text.as_deref());
    let rulings::Suppression {
        kept,
        suppressed,
        stale,
    } = rulings::suppress(merge::merge(all_findings), &history, &current_taste_hash);

    // The merge chain: what survived suppression (what the verdict is
    // actually judged on) and what a prior ruling already disposed of,
    // both written in full so an audit never has to take the verdict's
    // word for what got filtered out.
    let merge_dir = evidence::merge_dir(run_dir);
    write_merge_json(&merge_dir.join("survivors.json"), &kept)?;
    write_merge_json(&merge_dir.join("suppressed.json"), &suppressed)?;

    let input = VerdictInput {
        run_id,
        reports: lens_reports,
        findings: kept,
        suppressed,
        stale_rulings: stale,
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
