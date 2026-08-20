//! The serialized contract for taking an action on another machine.
//!
//! [`crate::wire`] is the same idea for observation, and the two follow
//! the same rules: unknown fields are ignored rather than rejected,
//! because two programs on two machines upgrade at different times; and
//! a type that means something different on each end is written twice
//! rather than shared.
//!
//! **Authorization is not on the wire, and cannot be.** [`Authorized`]
//! is safe inside one process because its field is private, so the
//! compiler can promise every value of it came from [`authorize`]. A
//! network erases that promise — whatever bytes meant "already
//! authorized" could be typed into `curl`. So the request carries the
//! action and the fingerprint the operator confirmed, and the machine
//! that will execute calls [`authorize`] itself. See
//! [`ActionRequest::authorize_here`].

use super::{authorize, fingerprint, Action, ActionError, Authorized, Confirmation};
use serde::{Deserialize, Serialize};

/// The action wire format's version. Bumped only for a change a client
/// of the previous version could not read.
pub const ACTION_API_VERSION: &str = "parallax-action/v1";

/// A client's name for one action, so it can ask about it again.
///
/// Opaque, and **supplied rather than invented** — this module has no
/// random source, which is what lets every test name its own ids. See
/// [`IdSource`] for the sequence a running cockpit uses.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionId(String);

impl ActionId {
    /// An id from a value the caller already has.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The id as it goes on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A cockpit's supply of action ids.
///
/// **The `run` part is the whole point.** A probe answers a repeated id
/// with the outcome it already recorded, which is what makes retrying
/// safe. A source that counted from one on every start would therefore
/// have its *second* run's first action answered with its *first* run's
/// first outcome — a merge reported as done that never happened. `run`
/// is a marker the caller varies per process (its start time serves),
/// and a test passes a literal.
#[derive(Debug, Clone)]
pub struct IdSource {
    client: String,
    run: u64,
    next: u64,
}

impl IdSource {
    /// A source for `client`, distinct per `run`.
    pub fn new(client: impl Into<String>, run: u64) -> Self {
        Self {
            client: client.into(),
            run,
            next: 1,
        }
    }

    /// The next id in the sequence.
    pub fn next_id(&mut self) -> ActionId {
        let n = self.next;
        self.next += 1;
        ActionId::new(format!("{}-{}-{n}", self.client, self.run))
    }
}

/// Identifies one run of one probe process.
///
/// **The honest alternative to comparing two clocks.** A probe's ledger
/// is memory: it forgets everything when the process ends, so "I have
/// never seen this id" means one thing from the process that was asked
/// to run the action and something else entirely from its successor.
/// Distinguishing them by timestamp would mean subtracting a client's
/// wall clock from a probe's, which [`crate::wire`] forbids for good
/// reason — a Raspberry Pi 5 has no RTC. Comparing identity needs no
/// clock at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProbeRun(String);

impl ProbeRun {
    /// A run marker from a value the caller already has.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The marker as it goes on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProbeRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One action, offered to the machine that owns the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRequest {
    /// The format this was written in.
    pub api_version: String,
    /// The client's name for this action.
    pub id: ActionId,
    /// Who *says* they asked.
    ///
    /// **A claim, not an identity.** Nothing authenticates it. It is
    /// worth carrying because the socket reports `127.0.0.1` for every
    /// request — `tailscale serve` is the thing connecting — so without
    /// it an audit line cannot name a machine at all.
    pub requested_by: String,
    /// What to do.
    pub action: Action,
    /// The fingerprint of the action the operator confirmed, when one
    /// was confirmed. **Never a [`Confirmation`]**: see the module note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<String>,
}

impl ActionRequest {
    /// A request for `action`, carrying `confirmation` if the operator
    /// gave one.
    pub fn new(
        id: ActionId,
        requested_by: impl Into<String>,
        action: Action,
        confirmation: Option<&Confirmation>,
    ) -> Self {
        Self {
            api_version: ACTION_API_VERSION.to_string(),
            id,
            requested_by: requested_by.into(),
            confirmed: confirmation.map(|c| c.fingerprint().to_string()),
            action,
        }
    }

    /// Whether this request was written in a format we can read.
    pub fn version_matches(&self) -> bool {
        self.api_version == ACTION_API_VERSION
    }

