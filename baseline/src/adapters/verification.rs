//! The verification family: whatever decides a unit of work is done.
//! Two built-in implementations — `command` (Task 13) and `plumb`
//! (Task 14). **Neither links Plumb**: the `plumb` adapter reads the
//! `verdict.md` Plumb writes, as text.

use super::{AdapterError, ProjectContext};
use crate::freshness::Observed;
use std::time::SystemTime;

/// What a verification check concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationOutcome {
    /// The check succeeded.
    Pass,
    /// The check failed.
    Fail,
    /// The check could not reach a conclusion. Never upgraded to a pass.
    Hold,
    /// The check has not run yet.
    NotRun,
}

/// One verification check's current standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationStatus {
    /// The manifest's display label for this check, e.g. `lint`.
    pub kind: String,
    /// What it concluded.
    pub outcome: VerificationOutcome,
    /// A one-line explanation, when the adapter has one.
    pub detail: Option<String>,
}

/// A source of verification outcomes.
pub trait VerificationAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Reads this check's current standing as of `now`.
    fn check(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<VerificationStatus>, AdapterError>;
}
