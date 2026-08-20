//! Taking an action on another machine.
//!
//! **This type is deliberately not an [`super::ActionExecutor`].** That
//! trait returns `Result<ActionOutcome, ActionError>` — a two-valued
//! shape in which everything that is not success is failure, which is
//! exactly right for a call that either happened or did not. A
//! submission that crossed a network has a third possibility, and
//! implementing the trait would mean choosing which of the two lies to
//! tell about it. See [`Submitted`].

use super::wire::{
    ActionId, ActionRequest, ActionStatus, IdSource, ProbeRun, StatusReply, SubmitReply,
};
use super::{Action, Confirmation};
use crate::adapters::http::{HttpRequest, HttpResponse, HttpTransport, Method};
use crate::adapters::AdapterError;
use std::collections::HashMap;

/// The path a probe accepts actions on.
pub const ACTION_PATH: &str = "/action";

/// What came of offering an action to another machine.
///
/// **Three variants, and no way to collapse them into two.** When a
/// request does not complete, three things are equally consistent with
/// what the caller saw: it never arrived; it arrived and ran; it
/// arrived, ran, and the answer was lost. Reporting that as a failure is
/// the same lie as reporting a remote observation as `Live`, and a more
/// expensive one — an operator who reads "merge failed" presses the key
/// again.
///
/// There is deliberately no `is_ok`, for the reason
/// [`crate::wire::ObservedWire`] has no `freshness`: a caller that could
/// flatten three states into two would, and the state it would drop is
/// the only one that needed saying.
///
/// ```compile_fail
/// use parallax_baseline::actions::Submitted;
/// let s = Submitted::Refused { reason: "control is not enabled".into() };
/// // No `is_ok` exists — three states cannot be flattened into two.
/// let _ = s.is_ok();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Submitted {
    /// The probe has it and will run it. Ask again by id.
    Accepted {
        /// The id it was filed under.
        id: ActionId,
        /// Which run of the probe holds it.
        run: ProbeRun,
    },
    /// The probe refused it, and that is final: it did not run.
    Refused {
        /// Why, in one sentence an operator can act on.
        reason: String,
    },
    /// The exchange did not complete. **Whether the action ran is not
    /// known**, and the id is carried so it can be asked about.
    Unknown {
        /// The id it was offered under.
        id: ActionId,
        /// What went wrong with the exchange — not with the action.
        reason: String,
    },
}

/// What a client concludes about an action it submitted.
///
/// [`ActionStatus`] is the probe's half of this. The difference is
/// [`Standing::NeverArrived`], which no probe can report on its own:
/// "this run has no record of that id" only means the action never
/// arrived if it is the *same run* the client submitted to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Accepted, still going.
    Running,
    /// It ran and succeeded.
    Done {
        /// What happened.
        summary: String,
    },
    /// It ran and failed — a real answer.
    Failed {
        /// Why.
        reason: String,
    },
    /// It never ran, by the executing machine's decision.
    Refused {
        /// Why.
        reason: String,
    },
    /// The same probe process that answered the submission has no record
    /// of it. It never arrived, and offering it again is safe.
    NeverArrived,
    /// Nobody can say. **Never render this as a failure.**
    Unknown {
        /// Why the answer is not knowable.
        reason: String,
    },
}

/// Something that can offer an action to another machine.
///
/// The trait exists for the same reason [`HttpTransport`] does: a
/// frontend holding one per peer needs them in one collection, and a
/// recorded one and a live one are different concrete types. It is
/// deliberately *not* [`super::ActionExecutor`] — see the module note.
pub trait Submitter {
    /// Which machine this acts on.
    fn peer(&self) -> &str;
    /// Offers an action, returning as soon as the probe has it.
    fn submit(&mut self, action: &Action, confirmation: Option<&Confirmation>) -> Submitted;
    /// Asks what became of an action.
    fn standing(&mut self, id: &ActionId) -> Standing;
}

impl<T: HttpTransport> Submitter for RemoteExecutor<T> {
    fn peer(&self) -> &str {
        self.peer()
    }

    fn submit(&mut self, action: &Action, confirmation: Option<&Confirmation>) -> Submitted {
        self.submit(action, confirmation)
    }

    fn standing(&mut self, id: &ActionId) -> Standing {
        self.standing(id)
    }
}

