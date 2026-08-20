//! `ProcessControl` against the local machine.
//!
//! Two of the four actions are deliberately unimplemented. See
//! `dispatch` for why, and
//! `docs/design/specs/panopticon/2026-08-20-cockpit-control-design.md`
//! for the decision.

use crate::actions::executor::ProcessControl;
use crate::adapters::verification::CommandRunner;
use crate::adapters::AdapterError;
use std::path::PathBuf;

/// Runs capture and push against a project's working tree.
///
/// Goes through `CommandRunner` rather than `std::process` so a test
/// asserts the exact command line without running it — the same seam
/// the verification adapter already uses, rather than a second way to
/// spawn things.
pub struct LocalProcessControl<R: CommandRunner> {
    runner: R,
    root: PathBuf,
    plumb: String,
}

impl<R: CommandRunner> LocalProcessControl<R> {
    /// Control over the working tree at `root`.
    pub fn new(runner: R, root: impl Into<PathBuf>) -> Self {
        Self {
            runner,
            root: root.into(),
            plumb: "plumb".to_string(),
        }
    }

    /// Names the `plumb` binary explicitly, for a checkout that has not
    /// installed it.
    pub fn with_plumb(mut self, plumb: impl Into<String>) -> Self {
        self.plumb = plumb.into();
        self
    }

    /// The runner, for asserting what it was asked to run.
    pub fn runner(&self) -> &R {
        &self.runner
    }

    /// Runs `command`, treating a non-zero exit as a failure carrying
    /// whatever the process said about it.
    fn run(&mut self, command: &str) -> Result<(), AdapterError> {
        let out = self
            .runner
            .run(command, &self.root)
            .map_err(AdapterError::Io)?;
        if out.status == 0 {
            return Ok(());
        }
        // stderr first: a tool that failed usually explains itself
        // there, and an empty explanation is worse than a long one.
        let said = if out.stderr.trim().is_empty() {
            out.stdout.trim()
        } else {
            out.stderr.trim()
        };
        Err(AdapterError::Parse(format!(
            "`{command}` exited {}: {said}",
            out.status
        )))
    }
}

impl<R: CommandRunner> ProcessControl for LocalProcessControl<R> {
    fn capture(&mut self, project: &str, scenario: Option<&str>) -> Result<(), AdapterError> {
        let _ = project; // the root already identifies it
        let command = match scenario {
            Some(s) => format!("{} capture --scenario {s}", self.plumb),
            None => format!("{} capture --all", self.plumb),
        };
        self.run(&command)
    }

    /// Not implemented, and not an oversight.
    ///
    /// How an agent session starts is harness-specific — this machine
    /// drives Claude Code jobs, the Pi drives something else, and a
    /// future runner will differ again. Choosing one here would bake a
    /// single harness into the platform, which is the coupling the
    /// adapter families exist to avoid. When a harness contract exists
    /// it becomes another `ProcessControl` and nothing else changes.
    fn dispatch(&mut self, project: &str, item: u64, prompt: &str) -> Result<String, AdapterError> {
        let _ = (project, item, prompt);
        Err(AdapterError::Unsupported(
            "dispatching an agent run needs a harness contract this platform does not have yet"
                .to_string(),
        ))
    }

    /// Not implemented. See `dispatch` — you cannot stop what you
    /// cannot start.
    fn stop(&mut self, session: &str) -> Result<(), AdapterError> {
        let _ = session;
        Err(AdapterError::Unsupported(
            "stopping an agent run needs a harness contract this platform does not have yet"
                .to_string(),
        ))
    }

    fn push(&mut self, project: &str, branch: &str) -> Result<(), AdapterError> {
        let _ = project;
        // Explicit refspec rather than a bare `git push`: what a bare
        // push does depends on the branch's upstream and on
        // `push.default`, and an action the operator confirmed should
        // not mean different things on different machines.
        self.run(&format!("git push origin {branch}:{branch}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::verification::{CommandOutput, ScriptedRunner};

    /// A runner with nothing queued: every command succeeds.
    fn control() -> LocalProcessControl<ScriptedRunner> {
        LocalProcessControl::new(ScriptedRunner::new(), "/projects/ttui")
    }

    #[test]
    fn capturing_one_scenario_names_it() {
        let mut c = control();
        c.capture("ttui", Some("cockpit-work")).unwrap();
        assert_eq!(
            c.runner().calls(),
            ["plumb capture --scenario cockpit-work"]
        );
    }

    #[test]
    fn capturing_without_a_scenario_captures_every_one() {
        let mut c = control();
        c.capture("ttui", None).unwrap();
        assert_eq!(c.runner().calls(), ["plumb capture --all"]);
    }

    /// A bare `git push` means whatever the branch's upstream and
    /// `push.default` say it means. A confirmed action must not.
    #[test]
    fn pushing_names_the_remote_and_both_ends_of_the_refspec() {
        let mut c = control();
        c.push("ttui", "worktree-arc-3").unwrap();
        assert_eq!(
            c.runner().calls(),
            ["git push origin worktree-arc-3:worktree-arc-3"]
        );
    }

    #[test]
    fn every_command_runs_in_the_project_root() {
        let mut c = control();
        c.capture("ttui", None).unwrap();
        assert_eq!(c.runner().cwds(), [PathBuf::from("/projects/ttui")]);
    }

    /// A command that failed must not be reported as one that worked,
    /// and the operator gets what the tool actually said.
    #[test]
    fn a_non_zero_exit_is_an_error_carrying_what_the_process_said() {
        let mut runner = ScriptedRunner::new();
        runner.push(CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: "fatal: no upstream configured".into(),
        });
        let mut c = LocalProcessControl::new(runner, "/projects/ttui");
        let err = c.push("ttui", "b").unwrap_err();
        assert!(
            err.to_string().contains("no upstream configured"),
            "swallowed the reason: {err}"
        );
    }

    /// The deferral is in the test suite, not only in prose: these two
    /// report that they cannot, rather than pretending they did.
    #[test]
    fn agent_dispatch_and_stop_say_they_are_unsupported() {
        let mut c = control();
        let dispatch = c.dispatch("ttui", 1, "do the thing").unwrap_err();
        let stop = c.stop("session-1").unwrap_err();
        for err in [dispatch, stop] {
            assert!(
                matches!(err, AdapterError::Unsupported(_)),
                "a deferred action must say so: {err}"
            );
            assert!(err.to_string().contains("harness"));
        }
        assert!(
            c.runner().calls().is_empty(),
            "an unsupported action ran something anyway"
        );
    }
}
