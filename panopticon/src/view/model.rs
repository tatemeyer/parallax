//! What each pane shows, derived from a `PlatformState`.
//!
//! Pure functions, no terminal, no clock of their own — every `now` is
//! injected. This is the half of the cockpit where the interesting
//! cases live: a degraded source, a project that declares only `work:`,
//! a work item whose labels the manifest never mentions.

use parallax_baseline::adapters::verification::VerificationOutcome;
use parallax_baseline::freshness::Freshness;
use parallax_baseline::state::{PlatformState, ProjectState};
use parallax_baseline::validate::{Family, Validated};
use std::time::SystemTime;

/// How worried a reader should be about a project, at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Everything declared is fresh and passing.
    Ok,
    /// Something is stale, or a check has not finished.
    Pending,
    /// A check failed or held, or a source could not be read.
    Broken,
}

/// One row of the project rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailRow {
    /// The project's short name.
    pub name: String,
    /// Its primary language, for display.
    pub language: Option<String>,
    /// The worst thing the project currently knows about itself.
    pub health: Health,
}

/// Which adapter families a project's manifest declares.
///
/// `ProjectState` records what arrived, not what was asked for, so an
/// undeclared family and an empty one are indistinguishable without
/// this. They are different statements and only one of them is honest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Declared {
    /// Whether the manifest declares a work feed.
    pub work: bool,
    /// Whether it declares any verification checks.
    pub verification: bool,
    /// Whether it declares any artifact feeds.
    pub artifacts: bool,
    /// Whether it declares a session feed.
    pub sessions: bool,
}

impl Declared {
    /// Reads what a validated manifest declares.
    pub fn of(validated: &Validated) -> Self {
        Self {
            work: validated.declares(Family::Work),
            verification: validated.declares(Family::Verification),
            artifacts: validated.declares(Family::Artifact),
            sessions: validated.declares(Family::Session),
        }
    }
}

/// The worst of what a project knows about itself at `now`.
///
/// Two inputs: how current its sources are, and what its checks
/// concluded. A project that declares nothing is `Ok` — it has no bad
/// news, and inventing some would be a lie.
pub fn health(project: &ProjectState, now: SystemTime) -> Health {
    let sources_say = project
        .sources(now)
        .into_iter()
        .map(|s| match s.freshness {
            Freshness::Unavailable { .. } => Health::Broken,
            Freshness::Stale { .. } => Health::Pending,
            _ => Health::Ok,
        })
        .max_by_key(rank)
        .unwrap_or(Health::Ok);

    let checks_say = project
        .verification
        .iter()
        .map(|v| match v.value.outcome {
            // A hold is never upgraded, and the cockpit is not where
            // that gets softened.
            VerificationOutcome::Fail | VerificationOutcome::Hold => Health::Broken,
            VerificationOutcome::NotRun => Health::Pending,
            VerificationOutcome::Pass => Health::Ok,
        })
        .max_by_key(rank)
        .unwrap_or(Health::Ok);

    [sources_say, checks_say]
        .into_iter()
        .max_by_key(rank)
        .unwrap_or(Health::Ok)
}

fn rank(h: &Health) -> u8 {
    match h {
        Health::Ok => 0,
        Health::Pending => 1,
        Health::Broken => 2,
    }
}

/// One rail row per registered project, in registration order.
pub fn rail_rows(state: &PlatformState, now: SystemTime) -> Vec<RailRow> {
    state
        .projects
        .iter()
        .map(|p| RailRow {
            name: p.name.clone(),
            language: p.language.clone(),
            health: health(p, now),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::test_support::*;

    #[test]
    fn a_project_with_nothing_declared_is_ok_rather_than_unknown() {
        let p = bare_project("empty");
        assert_eq!(health(&p, at(0)), Health::Ok);
    }

    #[test]
    fn everything_fresh_and_passing_is_ok() {
        let p = project_with(|p| {
            p.work = Some(polled(work_snapshot(&[]), at(0)));
            p.verification
                .push(watched(check("tests", VerificationOutcome::Pass), at(0)));
        });
        assert_eq!(health(&p, at(5)), Health::Ok);
    }

    #[test]
    fn a_stale_polled_source_is_pending() {
        let p = project_with(|p| p.work = Some(polled(work_snapshot(&[]), at(0))));
        assert_eq!(
            health(&p, at(45)),
            Health::Pending,
            "45s past a 30s interval"
        );
    }

    #[test]
    fn a_failing_check_is_broken() {
        let p = project_with(|p| {
            p.verification
                .push(watched(check("tests", VerificationOutcome::Fail), at(0)));
        });
        assert_eq!(health(&p, at(0)), Health::Broken);
    }

    /// A hold is never upgraded — not by the merger, and not here.
    #[test]
    fn a_held_check_is_broken_rather_than_pending() {
        let p = project_with(|p| {
            p.verification.push(watched(
                check("perceptual", VerificationOutcome::Hold),
                at(0),
            ));
        });
        assert_eq!(health(&p, at(0)), Health::Broken);
    }

    #[test]
    fn a_check_that_has_not_run_is_pending() {
        let p = project_with(|p| {
            p.verification.push(watched(
                check("perceptual", VerificationOutcome::NotRun),
                at(0),
            ));
        });
        assert_eq!(health(&p, at(0)), Health::Pending);
    }

    #[test]
    fn a_degraded_source_is_broken_even_when_every_check_passes() {
        let p = project_with(|p| {
            p.verification
                .push(watched(check("tests", VerificationOutcome::Pass), at(0)));
            p.degradations
                .push(degradation("work:github", "http 403: rate limit exceeded"));
        });
        assert_eq!(health(&p, at(0)), Health::Broken);
    }

    #[test]
    fn an_empty_platform_has_no_rail_rows() {
        assert!(rail_rows(&PlatformState::default(), at(0)).is_empty());
    }

    #[test]
    fn rail_rows_follow_registration_order() {
        let mut state = PlatformState::default();
        state.projects.push(bare_project("zebra"));
        state.projects.push(bare_project("aardvark"));
        let names: Vec<String> = rail_rows(&state, at(0))
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, vec!["zebra", "aardvark"], "not sorted");
    }
}