/// Offers actions to one peer's probe.
pub struct RemoteExecutor<T: HttpTransport> {
    transport: T,
    base_url: String,
    peer: String,
    client: String,
    ids: IdSource,
    /// The run each accepted id was filed under, so a later
    /// `not-submitted` can be read against the run that said so.
    accepted: HashMap<ActionId, ProbeRun>,
}

impl<T: HttpTransport> RemoteExecutor<T> {
    /// An executor for the probe at `base_url`, naming itself `client`.
    ///
    /// `run` distinguishes this process from an earlier one; see
    /// [`IdSource`] for why reusing ids across runs would be a bug.
    pub fn new(
        transport: T,
        base_url: impl Into<String>,
        peer: impl Into<String>,
        client: impl Into<String>,
        run: u64,
    ) -> Self {
        let client = client.into();
        Self {
            transport,
            base_url: base_url.into(),
            peer: peer.into(),
            ids: IdSource::new(client.clone(), run),
            client,
            accepted: HashMap::new(),
        }
    }

    /// Which machine this acts on.
    pub fn peer(&self) -> &str {
        &self.peer
    }

    /// Offers an action, returning as soon as the probe has it.
    ///
    /// **The wait is bounded because the answer is short.** The probe
    /// enqueues rather than executing, so this is never waiting on a
    /// build — it is waiting on a machine saying "I have this", which a
    /// machine that is awake does immediately.
    pub fn submit(&mut self, action: &Action, confirmation: Option<&Confirmation>) -> Submitted {
        let id = self.ids.next_id();
        let request = ActionRequest::new(
            id.clone(),
            self.client.clone(),
            action.clone(),
            confirmation,
        );
        let body = match serde_json::to_string(&request) {
            Ok(b) => b,
            // Serializing our own request failing is this crate's bug,
            // and it happened before anything was sent — which is the
            // one case here that is genuinely not unknown.
            Err(e) => {
                return Submitted::Refused {
                    reason: format!("could not encode the request: {e}"),
                }
            }
        };
        let url = format!("{}{ACTION_PATH}", self.base_url);
        match self
            .transport
            .send(&HttpRequest::write(Method::Post, url, body))
        {
            Ok(HttpResponse::Ok { body, .. }) => match serde_json::from_str::<SubmitReply>(&body) {
                Ok(SubmitReply::Accepted { id, run }) => {
                    self.accepted.insert(id.clone(), run.clone());
                    Submitted::Accepted { id, run }
                }
                Ok(SubmitReply::Refused { reason }) => Submitted::Refused { reason },
                // A reply we cannot read is not a reply that says no.
                Err(e) => Submitted::Unknown {
                    id,
                    reason: format!("{} answered with something unreadable: {e}", self.peer),
                },
            },
            // Nonsense for a write, and nonsense is not a refusal.
            Ok(HttpResponse::NotModified) => Submitted::Unknown {
                id,
                reason: format!("{} answered `304` to a submission", self.peer),
            },
            Err(e) => self.submission_failure(id, e),
        }
    }

    /// Classifies a transport failure into refused or unknown.
    ///
    /// **A `4xx` is the probe having read the request and declined it**
    /// — control disabled, a body it could not parse — so nothing ran.
    /// Everything else, `5xx` and timeouts alike, leaves the question
    /// open: a probe can accept an action and die before answering.
    fn submission_failure(&self, id: ActionId, error: AdapterError) -> Submitted {
        match &error {
            AdapterError::Http { status, message } if (400..500).contains(status) => {
                Submitted::Refused {
                    reason: if message.is_empty() {
                        format!("{} refused it ({status})", self.peer)
                    } else {
                        format!("{}: {message}", self.peer)
                    },
                }
            }
            _ => Submitted::Unknown {
                id,
                reason: format!("{}: {error}", self.peer),
            },
        }
    }

    /// Asks what became of an action.
    pub fn standing(&mut self, id: &ActionId) -> Standing {
        let url = format!("{}{ACTION_PATH}/{id}", self.base_url);
        let body = match self.transport.send(&HttpRequest::get(url)) {
            Ok(HttpResponse::Ok { body, .. }) => body,
            Ok(HttpResponse::NotModified) => {
                return Standing::Unknown {
                    reason: format!("{} answered `304` to a status query", self.peer),
                }
            }
            Err(e) => {
                return Standing::Unknown {
                    reason: format!("{}: {e}", self.peer),
                }
            }
        };
        let reply: StatusReply = match serde_json::from_str(&body) {
            Ok(r) => r,
            Err(e) => {
                return Standing::Unknown {
                    reason: format!("{} answered with something unreadable: {e}", self.peer),
                }
            }
        };
        match reply.status {
            ActionStatus::Running => Standing::Running,
            ActionStatus::Done { summary } => Standing::Done { summary },
            ActionStatus::Failed { reason } => Standing::Failed { reason },
            ActionStatus::Refused { reason } => Standing::Refused { reason },
            ActionStatus::NotSubmitted => self.never_arrived_or_unknown(id, &reply.run),
        }
    }

