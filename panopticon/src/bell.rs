//! The Cloister Bell.
//!
//! It rings when a project **enters** a blocker state — a check turning
//! `Fail` or `Hold`, a Plumb NO-GO landing, a source becoming
//! unavailable — not continuously while one holds. A bell that rings
//! every tick is a bell nobody hears.
//!
//! Two rules it will not break: it never opens a modal, and it never
//! swallows a keystroke. The operator has to be able to keep working
//! while something is on fire, because something usually is.

use crate::view::model::{health, Health};
use parallax_baseline::state::PlatformState;
use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

/// How long a ring stays visible before it decays.
pub const DEFAULT_RING: Duration = Duration::from_secs(4);

/// Watches for projects entering a blocker state.
#[derive(Debug, Clone)]
pub struct Bell {
    /// Projects that were broken as of the last observation.
    broken: BTreeSet<String>,
    /// Whether anything has been observed yet.
    started: bool,
    /// When it last rang.
    rang_at: Option<SystemTime>,
    /// How long a ring lasts.
    ring: Duration,
}

impl Default for Bell {
    fn default() -> Self {
        Self::new(DEFAULT_RING)
    }
}

impl Bell {
    /// A bell whose rings last `ring`.
    pub fn new(ring: Duration) -> Self {
        Self {
            broken: BTreeSet::new(),
            started: false,
            rang_at: None,
            ring,
        }
    }

    /// Records what the platform looks like now, and says whether that
    /// is news.
    ///
    /// The **first** observation never rings, however bad it is: the
    /// cockpit has just started and is reporting the world as found, not
    /// something that happened. Ringing there would train the operator
    /// to ignore it, which is the only failure mode an alert really has.
    pub fn observe(&mut self, state: &PlatformState, now: SystemTime) -> bool {
        let current: BTreeSet<String> = state
            .projects
            .iter()
            .filter(|p| health(p, now) == Health::Broken)
            .map(|p| p.name.clone())
            .collect();

        if !self.started {
            self.started = true;
            self.broken = current;
            return false;
        }

        let newly_broken = current.difference(&self.broken).count() > 0;
        self.broken = current;
        if newly_broken {
            self.rang_at = Some(now);
        }
        newly_broken
    }

    /// Whether the ring is still showing at `now`.
    pub fn ringing(&self, now: SystemTime) -> bool {
        self.rang_at
            .map(|at| now.duration_since(at).unwrap_or(Duration::ZERO) < self.ring)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_baseline::adapters::verification::{VerificationOutcome, VerificationStatus};
    use parallax_baseline::freshness::Observed;
    use parallax_baseline::state::ProjectState;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    /// One project, healthy or not.
    fn platform(name: &str, outcome: VerificationOutcome) -> PlatformState {
        let mut state = PlatformState::default();
        let mut project = ProjectState {
            name: name.to_string(),
            ..Default::default()
        };
        project.verification.push(Observed::watched(
            VerificationStatus {
                kind: "tests".into(),
                outcome,
                detail: None,
            },
            at(0),
        ));
        state.projects.push(project);
        state
    }

    /// The cockpit starting up in front of a fire is not news.
    #[test]
    fn the_first_observation_never_rings_however_bad_it_is() {
        let mut bell = Bell::default();
        assert!(!bell.observe(&platform("ttui", VerificationOutcome::Fail), at(0)));
        assert!(!bell.ringing(at(0)));
    }

    #[test]
    fn entering_a_blocker_state_rings() {
        let mut bell = Bell::default();
        bell.observe(&platform("ttui", VerificationOutcome::Pass), at(0));
        assert!(bell.observe(&platform("ttui", VerificationOutcome::Fail), at(1)));
        assert!(bell.ringing(at(1)));
    }

    /// A bell that rings every tick is a bell nobody hears.
    #[test]
    fn holding_a_blocker_state_rings_once() {
        let mut bell = Bell::default();
        bell.observe(&platform("ttui", VerificationOutcome::Pass), at(0));
        assert!(bell.observe(&platform("ttui", VerificationOutcome::Fail), at(1)));
        for tick in 2..12 {
            assert!(
                !bell.observe(&platform("ttui", VerificationOutcome::Fail), at(tick)),
                "still broken at {tick}, and still not news"
            );
        }
    }

    #[test]
    fn recovering_and_breaking_again_rings_again() {
        let mut bell = Bell::default();
        bell.observe(&platform("ttui", VerificationOutcome::Pass), at(0));
        assert!(bell.observe(&platform("ttui", VerificationOutcome::Fail), at(1)));
        assert!(!bell.observe(&platform("ttui", VerificationOutcome::Pass), at(2)));
        assert!(bell.observe(&platform("ttui", VerificationOutcome::Fail), at(3)));
    }

    /// A second project breaking is its own news, even while the first
    /// one is still broken.
    #[test]
    fn a_second_project_breaking_rings_even_while_the_first_still_is() {
        let mut bell = Bell::default();
        let mut both = platform("ttui", VerificationOutcome::Pass);
        both.projects.push(ProjectState {
            name: "sesh".into(),
            ..Default::default()
        });
        bell.observe(&both, at(0));

        let mut ttui_broken = platform("ttui", VerificationOutcome::Fail);
        ttui_broken.projects.push(ProjectState {
            name: "sesh".into(),
            ..Default::default()
        });
        assert!(bell.observe(&ttui_broken, at(1)));

        let mut sesh_broken_too = platform("ttui", VerificationOutcome::Fail);
        let mut sesh = ProjectState {
            name: "sesh".into(),
            ..Default::default()
        };
        sesh.degradations
            .push(parallax_baseline::state::Degradation {
                source: "work:github".into(),
                reason: "http 403: rate limited".into(),
            });
        sesh_broken_too.projects.push(sesh);
        assert!(bell.observe(&sesh_broken_too, at(2)), "sesh is new news");
    }

    #[test]
    fn a_ring_decays() {
        let mut bell = Bell::new(Duration::from_secs(4));
        bell.observe(&platform("ttui", VerificationOutcome::Pass), at(0));
        bell.observe(&platform("ttui", VerificationOutcome::Fail), at(10));
        assert!(bell.ringing(at(13)));
        assert!(!bell.ringing(at(14)), "four seconds, then quiet");
    }

    /// A degraded source is a blocker too — the cockpit going blind to a
    /// project is as much news as a check failing.
    #[test]
    fn a_source_becoming_unavailable_rings() {
        let mut bell = Bell::default();
        bell.observe(&platform("ttui", VerificationOutcome::Pass), at(0));

        let mut degraded = platform("ttui", VerificationOutcome::Pass);
        degraded.projects[0]
            .degradations
            .push(parallax_baseline::state::Degradation {
                source: "work:github".into(),
                reason: "http 403: rate limited".into(),
            });
        assert!(bell.observe(&degraded, at(1)));
    }
}
