//! The verification family: whatever decides a unit of work is done.
//! Two built-in implementations — `command` (Task 13) and `plumb`
//! (Task 14). **Neither links Plumb**: the `plumb` adapter reads the
//! `verdict.md` Plumb writes, as text.

use super::{AdapterError, ProjectContext};
use crate::freshness::Observed;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// What a verification check concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationStatus {
    /// The manifest's display label for this check, e.g. `lint`.
    pub kind: String,
    /// What it concluded.
    pub outcome: VerificationOutcome,
    /// A one-line explanation, when the adapter has one.
    pub detail: Option<String>,
}

/// What calling [`VerificationAdapter::check`] costs.
///
/// The two built-ins sit on opposite sides of this: the `plumb` adapter
/// reads a `verdict.md` off disk, and the `command` adapter runs
/// whatever the manifest declared — for TTUI, `cargo clippy` and `cargo
/// test`. A caller that polls on a cadence needs to tell them apart
/// before it schedules anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckCost {
    /// Reads state something else produced. Safe on any cadence.
    Read,
    /// Produces the state by running something. Operator-initiated.
    Execute,
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

    /// What calling `check` costs.
    ///
    /// Defaults to [`CheckCost::Execute`]: the safe assumption about an
    /// adapter this crate has never seen is that calling it *does*
    /// something. A reader misclassified as an executor merely refreshes
    /// less often than it could; an executor misclassified as a reader
    /// spawns processes in a loop.
    fn cost(&self) -> CheckCost {
        CheckCost::Execute
    }
}

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

/// Something that can run a **shell** command.
///
/// **For manifest-declared commands only** — trust tier 2 in
/// `docs/design/specs/parallax/2026-08-21-what-an-action-may-name-design.md`.
/// A `verification[].command` lives in the repository it describes,
/// under that repository's review, so it is trusted *as code* because it
/// is code, and it genuinely needs a shell: SESH declares `cd surfaces
/// && npm test && npm run build`, which is two commands and a directory
/// change, not an argv.
///
/// **Nothing the platform constructs may come through here**, and
/// nothing carrying a value that arrived from outside — a wire field, or
/// something the operator typed. Those go to [`ProgramRunner`], which
/// has no interpreter in the middle. See its doc for what that buys.
pub trait ShellRunner {
    /// Runs `command` with `cwd` as the working directory.
    fn run(&mut self, command: &str, cwd: &Path) -> std::io::Result<CommandOutput>;
}

/// The real shell runner. Invokes the platform shell so a command's
/// quoting and `--` separators survive verbatim.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessShellRunner;

impl ShellRunner for ProcessShellRunner {
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

/// A shell runner that replays scripted outputs. Public so integration
/// tests and frontend demos can both reach it.
#[derive(Debug, Default)]
pub struct ScriptedShellRunner {
    outputs: std::collections::VecDeque<CommandOutput>,
    next_error: Option<std::io::Error>,
    calls: Vec<String>,
    cwds: Vec<PathBuf>,
}

impl ScriptedShellRunner {
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

