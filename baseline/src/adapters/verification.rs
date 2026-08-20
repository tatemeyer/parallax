//! The verification family: whatever decides a unit of work is done.
//! Two built-in implementations — `command` (Task 13) and `plumb`
//! (Task 14). **Neither links Plumb**: the `plumb` adapter reads the
//! `verdict.md` Plumb writes, as text.

use super::{AdapterError, ProjectContext};
use crate::freshness::Observed;
use std::time::SystemTime;

/// What a verification check concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationOutcome {
    /// The check succeeded.
    Pass,
    /// The check failed.
    Fail,
    /// The check could not reach a conclusion. Never upgraded to a pass.
    Hold,
    /// The check has not run yet.
    NotRun,
}

/// One verification check's current standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationStatus {
    /// The manifest's display label for this check, e.g. `lint`.
    pub kind: String,
    /// What it concluded.
    pub outcome: VerificationOutcome,
    /// A one-line explanation, when the adapter has one.
    pub detail: Option<String>,
}

/// A source of verification outcomes.
pub trait VerificationAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Reads this check's current standing as of `now`.
    fn check(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<VerificationStatus>, AdapterError>;
}

use std::path::Path;
use std::process::Command;

/// What running a command produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// The process exit status, or 1 when it was killed by a signal.
    pub status: i32,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

/// Something that can run a shell command.
pub trait CommandRunner {
    /// Runs `command` with `cwd` as the working directory.
    fn run(&mut self, command: &str, cwd: &Path) -> std::io::Result<CommandOutput>;
}

