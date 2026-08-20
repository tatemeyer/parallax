//! The half of the cockpit that acts.
//!
//! **The only module here allowed to name `parallax_baseline::actions`**
//! — `tests/read_only.rs` fails if any other one does. Observation stays
//! structurally incapable of mutating anything, which is the property
//! that makes the rest of the screen safe to leave running.
//!
//! Nothing here decides whether an action is allowed. `authorize` does,
//! in the library, and re-deciding it up here would put the confirmation
//! contract in two places that could disagree.

pub mod prompt;

use parallax_baseline::actions::wire::ActionId;
use parallax_baseline::actions::{
    authorize, Action, ActionError, ActionExecutor, Confirmation, Effect, Reversibility, Standing,
    Submitted,
};
use prompt::{Answer, Prompt};

/// How an attempt stands.
///
/// **Four states, not two.** A local action either worked or did not,
/// and a `bool` said that exactly. Once an action can be taken on
/// another machine there are two more: one that has been accepted and
/// is still going, and one whose answer was lost — and that last is the
/// whole point. A request that did not complete is not a failed action;
/// the merge may well have happened. Rendering it as a failure is what
/// makes an operator press the key again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// It happened.
    Done,
    /// It did not happen, and that is settled.
    Failed,
    /// Accepted by the machine that will run it, not yet finished.
    Running,
    /// Nobody can say whether it happened. **Not a failure.**
    Unknown,
}

impl Outcome {
    /// The two characters the log pane puts in front of the line.
    pub fn mark(&self) -> &'static str {
        match self {
            Outcome::Done => "ok",
            Outcome::Failed => "!!",
            Outcome::Running => "..",
            // Deliberately not `!!`. An operator scanning the column for
            // trouble must not read this as a failure, and must not skip
            // it either.
            Outcome::Unknown => "??",
        }
    }
}

/// One attempted action and what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// What was attempted, in the action's own words.
    pub summary: String,
    /// What happened, rendered for the log pane.
    pub result: String,
    /// How it stands.
    pub outcome: Outcome,
    /// The id it was submitted under, when it went to another machine —
    /// which is how a later answer finds the line it belongs to.
    pub id: Option<ActionId>,
}

impl LogEntry {
    /// Whether this worked. `false` for anything unsettled, so a caller
    /// that only wants "did it definitely work" still gets a safe
    /// answer — but the log pane uses [`Outcome::mark`], because
    /// collapsing the four back into two is the bug this replaced.
    pub fn ok(&self) -> bool {
        self.outcome == Outcome::Done
    }
}

/// An action bound for another machine.
///
/// **The cockpit does not submit it here.** A submission is an HTTP
/// request, and a peer that is asleep costs the connect timeout — five
/// seconds of a frozen terminal on a keystroke. It goes into an outbox
/// that the caller hands to the courier, whose whole job is being the
/// thread allowed to wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    /// Which machine to ask.
    pub peer: String,
    /// What to ask it to do.
    pub action: Action,
    /// The operator's confirmation, when one was given.
    pub confirmation: Option<Confirmation>,
}

/// Where a local row's actions go.
pub enum Destination {
    /// This machine, through an executor built from its own manifest.
    Local(Box<dyn ActionExecutor>),
    /// Nowhere: no work feed, or fixture mode.
    Nowhere(String),
}

impl Destination {
    /// A destination for an executor.
    pub fn local(executor: impl ActionExecutor + 'static) -> Self {
        Destination::Local(Box::new(executor))
    }
}

/// Which machine an action is aimed at.
///
/// **Not an index into one list.** Local rows sit at a stable position
/// in registry order, but a peer's rows arrive and leave as that machine
/// answers — so a peer's destination cannot be looked up by row number
/// without the number meaning something different one frame later. The
/// machine is named instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// This machine, at this row in registry order.
    Here(usize),
    /// Another machine, by name.
    On(String),
}

impl Target {
    /// The machine to name in a prompt, when it is not this one.
    fn peer(&self) -> Option<&str> {
        match self {
            Target::Here(_) => None,
            Target::On(peer) => Some(peer),
        }
    }
}

/// The cockpit's control surface: one pending question at most, an
/// executor per project, and a record of everything attempted.
///
/// The log is in-memory and dies with the process, consistent with a
/// cockpit that holds no state across runs. A durable audit trail is a
/// different thing and is not this.
#[derive(Default)]
pub struct Control {
    destinations: Vec<Destination>,
    /// The question on screen, and the machine its answer applies to.
    /// Held together so an answer cannot be delivered to a different
    /// machine than the one the question named.
    pending: Option<(Prompt, Target)>,
    log: Vec<LogEntry>,
    /// Actions bound for another machine, waiting to be handed to the
    /// courier. Drained by the caller that owns both.
    outbox: Vec<Submission>,
}

