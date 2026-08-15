//! Aggregates the lens poll into GO / NO-GO / HOLD and renders
//! `verdict.md`. Every lens reports on its own domain only, no lens can
//! clear another's, and the aggregate carries the most severe report
//! received. Rendering lives in the private `render` submodule to keep
//! aggregation logic and presentation independently readable and both
//! under the project's soft line-count ceiling.

mod render;
#[cfg(test)]
mod tests;

pub use render::render_verdict;

use crate::finding::{Lens, Severity};
use crate::merge::MergedFinding;
use crate::prompt::Skip;
use std::path::{Path, PathBuf};

/// The aggregate poll result. `exit_code` is the contract `cli::dispatch`
/// (Task 6) already committed to: 0 GO, 1 NO-GO, 2 HOLD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No findings, or advisory findings only.
    Go,
    /// At least one unresolved blocker from a blocker-capable lens.
    NoGo,
    /// A lens could not reach a verdict, or a capture failed outright.
    Hold,
}

impl Verdict {
    /// The process exit code this verdict maps to: 0 GO, 1 NO-GO, 2 HOLD.
    pub fn exit_code(self) -> i32 {
        match self {
            Verdict::Go => 0,
            Verdict::NoGo => 1,
            Verdict::Hold => 2,
        }
    }
}

/// One lens's outcome for one scenario, as reported to the poll.
#[derive(Debug, Clone)]
pub struct LensReport {
    /// Which scenario this report is about.
    pub scenario: String,
    /// Which lens reported it.
    pub lens: Lens,
    /// What the lens actually managed to do.
    pub outcome: LensOutcome,
}

/// What a dispatched lens actually produced. `Skipped` is a checked
/// non-applicability (never holds the run); `Held` is an unknown (always
/// does).
#[derive(Debug, Clone)]
pub enum LensOutcome {
    /// The lens ran and reported (findings may still be empty).
    Reported,
    /// The lens did not apply to this capture, and why.
    Skipped(Skip),
    /// Capture failed or the agent's output was unparseable twice; the
    /// lens could not reach a verdict. Carries why.
    Held(String),
}

/// Aggregates the poll. Every lens reports on its own domain only, no
/// lens can clear another's, and the run carries the most severe report
/// received. A `Hold` is never upgraded to a `Go`.
///
/// Deliberately `pub(crate)`, not `pub`: this function cannot see
/// `VerdictInput::capture_failures`, so a caller outside this module
/// that used it directly instead of [`verdict_for`] would silently
/// regain the "capture failure reads as GO" bug. Keeping it crate-
/// private makes `verdict_for` the only route out of the module, so
/// the capture-failure rule is enforced by the compiler, not by a
/// convention every call site has to remember.
pub(crate) fn aggregate(reports: &[LensReport], findings: &[MergedFinding]) -> Verdict {
    let blocked = findings
        .iter()
        .any(|m| m.finding.severity == Severity::Blocker && m.finding.lens.is_blocker_capable());
    if blocked {
        return Verdict::NoGo;
    }
    if reports
        .iter()
        .any(|r| matches!(r.outcome, LensOutcome::Held(_)))
    {
        return Verdict::Hold;
    }
    Verdict::Go
}

/// Everything `render_verdict` needs to fully account for one run: the
/// poll, the merged findings, and everything that could not be checked
/// — suppressed findings (Arc 4's affordance), stale rulings, regionless
/// drops, deferred scenarios, and capture failures. Nothing here is
/// optional to report; a field being empty is itself the report.
pub struct VerdictInput {
    /// The run's timestamp id.
    pub run_id: String,
    /// Every lens's outcome, across every scenario in the run.
    pub reports: Vec<LensReport>,
    /// Merged findings that survived enforcement, most severe first.
    pub findings: Vec<MergedFinding>,
    /// Findings a prior ruling already overruled (Arc 4). Rendered as a
    /// single collapsed count — rulings themselves are out of scope
    /// here.
    pub suppressed: Vec<MergedFinding>,
    /// Fingerprints of rulings that need re-validation against this run.
    pub stale_rulings: Vec<String>,
    /// How many findings were dropped for naming no region (Task 7).
    pub dropped_no_region: usize,
    /// Scenarios whose review was deferred to a later batch (over cap).
    pub deferred: Vec<String>,
    /// Scenarios whose capture failed outright, with the adapter error.
    pub capture_failures: Vec<(String, String)>,
}

/// The single source of truth for a run's overall verdict, including
/// what `aggregate` cannot see on its own: a capture failure is never a
/// GO, so a non-empty `capture_failures` folds into `Hold` unless the
/// poll already produced a `NoGo` (which still outranks it). Both
/// `render_verdict`'s header and any caller computing an exit code must
/// go through this function rather than calling `aggregate` directly,
/// so the "capture failure is never a GO" rule is structural, not a
/// convention repeated at every call site.
pub fn verdict_for(input: &VerdictInput) -> Verdict {
    let via_poll = aggregate(&input.reports, &input.findings);
    if via_poll == Verdict::NoGo {
        return Verdict::NoGo;
    }
    if !input.capture_failures.is_empty() {
        return Verdict::Hold;
    }
    via_poll
}

/// Renders `input` and writes it to `<run_dir>/verdict.md`.
pub fn write_verdict(input: &VerdictInput, run_dir: &Path) -> std::io::Result<PathBuf> {
    let path = run_dir.join("verdict.md");
    std::fs::write(&path, render_verdict(input))?;
    Ok(path)
}
