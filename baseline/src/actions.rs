//! Control actions. Plain data plus a plain API, so every action is
//! available headless and the cockpit is only one caller. Each action is
//! classified by reversibility, and the irreversible group cannot reach
//! an executor without a `Confirmation` — enforced by the type system,
//! not by a convention.

use crate::adapters::AdapterError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;

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

/// A side effect an action actually had, for reporting and dry runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// A file was written or appended to.
    WroteFile(PathBuf),
    /// A remote API was called.
    CalledApi {
        /// The HTTP method.
        method: String,
        /// The URL called.
        url: String,
    },
    /// A process or agent run was started or stopped.
    Spawned(String),
}

/// What an action did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    /// A one-line description of what happened.
    pub summary: String,
    /// The side effects it had.
    pub effects: Vec<Effect>,
}

/// Something that can perform an authorized action.
///
/// The parameter is `Authorized`, not `&Action`, so an executor cannot
/// be handed an unauthorized action — "did you check confirmation?" is
/// not a question an implementer can get wrong.
pub trait ActionExecutor {
    /// Performs the action.
    fn execute(&mut self, authorized: Authorized<'_>) -> Result<ActionOutcome, ActionError>;
}

/// An executor that records what it was asked to do and does nothing.
/// For dry runs, for tests, and for a frontend previewing a batch.
#[derive(Debug, Default)]
pub struct RecordingExecutor {
    executed: Vec<Action>,
}

impl RecordingExecutor {
    /// A fresh recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every action it was asked to perform, in order.
    pub fn executed(&self) -> &[Action] {
        &self.executed
    }
}

impl ActionExecutor for RecordingExecutor {
    fn execute(&mut self, authorized: Authorized<'_>) -> Result<ActionOutcome, ActionError> {
        let action = authorized.action();
        self.executed.push(action.clone());
        Ok(ActionOutcome {
            summary: action.summary(),
            effects: Vec::new(),
        })
    }
}

/// The work-side effects an executor needs. Separated from the executor
/// so the GitHub calls stay real-external-service exempt while every
/// decision above them is tested.
pub trait WorkControl {
    /// Adds a label to a work item.
    fn set_label(&mut self, repo: &str, item: u64, label: &str) -> Result<(), AdapterError>;
    /// Requests a fresh review of a work item.
    fn request_review(&mut self, repo: &str, item: u64) -> Result<(), AdapterError>;
    /// Merges a pull request.
    fn merge(&mut self, repo: &str, number: u64) -> Result<(), AdapterError>;
}

/// The process-side effects an executor needs.
pub trait ProcessControl {
    /// Triggers a capture run.
    fn capture(&mut self, project: &str, scenario: Option<&str>) -> Result<(), AdapterError>;
    /// Starts an agent run, returning the new session's name.
    fn dispatch(&mut self, project: &str, item: u64, prompt: &str) -> Result<String, AdapterError>;
    /// Stops a running agent.
    fn stop(&mut self, session: &str) -> Result<(), AdapterError>;
    /// Pushes a branch.
    fn push(&mut self, project: &str, branch: &str) -> Result<(), AdapterError>;
}

/// Performs actions against one project: rulings on disk, work items
/// through `WorkControl`, runs through `ProcessControl`.
pub struct LocalExecutor<W: WorkControl, P: ProcessControl> {
    repo: String,
    rulings_path: PathBuf,
    work: W,
    process: P,
}

impl<W: WorkControl, P: ProcessControl> LocalExecutor<W, P> {
    /// An executor for `repo`, appending rulings to `rulings_path`.
    pub fn new(
        repo: impl Into<String>,
        rulings_path: impl Into<PathBuf>,
        work: W,
        process: P,
    ) -> Self {
        Self {
            repo: repo.into(),
            rulings_path: rulings_path.into(),
            work,
            process,
        }
    }

    /// The work-side control, for asserting what it was asked to do.
    pub fn work(&self) -> &W {
        &self.work
    }

    /// The process-side control, for asserting what it was asked to do.
    pub fn process(&self) -> &P {
        &self.process
    }

