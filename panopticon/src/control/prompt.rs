//! The one modal the cockpit has.
//!
//! The Cloister Bell is never modal, because it is information. A
//! confirmation is a question, and a question that can be answered by
//! accident is not one.

use parallax_baseline::actions::Action;

/// What the operator has to do to proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Press `y`. Anything else cancels.
    Yes,
    /// Type something and press Enter.
    ///
    /// `expect` names the exact string that will be accepted, when only
    /// one will do — a merge asks for the pull request's number, so an
    /// operator who confirmed while looking at a different row cannot
    /// merge the row they are looking at now by reflex.
    Type {
        /// What has been typed so far.
        typed: String,
        /// The only accepted answer, when there is one.
        expect: Option<String>,
    },
}

/// A question on screen, and the action waiting on the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The action as it stands. For a `Type` prompt with no `expect`,
    /// the typed text completes it — see `complete`.
    action: Action,
    /// The line shown to the operator.
    question: String,
    /// What answering looks like.
    ask: Ask,
}

/// What a keypress did to a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Still asking.
    Waiting,
    /// The operator declined, or typed the wrong thing.
    Cancelled,
    /// Perform this.
    Confirmed(Action),
}

impl Prompt {
    /// A yes/no question.
    pub fn confirm(action: Action) -> Self {
        let question = format!(
            "{} — press y to confirm, any other key cancels",
            action.summary()
        );
        Self {
            action,
            question,
            ask: Ask::Yes,
        }
    }

    /// A question answered by typing `expect` exactly.
    pub fn confirm_by_typing(action: Action, expect: impl Into<String>) -> Self {
        let expect = expect.into();
        let question = format!(
            "{} — type {expect} and press Enter, Esc cancels",
            action.summary()
        );
        Self {
            action,
            question,
            ask: Ask::Type {
                typed: String::new(),
                expect: Some(expect),
            },
        }
    }

    /// A question answered by typing anything, which then completes the
    /// action. Used where the action is not fully known until the
    /// operator says so — which label to apply, which branch to push.
    pub fn ask_for(action: Action, question: impl Into<String>) -> Self {
        Self {
            action,
            question: question.into(),
            ask: Ask::Type {
                typed: String::new(),
                expect: None,
            },
        }
    }

    /// The line to render.
    pub fn line(&self) -> String {
        match &self.ask {
            Ask::Yes => self.question.clone(),
            Ask::Type { typed, .. } => format!("{}: {typed}_", self.question),
        }
    }

    /// The action this prompt is about, before any typed completion.
    pub fn action(&self) -> &Action {
        &self.action
    }

    /// Feeds one key. `None` is Enter; `Some(c)` is a character key,
    /// with `\u{8}` for backspace. Escape is the caller's to handle: it
    /// cancels every prompt, so it never reaches here.
    pub fn key(&mut self, ch: Option<char>) -> Answer {
        match (&mut self.ask, ch) {
            (Ask::Yes, Some('y')) => Answer::Confirmed(self.action.clone()),
            (Ask::Yes, _) => Answer::Cancelled,
            (Ask::Type { typed, .. }, Some('\u{8}')) => {
                typed.pop();
                Answer::Waiting
            }
            (Ask::Type { typed, .. }, Some(c)) => {
                typed.push(c);
                Answer::Waiting
            }
            (Ask::Type { typed, expect }, None) => match expect {
                // Typed the wrong thing: cancelled, not re-asked. An
                // operator who has to guess twice is being trained to
                // stop reading the question.
                Some(want) if want != typed => Answer::Cancelled,
                Some(_) => Answer::Confirmed(self.action.clone()),
                None if typed.is_empty() => Answer::Cancelled,
                None => Answer::Confirmed(complete(&self.action, typed)),
            },
        }
    }
}

/// Fills the operator's answer into the action that asked for it.
fn complete(action: &Action, typed: &str) -> Action {
    match action.clone() {
        Action::SetAutonomyLabel { project, item, .. } => Action::SetAutonomyLabel {
            project,
            item,
            label: typed.to_string(),
        },
        Action::Push { project, .. } => Action::Push {
            project,
            branch: typed.to_string(),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge() -> Action {
        Action::MergePullRequest {
            project: "ttui".into(),
            number: 36,
        }
    }

    fn label() -> Action {
        Action::SetAutonomyLabel {
            project: "ttui".into(),
            item: 170,
            label: String::new(),
        }
    }

    #[test]
    fn y_confirms_and_anything_else_does_not() {
        assert_eq!(
            Prompt::confirm(merge()).key(Some('y')),
            Answer::Confirmed(merge())
        );
        for key in ['n', 'Y', ' ', 'q'] {
            assert_eq!(
                Prompt::confirm(merge()).key(Some(key)),
                Answer::Cancelled,
                "`{key}` answered a question it was not asked"
            );
        }
    }

    #[test]
    fn a_typed_confirmation_needs_the_exact_string() {
        let mut p = Prompt::confirm_by_typing(merge(), "36");
        assert_eq!(p.key(Some('3')), Answer::Waiting);
        assert_eq!(p.key(Some('6')), Answer::Waiting);
        assert_eq!(p.key(None), Answer::Confirmed(merge()));
    }

    /// The scenario the typed gate exists for: the operator confirmed
    /// while looking at one row and answered for another.
    #[test]
    fn a_typed_confirmation_with_the_wrong_number_cancels() {
        let mut p = Prompt::confirm_by_typing(merge(), "36");
        for c in "35".chars() {
            p.key(Some(c));
        }
        assert_eq!(p.key(None), Answer::Cancelled);
    }

    #[test]
    fn backspace_takes_back_a_character() {
        let mut p = Prompt::confirm_by_typing(merge(), "36");
        for c in ['3', '9'] {
            p.key(Some(c));
        }
        p.key(Some('\u{8}'));
        p.key(Some('6'));
        assert_eq!(p.key(None), Answer::Confirmed(merge()));
    }

    #[test]
    fn typing_a_label_completes_the_action_with_it() {
        let mut p = Prompt::ask_for(label(), "label #170");
        for c in "semver:minor".chars() {
            p.key(Some(c));
        }
        assert_eq!(
            p.key(None),
            Answer::Confirmed(Action::SetAutonomyLabel {
                project: "ttui".into(),
                item: 170,
                label: "semver:minor".into(),
            })
        );
    }

    /// An empty answer is not an answer. Applying a label named `""`
    /// because Enter was pressed twice is not what anybody meant.
    #[test]
    fn an_empty_answer_cancels() {
        let mut p = Prompt::ask_for(label(), "label #170");
        assert_eq!(p.key(None), Answer::Cancelled);
    }

    #[test]
    fn the_line_shows_what_is_being_asked_and_what_has_been_typed() {
        let mut p = Prompt::confirm_by_typing(merge(), "36");
        p.key(Some('3'));
        let line = p.line();
        assert!(line.contains("merge"), "{line}");
        assert!(line.contains("36"), "{line}");
        assert!(
            line.ends_with("3_"),
            "the caret does not follow the text: {line}"
        );
    }
}