    /// The working directory each call was made in, in the same order.
    /// A command that runs in the wrong tree is not the command the
    /// caller asked for, so a test has to be able to see it.
    pub fn cwds(&self) -> &[PathBuf] {
        &self.cwds
    }
}

impl ShellRunner for ScriptedShellRunner {
    fn run(&mut self, command: &str, cwd: &Path) -> std::io::Result<CommandOutput> {
        self.calls.push(command.to_string());
        self.cwds.push(cwd.to_path_buf());
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

/// One invocation of a program: what to run, and the arguments it was
/// given as separate strings.
///
/// The point of the type is that the arguments are a `Vec`, not a
/// sentence. A test asserting an invocation is asserting *how many
/// arguments there were*, which is the property a shell string cannot
/// express and the reason a payload can no longer hide inside one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The program to run.
    pub program: String,
    /// Its arguments, one per element.
    pub args: Vec<String>,
}

impl Invocation {
    /// An invocation of `program` with `args`.
    pub fn new<I, S>(program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

/// Something that can run a program **with arguments, and no shell**.
///
/// **This is where everything the platform constructs goes**, and
/// everything carrying a value that arrived from outside it: an action's
/// parameters off the wire, and whatever the operator typed at a prompt.
/// See
/// `docs/design/specs/parallax/2026-08-21-what-an-action-may-name-design.md`.
///
/// The difference from [`ShellRunner`] is not degree. A shell string has
/// to be *parsed* to find out how many commands are in it, so a value
/// interpolated into one can add commands that were never intended —
/// which is exactly how a `scenario` field became a remote shell. An
/// argument list has no such question in it: the count is fixed before
/// the process starts, and a payload lands in one element rather than
/// becoming several.
///
/// Note that this deletes the class rather than filtering it. There is
/// no escaping here and there should not be: escaping is a denylist,
/// and this platform runs on both `sh` and `cmd`, whose quoting rules
/// differ.
pub trait ProgramRunner {
    /// Runs `program` with `args`, in `cwd`.
    fn run(&mut self, program: &str, args: &[&str], cwd: &Path) -> std::io::Result<CommandOutput>;
}

/// The real program runner. Spawns the program directly.
///
/// No `cfg!(windows)` fork, unlike [`ProcessShellRunner`] — that fork
/// existed only to pick between `sh -c` and `cmd /C`, and there is no
/// interpreter to pick any more. One code path on both platforms is a
/// consequence of the design rather than a tidy-up.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessProgramRunner;

impl ProgramRunner for ProcessProgramRunner {
    fn run(&mut self, program: &str, args: &[&str], cwd: &Path) -> std::io::Result<CommandOutput> {
        let output = Command::new(program).args(args).current_dir(cwd).output()?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// A program runner that records invocations and replays scripted
/// outputs. The [`ScriptedShellRunner`] of the other tier.
#[derive(Debug, Default)]
pub struct ScriptedProgramRunner {
    outputs: std::collections::VecDeque<CommandOutput>,
    next_error: Option<std::io::Error>,
    calls: Vec<Invocation>,
    cwds: Vec<PathBuf>,
}

impl ScriptedProgramRunner {
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

    /// Every invocation this runner was asked to perform, in order.
    pub fn calls(&self) -> &[Invocation] {
        &self.calls
    }

    /// The working directory each call was made in, in the same order.
    pub fn cwds(&self) -> &[PathBuf] {
        &self.cwds
    }
}

impl ProgramRunner for ScriptedProgramRunner {
    fn run(&mut self, program: &str, args: &[&str], cwd: &Path) -> std::io::Result<CommandOutput> {
        self.calls
            .push(Invocation::new(program, args.iter().copied()));
        self.cwds.push(cwd.to_path_buf());
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

/// Runs a manifest-declared command and reads its exit status.
///
/// Tier 2: the command is a string from a `parallax.yaml`, so it runs
/// through a [`ShellRunner`] deliberately. See that trait's doc.
pub struct CommandVerificationAdapter<R: ShellRunner> {
    kind: String,
    command: String,
    runner: R,
}

impl<R: ShellRunner> CommandVerificationAdapter<R> {
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

impl<R: ShellRunner> VerificationAdapter for CommandVerificationAdapter<R> {
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

/// Reads Plumb's overall verdict from a rendered `verdict.md`.
///
/// Deliberately narrow: only the first line naming one of the three
/// states counts, and `NO-GO` is tested before `GO` because it contains
/// it. **This parses text; it does not link Plumb.**
pub fn parse_verdict(text: &str) -> Option<VerificationOutcome> {
    for line in text.lines() {
        if line.contains("NO-GO") {
            return Some(VerificationOutcome::Fail);
        }
        if line.contains("HOLD") {
            return Some(VerificationOutcome::Hold);
        }
        if line.contains("GO") {
            return Some(VerificationOutcome::Pass);
        }
    }
    None
}

/// Reads the most recent Plumb run's verdict from a runs directory.
/// Run directories are named with sortable UTC run ids, so "most
/// recent" is the lexicographically greatest name that has a verdict.
pub struct PlumbVerificationAdapter {
    kind: String,
    runs_dir: PathBuf,
}

impl PlumbVerificationAdapter {
    /// An adapter reading `runs_dir`, reporting as `kind`.
    pub fn new(kind: impl Into<String>, runs_dir: impl Into<PathBuf>) -> Self {
        Self {
            kind: kind.into(),
            runs_dir: runs_dir.into(),
        }
    }
}

impl VerificationAdapter for PlumbVerificationAdapter {
    fn source_name(&self) -> String {
        format!("verification:plumb:{}", self.kind)
    }

    fn cost(&self) -> CheckCost {
        CheckCost::Read
    }

    fn check(
        &mut self,
        _ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<VerificationStatus>, AdapterError> {
        let mut best: Option<(String, VerificationOutcome)> = None;
        if let Ok(entries) = std::fs::read_dir(&self.runs_dir) {
            for entry in entries.flatten() {
                let run_id = entry.file_name().to_string_lossy().into_owned();
                let verdict_path = entry.path().join("verdict.md");
                let Ok(text) = std::fs::read_to_string(&verdict_path) else {
                    // An in-progress run has no verdict yet. Skipping it
                    // is not a failure of the check.
                    continue;
                };
                let Some(outcome) = parse_verdict(&text) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(id, _)| run_id > *id) {
                    best = Some((run_id, outcome));
                }
            }
        }

        let status = match best {
            Some((run_id, outcome)) => VerificationStatus {
                kind: self.kind.clone(),
                outcome,
                detail: Some(run_id),
            },
            None => VerificationStatus {
                kind: self.kind.clone(),
                outcome: VerificationOutcome::NotRun,
                detail: Some(format!(
                    "no completed run under {}",
                    self.runs_dir.display()
                )),
            },
        };
        Ok(Observed::watched(status, now))
    }
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
        let mut runner = ScriptedShellRunner::new();
        runner.push(ok("test result: ok. 412 passed"));
        let mut a = CommandVerificationAdapter::new("tests", "cargo test", runner);
        let status = a.check(&ctx(), at(0)).unwrap().value;
        assert_eq!(status.kind, "tests");
        assert_eq!(status.outcome, VerificationOutcome::Pass);
    }

    #[test]
    fn a_nonzero_exit_is_a_fail_carrying_the_last_line_of_output_as_detail() {
        let mut runner = ScriptedShellRunner::new();
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
        let mut runner = ScriptedShellRunner::new();
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
        let mut runner = ScriptedShellRunner::new();
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
        let mut runner = ScriptedShellRunner::new();
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
    fn a_command_check_costs_a_process() {
        let a = CommandVerificationAdapter::new("tests", "cargo test", ScriptedShellRunner::new());
        assert_eq!(a.cost(), CheckCost::Execute);
    }

    #[test]
    fn a_plumb_check_only_reads() {
        let a = PlumbVerificationAdapter::new("perceptual", "/tmp/runs");
        assert_eq!(a.cost(), CheckCost::Read);
    }

    /// An adapter defined outside this crate that overrides nothing is
    /// assumed to cost something. A scheduler that polls it on a cadence
    /// would be spawning whatever it spawns, forever.
    #[test]
    fn an_adapter_that_says_nothing_is_assumed_to_execute() {
        struct Unknown;
        impl VerificationAdapter for Unknown {
            fn source_name(&self) -> String {
                "verification:unknown".into()
            }
            fn check(
                &mut self,
                _ctx: &ProjectContext,
                now: SystemTime,
            ) -> Result<Observed<VerificationStatus>, AdapterError> {
                Ok(Observed::watched(
                    VerificationStatus {
                        kind: "unknown".into(),
                        outcome: VerificationOutcome::NotRun,
                        detail: None,
                    },
                    now,
                ))
            }
        }
        assert_eq!(Unknown.cost(), CheckCost::Execute);
    }

    #[test]
    fn the_source_name_names_the_kind_so_degradation_reporting_can_be_specific() {
        let a = CommandVerificationAdapter::new("lint", "cargo clippy", ScriptedShellRunner::new());
        assert_eq!(a.source_name(), "verification:command:lint");
    }
}