    /// Authorizes this request **against this machine's rules**.
    ///
    /// Two properties, and only one of them is security.
    ///
    /// **The executing machine's classification wins.** Whether
    /// [`Action::Push`] needs confirming is decided by the
    /// [`super::Reversibility`] table compiled into *this* binary. A
    /// caller that is out of date, or wrong, or hostile cannot make this
    /// machine treat an irreversible action as a reversible one.
    ///
    /// **The fingerprint is a consistency check, not an
    /// authentication.** It catches a caller that confused two
    /// actions — the bug class the local contract was built against —
    /// and does not catch one that is lying, because a liar can compute
    /// the fingerprint of whatever it is sending. The boundary for
    /// control is the boundary for reads: the probe binds loopback and
    /// the tailnet decides who may reach it.
    ///
    /// Note what this does *not* do: it never builds a [`Confirmation`]
    /// out of a string. Having checked that the carried fingerprint is
    /// the one this action hashes to, `Confirmation::of` produces that
    /// same value from the action itself, so the wire needs no back door
    /// into a type whose whole job is to have only one constructor.
    pub fn authorize_here(&self) -> Result<Authorized<'_>, ActionError> {
        let expected = fingerprint(&self.action);
        match &self.confirmed {
            Some(got) if *got != expected => Err(ActionError::ConfirmationMismatch {
                expected,
                got: got.clone(),
            }),
            Some(_) => authorize(&self.action, Some(&Confirmation::of(&self.action))),
            None => authorize(&self.action, None),
        }
    }
}

/// A probe's answer to a submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case")]
pub enum SubmitReply {
    /// The probe has it and will run it. Ask again by id.
    Accepted {
        /// The id it was filed under — the client's own.
        id: ActionId,
        /// Which run of the probe holds it. See [`ProbeRun`].
        run: ProbeRun,
    },
    /// The probe will not run it, and that is final.
    Refused {
        /// Why, in one sentence an operator can act on.
        reason: String,
    },
}

/// What a probe knows about an action it was asked to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ActionStatus {
    /// Accepted, and still going.
    Running,
    /// It ran and succeeded.
    Done {
        /// What happened, in the action's own words.
        summary: String,
    },
    /// It ran and failed. **A real answer**, unlike a lost one.
    Failed {
        /// Why it failed.
        reason: String,
    },
    /// It never ran: authorization, or a project this machine has not
    /// got.
    Refused {
        /// Why it was refused.
        reason: String,
    },
    /// This run of this probe has no record of that id.
    ///
    /// **Only the probe's half of the answer.** Whether it means the
    /// action never arrived depends on whether this is the same run the
    /// client submitted to, which the client is the one that knows.
    NotSubmitted,
}

