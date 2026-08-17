//! Freshness: how the core represents "how current is this?" without
//! knowing that a frontend exists. Every adapter returns an
//! `Observed<T>` — a value stamped with when and how it was seen — and
//! `Freshness` is computed against an injected `now`, never a wall
//! clock, so it is both unit-testable and honest about the moment the
//! caller cares about.

use std::time::{Duration, SystemTime};

/// The spec's default GitHub poll interval.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Where a value came from, which is what determines how it goes stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Fetched on an interval; current only to within that interval.
    Polled {
        /// How often the source is refreshed.
        interval: Duration,
    },
    /// Read from the filesystem on demand; effectively immediate.
    Watched,
}

/// A value together with when and how it was observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed<T> {
    /// The observation itself.
    pub value: T,
    /// When it was last confirmed current.
    pub observed_at: SystemTime,
    /// How it was obtained.
    pub source: SourceKind,
}

impl<T> Observed<T> {
    /// An observation from a source polled on `interval`.
    pub fn polled(value: T, observed_at: SystemTime, interval: Duration) -> Self {
        Self {
            value,
            observed_at,
            source: SourceKind::Polled { interval },
        }
    }

    /// An observation read straight from the filesystem.
    pub fn watched(value: T, observed_at: SystemTime) -> Self {
        Self {
            value,
            observed_at,
            source: SourceKind::Watched,
        }
    }

    /// How long ago this was observed, saturating at zero if `now`
    /// precedes the observation.
    pub fn age(&self, now: SystemTime) -> Duration {
        now.duration_since(self.observed_at)
            .unwrap_or(Duration::ZERO)
    }

    /// How much a caller should trust this observation at `now`.
    pub fn freshness(&self, now: SystemTime) -> Freshness {
        let age = self.age(now);
        match self.source {
            SourceKind::Watched => Freshness::Live,
            SourceKind::Polled { interval } => match age.checked_sub(interval) {
                Some(overdue) if overdue > Duration::ZERO => Freshness::Stale { age, overdue },
                _ => Freshness::Fresh { age },
            },
        }
    }

    /// Rewrites the value, preserving when and how it was observed.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Observed<U> {
        Observed {
            value: f(self.value),
            observed_at: self.observed_at,
            source: self.source,
        }
    }

    /// Records that the source confirmed this value is still current —
    /// the `304 Not Modified` case. The value does not change; its
    /// freshness does.
    pub fn confirm_unchanged(&mut self, at: SystemTime) {
        self.observed_at = at;
    }
}

/// How current an observation is, from the caller's point of view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Filesystem-backed: current as of the read.
    Live,
    /// Polled and within its interval.
    Fresh {
        /// How long ago it was observed.
        age: Duration,
    },
    /// Polled and past its interval.
    Stale {
        /// How long ago it was observed.
        age: Duration,
        /// How far past the interval that is.
        overdue: Duration,
    },
    /// The source could not be read at all.
    Unavailable {
        /// When it was last read successfully, if ever.
        since: Option<SystemTime>,
        /// Why it could not be read, in one sentence.
        reason: String,
    },
}

impl Freshness {
    /// Whether a caller should visibly mark this as not-current.
    pub fn is_stale(&self) -> bool {
        matches!(
            self,
            Freshness::Stale { .. } | Freshness::Unavailable { .. }
        )
    }

    /// How old the observation is, or `None` when there is no
    /// observation to age.
    pub fn age(&self) -> Option<Duration> {
        match self {
            Freshness::Live => Some(Duration::ZERO),
            Freshness::Fresh { age } | Freshness::Stale { age, .. } => Some(*age),
            Freshness::Unavailable { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1_700_000_000);

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + T0 + Duration::from_secs(secs)
    }

    #[test]
    fn the_default_poll_interval_is_thirty_seconds() {
        assert_eq!(DEFAULT_POLL_INTERVAL, Duration::from_secs(30));
    }

    #[test]
    fn a_watched_source_is_always_live_regardless_of_age() {
        let o = Observed::watched(7u32, at(0));
        assert_eq!(o.freshness(at(0)), Freshness::Live);
        assert_eq!(o.freshness(at(600)), Freshness::Live);
    }

    #[test]
    fn a_polled_source_within_its_interval_is_fresh_and_carries_its_age() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert_eq!(
            o.freshness(at(110)),
            Freshness::Fresh {
                age: Duration::from_secs(10)
            }
        );
    }

    #[test]
    fn a_polled_source_exactly_at_its_interval_is_still_fresh() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert_eq!(
            o.freshness(at(130)),
            Freshness::Fresh {
                age: Duration::from_secs(30)
            }
        );
    }

    #[test]
    fn a_polled_source_past_its_interval_is_stale_and_says_by_how_much() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert_eq!(
            o.freshness(at(145)),
            Freshness::Stale {
                age: Duration::from_secs(45),
                overdue: Duration::from_secs(15)
            }
        );
    }

    /// A clock that goes backwards must not panic or report a wild age.
    #[test]
    fn a_now_earlier_than_the_observation_saturates_to_zero_age() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert_eq!(o.age(at(50)), Duration::ZERO);
        assert_eq!(
            o.freshness(at(50)),
            Freshness::Fresh {
                age: Duration::ZERO
            }
        );
    }

    /// The ETag case: a 304 proves the value is current now, even though
    /// the value did not change.
    #[test]
    fn confirm_unchanged_advances_the_observation_without_touching_the_value() {
        let mut o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert!(o.freshness(at(200)).is_stale());
        o.confirm_unchanged(at(200));
        assert_eq!(o.value, 7);
        assert_eq!(
            o.freshness(at(200)),
            Freshness::Fresh {
                age: Duration::ZERO
            }
        );
    }

    #[test]
    fn map_rewrites_the_value_and_keeps_the_observation_metadata() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        let mapped = o.map(|v| v.to_string());
        assert_eq!(mapped.value, "7");
        assert_eq!(mapped.observed_at, at(100));
        assert_eq!(
            mapped.source,
            SourceKind::Polled {
                interval: Duration::from_secs(30)
            }
        );
    }

    #[test]
    fn only_stale_and_unavailable_count_as_stale() {
        assert!(!Freshness::Live.is_stale());
        assert!(!Freshness::Fresh {
            age: Duration::ZERO
        }
        .is_stale());
        assert!(Freshness::Stale {
            age: Duration::from_secs(9),
            overdue: Duration::from_secs(1)
        }
        .is_stale());
        assert!(Freshness::Unavailable {
            since: None,
            reason: "rate limited".into()
        }
        .is_stale());
    }

    #[test]
    fn unavailable_reports_no_age_and_everything_else_reports_one() {
        assert_eq!(Freshness::Live.age(), Some(Duration::ZERO));
        assert_eq!(
            Freshness::Fresh {
                age: Duration::from_secs(3)
            }
            .age(),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            Freshness::Stale {
                age: Duration::from_secs(9),
                overdue: Duration::from_secs(1)
            }
            .age(),
            Some(Duration::from_secs(9))
        );
        assert_eq!(
            Freshness::Unavailable {
                since: None,
                reason: String::new()
            }
            .age(),
            None
        );
    }
}