/// The real runner. Invokes the platform shell so a command's quoting
/// and `--` separators survive verbatim.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl CommandRunner for ProcessRunner {
    fn run(&mut self, command: &str, cwd: &Path) -> std::io::Result<CommandOutput> {
        let output = if cfg!(windows) {
            Command::new("cmd")
                .arg("/C")
                .arg(command)
                .current_dir(cwd)
                .output()?
        } else {
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(cwd)
                .output()?
        };
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// A runner that replays scripted outputs. Public so integration tests
/// and frontend demos can both reach it.
#[derive(Debug, Default)]
pub struct ScriptedRunner {
    outputs: std::collections::VecDeque<CommandOutput>,
    next_error: Option<std::io::Error>,
    calls: Vec<String>,
}

impl ScriptedRunner {
    /// An empty runner. Running with nothing queued yields exit 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues one output.
    pub fn push(&mut self, output: CommandOutput) {
        self.outputs.push_back(output);
    }

    /// Makes the next run fail to spawn, once.
    pub fn fail_next(&mut self, error: std::io::Error) {
        self.next_error = Some(error);
    }

    /// Every command this runner was asked to run, in order.
    pub fn calls(&self) -> &[String] {
        &self.calls
    }
}

impl CommandRunner for ScriptedRunner {
    fn run(&mut self, command: &str, _cwd: &Path) -> std::io::Result<CommandOutput> {
        self.calls.push(command.to_string());
        if let Some(e) = self.next_error.take() {
            return Err(e);
        }
        Ok(self.outputs.pop_front().unwrap_or(CommandOutput {
            status: 0,
            stdout: String::new(),
            stderr: String::new(),
        }))
    }
}

/// Runs a declared command and reads its exit status.
pub struct CommandVerificationAdapter<R: CommandRunner> {
    kind: String,
    command: String,
    runner: R,
}

impl<R: CommandRunner> CommandVerificationAdapter<R> {
    /// An adapter running `command` and reporting it as `kind`.
    pub fn new(kind: impl Into<String>, command: impl Into<String>, runner: R) -> Self {
        Self {
            kind: kind.into(),
            command: command.into(),
            runner,
        }
    }

    /// The runner, for asserting what was run.
    pub fn runner(&self) -> &R {
        &self.runner
    }
}

impl<R: CommandRunner> VerificationAdapter for CommandVerificationAdapter<R> {
    fn source_name(&self) -> String {
        format!("verification:command:{}", self.kind)
    }

    fn check(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<VerificationStatus>, AdapterError> {
        let status = match self.runner.run(&self.command, &ctx.root) {
            // A command that could not be spawned reached no conclusion,
            // and a Hold is never upgraded to a Pass.
            Err(e) => VerificationStatus {
                kind: self.kind.clone(),
                outcome: VerificationOutcome::Hold,
                detail: Some(format!("could not run `{}`: {e}", self.command)),
            },
            Ok(output) if output.status == 0 => VerificationStatus {
                kind: self.kind.clone(),
                outcome: VerificationOutcome::Pass,
                detail: last_line(&output.stdout),
            },
            Ok(output) => VerificationStatus {
                kind: self.kind.clone(),
                outcome: VerificationOutcome::Fail,
                detail: last_line(&output.stderr).or_else(|| last_line(&output.stdout)),
            },
        };
        Ok(Observed::watched(status, now))
    }
}

/// The last non-empty line of some output, for a one-line detail.
fn last_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod command_tests {
    use super::*;
    use crate::adapters::ProjectContext;
    use std::time::{Duration, SystemTime};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    fn ok(stdout: &str) -> CommandOutput {
        CommandOutput {
            status: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    fn ctx() -> ProjectContext {
        ProjectContext::new("ttui", "<projects-root>/TTUI")
    }

    #[test]
    fn exit_zero_is_a_pass() {
        let mut runner = ScriptedRunner::new();
        runner.push(ok("test result: ok. 412 passed"));
        let mut a = CommandVerificationAdapter::new("tests", "cargo test", runner);
        let status = a.check(&ctx(), at(0)).unwrap().value;
        assert_eq!(status.kind, "tests");
        assert_eq!(status.outcome, VerificationOutcome::Pass);
    }

    #[test]
    fn a_nonzero_exit_is_a_fail_carrying_the_last_line_of_output_as_detail() {
        let mut runner = ScriptedRunner::new();
        runner.push(CommandOutput {
            status: 101,
            stdout: String::new(),
            stderr: "error: unused variable `x`\nerror: could not compile `ttui`".into(),
        });
        let mut a = CommandVerificationAdapter::new("lint", "cargo clippy", runner);
        let status = a.check(&ctx(), at(0)).unwrap().value;
        assert_eq!(status.outcome, VerificationOutcome::Fail);
        assert_eq!(
            status.detail.as_deref(),
            Some("error: could not compile `ttui`")
        );
    }

    #[test]
    fn the_command_is_passed_through_unmodified_including_its_double_dash() {
        let mut runner = ScriptedRunner::new();
        runner.push(ok(""));
        let command = "cargo clippy --all-targets -- -D warnings";
        let mut a = CommandVerificationAdapter::new("lint", command, runner);
        a.check(&ctx(), at(0)).unwrap();
        assert_eq!(a.runner().calls(), &[command.to_string()]);
    }

    /// A command that could not even be spawned is a Hold, not a Fail:
    /// the check did not run, so it reached no conclusion, and a Hold is
    /// never upgraded.
    #[test]
    fn a_command_that_cannot_be_spawned_is_a_hold_rather_than_a_fail() {
        let mut runner = ScriptedRunner::new();
        runner.fail_next(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cargo not found",
        ));
        let mut a = CommandVerificationAdapter::new("tests", "cargo test", runner);
        let status = a.check(&ctx(), at(0)).unwrap().value;
        assert_eq!(status.outcome, VerificationOutcome::Hold);
        assert!(status.detail.unwrap().contains("cargo not found"));
    }

    /// Running a command is not polling: the result is current as of the
    /// moment it finished.
    #[test]
    fn a_command_result_is_watched_not_polled() {
        let mut runner = ScriptedRunner::new();
        runner.push(ok(""));
        let mut a = CommandVerificationAdapter::new("tests", "true", runner);
        let observed = a.check(&ctx(), at(0)).unwrap();
        assert_eq!(observed.source, crate::freshness::SourceKind::Watched);
        assert_eq!(
            observed.freshness(at(9999)),
            crate::freshness::Freshness::Live
        );
    }

    #[test]
    fn the_source_name_names_the_kind_so_degradation_reporting_can_be_specific() {
        let a = CommandVerificationAdapter::new("lint", "cargo clippy", ScriptedRunner::new());
        assert_eq!(a.source_name(), "verification:command:lint");
    }
}
