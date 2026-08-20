//! The serialized contract between a probe and a client.
//!
//! A probe scans its own disk with the adapters this crate already
//! provides and serves the aggregated result; a client parses it back
//! into [`ProjectState`] and merges it with its own. This module is the
//! format in between, and the one place that knows an observation made
//! on another machine is not the same claim as one made here.
//!
//! **Unknown fields are ignored, not rejected.** That is the opposite
//! of the rule [`crate::manifest`] and [`crate::registry`] follow, and
//! the difference is who writes the document. A human types a manifest,
//! so a typo'd key must be an error or it silently does nothing. A
//! program emits an envelope, and the two programs are on different
//! machines that upgrade at different times — a Pi that builds on
//! itself lags a desktop that does not. A client that rejected a field
//! a newer probe added would break on exactly the upgrade it was
//! supposed to tolerate. Breaking changes are what [`WIRE_API_VERSION`]
//! is for.

use crate::adapters::artifact::Artifact;
use crate::adapters::session::Session;
use crate::adapters::verification::VerificationStatus;
use crate::adapters::work::WorkSnapshot;
use crate::freshness::{Observed, SourceKind};
use crate::state::{Degradation, ItemAutonomy, ProjectState};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

/// The wire format's version. Bumped only for a change a client of the
/// previous version could not read.
pub const WIRE_API_VERSION: &str = "parallax/v1";

/// An envelope that could not be understood, and why.
///
/// Deliberately the same shape as [`crate::registry::RegistryError`]: a
/// source and one sentence. A peer that answers with nonsense degrades
/// exactly like a project that fails to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError {
    /// Which peer sent it.
    pub source: String,
    /// What was wrong with it, in one sentence.
    pub problem: String,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.source, self.problem)
    }
}
impl std::error::Error for WireError {}

/// How a value was obtained, as it travels.
///
/// Mirrors [`SourceKind`] rather than serializing it, because the two
/// do not mean the same thing on both ends: see [`ObservedWire::receive`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SourceKindWire {
    /// Fetched on an interval, in seconds.
    Polled {
        /// How often the source is refreshed, in seconds.
        interval_secs: u64,
    },
    /// Read from the probe's own filesystem on demand.
    Watched,
}

/// A value together with when and how the **probe** observed it.
///
/// This type deliberately has no `freshness` method. [`Observed`] has
/// one, and it answers `Live` for anything `Watched` — which is correct
/// for a local read and false for a value that arrived over a network.
/// Leaving the method off means a caller cannot ask this type how fresh
/// it is; it has to call [`receive`](Self::receive) and say what time it
/// is here, which is the only way to get an honest answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedWire<T> {
    /// The observation itself.
    pub value: T,
    /// When the probe last confirmed it current, on the probe's clock.
    pub observed_at: SystemTime,
    /// How the probe obtained it.
    pub source: SourceKindWire,
}

impl<T> ObservedWire<T> {
    /// Prepares a local observation for transmission.
    pub fn send(observed: Observed<T>) -> Self {
        Self {
            value: observed.value,
            observed_at: observed.observed_at,
            source: match observed.source {
                SourceKind::Watched => SourceKindWire::Watched,
                SourceKind::Polled { interval } => SourceKindWire::Polled {
                    interval_secs: interval.as_secs(),
                },
            },
        }
    }

