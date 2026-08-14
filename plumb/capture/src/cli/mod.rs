//! The subcommand dispatch behind `plumb`'s CLI: turns a parsed
//! `Command` into stdout/stderr text and a process exit code. Each
//! subcommand's real work lives in its own sibling module
//! (`init`/`select`/`capture`), each a pure `Result`-returning wrapper
//! over Tasks 2-5's already-tested library functions; `dispatch` here
//! is the only place in this binary that decides an exit code.

mod capture;
mod init;
mod select;

use crate::Command;
use std::path::PathBuf;

/// A file operation failure, together with the path that caused it —
/// the same `{path, source}` shape `config::IoFailure`/`manifest::IoFailure`
/// use, so a CLI-level error reads the same way as the errors it wraps.
/// Shared by all three subcommand modules (each of which is a
/// descendant of this one, so they can name it via `super::IoFailure`).
#[derive(Debug)]
struct IoFailure {
    path: PathBuf,
    source: std::io::Error,
}

impl std::fmt::Display for IoFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.source)
    }
}

/// Runs the parsed command and returns the process exit code, printing
/// results and errors along the way. The only place in this binary
/// that decides an exit code — every subcommand module's `run_*`
/// function stays a pure `Result`.
pub(crate) fn dispatch(command: Command) -> i32 {
    match command {
        Command::Init { dir } => match init::run_init(&dir) {
            Ok(actions) => {
                for action in actions {
                    match action {
                        init::InitAction::Wrote(p) => println!("wrote {}", p.display()),
                        init::InitAction::Kept(p) => println!("kept existing {}", p.display()),
                    }
                }
                0
            }
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        },
        Command::Select {
            config,
            changed,
            scenario,
        } => match select::run_select(&config, changed.as_deref(), scenario.as_deref()) {
            Ok(selection) => {
                let json = serde_json::to_string_pretty(&selection)
                    .expect("Selection serializes infallibly");
                println!("{json}");
                if selection.selected.is_empty() {
                    eprintln!(
                        "no scenario's `touches` globs matched the changed paths, and no \
                         --scenario was named: nothing to review. Stopping rather than \
                         reviewing everything."
                    );
                    3
                } else {
                    0
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        },
        Command::Capture {
            config,
            run_dir,
            scenario,
        } => match capture::run_capture(&config, &run_dir, &scenario) {
            Ok(path) => {
                println!("{}", path.display());
                0
            }
            // A usage error: the operator named a scenario the config
            // does not declare. Capture never ran, but this is squarely
            // an invocation mistake, not "the check could not be run" —
            // stays a plain exit 1, same as any other bad-argument error.
            Err(e @ capture::CaptureCliError::UnknownScenario(_)) => {
                eprintln!("Error: {e}");
                1
            }
            // Everything else here means the check itself could not be
            // run at all: a missing/malformed config, an unwritable run
            // directory, or the adapter failing outright. None of these
            // assert a real defect was found in the UI, so none may ever
            // read as NO-GO (1) any more than as GO (0) — all three
            // surface as HOLD (2), distinguished only in the message.
            Err(e @ capture::CaptureCliError::Config(_)) => {
                eprintln!("HOLD: could not load the config, so capture could not run: {e}");
                2
            }
            Err(e @ capture::CaptureCliError::Io(_)) => {
                eprintln!("HOLD: could not prepare the run, so capture could not run: {e}");
                2
            }
            Err(e @ capture::CaptureCliError::Adapter(_)) => {
                eprintln!("HOLD: capture failed: {e}");
                2
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_config(dir: &Path, yaml: &str) -> PathBuf {
        let path = dir.join("config.yaml");
        std::fs::write(&path, yaml).unwrap();
        path
    }

    const SAMPLE_CONFIG: &str = "scenarios:\n  - name: dial\n    adapter: command\n    args: 'x {out}.png'\n    touches: ['src/widgets/dial.rs']\n";

    #[test]
    fn dispatch_select_exits_3_when_nothing_matched() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);
        let changed = tmp.path().join("changed.txt");
        std::fs::write(&changed, "README.md\n").unwrap();

        let code = dispatch(Command::Select {
            config,
            changed: Some(changed),
            scenario: None,
        });

        assert_eq!(code, 3);
    }

    #[test]
    fn dispatch_select_exits_0_when_something_matched() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);

        let code = dispatch(Command::Select {
            config,
            changed: None,
            scenario: Some("dial".into()),
        });

        assert_eq!(code, 0);
    }

    #[test]
    fn dispatch_capture_exits_2_on_adapter_failure_never_0() {
        let tmp = tempfile::tempdir().unwrap();
        let yaml = "scenarios:\n  - name: fixture\n    adapter: command\n    args: 'this-command-does-not-exist --out {out}.png'\n    touches: ['src/**']\n";
        let config = write_config(tmp.path(), yaml);
        let run_dir = tmp.path().join("run1");

        let code = dispatch(Command::Capture {
            config,
            run_dir,
            scenario: "fixture".into(),
        });

        assert_eq!(code, 2, "a failed capture must never be a GO (0)");
    }

    /// The reviewed correction: a config that fails to load means the
    /// pipeline never got far enough to attempt a capture — that is
    /// "the check could not be run" (HOLD), not "the check ran and
    /// found nothing wrong" (GO) or "found a blocker" (NO-GO).
    #[test]
    fn dispatch_capture_exits_2_when_the_config_fails_to_load() {
        let tmp = tempfile::tempdir().unwrap();
        let missing_config = tmp.path().join("does-not-exist.yaml");
        let run_dir = tmp.path().join("run1");

        let code = dispatch(Command::Capture {
            config: missing_config,
            run_dir,
            scenario: "anything".into(),
        });

        assert_eq!(
            code, 2,
            "a config that never loads means no check ran at all — HOLD, not NO-GO (1)"
        );
    }

    /// Same reasoning as above, for the other pre-adapter failure mode:
    /// the run directory itself could not be prepared.
    #[test]
    fn dispatch_capture_exits_2_when_the_run_dir_cannot_be_created() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let run_dir = blocker.join("run1");

        let code = dispatch(Command::Capture {
            config,
            run_dir,
            scenario: "dial".into(),
        });

        assert_eq!(code, 2);
    }

    /// The one capture failure mode that stays exit 1: the operator
    /// naming a scenario the config does not declare is a usage
    /// mistake, not "the check could not be run."
    #[test]
    fn dispatch_capture_exits_1_on_an_unknown_scenario() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);
        let run_dir = tmp.path().join("run1");

        let code = dispatch(Command::Capture {
            config,
            run_dir,
            scenario: "nope".into(),
        });

        assert_eq!(code, 1);
    }

    #[test]
    fn dispatch_init_exits_0_on_a_fresh_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".plumb");

        let code = dispatch(Command::Init { dir });

        assert_eq!(code, 0);
    }
}