impl Control {
    /// A surface with one destination per project, in rail order.
    pub fn new(destinations: Vec<Destination>) -> Self {
        Self {
            destinations,
            pending: None,
            log: Vec::new(),
            outbox: Vec::new(),
        }
    }

    /// A surface that can do nothing, for a cockpit started against
    /// fixtures. Every action it is asked to perform is refused and
    /// says why, rather than appearing to work.
    pub fn inert(projects: usize) -> Self {
        Self::new(
            (0..projects)
                .map(|_| {
                    Destination::Nowhere(
                        "no executor: this project declares no work feed, or the cockpit is \
                         running against fixtures"
                            .to_string(),
                    )
                })
                .collect(),
        )
    }

    /// Takes everything bound for another machine.
    pub fn take_outbox(&mut self) -> Vec<Submission> {
        std::mem::take(&mut self.outbox)
    }

    /// The question on screen, if any.
    pub fn prompt(&self) -> Option<&Prompt> {
        self.pending.as_ref().map(|(prompt, _)| prompt)
    }

    /// Everything attempted this session, oldest first.
    pub fn log(&self) -> &[LogEntry] {
        &self.log
    }

    /// Offers an action. A reversible one is performed; one that needs
    /// confirming raises the prompt instead and performs nothing.
    ///
    /// This is the only entry point, so there is no path to an executor
    /// that skips the question.
    pub fn offer(&mut self, target: Target, action: Action) {
        match action.reversibility() {
            Reversibility::Reversible => self.perform(target, action, None),
            Reversibility::ConfirmationRequired => {
                // The operator is being asked to approve an action *and*
                // a destination, and only one of those is on the screen.
                let on = target.peer().map(str::to_string);
                let prompt = match &action {
                    // The one action no other action can undo. Typing
                    // the number is the strongest gate available, and a
                    // merge is where it belongs.
                    Action::MergePullRequest { number, .. } => {
                        Prompt::confirm_by_typing(action.clone(), number.to_string(), on.as_deref())
                    }
                    _ => Prompt::confirm(action.clone(), on.as_deref()),
                };
                self.pending = Some((prompt, target));
            }
        }
    }

    /// Raises a question whose answer completes the action — which
    /// label, which branch.
    pub fn ask(&mut self, target: Target, action: Action, question: impl Into<String>) {
        let on = target.peer().map(str::to_string);
        self.pending = Some((Prompt::ask_for(action, question, on.as_deref()), target));
    }

    /// Feeds a key to the pending prompt. `None` is Enter. Returns
    /// whether a prompt consumed it.
    pub fn key(&mut self, ch: Option<char>) -> bool {
        let Some((prompt, _)) = &mut self.pending else {
            return false;
        };
        match prompt.key(ch) {
            Answer::Waiting => {}
            Answer::Cancelled => {
                let summary = prompt.action().summary();
                self.pending = None;
                self.note(summary, "cancelled".to_string(), Outcome::Failed);
            }
            Answer::Confirmed(action) => {
                // The target travels with the question, so an answer
                // reaches the machine the operator was asked about even
                // if the cursor has moved since.
                let target = self.pending.take().map(|(_, target)| target);
                // The confirmation is built from the action that was
                // actually answered for, so `authorize`'s fingerprint
                // check is a real check rather than a ceremony.
                let confirmation = Confirmation::of(&action);
                if let Some(target) = target {
                    self.perform(target, action, Some(confirmation));
                }
            }
        }
        true
    }

    /// Abandons the pending question.
    pub fn cancel(&mut self) {
        if let Some((prompt, _)) = self.pending.take() {
            self.note(
                prompt.action().summary(),
                "cancelled".to_string(),
                Outcome::Failed,
            );
        }
    }

    fn perform(&mut self, target: Target, action: Action, confirmation: Option<Confirmation>) {
        let summary = action.summary();
        let row = match target {
            // Not sent here: see `Submission`. The log line waits for
            // the answer rather than claiming one, because "sent" is not
            // an outcome and an operator reading it as one is the whole
            // failure mode this arc is about.
            Target::On(peer) => {
                self.outbox.push(Submission {
                    peer: peer.clone(),
                    action,
                    confirmation,
                });
                self.note(summary, format!("offered to {peer}"), Outcome::Running);
                return;
            }
            Target::Here(row) => row,
        };
        match self.destinations.get_mut(row) {
            None => self.note(
                summary,
                "no executor for this row".to_string(),
                Outcome::Failed,
            ),
            Some(Destination::Nowhere(reason)) => {
                let reason = reason.clone();
                self.note(summary, reason, Outcome::Failed);
            }
            Some(Destination::Local(executor)) => {
                let result = authorize(&action, confirmation.as_ref())
                    .and_then(|authorized| executor.execute(authorized));
                match result {
                    Ok(outcome) => {
                        let effects = describe(&outcome.effects);
                        self.note(summary, effects, Outcome::Done)
                    }
                    Err(e) => self.note(summary, describe_error(&e), Outcome::Failed),
                }
            }
        }
    }