    /// Appends one ruling record as a JSON line.
    fn append_ruling(
        &self,
        project: &str,
        finger: &str,
        ruling: Ruling,
    ) -> Result<(), AdapterError> {
        if let Some(parent) = self.rulings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let record = serde_json::json!({
            "project": project,
            "fingerprint": finger,
            "ruling": ruling,
        });
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.rulings_path)?;
        writeln!(file, "{record}")?;
        Ok(())
    }
}

impl<W: WorkControl, P: ProcessControl> ActionExecutor for LocalExecutor<W, P> {
    fn execute(&mut self, authorized: Authorized<'_>) -> Result<ActionOutcome, ActionError> {
        let action = authorized.action();
        let summary = action.summary();
        let effects = match action {
            Action::RuleFinding {
                project,
                fingerprint,
                ruling,
            } => {
                self.append_ruling(project, fingerprint, *ruling)?;
                vec![Effect::WroteFile(self.rulings_path.clone())]
            }
            Action::SetAutonomyLabel { item, label, .. } => {
                self.work.set_label(&self.repo, *item, label)?;
                vec![Effect::CalledApi {
                    method: "POST".into(),
                    url: format!("repos/{}/issues/{item}/labels", self.repo),
                }]
            }
            Action::RequestReReview { item, .. } => {
                self.work.request_review(&self.repo, *item)?;
                vec![Effect::CalledApi {
                    method: "POST".into(),
                    url: format!("repos/{}/pulls/{item}/requested_reviewers", self.repo),
                }]
            }
            Action::TriggerCapture { project, scenario } => {
                self.process.capture(project, scenario.as_deref())?;
                vec![Effect::Spawned("capture".into())]
            }
            Action::DispatchAgentRun {
                project,
                item,
                prompt,
            } => {
                let session = self.process.dispatch(project, *item, prompt)?;
                return Ok(ActionOutcome {
                    summary: format!("{summary} (session `{session}`)"),
                    effects: vec![Effect::Spawned(session)],
                });
            }
            // The confirmation-required arms land in Task 24.
            other => return Err(ActionError::NotSupported(other.summary())),
        };
        Ok(ActionOutcome { summary, effects })
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

#[cfg(test)]
mod executor_tests {
    use super::*;

    #[derive(Default)]
    pub(super) struct FakeWork {
        pub calls: Vec<String>,
    }

    impl WorkControl for FakeWork {
        fn set_label(&mut self, repo: &str, item: u64, label: &str) -> Result<(), AdapterError> {
            self.calls.push(format!("label {repo}#{item} {label}"));
            Ok(())
        }
        fn request_review(&mut self, repo: &str, item: u64) -> Result<(), AdapterError> {
            self.calls.push(format!("re-review {repo}#{item}"));
            Ok(())
        }
        fn merge(&mut self, repo: &str, number: u64) -> Result<(), AdapterError> {
            self.calls.push(format!("merge {repo}#{number}"));
            Ok(())
        }
    }

    #[derive(Default)]
    pub(super) struct FakeProcess {
        pub calls: Vec<String>,
    }

    impl ProcessControl for FakeProcess {
        fn capture(&mut self, project: &str, scenario: Option<&str>) -> Result<(), AdapterError> {
            self.calls.push(format!("capture {project} {scenario:?}"));
            Ok(())
        }
        fn dispatch(
            &mut self,
            project: &str,
            item: u64,
            _prompt: &str,
        ) -> Result<String, AdapterError> {
            self.calls.push(format!("dispatch {project}#{item}"));
            Ok("session-1".into())
        }
        fn stop(&mut self, session: &str) -> Result<(), AdapterError> {
            self.calls.push(format!("stop {session}"));
            Ok(())
        }
        fn push(&mut self, project: &str, branch: &str) -> Result<(), AdapterError> {
            self.calls.push(format!("push {project} {branch}"));
            Ok(())
        }
    }

    pub(super) fn local(dir: &std::path::Path) -> LocalExecutor<FakeWork, FakeProcess> {
        LocalExecutor::new(
            "tatemeyer/ttui",
            dir.join(".plumb/rulings.jsonl"),
            FakeWork::default(),
            FakeProcess::default(),
        )
    }

    fn run(
        executor: &mut impl ActionExecutor,
        action: &Action,
    ) -> Result<ActionOutcome, ActionError> {
        let authorized = authorize(action, None)?;
        executor.execute(authorized)
    }

    #[test]
    fn a_recording_executor_records_without_side_effects() {
        let mut executor = RecordingExecutor::new();
        let action = Action::RequestReReview {
            project: "ttui".into(),
            item: 142,
        };
        let outcome = run(&mut executor, &action).unwrap();
        assert_eq!(executor.executed(), std::slice::from_ref(&action));
        assert!(outcome.effects.is_empty(), "a dry run has no effects");
        assert_eq!(outcome.summary, action.summary());
    }

    #[test]
    fn ruling_on_a_finding_appends_one_record_to_the_rulings_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        let action = Action::RuleFinding {
            project: "ttui".into(),
            fingerprint: "a1b2c3d4".into(),
            ruling: Ruling::Overruled,
        };
        let outcome = run(&mut executor, &action).unwrap();

        let text = std::fs::read_to_string(dir.path().join(".plumb/rulings.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("a1b2c3d4") && text.contains("overruled"));
        assert!(matches!(outcome.effects[0], Effect::WroteFile(_)));
    }

    #[test]
    fn ruling_twice_appends_rather_than_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        for fp in ["aaaa", "bbbb"] {
            let action = Action::RuleFinding {
                project: "ttui".into(),
                fingerprint: fp.into(),
                ruling: Ruling::Upheld,
            };
            run(&mut executor, &action).unwrap();
        }
        let text = std::fs::read_to_string(dir.path().join(".plumb/rulings.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn setting_a_label_and_requesting_a_re_review_go_through_work_control() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        run(
            &mut executor,
            &Action::SetAutonomyLabel {
                project: "ttui".into(),
                item: 142,
                label: "gated".into(),
            },
        )
        .unwrap();
        run(
            &mut executor,
            &Action::RequestReReview {
                project: "ttui".into(),
                item: 142,
            },
        )
        .unwrap();
        assert_eq!(
            executor.work().calls,
            vec![
                "label tatemeyer/ttui#142 gated".to_string(),
                "re-review tatemeyer/ttui#142".to_string()
            ]
        );
    }

    #[test]
    fn triggering_a_capture_and_dispatching_a_run_go_through_process_control() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        run(
            &mut executor,
            &Action::TriggerCapture {
                project: "ttui".into(),
                scenario: Some("dial".into()),
            },
        )
        .unwrap();
        let outcome = run(
            &mut executor,
            &Action::DispatchAgentRun {
                project: "ttui".into(),
                item: 140,
                prompt: "audit".into(),
            },
        )
        .unwrap();
        assert_eq!(executor.process().calls.len(), 2);
        assert!(
            outcome.summary.contains("session-1"),
            "the new session is named back to the caller"
        );
    }

    #[test]
    fn a_side_effect_that_fails_surfaces_as_an_action_error_rather_than_a_silent_success() {
        struct FailingWork;
        impl WorkControl for FailingWork {
            fn set_label(&mut self, _r: &str, _i: u64, _l: &str) -> Result<(), AdapterError> {
                Err(AdapterError::Http {
                    status: 422,
                    message: "label does not exist".into(),
                })
            }
            fn request_review(&mut self, _r: &str, _i: u64) -> Result<(), AdapterError> {
                Ok(())
            }
            fn merge(&mut self, _r: &str, _n: u64) -> Result<(), AdapterError> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut executor = LocalExecutor::new(
            "tatemeyer/ttui",
            dir.path().join("r.jsonl"),
            FailingWork,
            FakeProcess::default(),
        );
        let action = Action::SetAutonomyLabel {
            project: "ttui".into(),
            item: 1,
            label: "nope".into(),
        };
        let err = run(&mut executor, &action).unwrap_err().to_string();
        assert!(err.contains("422"), "got {err}");
    }
}
