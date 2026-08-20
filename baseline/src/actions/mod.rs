//! Control actions. Plain data plus a plain API, so every action is
//! available headless and the cockpit is only one caller. Each action is
//! classified by reversibility, and the irreversible group cannot reach
//! an executor without a `Confirmation` — enforced by the type system,
//! not by a convention.
//!
//! Split by responsibility: the action set here, the confirmation
//! contract in `confirm`, execution in `executor`, the serialized
//! contract in `wire`, and acting on another machine in `remote`. Every
//! type is re-exported, so no caller outside this module sees the split.
//!
//! **`remote` does not implement `ActionExecutor`, on purpose.** A local
//! call either happened or it did not, and `Result` says that exactly. A
//! submission that crossed a network has a third possibility, and the
//! two-valued shape has no room for it.

mod confirm;
mod executor;
mod github;
mod process;
mod remote;
pub mod wire;

pub use confirm::{authorize, fingerprint, ActionError, Authorized, Confirmation};
pub use executor::{
    ActionExecutor, ActionOutcome, Effect, LocalExecutor, ProcessControl, RecordingExecutor,
    WorkControl,
};
pub use github::GithubWorkControl;
pub use process::LocalProcessControl;
pub use remote::{RemoteExecutor, Standing, Submitted, ACTION_PATH};

use serde::{Deserialize, Serialize};

/// How a human disposed of a Plumb finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ruling {
    /// The finding stands.
    Upheld,
    /// The finding is overruled and suppressed in future runs.
    Overruled,
}

/// Something the operator can do to a registered project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum Action {
    /// Rule on a Plumb finding. The highest-leverage action there is:
    /// it is the one input Plumb's learned-rejection store depends on.
    RuleFinding {
        /// Which project's finding.
        project: String,
        /// The finding's fingerprint.
        fingerprint: String,
        /// Upheld or overruled.
        ruling: Ruling,
    },
    /// Set or change a work item's autonomy label.
    SetAutonomyLabel {
        /// Which project.
        project: String,
        /// The item's number.
        item: u64,
        /// The native label to apply.
        label: String,
    },
    /// Ask for a work item to be reviewed again.
    RequestReReview {
        /// Which project.
        project: String,
        /// The item's number.
        item: u64,
    },
    /// Trigger a capture run.
    TriggerCapture {
        /// Which project.
        project: String,
        /// A specific scenario, or every selected one.
        scenario: Option<String>,
    },
    /// Start an agent run against a work item.
    DispatchAgentRun {
        /// Which project.
        project: String,
        /// The item's number.
        item: u64,
        /// What the agent is being asked to do.
        prompt: String,
    },
    /// Stop a running agent. **Confirmation required.**
    StopAgentRun {
        /// Which project.
        project: String,
        /// The session's name.
        session: String,
    },
    /// Merge a pull request. **Confirmation required.**
    MergePullRequest {
        /// Which project.
        project: String,
        /// The pull request's number.
        number: u64,
    },
    /// Push a branch. **Confirmation required.**
    Push {
        /// Which project.
        project: String,
        /// The branch to push.
        branch: String,
    },
}

/// Whether an action can be taken back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reversibility {
    /// Additive or undoable; safe to take on a single keystroke.
    Reversible,
    /// Outward-facing or hard to undo; requires explicit confirmation.
    ConfirmationRequired,
}

impl Action {
    /// How reversible this action is. **The classification is the
    /// platform spec's, verbatim — do not reclassify an action here
    /// without the spec changing.**
    pub fn reversibility(&self) -> Reversibility {
        match self {
            Action::RuleFinding { .. }
            | Action::SetAutonomyLabel { .. }
            | Action::RequestReReview { .. }
            | Action::TriggerCapture { .. }
            | Action::DispatchAgentRun { .. } => Reversibility::Reversible,
            Action::StopAgentRun { .. } | Action::MergePullRequest { .. } | Action::Push { .. } => {
                Reversibility::ConfirmationRequired
            }
        }
    }