    /// Turns a received observation into a local one, honestly.
    ///
    /// Two things happen here, and both are the point of the module.
    ///
    /// **The source kind is re-stamped.** A `Watched` observation
    /// becomes `Polled` at the peer's interval, because that is what it
    /// now is: this machine did not read a file, it fetched an answer
    /// that will not change until the next fetch. `Live` means "I read
    /// this myself," and nothing received over a network may claim it.
    ///
    /// **The clock is re-based, never compared.** `observed_at` and
    /// `probe_now` both come from the probe's clock, so their difference
    /// is a true age even when that clock is wrong — and a Pi 5 has no
    /// RTC, so it usually is, until NTP lands. That age is then measured
    /// back from `client_now`. The two machines' wall clocks are never
    /// subtracted from one another.
    pub fn receive(
        self,
        probe_now: SystemTime,
        client_now: SystemTime,
        peer_interval: Duration,
    ) -> Observed<T> {
        // Saturating, like `Observed::age`: a probe whose clock moved
        // backwards mid-scan reports an observation "after" its own
        // `now`, and zero is the only defensible age for that.
        let age = probe_now
            .duration_since(self.observed_at)
            .unwrap_or(Duration::ZERO);
        Observed {
            value: self.value,
            observed_at: client_now
                .checked_sub(age)
                .unwrap_or(SystemTime::UNIX_EPOCH),
            source: match self.source {
                SourceKindWire::Watched => SourceKind::Polled {
                    interval: peer_interval,
                },
                SourceKindWire::Polled { interval_secs } => SourceKind::Polled {
                    interval: Duration::from_secs(interval_secs),
                },
            },
        }
    }
}

/// One project's state, as it travels.
///
/// Every collection defaults to empty and every option to `None`, so a
/// project declaring only `work:` crosses as a valid reduced view —
/// partial support is normal here for the same reason it is normal in a
/// manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWire {
    /// The project's short name, from its own manifest.
    pub name: String,
    /// Its declared methodology. Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub methodology: Option<String>,
    /// Its primary language, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The most recent work snapshot, when the feed was reachable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<ObservedWire<WorkSnapshot>>,
    /// Each work item's projected autonomy, in snapshot order.
    #[serde(default)]
    pub autonomy: Vec<ItemAutonomy>,
    /// Labels seen on work items the manifest does not declare.
    #[serde(default)]
    pub unmapped_labels: Vec<String>,
    /// Each declared verification check's standing.
    #[serde(default)]
    pub verification: Vec<ObservedWire<VerificationStatus>>,
    /// Each declared artifact feed's contents.
    #[serde(default)]
    pub artifacts: Vec<ObservedWire<Vec<Artifact>>>,
    /// The session feed's contents, when declared and reachable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<ObservedWire<Vec<Session>>>,
    /// Sources that failed on the probe this cycle.
    #[serde(default)]
    pub degradations: Vec<Degradation>,
}

impl ProjectWire {
    /// Prepares one project's state for transmission.
    pub fn send(state: ProjectState) -> Self {
        Self {
            name: state.name,
            methodology: state.methodology,
            language: state.language,
            work: state.work.map(ObservedWire::send),
            autonomy: state.autonomy,
            unmapped_labels: state.unmapped_labels,
            verification: state
                .verification
                .into_iter()
                .map(ObservedWire::send)
                .collect(),
            artifacts: state
                .artifacts
                .into_iter()
                .map(ObservedWire::send)
                .collect(),
            sessions: state.sessions.map(ObservedWire::send),
            degradations: state.degradations,
        }
    }

    /// Turns a received project into local state, re-stamping and
    /// re-basing every observation it carries.
    pub fn receive(
        self,
        probe_now: SystemTime,
        client_now: SystemTime,
        peer_interval: Duration,
    ) -> ProjectState {
        // Spelled out at each site rather than bound to one closure: a
        // closure is not generic, so the first family it is used with
        // fixes its type and the other three stop compiling.
        ProjectState {
            name: self.name,
            // Not a per-project field on the wire: the envelope names
            // the peer once, for all of them. `PeerClient::fetch` stamps
            // it onto each after parsing, so a probe cannot claim one
            // name for itself and a different one for a project it
            // serves.
            peer: None,
            methodology: self.methodology,
            language: self.language,
            work: self
                .work
                .map(|o| o.receive(probe_now, client_now, peer_interval)),
            autonomy: self.autonomy,
            unmapped_labels: self.unmapped_labels,
            verification: self
                .verification
                .into_iter()
                .map(|o| o.receive(probe_now, client_now, peer_interval))
                .collect(),
            artifacts: self
                .artifacts
                .into_iter()
                .map(|o| o.receive(probe_now, client_now, peer_interval))
                .collect(),
            sessions: self
                .sessions
                .map(|o| o.receive(probe_now, client_now, peer_interval)),
            degradations: self.degradations,
        }
    }
}

