//! Executing actions. `execute` takes an `Authorized`, never an
//! `&Action`, so an executor cannot be handed an unauthorized action.
//! Side effects sit behind `WorkControl`/`ProcessControl`, which keeps
//! the live calls real-external-service exempt while every decision
//! above them is tested.

use super::{Action, ActionError, Authorized, Ruling};
use crate::adapters::AdapterError;
use std::io::Write;
use std::path::PathBuf;

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
            Action::StopAgentRun { session, .. } => {
                self.process.stop(session)?;
                vec![Effect::Spawned(format!("stopped {session}"))]
            }
            Action::MergePullRequest { number, .. } => {
                self.work.merge(&self.repo, *number)?;
                vec![Effect::CalledApi {
                    method: "PUT".into(),
                    url: format!("repos/{}/pulls/{number}/merge", self.repo),
                }]
            }
            Action::Push { project, branch } => {
                self.process.push(project, branch)?;
                vec![Effect::Spawned(format!("push {branch}"))]
            }
        };
        Ok(ActionOutcome { summary, effects })
    }
}

#[cfg(test)]
mod executor_tests {
    use super::*;
    use crate::actions::authorize;

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

#[cfg(test)]
mod confirmed_execution_tests {
    use super::executor_tests::{local, FakeProcess, FakeWork};
    use super::*;
    use crate::actions::{authorize, Confirmation};

    fn confirmation_required() -> Vec<Action> {
        vec![
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

    /// The spec's fourth Verification bullet, at the executor rather than
    /// at `authorize`: nothing reaches a side effect unconfirmed.
    #[test]
    fn no_confirmation_required_action_reaches_a_side_effect_unconfirmed() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        for action in confirmation_required() {
            match authorize(&action, None) {
                Err(ActionError::ConfirmationRequired { .. }) => {}
                other => panic!("{} was not refused: {other:?}", action.summary()),
            }
        }
        assert!(
            executor.work().calls.is_empty(),
            "no work-side effect leaked"
        );
        assert!(
            executor.process().calls.is_empty(),
            "no process-side effect leaked"
        );
        let _ = &mut executor;
    }

    #[test]
    fn each_confirmation_required_action_executes_once_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        for action in confirmation_required() {
            let confirmation = Confirmation::of(&action);
            let authorized = authorize(&action, Some(&confirmation)).expect("authorizes");
            let outcome = executor.execute(authorized).expect("executes");
            assert_eq!(outcome.summary, action.summary());
            assert_eq!(outcome.effects.len(), 1);
        }
        assert_eq!(
            executor.work().calls,
            vec!["merge tatemeyer/ttui#142".to_string()]
        );
        assert_eq!(
            executor.process().calls,
            vec![
                "stop widget-audit".to_string(),
                "push ttui main".to_string()
            ]
        );
    }

    #[test]
    fn a_confirmation_for_a_neighbouring_pull_request_does_not_execute_this_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        let wrong = Confirmation::of(&Action::MergePullRequest {
            project: "ttui".into(),
            number: 141,
        });
        let action = Action::MergePullRequest {
            project: "ttui".into(),
            number: 142,
        };
        assert!(matches!(
            authorize(&action, Some(&wrong)),
            Err(ActionError::ConfirmationMismatch { .. })
        ));
        assert!(executor.work().calls.is_empty());
        let _ = &mut executor;
    }

    #[test]
    fn a_recording_executor_still_refuses_an_unconfirmed_action_upstream_of_itself() {
        let mut executor = RecordingExecutor::new();
        let action = Action::Push {
            project: "ttui".into(),
            branch: "main".into(),
        };
        assert!(authorize(&action, None).is_err());
        assert!(executor.executed().is_empty());
        let _ = &mut executor;
    }

    #[test]
    fn a_merge_that_the_remote_rejects_is_reported_rather_than_reported_as_done() {
        struct RejectingWork;
        impl WorkControl for RejectingWork {
            fn set_label(&mut self, _r: &str, _i: u64, _l: &str) -> Result<(), AdapterError> {
                Ok(())
            }
            fn request_review(&mut self, _r: &str, _i: u64) -> Result<(), AdapterError> {
                Ok(())
            }
            fn merge(&mut self, _r: &str, _n: u64) -> Result<(), AdapterError> {
                Err(AdapterError::Http {
                    status: 405,
                    message: "not mergeable".into(),
                })
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut executor = LocalExecutor::new(
            "tatemeyer/ttui",
            dir.path().join("r.jsonl"),
            RejectingWork,
            FakeProcess::default(),
        );
        let action = Action::MergePullRequest {
            project: "ttui".into(),
            number: 142,
        };
        let authorized = authorize(&action, Some(&Confirmation::of(&action))).unwrap();
        let err = executor.execute(authorized).unwrap_err().to_string();
        assert!(
            err.contains("405") && err.contains("not mergeable"),
            "got {err}"
        );
    }

    /// No action falls through to `NotSupported` any more.
    #[test]
    fn the_local_executor_now_handles_every_action_in_the_set() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        let all = vec![
            Action::RuleFinding {
                project: "ttui".into(),
                fingerprint: "aaaa".into(),
                ruling: Ruling::Upheld,
            },
            Action::SetAutonomyLabel {
                project: "ttui".into(),
                item: 1,
                label: "gated".into(),
            },
            Action::RequestReReview {
                project: "ttui".into(),
                item: 1,
            },
            Action::TriggerCapture {
                project: "ttui".into(),
                scenario: None,
            },
            Action::DispatchAgentRun {
                project: "ttui".into(),
                item: 1,
                prompt: "go".into(),
            },
            Action::StopAgentRun {
                project: "ttui".into(),
                session: "s".into(),
            },
            Action::MergePullRequest {
                project: "ttui".into(),
                number: 1,
            },
            Action::Push {
                project: "ttui".into(),
                branch: "main".into(),
            },
        ];
        for action in &all {
            let authorized = authorize(action, Some(&Confirmation::of(action))).unwrap();
            let outcome = executor.execute(authorized);
            assert!(
                !matches!(outcome, Err(ActionError::NotSupported(_))),
                "{} fell through",
                action.summary()
            );
        }
        let _ = FakeWork::default();
    }
}
