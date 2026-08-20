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

use parallax_baseline::actions::{
    authorize, Action, ActionError, ActionExecutor, Confirmation, Effect, Reversibility,
};
use prompt::{Answer, Prompt};

/// One attempted action and what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// What was attempted, in the action's own words.
    pub summary: String,
    /// What happened, rendered for the log pane.
    pub result: String,
    /// Whether it worked.
    pub ok: bool,
}

/// The cockpit's control surface: one pending question at most, an
/// executor per project, and a record of everything attempted.
///
/// The log is in-memory and dies with the process, consistent with a
/// cockpit that holds no state across runs. A durable audit trail is a
/// different thing and is not this.
#[derive(Default)]
pub struct Control {
    executors: Vec<Option<Box<dyn ActionExecutor>>>,
    pending: Option<Prompt>,
    log: Vec<LogEntry>,
}

impl Control {
    /// A surface with one executor slot per project, in rail order.
    /// `None` for a project that has no work feed to address.
    pub fn new(executors: Vec<Option<Box<dyn ActionExecutor>>>) -> Self {
        Self {
            executors,
            pending: None,
            log: Vec::new(),
        }
    }

    /// A surface that can do nothing, for a cockpit started against
    /// fixtures. Every action it is asked to perform is refused and
    /// says why, rather than appearing to work.
    pub fn inert(projects: usize) -> Self {
        Self::new((0..projects).map(|_| None).collect())
    }

    /// The question on screen, if any.
    pub fn prompt(&self) -> Option<&Prompt> {
        self.pending.as_ref()
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
    pub fn offer(&mut self, project: usize, action: Action) {
        match action.reversibility() {
            Reversibility::Reversible => self.perform(project, action, None),
            Reversibility::ConfirmationRequired => {
                self.pending = Some(match &action {
                    // The one action no other action can undo. Typing
                    // the number is the strongest gate available, and a
                    // merge is where it belongs.
                    Action::MergePullRequest { number, .. } => {
                        Prompt::confirm_by_typing(action.clone(), number.to_string())
                    }
                    _ => Prompt::confirm(action.clone()),
                });
            }
        }
    }

    /// Raises a question whose answer completes the action — which
    /// label, which branch.
    pub fn ask(&mut self, action: Action, question: impl Into<String>) {
        self.pending = Some(Prompt::ask_for(action, question));
    }

    /// Feeds a key to the pending prompt. `None` is Enter. Returns
    /// whether a prompt consumed it.
    pub fn key(&mut self, project: usize, ch: Option<char>) -> bool {
        let Some(prompt) = &mut self.pending else {
            return false;
        };
        match prompt.key(ch) {
            Answer::Waiting => {}
            Answer::Cancelled => {
                let summary = prompt.action().summary();
                self.pending = None;
                self.note(summary, "cancelled".to_string(), false);
            }
            Answer::Confirmed(action) => {
                self.pending = None;
                // The confirmation is built from the action that was
                // actually answered for, so `authorize`'s fingerprint
                // check is a real check rather than a ceremony.
                let confirmation = Confirmation::of(&action);
                self.perform(project, action, Some(confirmation));
            }
        }
        true
    }

    /// Abandons the pending question.
    pub fn cancel(&mut self) {
        if let Some(prompt) = self.pending.take() {
            self.note(prompt.action().summary(), "cancelled".to_string(), false);
        }
    }

    fn perform(&mut self, project: usize, action: Action, confirmation: Option<Confirmation>) {
        let summary = action.summary();
        let Some(Some(executor)) = self.executors.get_mut(project) else {
            self.note(
                summary,
                "no executor: this project declares no work feed, or the cockpit is running \
                 against fixtures"
                    .to_string(),
                false,
            );
            return;
        };
        let result = authorize(&action, confirmation.as_ref())
            .and_then(|authorized| executor.execute(authorized));
        match result {
            Ok(outcome) => {
                let effects = describe(&outcome.effects);
                self.note(summary, effects, true)
            }
            Err(e) => self.note(summary, describe_error(&e), false),
        }
    }

    fn note(&mut self, summary: String, result: String, ok: bool) {
        self.log.push(LogEntry {
            summary,
            result,
            ok,
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
            Control::new(vec![Some(Box::new(executor))]),
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
            0,
            Action::RequestReReview {
                project: "ttui".into(),
                item: 36,
            },
        );
        assert!(c.prompt().is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(c.log()[0].ok);
    }

    /// The contract, asserted where the operator actually is: offering a
    /// confirmation-required action must reach nothing at all.
    #[test]
    fn an_action_needing_confirmation_performs_nothing_until_answered() {
        let (mut c, calls, _) = surface();
        c.offer(0, merge(36));
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
        c.offer(0, merge(36));
        for ch in "36".chars() {
            c.key(0, Some(ch));
        }
        c.key(0, None);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(performed.lock().unwrap()[0].contains("36"));
        assert!(c.prompt().is_none(), "the question outlived its answer");
    }

    #[test]
    fn answering_a_merge_with_the_wrong_number_performs_nothing() {
        let (mut c, calls, _) = surface();
        c.offer(0, merge(36));
        for ch in "35".chars() {
            c.key(0, Some(ch));
        }
        c.key(0, None);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(c.log()[0].result, "cancelled");
        assert!(!c.log()[0].ok);
    }

    #[test]
    fn cancelling_performs_nothing_and_says_so() {
        let (mut c, calls, _) = surface();
        c.offer(
            0,
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
        assert!(!c.key(0, Some('j')), "a keypress was eaten with no prompt");
        c.offer(0, merge(36));
        assert!(c.key(0, Some('3')), "the prompt did not take the key");
    }

    /// An asked-for answer completes the action rather than confirming
    /// a half-built one.
    #[test]
    fn a_typed_label_reaches_the_executor_with_the_label_in_it() {
        let (mut c, _, performed) = surface();
        c.ask(
            Action::SetAutonomyLabel {
                project: "ttui".into(),
                item: 170,
                label: String::new(),
            },
            "label #170",
        );
        for ch in "semver:minor".chars() {
            c.key(0, Some(ch));
        }
        c.key(0, None);
        assert!(performed.lock().unwrap()[0].contains("semver:minor"));
    }

    /// Fixture mode, and any project with no work feed. Refusing out
    /// loud beats appearing to work.
    #[test]
    fn an_inert_surface_refuses_and_says_why() {
        let mut c = Control::inert(1);
        c.offer(
            0,
            Action::RequestReReview {
                project: "ttui".into(),
                item: 1,
            },
        );
        assert!(!c.log()[0].ok);
        assert!(c.log()[0].result.contains("no executor"));
    }

    #[test]
    fn every_attempt_is_logged_in_the_order_it_was_made() {
        let (mut c, _, _) = surface();
        c.offer(
            0,
            Action::RequestReReview {
                project: "p".into(),
                item: 1,
            },
        );
        c.offer(0, merge(2));
        c.cancel();
        let summaries: Vec<&str> = c.log().iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(summaries.len(), 2);
        assert!(summaries[0].contains("re-review"));
        assert!(summaries[1].contains("merge"));
    }
}
