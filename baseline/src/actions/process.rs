//! `ProcessControl` against the local machine.
//!
//! Two of the four actions are deliberately unimplemented. See
//! `dispatch` for why, and
//! `docs/design/specs/panopticon/2026-08-20-cockpit-control-design.md`
//! for the decision.

use crate::actions::executor::ProcessControl;
use crate::adapters::verification::ProgramRunner;
use crate::adapters::AdapterError;
use std::path::PathBuf;

/// Runs capture and push against a project's working tree.
///
/// Goes through a runner rather than `std::process` directly so a test
/// asserts the exact invocation without performing it — the same kind of
/// seam the verification adapter uses, rather than a second way to spawn
/// things.
///
/// **The runner is a [`ProgramRunner`], and that is structural.** Every
/// argument these methods pass carries a value that arrived from
/// outside: an action's `scenario` or `branch` field off the wire, or
/// whatever the operator typed at a prompt. There is deliberately no
/// constructor that accepts a
/// [`ShellRunner`](crate::adapters::verification::ShellRunner) — the
/// refusal is the absence of a capability rather than a check in front
/// of one, the same shape the probe uses for `--allow-control`.
pub struct LocalProcessControl<R: ProgramRunner> {
    runner: R,
    root: PathBuf,
    plumb: String,
}

impl<R: ProgramRunner> LocalProcessControl<R> {
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

    /// Runs `program` with `args`, treating a non-zero exit as a failure
    /// carrying whatever the process said about it.
    fn run(&mut self, program: &str, args: &[&str]) -> Result<(), AdapterError> {
        let out = self
            .runner
            .run(program, args, &self.root)
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
        // Joined only to *report* what ran. Nothing parses this back,
        // and nothing may: the moment a caller splits it again, the
        // argument count this arc fixed becomes a guess.
        let shown = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        Err(AdapterError::Parse(format!(
            "`{shown}` exited {}: {said}",
            out.status
        )))
    }
}

impl<R: ProgramRunner> ProcessControl for LocalProcessControl<R> {
    fn capture(&mut self, project: &str, scenario: Option<&str>) -> Result<(), AdapterError> {
        let _ = project; // the root already identifies it
                         // `scenario` arrives from the wire and is not narrowed yet, so
                         // it must land in exactly one argument. It does, because it is
                         // one element of the list rather than a substring of a sentence.
        let plumb = self.plumb.clone();
        match scenario {
            Some(s) => self.run(&plumb, &["capture", "--scenario", s]),
            None => self.run(&plumb, &["capture", "--all"]),
        }
    }

    /// Not implemented, and not an oversight.
    ///
    /// How an agent session starts is harness-specific — this machine
    /// drives Claude Code jobs, the Pi drives something else, and a
    /// future runner will differ again. Choosing one here would bake a
    /// single harness into the platform, which is the coupling the
    /// adapter families exist to avoid. When a harness contract exists
    /// it becomes another `ProcessControl` and nothing else changes.
    ///
    /// **Being unimplemented is not what makes `prompt` safe**, and a
    /// harness arc must not read it that way. `prompt` is free text off
    /// the wire and no validator can narrow it — the whole point of a
    /// prompt is that it is arbitrary. What contains it is that it is
    /// **passed as one argument to a program**, through the
    /// [`ProgramRunner`] this type already holds. An implementation that
    /// reaches for a shell to interpolate it into re-opens the hole this
    /// arc closed, and would be the widest instance of it.
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
        //
        // **The surviving `format!` is correct and is not a leftover.**
        // It builds one argv element, `branch:branch`, which is what a
        // refspec is. `format!` was never the defect; `format!` into
        // something an interpreter reads was. Removing this would mean
        // inventing a way to say "one refspec" in two arguments, which
        // git has no spelling for.
        //
        // What this does *not* fix is a branch beginning with `-`,
        // which git reads as one of its own flags. Argument injection
        // survives argv, and closing it is `BranchName`'s job in the
        // next arc — named here so the gap is deliberate rather than
        // discovered.
        self.run("git", &["push", "origin", &format!("{branch}:{branch}")])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::verification::{CommandOutput, Invocation, ScriptedProgramRunner};

    /// A runner with nothing queued: every invocation succeeds.
    fn control() -> LocalProcessControl<ScriptedProgramRunner> {
        LocalProcessControl::new(ScriptedProgramRunner::new(), "/projects/ttui")
    }

    #[test]
    fn capturing_one_scenario_names_it() {
        let mut c = control();
        c.capture("ttui", Some("cockpit-work")).unwrap();
        assert_eq!(
            c.runner().calls(),
            [Invocation::new(
                "plumb",
                ["capture", "--scenario", "cockpit-work"]
            )]
        );
    }

    #[test]
    fn capturing_without_a_scenario_captures_every_one() {
        let mut c = control();
        c.capture("ttui", None).unwrap();
        assert_eq!(
            c.runner().calls(),
            [Invocation::new("plumb", ["capture", "--all"])]
        );
    }

    /// A bare `git push` means whatever the branch's upstream and
    /// `push.default` say it means. A confirmed action must not.
    ///
    /// **The claim this test was written for is unchanged** — the
    /// remote is named and so are both ends of the refspec. What changed
    /// is that it now asserts an argument *list*, so it also witnesses
    /// that the refspec is one argument rather than three words.
    #[test]
    fn pushing_names_the_remote_and_both_ends_of_the_refspec() {
        let mut c = control();
        c.push("ttui", "worktree-arc-3").unwrap();
        assert_eq!(
            c.runner().calls(),
            [Invocation::new(
                "git",
                ["push", "origin", "worktree-arc-3:worktree-arc-3"]
            )]
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
        let mut runner = ScriptedProgramRunner::new();
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