/// A probe's answer to a status query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReply {
    /// Which run of the probe is answering.
    pub run: ProbeRun,
    /// What it knows.
    pub status: ActionStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Reversibility;

    fn merge(number: u64) -> Action {
        Action::MergePullRequest {
            project: "sesh".into(),
            number,
        }
    }

    fn label() -> Action {
        Action::SetAutonomyLabel {
            project: "sesh".into(),
            item: 7,
            label: "gated".into(),
        }
    }

    fn request(action: Action, confirmation: Option<&Confirmation>) -> ActionRequest {
        ActionRequest::new(
            ActionId::new("desktop-1-1"),
            "desktop",
            action,
            confirmation,
        )
    }

    #[test]
    fn a_request_round_trips_with_and_without_a_confirmation() {
        for confirmed in [None, Some(Confirmation::of(&merge(12)))] {
            let req = request(merge(12), confirmed.as_ref());
            let json = serde_json::to_string(&req).unwrap();
            let back: ActionRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(req, back);
        }
    }

    /// The rule [`crate::wire`] argues for at length, holding here too:
    /// a newer probe adding a field must not break an older client.
    #[test]
    fn an_unknown_field_is_ignored_rather_than_rejected() {
        let json = r#"{"apiVersion":"parallax-action/v1","id":"a-1-1",
            "requestedBy":"desktop","action":{"action":"request-re-review",
            "project":"sesh","item":3},"somethingNewer":true}"#;
        let back: ActionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(back.requested_by, "desktop");
    }

    #[test]
    fn every_reply_and_status_round_trips() {
        let replies = [
            SubmitReply::Accepted {
                id: ActionId::new("a"),
                run: ProbeRun::new("r"),
            },
            SubmitReply::Refused {
                reason: "control is not enabled".into(),
            },
        ];
        for r in replies {
            let back: SubmitReply =
                serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
            assert_eq!(r, back);
        }
        let statuses = [
            ActionStatus::Running,
            ActionStatus::Done {
                summary: "merged".into(),
            },
            ActionStatus::Failed {
                reason: "conflict".into(),
            },
            ActionStatus::Refused {
                reason: "no such project".into(),
            },
            ActionStatus::NotSubmitted,
        ];
        for status in statuses {
            let reply = StatusReply {
                run: ProbeRun::new("r"),
                status,
            };
            let back: StatusReply =
                serde_json::from_str(&serde_json::to_string(&reply).unwrap()).unwrap();
            assert_eq!(reply, back);
        }
    }

    /// The spec's central authorization claim. Asserted over the whole
    /// confirmation-required group rather than one member, so a
    /// reclassification cannot slip through on the untested rows.
    #[test]
    fn the_executing_machines_classification_wins() {
        let irreversible = [
            merge(12),
            Action::Push {
                project: "sesh".into(),
                branch: "main".into(),
            },
            Action::StopAgentRun {
                project: "sesh".into(),
                session: "audit".into(),
            },
        ];
        for action in irreversible {
            assert_eq!(
                action.reversibility(),
                Reversibility::ConfirmationRequired,
                "the fixture stopped being irreversible: {}",
                action.summary()
            );
            let err = request(action.clone(), None)
                .authorize_here()
                .expect_err("an unconfirmed irreversible action was authorized");
            assert!(
                matches!(err, ActionError::ConfirmationRequired { .. }),
                "got {err}"
            );
        }
    }

    #[test]
    fn a_reversible_action_needs_no_confirmation_here_either() {
        assert!(request(label(), None).authorize_here().is_ok());
    }

    /// A caller that confused two actions is the bug class the local
    /// contract was built against, and it does not stop being one
    /// because the caller is on another machine.
    #[test]
    fn a_confirmation_for_a_different_action_is_refused_across_the_wire() {
        let mut req = request(merge(12), Some(&Confirmation::of(&merge(12))));
        req.action = merge(99);
        let err = req
            .authorize_here()
            .expect_err("a confirmation for #12 authorized #99");
        assert!(matches!(err, ActionError::ConfirmationMismatch { .. }));
    }

    #[test]
    fn a_confirmation_that_matches_authorizes() {
        let req = request(merge(12), Some(&Confirmation::of(&merge(12))));
        assert_eq!(req.authorize_here().unwrap().action(), &merge(12));
    }

    /// A confirmation cannot be smuggled in as a bare string: the
    /// fingerprint has to be the one the action hashes to.
    #[test]
    fn an_invented_fingerprint_does_not_authorize() {
        let mut req = request(merge(12), None);
        req.confirmed = Some("00000000".into());
        assert!(matches!(
            req.authorize_here(),
            Err(ActionError::ConfirmationMismatch { .. })
        ));
    }

    #[test]
    fn a_version_this_binary_does_not_know_is_visible_rather_than_assumed() {
        let mut req = request(label(), None);
        assert!(req.version_matches());
        req.api_version = "parallax-action/v2".into();
        assert!(!req.version_matches());
    }

    /// The bug this type exists to prevent: a second run reusing the
    /// first run's ids would have its actions answered from the first
    /// run's ledger.
    #[test]
    fn two_runs_of_one_client_never_share_an_id() {
        let mut first = IdSource::new("desktop", 1);
        let mut second = IdSource::new("desktop", 2);
        assert_ne!(first.next_id(), second.next_id());
    }

    #[test]
    fn ids_from_one_source_are_distinct() {
        let mut ids = IdSource::new("desktop", 1);
        let taken: Vec<_> = (0..16).map(|_| ids.next_id()).collect();
        let unique: std::collections::HashSet<_> = taken.iter().collect();
        assert_eq!(unique.len(), taken.len());
    }
}
