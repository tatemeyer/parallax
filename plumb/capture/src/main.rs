//! CLI entry point for `plumb`: scaffolding, scenario selection, and
//! capture. Judgment is a fan-out of subagents the orchestrating skill
//! dispatches — this binary never calls a model. Argument parsing lives
//! here, kept thin and free of I/O; each subcommand's real work and the
//! exit-code dispatch live in `cli`, which is unit-testable without
//! parsing a single argument.
#![warn(missing_docs)]

mod cli;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// `plumb`: perceptual verification, capture then judge.
#[derive(Parser)]
#[command(
    name = "plumb",
    about = "Perceptual verification: capture, then judge."
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

/// The implemented subcommands. `rule` arrives with a later task
/// (Arc 4).
#[derive(Subcommand)]
enum Command {
    /// Scaffold a `.plumb/` directory from the bundled templates.
    Init {
        /// Where to scaffold.
        #[arg(long, default_value = ".plumb")]
        dir: PathBuf,
    },
    /// Choose which scenarios a change warrants reviewing.
    Select {
        /// Path to `.plumb/config.yaml`.
        #[arg(long)]
        config: PathBuf,
        /// File holding one changed path per line (`-` for stdin).
        #[arg(long, conflicts_with = "scenario")]
        changed: Option<PathBuf>,
        /// Review exactly this scenario, ignoring `touches`.
        #[arg(long)]
        scenario: Option<String>,
    },
    /// Run one scenario's adapter and write its run manifest.
    Capture {
        /// Path to `.plumb/config.yaml`.
        #[arg(long)]
        config: PathBuf,
        /// Directory to write images and manifests into.
        #[arg(long)]
        run_dir: PathBuf,
        /// Which scenario to capture.
        #[arg(long)]
        scenario: String,
    },
    /// Plan a run's lens dispatch from its manifests, as JSON.
    Plan {
        /// Directory holding the run's `*.manifest.json` files.
        #[arg(long)]
        run_dir: PathBuf,
        /// Path to `taste.md`, reaching the design lens only.
        #[arg(long)]
        taste: Option<PathBuf>,
        /// Maximum lens dispatches per batch.
        #[arg(long, default_value_t = parallax_plumb::prompt::DEFAULT_CONCURRENCY_CAP)]
        cap: usize,
    },
    /// Merge lens reports, render `verdict.md`, and exit with its code.
    Merge {
        /// Directory to write `verdict.md` into.
        #[arg(long)]
        run_dir: PathBuf,
        /// One `lens:scenario:file` triple per lens report to ingest.
        #[arg(long)]
        report: Vec<String>,
        /// One `lens:scenario` pair per lens dispatched for this run;
        /// an expected lens with no matching `--report` holds the run
        /// rather than silently vanishing from the poll.
        #[arg(long)]
        expected: Vec<String>,
        /// One `scenario:reason` pair per scenario whose capture failed
        /// outright. A capture failure is never a GO.
        #[arg(long)]
        capture_failure: Vec<String>,
    },
}

fn main() {
    let args = Args::parse();
    std::process::exit(cli::dispatch(args.command));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_parses_a_changed_file_list_path() {
        let a = Args::try_parse_from([
            "plumb",
            "select",
            "--config",
            ".plumb/config.yaml",
            "--changed",
            "changed.txt",
        ])
        .unwrap();
        match a.command {
            Command::Select {
                config,
                changed,
                scenario,
            } => {
                assert_eq!(config, PathBuf::from(".plumb/config.yaml"));
                assert_eq!(changed, Some(PathBuf::from("changed.txt")));
                assert!(scenario.is_none());
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn select_rejects_naming_both_changed_and_scenario() {
        assert!(Args::try_parse_from([
            "plumb",
            "select",
            "--config",
            "c.yaml",
            "--changed",
            "f.txt",
            "--scenario",
            "dial",
        ])
        .is_err());
    }

    #[test]
    fn capture_requires_a_run_dir_and_a_scenario() {
        assert!(Args::try_parse_from(["plumb", "capture", "--config", "c.yaml"]).is_err());
    }

    #[test]
    fn init_defaults_its_target_directory_to_dot_plumb() {
        let a = Args::try_parse_from(["plumb", "init"]).unwrap();
        match a.command {
            Command::Init { dir } => assert_eq!(dir, PathBuf::from(".plumb")),
            _ => panic!("expected Init"),
        }
    }

    #[test]
    fn select_parses_a_scenario_name() {
        let a = Args::try_parse_from([
            "plumb",
            "select",
            "--config",
            "c.yaml",
            "--scenario",
            "dial",
        ])
        .unwrap();
        match a.command {
            Command::Select {
                changed, scenario, ..
            } => {
                assert!(changed.is_none());
                assert_eq!(scenario.as_deref(), Some("dial"));
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn capture_parses_config_run_dir_and_scenario() {
        let a = Args::try_parse_from([
            "plumb",
            "capture",
            "--config",
            "c.yaml",
            "--run-dir",
            "r",
            "--scenario",
            "dial",
        ])
        .unwrap();
        match a.command {
            Command::Capture {
                config,
                run_dir,
                scenario,
            } => {
                assert_eq!(config, PathBuf::from("c.yaml"));
                assert_eq!(run_dir, PathBuf::from("r"));
                assert_eq!(scenario, "dial");
            }
            _ => panic!("expected Capture"),
        }
    }
}