    /// Records what a peer said about a submission.
    ///
    /// The log line raised when the action was offered is updated in
    /// place. A second line would read as a second action, and an
    /// operator counting merges would count two.
    pub fn submitted(&mut self, summary: &str, outcome: Submitted) {
        let (result, state, id) = match outcome {
            Submitted::Accepted { id, .. } => {
                (format!("accepted as {id}"), Outcome::Running, Some(id))
            }
            Submitted::Refused { reason } => (reason, Outcome::Failed, None),
            Submitted::Unknown { id, reason } => (
                format!("unknown — {reason}. It may have happened."),
                Outcome::Unknown,
                Some(id),
            ),
        };
        self.update(summary, None, result, state, id);
    }

    /// Records what became of an action a peer accepted.
    pub fn resolved(&mut self, id: &ActionId, standing: Standing) {
        let (result, state) = match standing {
            Standing::Running => return,
            Standing::Done { summary } => (summary, Outcome::Done),
            Standing::Failed { reason } => (reason, Outcome::Failed),
            Standing::Refused { reason } => (reason, Outcome::Failed),
            // The one case where re-offering is safe, and saying so is
            // the useful half of the message.
            Standing::NeverArrived => (
                "never arrived — it did not run, and can be offered again".to_string(),
                Outcome::Failed,
            ),
            Standing::Unknown { reason } => (
                format!("unknown — {reason}. It may have happened."),
                Outcome::Unknown,
            ),
        };
        self.update("", Some(id), result, state, None);
    }

    /// Finds the line an answer belongs to and rewrites it.
    ///
    /// Matched by id where there is one, and otherwise by the most
    /// recent unfinished line with that summary — which is what a
    /// submission's own answer has, because it arrives before any id
    /// exists to match on.
    fn update(
        &mut self,
        summary: &str,
        id: Option<&ActionId>,
        result: String,
        outcome: Outcome,
        new_id: Option<ActionId>,
    ) {
        let found = self.log.iter_mut().rev().find(|e| match id {
            Some(id) => e.id.as_ref() == Some(id),
            None => e.summary == summary && e.outcome == Outcome::Running,
        });
        match found {
            Some(entry) => {
                entry.result = result;
                entry.outcome = outcome;
                if new_id.is_some() {
                    entry.id = new_id;
                }
            }
            // An answer with no line to update still gets one: losing it
            // would be a silent action, which is the thing the log
            // exists to prevent.
            None => self.log.push(LogEntry {
                summary: summary.to_string(),
                result,
                outcome,
                id: new_id.or_else(|| id.cloned()),
            }),
        }
    }

    /// Records that something was **not** attempted, and why.
    ///
    /// Distinct from an action that ran and failed: nothing was sent
    /// anywhere. It still goes in the log, because a keypress that
    /// silently does nothing is indistinguishable from one the cockpit
    /// never received.
    pub fn refuse(&mut self, summary: impl Into<String>, reason: impl Into<String>) {
        self.note(summary.into(), reason.into(), Outcome::Failed);
    }

    fn note(&mut self, summary: String, result: String, outcome: Outcome) {
        self.log.push(LogEntry {
            summary,
            result,
            outcome,
            id: None,
        });
    }
}

/// What an outcome did, for the log pane.
fn describe(effects: &[Effect]) -> String {
    if effects.is_empty() {
        return "done, no effects reported".to_string();
    }
    effects
        .iter()
        .map(|e| match e {
            Effect::WroteFile(p) => format!("wrote {}", p.display()),
            Effect::CalledApi { method, url } => format!("{method} {url}"),
            Effect::Spawned(what) => format!("spawned {what}"),
        })
        .collect::<Vec<_>>()
        .join("  ·  ")
}

