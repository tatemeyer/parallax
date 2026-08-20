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
    /// What to call this project on screen: its short name when it is on
    /// this machine, and `name@peer` when it is not.
    ///
    /// Qualified rather than bare because this desktop holds a clone of
    /// `sesh` and the Pi serves one too. Two rows both reading `sesh`
    /// would be worse than not showing the Pi at all — the operator
    /// could act on the wrong one and never know.
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

    /// What a peer's state demonstrates, for a row whose manifest this
    /// machine has never seen.
    ///
    /// A local row reads its manifest and can tell an undeclared family
    /// from a declared-but-empty one. A peer's manifest is on the peer,
    /// so there is only what arrived. Looking the name up in this
    /// machine's manifests instead would describe the *local* clone —
    /// the Pi's `sesh` pane shaped by this desktop's `sesh`, which is
    /// the same wrong-machine mistake as running its checks here.
    ///
    /// It under-claims rather than over-claims: a feed the peer declares
    /// but has nothing in yet reads as undeclared. That errs toward
    /// showing less, rather than toward inventing a source.
    pub fn observed(project: &ProjectState) -> Self {
        Self {
            work: project.work.is_some(),
            verification: !project.verification.is_empty(),
            artifacts: !project.artifacts.is_empty(),
            sessions: project.sessions.is_some(),
        }
    }
}

/// The worst of what a project knows about itself at `now`.
///
/// Two inputs: how current its sources are, and what its checks
/// concluded. A project that declares nothing is `Ok` — it has no bad
/// news, and inventing some would be a lie.
pub fn health(project: &ProjectState, now: SystemTime) -> Health {
    let sources = project.sources(now);

    // A peer's row with no sources at all means something different from
    // a local project's. A local project that declares nothing **was
    // read**, and found to declare nothing; there is genuinely no bad
    // news. A remote row with nothing on it has not been heard from —
    // it exists because a registry named the machine, not because
    // anything answered. Reporting `Ok` would claim a check that never
    // happened, which is the same species of lie as a fetched value
    // calling itself `Live`.
    if project.peer.is_some() && sources.is_empty() {
        return Health::Pending;
    }

    let sources_say = sources
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
            name: p.qualified_name(),
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

    /// The same emptiness means the opposite thing on a peer's row. A
    /// local project with no sources was read and found to declare
    /// none; a machine that has not answered yet has told us nothing at
    /// all, and `ok` there is a check that never happened.
    #[test]
    fn a_peer_that_has_not_answered_yet_is_pending_rather_than_ok() {
        let mut state = PlatformState::default();
        state.extend_from_peer("pi5", vec![bare_project("pi5")]);
        assert_eq!(health(&state.projects[0], at(0)), Health::Pending);
    }

    /// And once it has answered, its rows are judged on what it said —
    /// the rule above must not make every remote row permanently amber.
    #[test]
    fn a_peer_that_answered_is_judged_on_what_it_sent() {
        let mut state = PlatformState::default();
        state.extend_from_peer(
            "pi5",
            vec![project_with(|p| {
                p.verification = vec![polled(check("tests", VerificationOutcome::Pass), at(0))];
            })],
        );
        assert_eq!(health(&state.projects[0], at(0)), Health::Ok);
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

    /// The desktop holds a clone of `sesh` and the Pi serves one too, so
    /// this is the ordinary case. Two rows both reading `sesh` would be
    /// worse than not showing the Pi at all: the operator could press a
    /// key at the wrong machine's project and never find out.
    #[test]
    fn a_peers_project_is_named_for_its_machine_and_a_local_one_is_not() {
        let mut state = PlatformState::default();
        state.projects.push(bare_project("sesh"));
        state.extend_from_peer("pi5", vec![bare_project("sesh")]);

        let names: Vec<String> = rail_rows(&state, at(0))
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, vec!["sesh", "sesh@pi5"]);
    }

    /// The model is width-agnostic on purpose — it says what a project
    /// is called and the renderer decides what fits. That matters here:
    /// `ttui@tates-laptop` is 17 columns and the rail is 18 wide, so the
    /// renderer has to elide it rather than clip it silently.
    #[test]
    fn a_qualified_name_is_returned_whole_and_left_for_the_renderer_to_fit() {
        let mut state = PlatformState::default();
        state.extend_from_peer("tates-laptop", vec![bare_project("ttui")]);
        let row = &rail_rows(&state, at(0))[0];
        assert_eq!(row.name, "ttui@tates-laptop");
    }
}
