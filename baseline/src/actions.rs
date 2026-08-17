//! Control actions. Plain data plus a plain API, so every action is
//! available headless and the cockpit is only one caller. Each action is
//! classified by reversibility, and the irreversible group cannot reach
//! an executor without a `Confirmation` — enforced by the type system,
//! not by a convention.

use crate::adapters::AdapterError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

/// A stable short fingerprint of exactly this action, including its
/// arguments. Confirming "merge #12" therefore cannot authorize
/// "merge #99".
pub fn fingerprint(action: &Action) -> String {
    let canonical = serde_json::to_string(action).unwrap_or_else(|_| format!("{action:?}"));
    let digest = Sha256::digest(canonical.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Proof that a caller saw and approved one specific action.
///
/// The only constructor is `Confirmation::of`, which takes the action
/// itself — so a confirmation cannot be conjured from a bare `true`,
/// and cannot be reused for a different action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    fingerprint: String,
}

impl Confirmation {
    /// Confirms this exact action.
    pub fn of(action: &Action) -> Self {
        Self {
            fingerprint: fingerprint(action),
        }
    }

    /// The confirmed action's fingerprint.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

/// An action cleared to execute. **Its field is private**, so the only
/// way to obtain one is `authorize` — which means no executor can be
/// reached without passing the confirmation check.
///
/// ```compile_fail
/// use parallax_baseline::actions::{Action, Authorized};
/// let action = Action::Push { project: "ttui".into(), branch: "main".into() };
/// // `Authorized`'s field is private, so this does not compile — the
/// // only way to obtain one is `authorize`.
/// let sneaky = Authorized { action: &action };
/// ```
#[derive(Debug)]
pub struct Authorized<'a> {
    action: &'a Action,
}

impl Authorized<'_> {
    /// The action this authorizes.
    pub fn action(&self) -> &Action {
        self.action
    }
}

/// Why an action did not happen.
#[derive(Debug)]
pub enum ActionError {
    /// The action needs confirmation and none was given.
    ConfirmationRequired {
        /// What was refused, quoted for the caller.
        summary: String,
    },
    /// A confirmation was given, but for a different action.
    ConfirmationMismatch {
        /// The fingerprint the action needed.
        expected: String,
        /// The fingerprint the confirmation carried.
        got: String,
    },
    /// The action reached its side effect and that failed.
    Adapter(AdapterError),
    /// This executor cannot perform this action.
    NotSupported(String),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionError::ConfirmationRequired { summary } => {
                write!(f, "refused without confirmation: {summary}")
            }
            ActionError::ConfirmationMismatch { expected, got } => {
                write!(f, "confirmation is for {got}, not {expected}")
            }
            ActionError::Adapter(e) => write!(f, "action failed: {e}"),
            ActionError::NotSupported(m) => write!(f, "unsupported action: {m}"),
        }
    }
}
impl std::error::Error for ActionError {}

impl From<AdapterError> for ActionError {
    fn from(e: AdapterError) -> Self {
        ActionError::Adapter(e)
    }
}

/// Checks an action against its confirmation requirement.
///
/// A confirmation that names a different action is refused whether or
/// not the action needed one — a caller that confused two actions has a
/// bug, and silently proceeding would hide it.
pub fn authorize<'a>(
    action: &'a Action,
    confirmation: Option<&Confirmation>,
) -> Result<Authorized<'a>, ActionError> {
    let expected = fingerprint(action);
    match confirmation {
        Some(c) if c.fingerprint() != expected => Err(ActionError::ConfirmationMismatch {
            expected,
            got: c.fingerprint().to_string(),
        }),
        Some(_) => Ok(Authorized { action }),
        None => match action.reversibility() {
            Reversibility::Reversible => Ok(Authorized { action }),
            Reversibility::ConfirmationRequired => Err(ActionError::ConfirmationRequired {
                summary: action.summary(),
            }),
        },
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

#[cfg(test)]
mod confirmation_tests {
    use super::*;

    fn merge(number: u64) -> Action {
        Action::MergePullRequest {
            project: "ttui".into(),
            number,
        }
    }

    fn rule() -> Action {
        Action::RuleFinding {
            project: "ttui".into(),
            fingerprint: "a1b2c3d4".into(),
            ruling: Ruling::Overruled,
        }
    }

    #[test]
    fn a_reversible_action_authorizes_with_no_confirmation() {
        let action = rule();
        assert!(authorize(&action, None).is_ok());
    }

    /// The spec's fourth Verification bullet, directly.
    #[test]
    fn a_confirmation_required_action_refuses_to_authorize_unconfirmed() {
        for action in [
            merge(142),
            Action::Push {
                project: "ttui".into(),
                branch: "main".into(),
            },
            Action::StopAgentRun {
                project: "ttui".into(),
                session: "s".into(),
            },
        ] {
            match authorize(&action, None) {
                Err(ActionError::ConfirmationRequired { summary }) => {
                    assert_eq!(
                        summary,
                        action.summary(),
                        "the refusal quotes what was refused"
                    );
                }
                other => panic!("expected refusal for {}, got {other:?}", action.summary()),
            }
        }
    }

    #[test]
    fn a_confirmation_required_action_authorizes_with_its_own_confirmation() {
        let action = merge(142);
        let confirmation = Confirmation::of(&action);
        assert!(authorize(&action, Some(&confirmation)).is_ok());
    }

    /// Confirming one merge must not authorize a different one — this is
    /// what a bare `confirmed: bool` cannot express.
    #[test]
    fn a_confirmation_for_a_different_action_is_refused() {
        let confirmation = Confirmation::of(&merge(12));
        match authorize(&merge(99), Some(&confirmation)) {
            Err(ActionError::ConfirmationMismatch { expected, got }) => {
                assert_eq!(expected, fingerprint(&merge(99)));
                assert_eq!(got, fingerprint(&merge(12)));
            }
            other => panic!("expected a mismatch, got {other:?}"),
        }
    }

    #[test]
    fn a_stray_confirmation_on_a_reversible_action_is_harmless_when_it_matches() {
        let action = rule();
        assert!(authorize(&action, Some(&Confirmation::of(&action))).is_ok());
    }

    /// A confirmation naming the wrong action is refused even when the
    /// action would not have needed one — a caller that confused two
    /// actions has a bug worth surfacing.
    #[test]
    fn a_mismatched_confirmation_on_a_reversible_action_is_still_refused() {
        assert!(matches!(
            authorize(&rule(), Some(&Confirmation::of(&merge(12)))),
            Err(ActionError::ConfirmationMismatch { .. })
        ));
    }

    #[test]
    fn a_fingerprint_is_stable_for_the_same_action_and_differs_between_actions() {
        assert_eq!(fingerprint(&merge(142)), fingerprint(&merge(142)));
        assert_ne!(fingerprint(&merge(142)), fingerprint(&merge(143)));
        assert_ne!(fingerprint(&merge(142)), fingerprint(&rule()));
        assert_eq!(fingerprint(&merge(142)).len(), 16);
    }

    #[test]
    fn an_authorized_action_still_names_what_it_authorizes() {
        let action = merge(142);
        let authorized = authorize(&action, Some(&Confirmation::of(&action))).unwrap();
        assert_eq!(authorized.action(), &action);
    }
}
