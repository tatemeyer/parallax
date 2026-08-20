//! Fetching another machine's state.
//!
//! A peer is one more HTTP source, which is why this reuses the seam the
//! GitHub adapter already goes through: `FixtureTransport` records a
//! probe's answer exactly as it records GitHub's, so a cockpit rendering
//! three machines is testable with no network and no second machine.
//!
//! The important behaviour here is what happens when a peer does *not*
//! answer. Its projects must not vanish — a row that disappeared reads
//! as a project that was never registered, when what actually happened
//! is that a laptop went to sleep.

use crate::adapters::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::adapters::AdapterError;
use crate::freshness::DEFAULT_POLL_INTERVAL;
use crate::state::{Degradation, ProjectState};
use crate::wire::StateEnvelope;
use std::time::{Duration, SystemTime};

/// The path a probe serves its state on.
pub const STATE_PATH: &str = "/state";

/// Why a peer could not be read, and when it last could.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerFailure {
    /// The peer's name, or its host when it has never answered.
    pub peer: String,
    /// What went wrong, in one sentence.
    pub reason: String,
    /// When this peer last answered, if it ever has.
    pub last_success: Option<SystemTime>,
}

impl std::fmt::Display for PeerFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.peer, self.reason)
    }
}
impl std::error::Error for PeerFailure {}

/// What to call the peer at this URL.
///
/// **Derived from configuration, never from what the peer reports.** An
/// envelope carries the probe's name for itself, and using it would mean
/// a peer's identity changed the first time it answered — every row
/// keyed under its hostname would have to be re-keyed under whatever it
/// called itself, and a machine that is configured but has never
/// replied could not be named at all. It also means one probe cannot
/// present itself as another.
///
/// `https://pi5.tail-scale.ts.net` becomes `pi5`; an address stays whole,
/// because the first dotted piece of `127.0.0.1` names nothing.
pub fn peer_name_of(url: &str) -> String {
    let host = host_of(url);
    if host.parse::<std::net::IpAddr>().is_ok() {
        return host;
    }
    host.split('.').next().unwrap_or(&host).to_string()
}

/// The host part of a URL.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = rest.split('/').next().unwrap_or(rest);
    host.rsplit_once(':')
        // Only strip a trailing `:port`, never an IPv6 colon.
        .filter(|(_, port)| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
        .map(|(host, _)| host)
        .unwrap_or(host)
        .to_string()
}

/// One configured peer, and what it last said.
pub struct PeerClient<T: HttpTransport> {
    transport: T,
    url: String,
    name: String,
    interval: Duration,
    /// The project names this peer served last time it answered, so an
    /// unreachable peer still has rows to mark unavailable.
    known: Vec<String>,
    last_success: Option<SystemTime>,
}

impl<T: HttpTransport> PeerClient<T> {
    /// A client for the probe at `url`.
    pub fn new(transport: T, url: impl Into<String>) -> Self {
        let url = url.into();
        Self {
            name: peer_name_of(&url),
            url,
            transport,
            interval: DEFAULT_POLL_INTERVAL,
            known: Vec::new(),
            last_success: None,
        }
    }

    /// Sets the interval a fetched observation is re-stamped with.
    ///
    /// This is what a `Watched` value becomes on receipt — the peer is
    /// polled at this cadence, so that is how current its answers are.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// The peer's name, from its URL. Stable from the first frame,
    /// including before it has ever answered. See [`peer_name_of`].
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The configured URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Fetches this peer's projects, already re-stamped, re-based, and
    /// tagged with the peer they came from.
    pub fn fetch(&mut self, client_now: SystemTime) -> Result<Vec<ProjectState>, PeerFailure> {
        let request = HttpRequest::get(format!("{}{STATE_PATH}", self.url.trim_end_matches('/')));

        let body = match self.transport.send(&request) {
            Ok(HttpResponse::Ok { body, .. }) => body,
            // A probe sends no ETag, so this means something between
            // here and there is caching, which is worth saying rather
            // than silently treating as "unchanged".
            Ok(HttpResponse::NotModified) => {
                return Err(self.failure("answered 304, but a probe sends no ETag"))
            }
            // A gateway error means something is listening for this
            // machine and the probe behind it is not — the shape you get
            // from `tailscale serve` still proxying to a stopped probe,
            // which is the ordinary way a peer goes down rather than an
            // exotic one. The status alone does not say that, and
            // Tailscale's 502 carries no body to say it either.
            Err(AdapterError::Http { status, .. }) if (502..=504).contains(&status) => {
                return Err(self.failure(format!(
                    "http {status}: something is answering for this machine but the probe \
                     behind it is not — check `systemctl --user status parallax-probe` there"
                )))
            }
            Err(e) => return Err(self.failure(e.to_string())),
        };

        let envelope: StateEnvelope = match serde_json::from_str(&body) {
            Ok(envelope) => envelope,
            Err(e) => return Err(self.failure(format!("not a state envelope: {e}"))),
        };

        let projects = match envelope.receive(client_now, self.interval) {
            Ok(projects) => projects,
            Err(e) => return Err(self.failure(e.problem)),
        };

        self.known = projects.iter().map(|p| p.name.clone()).collect();
        self.last_success = Some(client_now);

        Ok(projects
            .into_iter()
            .map(|mut p| {
                p.peer = Some(self.name.clone());
                p
            })
            .collect())
    }