/// Why an action did not happen. `ConfirmationRequired` reaching here
/// would be a bug in `offer`, and says so rather than reading as an
/// ordinary refusal.
fn describe_error(e: &ActionError) -> String {
    match e {
        ActionError::ConfirmationRequired { .. } => {
            "refused: reached an executor unconfirmed (this is a bug in the cockpit)".to_string()
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_baseline::actions::{ActionOutcome, Authorized};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Counts executions. A panicking executor would prove nothing: the
    /// question is whether it was reached at all.
    #[derive(Default)]
    struct Counting {
        calls: Arc<AtomicUsize>,
        performed: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl ActionExecutor for Counting {
        fn execute(&mut self, authorized: Authorized<'_>) -> Result<ActionOutcome, ActionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let summary = authorized.action().summary();
            self.performed.lock().unwrap().push(summary.clone());
            Ok(ActionOutcome {
                summary,
                effects: vec![Effect::CalledApi {
                    method: "PUT".into(),
                    url: "https://api.github.com/x".into(),
                }],
            })
        }
    }

    fn surface() -> (
        Control,
        Arc<AtomicUsize>,
        Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let performed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let executor = Counting {
            calls: calls.clone(),
            performed: performed.clone(),
        };
        (
            Control::new(vec![Destination::local(executor)]),
            calls,
            performed,
        )
    }

    fn merge(number: u64) -> Action {
        Action::MergePullRequest {
            project: "ttui".into(),
            number,
        }
    }

    #[test]
    fn a_reversible_action_is_performed_without_asking() {
        let (mut c, calls, _) = surface();
        c.offer(
            Target::Here(0),
            Action::RequestReReview {
                project: "ttui".into(),
                item: 36,
            },
        );
        assert!(c.prompt().is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(c.log()[0].ok());
    }

    /// The contract, asserted where the operator actually is: offering a
    /// confirmation-required action must reach nothing at all.
    #[test]
    fn an_action_needing_confirmation_performs_nothing_until_answered() {
        let (mut c, calls, _) = surface();
        c.offer(Target::Here(0), merge(36));
        assert!(c.prompt().is_some(), "no question was asked");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "the executor was reached before the operator answered"
        );
        assert!(
            c.log().is_empty(),
            "an unanswered action was logged as done"
        );
    }

    #[test]
    fn answering_a_merge_with_its_number_performs_it() {
        let (mut c, calls, performed) = surface();
        c.offer(Target::Here(0), merge(36));
        for ch in "36".chars() {
            c.key(Some(ch));
        }
        c.key(None);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(performed.lock().unwrap()[0].contains("36"));
        assert!(c.prompt().is_none(), "the question outlived its answer");
    }

    #[test]
    fn answering_a_merge_with_the_wrong_number_performs_nothing() {
        let (mut c, calls, _) = surface();
        c.offer(Target::Here(0), merge(36));
        for ch in "35".chars() {
            c.key(Some(ch));
        }
        c.key(None);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(c.log()[0].result, "cancelled");
        assert!(!c.log()[0].ok());
    }

    #[test]
    fn cancelling_performs_nothing_and_says_so() {
        let (mut c, calls, _) = surface();
        c.offer(
            Target::Here(0),
            Action::Push {
                project: "ttui".into(),
                branch: "main".into(),
            },
        );
        c.cancel();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(c.log()[0].result, "cancelled");
        assert!(c.prompt().is_none());
    }

    #[test]
    fn a_key_is_only_consumed_while_a_question_is_up() {
        let (mut c, _, _) = surface();
        assert!(!c.key(Some('j')), "a keypress was eaten with no prompt");
        c.offer(Target::Here(0), merge(36));
        assert!(c.key(Some('3')), "the prompt did not take the key");
    }

    /// An asked-for answer completes the action rather than confirming
    /// a half-built one.
    #[test]
    fn a_typed_label_reaches_the_executor_with_the_label_in_it() {
        let (mut c, _, performed) = surface();
        c.ask(
            Target::Here(0),
            Action::SetAutonomyLabel {
                project: "ttui".into(),
                item: 170,
                label: String::new(),
            },
            "label #170",
        );
        for ch in "semver:minor".chars() {
            c.key(Some(ch));
        }
        c.key(None);
        assert!(performed.lock().unwrap()[0].contains("semver:minor"));
    }

    /// Fixture mode, and any project with no work feed. Refusing out
    /// loud beats appearing to work.
    #[test]
    fn an_inert_surface_refuses_and_says_why() {
        let mut c = Control::inert(1);
        c.offer(
            Target::Here(0),
            Action::RequestReReview {
                project: "ttui".into(),
                item: 1,
            },
        );
        assert!(!c.log()[0].ok());
        assert!(c.log()[0].result.contains("no executor"));
    }

    #[test]
    fn every_attempt_is_logged_in_the_order_it_was_made() {
        let (mut c, _, _) = surface();
        c.offer(
            Target::Here(0),
            Action::RequestReReview {
                project: "p".into(),
                item: 1,
            },
        );
        c.offer(Target::Here(0), merge(2));
        c.cancel();
        let summaries: Vec<&str> = c.log().iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(summaries.len(), 2);
        assert!(summaries[0].contains("re-review"));
        assert!(summaries[1].contains("merge"));
    }
}