/// What a probe serves: every project it knows about, and the moment it
/// says that was true.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateEnvelope {
    /// The format version. See [`WIRE_API_VERSION`].
    pub api_version: String,
    /// The probe's name for itself, usually its hostname.
    pub peer: String,
    /// The probe's clock at serialization. Every `observed_at` in this
    /// envelope is on the same clock, which is what makes them usable.
    pub now: SystemTime,
    /// One entry per project the probe has registered.
    #[serde(default)]
    pub projects: Vec<ProjectWire>,
}

impl StateEnvelope {
    /// Builds an envelope from local state, stamped with this machine's
    /// clock.
    pub fn send(peer: impl Into<String>, now: SystemTime, projects: Vec<ProjectState>) -> Self {
        Self {
            api_version: WIRE_API_VERSION.to_string(),
            peer: peer.into(),
            now,
            projects: projects.into_iter().map(ProjectWire::send).collect(),
        }
    }

    /// Turns a received envelope into local state.
    ///
    /// Rejects a version this build does not speak before reading any
    /// project, because a field that moved is worse read optimistically
    /// than not read at all.
    pub fn receive(
        self,
        client_now: SystemTime,
        peer_interval: Duration,
    ) -> Result<Vec<ProjectState>, WireError> {
        if self.api_version != WIRE_API_VERSION {
            return Err(WireError {
                source: self.peer,
                problem: format!(
                    "speaks `{}`, this build speaks `{WIRE_API_VERSION}`",
                    self.api_version
                ),
            });
        }
        let probe_now = self.now;
        Ok(self
            .projects
            .into_iter()
            .map(|p| p.receive(probe_now, client_now, peer_interval))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::verification::VerificationOutcome;
    use crate::freshness::Freshness;

    const PEER_INTERVAL: Duration = Duration::from_secs(30);

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    fn status() -> VerificationStatus {
        VerificationStatus {
            kind: "tests".into(),
            outcome: VerificationOutcome::Pass,
            detail: None,
        }
    }

    /// A project declaring only `work:` is the spec's headline partial
    /// case, and it has to survive the trip.
    #[test]
    fn a_reduced_project_round_trips_as_a_reduced_project() {
        let state = ProjectState {
            name: "sesh".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&ProjectWire::send(state)).unwrap();
        let back: ProjectWire = serde_json::from_str(&json).unwrap();
        let state = back.receive(at(0), at(0), PEER_INTERVAL);

        assert_eq!(state.name, "sesh");
        assert!(state.work.is_none());
        assert!(state.verification.is_empty());
        assert!(state.sessions.is_none());
        assert!(state.degradations.is_empty(), "absent is not degraded");
    }

    #[test]
    fn an_envelope_round_trips_through_json() {
        let state = ProjectState {
            name: "ttui".into(),
            language: Some("rust".into()),
            verification: vec![Observed::watched(status(), at(0))],
            ..Default::default()
        };
        let envelope = StateEnvelope::send("tates-laptop", at(10), vec![state]);
        let json = serde_json::to_string(&envelope).unwrap();
        let back: StateEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(back, envelope);
        assert_eq!(back.peer, "tates-laptop");
        assert_eq!(back.api_version, WIRE_API_VERSION);
    }

    /// The central claim of the design. `Observed::freshness` maps
    /// `Watched` to `Live` unconditionally, which is right for a local
    /// read and a lie for one that crossed a wire.
    #[test]
    fn a_remote_observation_is_never_live() {
        let observed = Observed::watched(status(), at(0));
        let wire = ObservedWire::send(observed);

        // Every `now` a client could plausibly hold, including the
        // instant the probe claims it read the file.
        for client_now in [at(0), at(1), at(29), at(30), at(3600)] {
            let received = wire.clone().receive(at(0), client_now, PEER_INTERVAL);
            assert_ne!(
                received.freshness(client_now),
                Freshness::Live,
                "a value fetched from another machine reported Live at {client_now:?}"
            );
        }
    }

    #[test]
    fn a_re_stamped_observation_goes_stale_once_the_peer_interval_lapses() {
        let wire = ObservedWire::send(Observed::watched(status(), at(0)));
        // Observed 60s before the probe serialized: already past a 30s
        // interval by the time it arrives.
        let received = wire.receive(at(60), at(1000), PEER_INTERVAL);
        assert!(received.freshness(at(1000)).is_stale());
    }

    /// A source that was already honest about being periodic keeps its
    /// own interval; the peer's is only for values that had none.
    #[test]
    fn a_polled_observation_keeps_the_interval_it_arrived_with() {
        let wire = ObservedWire::send(Observed::polled(status(), at(0), Duration::from_secs(900)));
        let received = wire.receive(at(0), at(0), PEER_INTERVAL);
        assert_eq!(
            received.source,
            SourceKind::Polled {
                interval: Duration::from_secs(900)
            }
        );
    }

    /// The probe's clock is an hour ahead. The age must still be the 30
    /// seconds that actually elapsed on it.
    #[test]
    fn a_probe_clock_running_ahead_still_yields_the_true_age() {
        let hour = Duration::from_secs(3600);
        let observed = Observed::watched(status(), at(0) + hour);
        let wire = ObservedWire::send(observed);
        let received = wire.receive(at(30) + hour, at(30), PEER_INTERVAL);

        assert_eq!(received.age(at(30)), Duration::from_secs(30));
    }

    /// And an hour behind, which is the Pi before NTP lands.
    #[test]
    fn a_probe_clock_running_behind_still_yields_the_true_age() {
        let hour = Duration::from_secs(3600);
        let observed = Observed::watched(status(), at(0) - hour);
        let wire = ObservedWire::send(observed);
        let received = wire.receive(at(30) - hour, at(30), PEER_INTERVAL);

        assert_eq!(received.age(at(30)), Duration::from_secs(30));
    }

    /// A clock that jumped backwards mid-scan: the observation is after
    /// the probe's own `now`. Zero, not a wrapped duration.
    #[test]
    fn an_observation_from_the_probes_future_ages_to_zero() {
        let wire = ObservedWire::send(Observed::watched(status(), at(100)));
        let received = wire.receive(at(0), at(500), PEER_INTERVAL);
        assert_eq!(received.age(at(500)), Duration::ZERO);
    }

    /// The rule this module exists to hold, in the direction that
    /// matters: a newer probe adding a field must not break an older
    /// client. This is the opposite of the manifest's rule, on purpose.
    #[test]
    fn a_field_this_build_does_not_know_is_ignored_rather_than_rejected() {
        let json = r#"{
            "apiVersion": "parallax/v1",
            "peer": "pi5",
            "now": { "secs_since_epoch": 1700000000, "nanos_since_epoch": 0 },
            "projects": [],
            "roomTemperature": 21
        }"#;
        let envelope: StateEnvelope = serde_json::from_str(json).expect("unknown key rejected");
        assert_eq!(envelope.peer, "pi5");
    }

    #[test]
    fn an_envelope_from_a_version_this_build_cannot_read_is_refused_by_name() {
        let envelope = StateEnvelope {
            api_version: "parallax/v2".into(),
            peer: "pi5".into(),
            now: at(0),
            projects: vec![],
        };
        let err = envelope.receive(at(0), PEER_INTERVAL).unwrap_err();
        assert_eq!(err.source, "pi5");
        assert!(err.problem.contains("parallax/v2"), "got {}", err.problem);
        assert!(err.problem.contains(WIRE_API_VERSION));
    }

    #[test]
    fn receiving_an_envelope_re_stamps_every_project_it_carries() {
        let state = ProjectState {
            name: "sesh".into(),
            verification: vec![Observed::watched(status(), at(0))],
            ..Default::default()
        };
        let envelope = StateEnvelope::send("pi5", at(0), vec![state]);
        let projects = envelope.receive(at(500), PEER_INTERVAL).unwrap();

        assert_eq!(projects.len(), 1);
        assert_ne!(
            projects[0].verification[0].freshness(at(500)),
            Freshness::Live
        );
    }
}