    /// Builds this failure.
    fn failure(&self, reason: impl Into<String>) -> PeerFailure {
        PeerFailure {
            peer: self.name.clone(),
            reason: reason.into(),
            last_success: self.last_success,
        }
    }

    /// The rows to show for a peer that did not answer.
    ///
    /// One per project it served last time, each carrying a degradation
    /// so `ProjectState::sources` reports it as
    /// [`crate::freshness::Freshness::Unavailable`]. A peer that has
    /// never answered gets a single row named for itself, because a
    /// machine that is configured and unreachable is a fact worth
    /// showing — and showing nothing is indistinguishable from never
    /// having configured it.
    pub fn unavailable(&self, failure: &PeerFailure) -> Vec<ProjectState> {
        let degradation = Degradation {
            source: format!("peer:{}", self.name),
            reason: failure.reason.clone(),
        };
        if self.known.is_empty() {
            return vec![ProjectState {
                name: self.name.clone(),
                peer: Some(self.name.clone()),
                degradations: vec![degradation],
                ..Default::default()
            }];
        }
        self.known
            .iter()
            .map(|name| ProjectState {
                name: name.clone(),
                peer: Some(self.name.clone()),
                degradations: vec![degradation.clone()],
                ..Default::default()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::http::FixtureTransport;
    use crate::adapters::verification::{VerificationOutcome, VerificationStatus};
    use crate::freshness::{Freshness, Observed};

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

    /// A probe's answer, as a probe would actually serve it.
    fn envelope_json(peer: &str, project: &str, now: SystemTime) -> String {
        let state = ProjectState {
            name: project.into(),
            verification: vec![Observed::watched(status(), now)],
            ..Default::default()
        };
        serde_json::to_string(&StateEnvelope::send(peer, now, vec![state])).unwrap()
    }

    fn client(url: &str, body: Option<String>) -> PeerClient<FixtureTransport> {
        let mut transport = FixtureTransport::new();
        if let Some(body) = body {
            transport.insert(format!("{url}/state"), body, None);
        }
        PeerClient::new(transport, url)
    }

    /// Identity comes from the registry entry, not from the answer. A
    /// peer whose name changed the moment it first replied would re-key
    /// every row the cockpit had already drawn for it.
    #[test]
    fn a_peer_is_named_from_its_url_and_not_from_what_it_claims() {
        let mut c = client(
            "https://pi5.tail-scale.ts.net",
            // The probe insists it is called something else entirely.
            Some(envelope_json("kitchen-nuc", "sesh", at(0))),
        );
        assert_eq!(c.name(), "pi5");
        let projects = c.fetch(at(0)).unwrap();
        assert_eq!(c.name(), "pi5", "the peer renamed itself by answering");
        assert_eq!(projects[0].qualified_name(), "sesh@pi5");
    }

    #[test]
    fn host_is_taken_from_a_url_with_or_without_a_port() {
        assert_eq!(host_of("https://pi5.ts.net"), "pi5.ts.net");
        assert_eq!(host_of("http://127.0.0.1:8737"), "127.0.0.1");
        assert_eq!(host_of("http://pi5.ts.net/state"), "pi5.ts.net");
    }

    /// The first dotted piece of an address names nothing, so an address
    /// stays whole.
    #[test]
    fn a_peer_name_is_the_first_label_unless_the_host_is_an_address() {
        assert_eq!(peer_name_of("https://pi5.tail-scale.ts.net"), "pi5");
        assert_eq!(peer_name_of("http://127.0.0.1:8737"), "127.0.0.1");
        assert_eq!(peer_name_of("http://[::1]:8737"), "[::1]");
        assert_eq!(peer_name_of("https://laptop"), "laptop");
    }

    #[test]
    fn a_fetched_project_is_tagged_with_the_peer_it_came_from() {
        let mut c = client(
            "https://pi5.ts.net",
            Some(envelope_json("pi5", "sesh", at(0))),
        );
        let projects = c.fetch(at(100)).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].peer.as_deref(), Some("pi5"));
        assert_eq!(projects[0].qualified_name(), "sesh@pi5");
    }

    /// The design's central claim, now at the layer that actually
    /// fetches: a `Watched` verification served by a probe must not
    /// arrive claiming to be live.
    #[test]
    fn a_fetched_observation_is_never_live() {
        let mut c = client(
            "https://pi5.ts.net",
            Some(envelope_json("pi5", "sesh", at(0))),
        );
        let projects = c.fetch(at(0)).unwrap();
        assert_ne!(
            projects[0].verification[0].freshness(at(0)),
            Freshness::Live
        );
    }

    #[test]
    fn a_peer_that_cannot_be_reached_fails_with_a_reason() {
        let mut transport = FixtureTransport::new();
        transport.fail_next(AdapterError::Timeout("connection refused".into()));
        let mut c = PeerClient::new(transport, "https://laptop.ts.net");

        let failure = c.fetch(at(0)).unwrap_err();
        assert!(failure.reason.contains("connection refused"));
        assert_eq!(failure.last_success, None);
    }

    /// The ordinary way a peer goes down: the probe is stopped and
    /// whatever publishes it keeps proxying, so the machine answers 502
    /// rather than going quiet. Seen for real on a Pi behind
    /// `tailscale serve`, whose 502 arrives with an empty body — so the
    /// status is the only thing there, and the status alone does not say
    /// which of the two is broken.
    #[test]
    fn a_gateway_error_says_the_probe_is_down_rather_than_only_the_status() {
        let mut transport = FixtureTransport::new();
        transport.fail_next(AdapterError::Http {
            status: 502,
            message: String::new(),
        });
        let mut c = PeerClient::new(transport, "https://tatepi.tail-scale.ts.net");

        let failure = c.fetch(at(0)).unwrap_err();
        assert!(failure.reason.contains("502"), "got {}", failure.reason);
        assert!(
            failure.reason.contains("probe"),
            "an operator is told a number and nothing to do about it: {}",
            failure.reason
        );
    }

    /// A 404 is a different thing entirely — the machine and its probe
    /// are both fine and something else is wrong — so it must not
    /// collect the gateway advice.
    #[test]
    fn a_non_gateway_status_keeps_its_own_message() {
        let mut transport = FixtureTransport::new();
        transport.fail_next(AdapterError::Http {
            status: 404,
            message: "no such path".into(),
        });
        let mut c = PeerClient::new(transport, "https://tatepi.tail-scale.ts.net");

        let failure = c.fetch(at(0)).unwrap_err();
        assert!(failure.reason.contains("no such path"));
        assert!(!failure.reason.contains("systemctl"));
    }

    #[test]
    fn a_peer_answering_with_nonsense_is_a_failure_rather_than_a_panic() {
        let mut c = client(
            "https://pi5.ts.net",
            Some("{\"not\":\"an envelope\"}".into()),
        );
        let failure = c.fetch(at(0)).unwrap_err();
        assert!(
            failure.reason.contains("not a state envelope"),
            "got {}",
            failure.reason
        );
    }

    /// The sleeping-laptop case. Its projects stay on screen, marked
    /// unavailable — a row that vanished would read as a project nobody
    /// ever registered.
    #[test]
    fn an_unreachable_peer_keeps_the_rows_it_served_last_time() {
        let mut transport = FixtureTransport::new();
        transport.insert(
            "https://pi5.ts.net/state",
            envelope_json("pi5", "sesh", at(0)),
            None,
        );
        let mut c = PeerClient::new(transport, "https://pi5.ts.net");
        c.fetch(at(0)).expect("first fetch works");

        let failure = PeerFailure {
            peer: "pi5".into(),
            reason: "connection refused".into(),
            last_success: Some(at(0)),
        };
        let rows = c.unavailable(&failure);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].qualified_name(), "sesh@pi5");
        match &rows[0].sources(at(500))[0].freshness {
            Freshness::Unavailable { reason, .. } => assert!(reason.contains("refused")),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    /// A machine that is configured and has never answered is a fact
    /// worth showing. Nothing at all looks like nothing configured.
    #[test]
    fn a_peer_that_has_never_answered_still_shows_as_one_unavailable_row() {
        let c = client("https://pi5.ts.net", None);
        let failure = PeerFailure {
            peer: "pi5".into(),
            reason: "no route to host".into(),
            last_success: None,
        };
        let rows = c.unavailable(&failure);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "pi5");
        assert_eq!(rows[0].peer.as_deref(), Some("pi5"));
        assert_eq!(rows[0].degradations.len(), 1);
    }

    #[test]
    fn a_successful_fetch_records_when_it_happened() {
        let mut c = client(
            "https://pi5.ts.net",
            Some(envelope_json("pi5", "sesh", at(0))),
        );
        c.fetch(at(42)).unwrap();

        // Force a failure and check the timestamp survived on it.
        let failure = c.failure("gone");
        assert_eq!(failure.last_success, Some(at(42)));
    }

    #[test]
    fn the_state_path_is_appended_once_even_to_a_trailing_slash() {
        let mut transport = FixtureTransport::new();
        transport.insert(
            "https://pi5.ts.net/state",
            envelope_json("pi5", "sesh", at(0)),
            None,
        );
        let mut c = PeerClient::new(transport, "https://pi5.ts.net/");
        assert!(c.fetch(at(0)).is_ok(), "the URL was built wrong");
    }
}
