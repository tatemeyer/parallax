//! The cockpit's view: what each pane shows, and how a frame is drawn.
//!
//! Split in two on purpose. `model` derives pane contents from a
//! `PlatformState` as pure data, and the pane modules render that data
//! into a `Buffer`. Neither half needs a terminal, because TTUI's
//! `Buffer` is inspectable in-process — which is what makes the
//! interesting cases cheap to test.

pub mod artifacts;
pub mod model;
pub mod sessions;
pub mod status;
pub mod verification;
pub mod work;

#[cfg(test)]
pub(crate) mod test_support {
    //! Hand-built `ProjectState`s, shared by every view test.
    //!
    //! Everything the cockpit shows comes out of aggregation, so the
    //! tests build the same shapes aggregation produces rather than
    //! going through adapters to get them.

    use parallax_baseline::adapters::verification::{VerificationOutcome, VerificationStatus};
    use parallax_baseline::adapters::work::{
        ChecksSummary, WorkItem, WorkKind, WorkSnapshot, WorkState,
    };
    use parallax_baseline::freshness::{Observed, DEFAULT_POLL_INTERVAL};
    use parallax_baseline::state::{Degradation, ProjectState};
    use std::time::SystemTime;

    /// A fixed instant, offset by `secs`. No wall clock anywhere.
    pub fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + secs)
    }

    /// A project that declares and knows nothing.
    pub fn bare_project(name: &str) -> ProjectState {
        ProjectState {
            name: name.to_string(),
            ..Default::default()
        }
    }

    /// A project named `ttui`, mutated by `f`.
    pub fn project_with(f: impl FnOnce(&mut ProjectState)) -> ProjectState {
        let mut p = bare_project("ttui");
        f(&mut p);
        p
    }

    /// An observation from a polled source, at the default interval.
    pub fn polled<T>(value: T, observed_at: SystemTime) -> Observed<T> {
        Observed::polled(value, observed_at, DEFAULT_POLL_INTERVAL)
    }

    /// An observation read from the filesystem.
    pub fn watched<T>(value: T, observed_at: SystemTime) -> Observed<T> {
        Observed::watched(value, observed_at)
    }

    /// A verification standing.
    pub fn check(kind: &str, outcome: VerificationOutcome) -> VerificationStatus {
        VerificationStatus {
            kind: kind.to_string(),
            outcome,
            detail: None,
        }
    }

    /// A source that could not be read.
    pub fn degradation(source: &str, reason: &str) -> Degradation {
        Degradation {
            source: source.to_string(),
            reason: reason.to_string(),
        }
    }

    /// A work snapshot over the given items.
    pub fn work_snapshot(items: &[WorkItem]) -> WorkSnapshot {
        WorkSnapshot {
            items: items.to_vec(),
        }
    }

    /// One open issue carrying `labels`.
    pub fn issue(number: u64, title: &str, labels: &[&str]) -> WorkItem {
        WorkItem {
            number,
            title: title.to_string(),
            kind: WorkKind::Issue,
            state: WorkState::Open,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            checks: ChecksSummary::none(),
            url: String::new(),
            updated_at: "2026-08-19T00:00:00Z".into(),
        }
    }

    /// One open pull request with the given check counts.
    pub fn pull(number: u64, title: &str, checks: ChecksSummary) -> WorkItem {
        WorkItem {
            number,
            title: title.to_string(),
            kind: WorkKind::PullRequest,
            state: WorkState::Open,
            labels: Vec::new(),
            checks,
            url: String::new(),
            updated_at: "2026-08-19T00:00:00Z".into(),
        }
    }
}
