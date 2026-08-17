//! The work family: issues, pull requests, their labels, and their
//! check status. One built-in implementation (`github`, Task 12).

use super::{AdapterError, ProjectContext};
use crate::freshness::Observed;
use std::time::SystemTime;

/// Whether a work item is an issue or a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkKind {
    /// An issue.
    Issue,
    /// A pull request.
    PullRequest,
}

/// Where a work item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkState {
    /// Open and ready.
    Open,
    /// Open but marked draft.
    Draft,
    /// Closed without merging.
    Closed,
    /// Merged.
    Merged,
}

/// How a work item's checks stand. Deliberately a count, not a verdict —
/// what "green enough" means is a policy question, and the manifest's
/// autonomy axes are where policy lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChecksSummary {
    /// Checks that succeeded.
    pub passed: usize,
    /// Checks that failed.
    pub failed: usize,
    /// Checks still running or queued.
    pub pending: usize,
}

impl ChecksSummary {
    /// A summary for an item with no checks reported.
    pub fn none() -> Self {
        Self::default()
    }

    /// How many checks were reported in total.
    pub fn total(&self) -> usize {
        self.passed + self.failed + self.pending
    }

    /// Whether every reported check passed and at least one ran.
    pub fn is_green(&self) -> bool {
        self.passed > 0 && self.failed == 0 && self.pending == 0
    }
}

/// One issue or pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// The item's number in its repository.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Issue or pull request.
    pub kind: WorkKind,
    /// Where it stands.
    pub state: WorkState,
    /// Its labels, verbatim — projection happens in `autonomy`.
    pub labels: Vec<String>,
    /// Its check status.
    pub checks: ChecksSummary,
    /// A link a frontend can open.
    pub url: String,
    /// The source's own last-updated string, carried opaquely for
    /// display. Freshness of the *observation* lives in `Observed`.
    pub updated_at: String,
}

/// Every work item one poll returned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkSnapshot {
    /// The items, in the order the source returned them.
    pub items: Vec<WorkItem>,
}

/// A source of work items.
pub trait WorkAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Fetches the current work items as of `now`.
    fn poll(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<WorkSnapshot>, AdapterError>;
}