    /// Which project this action targets.
    pub fn project(&self) -> &str {
        match self {
            Action::RuleFinding { project, .. }
            | Action::SetAutonomyLabel { project, .. }
            | Action::RequestReReview { project, .. }
            | Action::TriggerCapture { project, .. }
            | Action::DispatchAgentRun { project, .. }
            | Action::StopAgentRun { project, .. }
            | Action::MergePullRequest { project, .. }
            | Action::Push { project, .. } => project,
        }
    }

    /// A one-line description naming the action and its target, so a
    /// confirmation prompt can quote exactly what is about to happen.
    pub fn summary(&self) -> String {
        match self {
            Action::RuleFinding {
                project,
                fingerprint,
                ruling,
            } => {
                format!("{project}: rule {fingerprint} as {ruling:?}")
            }
            Action::SetAutonomyLabel {
                project,
                item,
                label,
            } => {
                format!("{project}: label #{item} `{label}`")
            }
            Action::RequestReReview { project, item } => {
                format!("{project}: request re-review of #{item}")
            }
            Action::TriggerCapture { project, scenario } => match scenario {
                Some(s) => format!("{project}: capture scenario `{s}`"),
                None => format!("{project}: capture every selected scenario"),
            },
            Action::DispatchAgentRun { project, item, .. } => {
                format!("{project}: dispatch an agent run on #{item}")
            }
            Action::StopAgentRun { project, session } => {
                format!("{project}: stop the agent in session `{session}`")
            }
            Action::MergePullRequest { project, number } => {
                format!("{project}: merge pull request #{number}")
            }
            Action::Push { project, branch } => format!("{project}: push `{branch}`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn every_action() -> Vec<Action> {
        vec![
            Action::RuleFinding {
                project: "ttui".into(),
                fingerprint: "a1b2c3d4".into(),
                ruling: Ruling::Overruled,
            },
            Action::SetAutonomyLabel {
                project: "ttui".into(),
                item: 142,
                label: "gated".into(),
            },
            Action::RequestReReview {
                project: "ttui".into(),
                item: 142,
            },
            Action::TriggerCapture {
                project: "ttui".into(),
                scenario: Some("omnitrix-dial".into()),
            },
            Action::DispatchAgentRun {
                project: "ttui".into(),
                item: 140,
                prompt: "audit docs".into(),
            },
            Action::StopAgentRun {
                project: "ttui".into(),
                session: "widget-audit".into(),
            },
            Action::MergePullRequest {
                project: "ttui".into(),
                number: 142,
            },
            Action::Push {
                project: "ttui".into(),
                branch: "main".into(),
            },
        ]
    }

    /// The spec's reversible/additive group, item for item.
    #[test]
    fn the_reversible_group_matches_the_spec() {
        for action in &every_action()[..5] {
            assert_eq!(
                action.reversibility(),
                Reversibility::Reversible,
                "{}",
                action.summary()
            );
        }
    }

    /// The spec's confirmation-required group, item for item.
    #[test]
    fn the_confirmation_required_group_matches_the_spec() {
        for action in &every_action()[5..] {
            assert_eq!(
                action.reversibility(),
                Reversibility::ConfirmationRequired,
                "{}",
                action.summary()
            );
        }
    }

    #[test]
    fn every_action_names_the_project_it_targets() {
        for action in every_action() {
            assert_eq!(action.project(), "ttui");
        }
    }

    #[test]
    fn a_summary_names_the_action_and_its_target_so_a_confirmation_prompt_can_quote_it() {
        let merge = Action::MergePullRequest {
            project: "ttui".into(),
            number: 142,
        };
        let s = merge.summary();
        assert!(s.contains("merge"), "got {s}");
        assert!(s.contains("142"), "got {s}");
        assert!(s.contains("ttui"), "got {s}");
    }

    #[test]
    fn actions_round_trip_through_json_so_a_confirmation_can_be_fingerprinted() {
        for action in every_action() {
            let json = serde_json::to_string(&action).unwrap();
            let back: Action = serde_json::from_str(&json).unwrap();
            assert_eq!(action, back);
        }
    }
}