    /// Reads a `not-submitted` against the run that said it.
    ///
    /// **A ledger is memory.** The probe that was asked to run the
    /// action forgets everything when the process ends, so its
    /// successor answering "I have no record of that" is not evidence
    /// about the world — and a client that took it for evidence would
    /// merge a pull request that had already been merged.
    fn never_arrived_or_unknown(&self, id: &ActionId, answering: &ProbeRun) -> Standing {
        match self.accepted.get(id) {
            Some(run) if run == answering => Standing::NeverArrived,
            Some(run) => Standing::Unknown {
                reason: format!(
                    "{} has restarted since it accepted this (run {run} became {answering}); \
                     whether it ran is not recorded",
                    self.peer
                ),
            },
            // No acceptance was ever received, so there is no run to
            // compare against and `not-submitted` proves nothing.
            None => Standing::Unknown {
                reason: format!(
                    "{} never confirmed receiving this, and has no record of it now",
                    self.peer
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::http::FixtureTransport;

    const URL: &str = "http://pi5.example";

    fn merge() -> Action {
        Action::MergePullRequest {
            project: "sesh".into(),
            number: 12,
        }
    }

    fn executor(transport: FixtureTransport) -> RemoteExecutor<FixtureTransport> {
        RemoteExecutor::new(transport, URL, "pi5", "desktop", 1)
    }

    fn accepting(id: &str, run: &str) -> FixtureTransport {
        let mut t = FixtureTransport::new();
        t.insert_write(
            Method::Post,
            format!("{URL}{ACTION_PATH}"),
            serde_json::to_string(&SubmitReply::Accepted {
                id: ActionId::new(id),
                run: ProbeRun::new(run),
            })
            .unwrap(),
        );
        t
    }

    #[test]
    fn a_submission_carries_the_action_and_the_confirmed_fingerprint() {
        let mut ex = executor(accepting("desktop-1-1", "r1"));
        let confirmation = Confirmation::of(&merge());
        ex.submit(&merge(), Some(&confirmation));
        let sent = ex.transport.writes()[0].body.clone().unwrap();
        let req: ActionRequest = serde_json::from_str(&sent).unwrap();
        assert_eq!(req.action, merge());
        assert_eq!(req.confirmed.as_deref(), Some(confirmation.fingerprint()));
        assert_eq!(req.requested_by, "desktop");
    }

    #[test]
    fn an_accepted_submission_reports_the_id_and_run() {
        let mut ex = executor(accepting("desktop-1-1", "r1"));
        match ex.submit(&merge(), Some(&Confirmation::of(&merge()))) {
            Submitted::Accepted { id, run } => {
                assert_eq!(id.as_str(), "desktop-1-1");
                assert_eq!(run.as_str(), "r1");
            }
            other => panic!("expected acceptance, got {other:?}"),
        }
    }

    /// The arc's central claim, at the point where it would be easiest
    /// to get wrong: a request that did not complete is not a refusal.
    #[test]
    fn a_lost_answer_is_unknown_and_never_a_refusal() {
        let mut t = accepting("desktop-1-1", "r1");
        t.fail_next(AdapterError::Timeout("read timed out".into()));
        let mut ex = executor(t);
        match ex.submit(&merge(), Some(&Confirmation::of(&merge()))) {
            Submitted::Unknown { id, reason } => {
                assert_eq!(id.as_str(), "desktop-1-1");
                assert!(reason.contains("pi5"), "the reason must name it: {reason}");
            }
            other => panic!("a timeout was reported as {other:?}"),
        }
    }

    /// A `5xx` leaves the question open the same way a timeout does: a
    /// probe can accept an action and die before answering.
    #[test]
    fn a_server_error_is_unknown_too() {
        let mut t = accepting("desktop-1-1", "r1");
        t.fail_next(AdapterError::Http {
            status: 500,
            message: "could not serialize".into(),
        });
        let mut ex = executor(t);
        assert!(matches!(
            ex.submit(&merge(), Some(&Confirmation::of(&merge()))),
            Submitted::Unknown { .. }
        ));
    }

    /// A `403` is the probe having read the request and declined it, so
    /// nothing ran and the operator can be told so plainly.
    #[test]
    fn a_control_disabled_peer_is_a_refusal_not_an_unknown() {
        let mut t = accepting("desktop-1-1", "r1");
        t.fail_next(AdapterError::Http {
            status: 403,
            message: "control is not enabled on this probe; start it with --allow-control".into(),
        });
        let mut ex = executor(t);
        match ex.submit(&merge(), Some(&Confirmation::of(&merge()))) {
            Submitted::Refused { reason } => {
                assert!(reason.contains("--allow-control"), "got {reason}");
            }
            other => panic!("a 403 was reported as {other:?}"),
        }
    }

    #[test]
    fn a_reply_that_cannot_be_read_is_unknown_rather_than_refused() {
        let mut t = FixtureTransport::new();
        t.insert_write(Method::Post, format!("{URL}{ACTION_PATH}"), "not json");
        let mut ex = executor(t);
        assert!(matches!(
            ex.submit(&merge(), Some(&Confirmation::of(&merge()))),
            Submitted::Unknown { .. }
        ));
    }

    fn status_transport(id: &str, reply: StatusReply) -> FixtureTransport {
        let mut t = accepting(id, "r1");
        t.insert(
            format!("{URL}{ACTION_PATH}/{id}"),
            serde_json::to_string(&reply).unwrap(),
            None,
        );
        t
    }

    #[test]
    fn a_finished_action_reports_what_it_did() {
        let mut ex = executor(status_transport(
            "desktop-1-1",
            StatusReply {
                run: ProbeRun::new("r1"),
                status: ActionStatus::Done {
                    summary: "sesh: merge pull request #12".into(),
                },
            },
        ));
        ex.submit(&merge(), Some(&Confirmation::of(&merge())));
        match ex.standing(&ActionId::new("desktop-1-1")) {
            Standing::Done { summary } => assert!(summary.contains("#12")),
            other => panic!("got {other:?}"),
        }
    }

    /// `not-submitted` from the same run that accepted it is the one
    /// case where it means what it says.
    #[test]
    fn the_same_run_saying_it_never_arrived_is_believed() {
        let mut ex = executor(status_transport(
            "desktop-1-1",
            StatusReply {
                run: ProbeRun::new("r1"),
                status: ActionStatus::NotSubmitted,
            },
        ));
        ex.submit(&merge(), Some(&Confirmation::of(&merge())));
        assert_eq!(
            ex.standing(&ActionId::new("desktop-1-1")),
            Standing::NeverArrived
        );
    }

    /// The trap this design exists to close: a probe that restarted has
    /// forgotten, and its forgetting is not evidence.
    #[test]
    fn a_probe_that_restarted_cannot_say_the_action_never_arrived() {
        let mut ex = executor(status_transport(
            "desktop-1-1",
            StatusReply {
                run: ProbeRun::new("r2"),
                status: ActionStatus::NotSubmitted,
            },
        ));
        ex.submit(&merge(), Some(&Confirmation::of(&merge())));
        match ex.standing(&ActionId::new("desktop-1-1")) {
            Standing::Unknown { reason } => {
                assert!(reason.contains("restarted"), "got {reason}");
            }
            other => panic!("a restarted probe was believed: {other:?}"),
        }
    }

    /// And with no acceptance ever received there is no run to compare
    /// against, so `not-submitted` proves nothing either.
    #[test]
    fn without_an_acceptance_not_submitted_is_still_unknown() {
        let mut t = FixtureTransport::new();
        t.insert(
            format!("{URL}{ACTION_PATH}/desktop-1-1"),
            serde_json::to_string(&StatusReply {
                run: ProbeRun::new("r1"),
                status: ActionStatus::NotSubmitted,
            })
            .unwrap(),
            None,
        );
        let mut ex = executor(t);
        assert!(matches!(
            ex.standing(&ActionId::new("desktop-1-1")),
            Standing::Unknown { .. }
        ));
    }

    #[test]
    fn an_unreachable_peer_leaves_the_standing_unknown() {
        let mut t = FixtureTransport::new();
        t.fail_next(AdapterError::Timeout("no route to host".into()));
        let mut ex = executor(t);
        assert!(matches!(
            ex.standing(&ActionId::new("desktop-1-1")),
            Standing::Unknown { .. }
        ));
    }
}
