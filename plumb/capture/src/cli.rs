//! The subcommand bodies behind `plumb`'s CLI, and the exit-code
//! mapping. Each `run_*` function is a pure `Result`-returning wrapper
//! over Tasks 2-5's already-tested library functions; `dispatch` is the
//! only place that turns a result into stdout/stderr text and a
//! process exit code, kept separate so both halves are unit-testable
//! without spawning a process or parsing an argument.

use crate::Command;
use parallax_plumb::{adapter, config, manifest, select};
use std::path::{Path, PathBuf};

/// The bundled `config.yaml` template, embedded so a cached binary can
/// scaffold `.plumb/` with no plugin directory in scope.
const CONFIG_TEMPLATE: &str = include_str!("../../templates/config.example.yaml");
/// The bundled `taste.md` template; same embedding rationale as above.
const TASTE_TEMPLATE: &str = include_str!("../../templates/taste.md");

/// A file operation failure, together with the path that caused it —
/// the same `{path, source}` shape `config::IoFailure`/`manifest::IoFailure`
/// use, so a CLI-level error reads the same way as the errors it wraps.
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

/// One action taken while scaffolding `.plumb/`, reported to the
/// operator once `run_init` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InitAction {
    /// A template was written because the target did not exist.
    Wrote(PathBuf),
    /// The target already existed and was left untouched.
    Kept(PathBuf),
}

/// Scaffolds `dir` from the bundled templates: creates `scripts/` and
/// `runs/` subdirectories unconditionally, and writes `config.yaml`/
/// `taste.md` only when absent. A missing `.plumb/` is the expected
/// first-run state, not a failure — this never errors on that account,
/// and never overwrites a file the operator may have already edited.
fn run_init(dir: &Path) -> Result<Vec<InitAction>, IoFailure> {
    for sub in ["scripts", "runs"] {
        let path = dir.join(sub);
        std::fs::create_dir_all(&path).map_err(|source| IoFailure { path, source })?;
    }
    let mut actions = Vec::new();
    for (template, target) in [
        (CONFIG_TEMPLATE, "config.yaml"),
        (TASTE_TEMPLATE, "taste.md"),
    ] {
        let dest = dir.join(target);
        if dest.exists() {
            actions.push(InitAction::Kept(dest));
            continue;
        }
        std::fs::write(&dest, template).map_err(|source| IoFailure {
            path: dest.clone(),
            source,
        })?;
        actions.push(InitAction::Wrote(dest));
    }
    Ok(actions)
}

/// Reads `path`'s changed-path list, one per line, blank lines skipped;
/// `-` reads stdin instead of a file.
fn read_changed_list(path: &Path) -> Result<Vec<String>, IoFailure> {
    let text = if path == Path::new("-") {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).map_err(|source| {
            IoFailure {
                path: path.to_path_buf(),
                source,
            }
        })?;
        buf
    } else {
        std::fs::read_to_string(path).map_err(|source| IoFailure {
            path: path.to_path_buf(),
            source,
        })?
    };
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

/// Failure choosing scenarios to review.
#[derive(Debug)]
enum SelectCliError {
    /// The config file failed to load or validate.
    Config(config::ConfigError),
    /// Matching changed paths against `touches` globs failed.
    Select(select::SelectError),
    /// Reading the `--changed` file (or stdin) failed.
    Io(IoFailure),
    /// Neither `--changed` nor `--scenario` was given.
    Usage(&'static str),
}

impl std::fmt::Display for SelectCliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectCliError::Config(e) => write!(f, "{e}"),
            SelectCliError::Select(e) => write!(f, "{e}"),
            SelectCliError::Io(e) => write!(f, "{e}"),
            SelectCliError::Usage(m) => write!(f, "{m}"),
        }
    }
}
impl From<config::ConfigError> for SelectCliError {
    fn from(e: config::ConfigError) -> Self {
        SelectCliError::Config(e)
    }
}
impl From<select::SelectError> for SelectCliError {
    fn from(e: select::SelectError) -> Self {
        SelectCliError::Select(e)
    }
}

/// Loads `config_path` and runs either `select_by_name` or
/// `select_by_paths`, depending on which of `changed`/`scenario` was
/// given. The empty-selection outcome is not an error here — it is a
/// legitimate `Selection` the caller inspects and reports.
fn run_select(
    config_path: &Path,
    changed: Option<&Path>,
    scenario: Option<&str>,
) -> Result<select::Selection, SelectCliError> {
    let cfg = config::load_config(config_path)?;
    match (changed, scenario) {
        (_, Some(name)) => Ok(select::select_by_name(&cfg, name)?),
        (Some(path), None) => {
            let paths = read_changed_list(path).map_err(SelectCliError::Io)?;
            Ok(select::select_by_paths(&cfg, &paths)?)
        }
        (None, None) => Err(SelectCliError::Usage(
            "one of --changed or --scenario is required",
        )),
    }
}

/// Failure running a capture. `Adapter` is the one variant that means
/// the scenario itself failed to capture — every other variant means
/// capture never ran at all (bad config, unknown scenario, no writable
/// run directory).
#[derive(Debug)]
enum CaptureCliError {
    /// The config file failed to load or validate.
    Config(config::ConfigError),
    /// `--scenario` named something the config does not declare.
    UnknownScenario(String),
    /// The run directory could not be created, or the manifest could
    /// not be written into it.
    Io(IoFailure),
    /// The adapter itself failed. Capture failure is never a GO.
    Adapter(adapter::CaptureError),
}

impl std::fmt::Display for CaptureCliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureCliError::Config(e) => write!(f, "{e}"),
            CaptureCliError::UnknownScenario(n) => write!(f, "no scenario named {n:?} in config"),
            CaptureCliError::Io(e) => write!(f, "{e}"),
            CaptureCliError::Adapter(e) => write!(f, "{e}"),
        }
    }
}
impl From<config::ConfigError> for CaptureCliError {
    fn from(e: config::ConfigError) -> Self {
        CaptureCliError::Config(e)
    }
}
impl From<adapter::CaptureError> for CaptureCliError {
    fn from(e: adapter::CaptureError) -> Self {
        CaptureCliError::Adapter(e)
    }
}

/// Loads `config_path`, finds `scenario_name`, runs its adapter into
/// `run_dir`, and writes the run manifest. Returns the manifest's path.
fn run_capture(
    config_path: &Path,
    run_dir: &Path,
    scenario_name: &str,
) -> Result<PathBuf, CaptureCliError> {
    let cfg = config::load_config(config_path)?;
    let scenario = cfg
        .scenarios
        .iter()
        .find(|s| s.name == scenario_name)
        .ok_or_else(|| CaptureCliError::UnknownScenario(scenario_name.to_string()))?;
    std::fs::create_dir_all(run_dir).map_err(|source| {
        CaptureCliError::Io(IoFailure {
            path: run_dir.to_path_buf(),
            source,
        })
    })?;
    let run_id = run_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .unwrap_or_else(manifest::new_run_id);
    let m = adapter::capture(scenario, run_dir, &run_id)?;
    let expected_manifest_path = run_dir.join(format!("{}.manifest.json", m.scenario));
    manifest::write_manifest(&m, run_dir).map_err(|source| {
        CaptureCliError::Io(IoFailure {
            path: expected_manifest_path,
            source,
        })
    })
}

/// Runs the parsed command and returns the process exit code, printing
/// results and errors along the way. The only place in this binary
/// that decides an exit code — every `run_*` function above stays a
/// pure `Result`.
pub(crate) fn dispatch(command: Command) -> i32 {
    match command {
        Command::Init { dir } => match run_init(&dir) {
            Ok(actions) => {
                for action in actions {
                    match action {
                        InitAction::Wrote(p) => println!("wrote {}", p.display()),
                        InitAction::Kept(p) => println!("kept existing {}", p.display()),
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
        } => match run_select(&config, changed.as_deref(), scenario.as_deref()) {
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
        } => match run_capture(&config, &run_dir, &scenario) {
            Ok(path) => {
                println!("{}", path.display());
                0
            }
            // Capture failure is never a GO: surfaced as HOLD with the
            // adapter's own error, not folded into a generic exit 1.
            Err(e @ CaptureCliError::Adapter(_)) => {
                eprintln!("HOLD: capture failed: {e}");
                2
            }
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- run_init --------------------------------------------------------

    #[test]
    fn init_scaffolds_a_missing_plumb_dir_rather_than_erroring() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".plumb");
        assert!(!dir.exists());

        let actions = run_init(&dir).unwrap();

        assert!(dir.join("scripts").is_dir());
        assert!(dir.join("runs").is_dir());
        assert!(dir.join("config.yaml").is_file());
        assert!(dir.join("taste.md").is_file());
        assert_eq!(
            actions,
            vec![
                InitAction::Wrote(dir.join("config.yaml")),
                InitAction::Wrote(dir.join("taste.md")),
            ]
        );
        let written = std::fs::read_to_string(dir.join("config.yaml")).unwrap();
        assert_eq!(written, CONFIG_TEMPLATE);
    }

    #[test]
    fn init_never_overwrites_an_existing_config_or_taste_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".plumb");
        run_init(&dir).unwrap();
        std::fs::write(dir.join("config.yaml"), "# hand-edited\n").unwrap();

        let actions = run_init(&dir).unwrap();

        assert_eq!(
            actions,
            vec![
                InitAction::Kept(dir.join("config.yaml")),
                InitAction::Kept(dir.join("taste.md")),
            ]
        );
        let contents = std::fs::read_to_string(dir.join("config.yaml")).unwrap();
        assert_eq!(contents, "# hand-edited\n", "must not clobber edits");
    }

    // --- run_select --------------------------------------------------------

    fn write_config(dir: &Path, yaml: &str) -> PathBuf {
        let path = dir.join("config.yaml");
        std::fs::write(&path, yaml).unwrap();
        path
    }

    const SAMPLE_CONFIG: &str = "scenarios:\n  - name: dial\n    adapter: command\n    args: 'x {out}.png'\n    touches: ['src/widgets/dial.rs']\n";

    #[test]
    fn select_by_changed_file_reports_an_empty_selection_rather_than_erroring() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);
        let changed = tmp.path().join("changed.txt");
        std::fs::write(&changed, "README.md\n").unwrap();

        let selection = run_select(&config, Some(&changed), None).unwrap();

        assert!(selection.selected.is_empty());
        assert_eq!(selection.unmatched, vec!["README.md".to_string()]);
    }

    #[test]
    fn select_by_changed_file_matching_touches_selects_it() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);
        let changed = tmp.path().join("changed.txt");
        std::fs::write(&changed, "src/widgets/dial.rs\n\n").unwrap();

        let selection = run_select(&config, Some(&changed), None).unwrap();

        assert_eq!(selection.selected.len(), 1);
        assert_eq!(selection.selected[0].name, "dial");
    }

    #[test]
    fn select_by_name_ignores_touches() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);

        let selection = run_select(&config, None, Some("dial")).unwrap();

        assert_eq!(selection.selected.len(), 1);
    }

    #[test]
    fn select_with_neither_changed_nor_scenario_is_a_usage_error() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);

        assert!(matches!(
            run_select(&config, None, None),
            Err(SelectCliError::Usage(_))
        ));
    }

    #[test]
    fn select_names_the_missing_changed_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);
        let missing = tmp.path().join("nope.txt");

        let err = run_select(&config, Some(&missing), None).unwrap_err();

        assert!(matches!(err, SelectCliError::Io(_)));
        assert!(err.to_string().contains(&missing.display().to_string()));
    }

    // --- run_capture --------------------------------------------------------

    #[test]
    fn capture_rejects_an_unknown_scenario() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);
        let run_dir = tmp.path().join("run1");

        let err = run_capture(&config, &run_dir, "nope").unwrap_err();

        assert!(matches!(err, CaptureCliError::UnknownScenario(n) if n == "nope"));
    }

    #[test]
    fn a_successful_capture_writes_an_image_and_a_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let run_dir = tmp.path().join("run1");
        std::fs::create_dir_all(&run_dir).unwrap();
        // A source fixture in its own directory, distinct from the run
        // directory, so `copy`'s destination never collides with it —
        // mirrors tests/command_adapter.rs's fix for this exact trap.
        let src_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("fixture.png");
        image::RgbaImage::new(4, 4).save(&src).unwrap();
        let copy_cmd = if cfg!(windows) {
            format!("copy \"{}\" \"{{out}}.png\"", src.display())
        } else {
            format!("cp '{}' '{{out}}.png'", src.display())
        };
        let config = write_config(
            tmp.path(),
            &format!(
                "scenarios:\n  - name: fixture\n    adapter: command\n    args: '{copy_cmd}'\n    touches: ['src/**']\n"
            ),
        );

        let manifest_path = run_capture(&config, &run_dir, "fixture").unwrap();

        assert!(manifest_path.is_file());
        assert!(run_dir.join("fixture.png").is_file());
    }

    #[test]
    fn a_failing_adapter_is_reported_as_capturecli_adapter_error() {
        let tmp = tempfile::tempdir().unwrap();
        let yaml = "scenarios:\n  - name: fixture\n    adapter: command\n    args: 'this-command-does-not-exist --out {out}.png'\n    touches: ['src/**']\n";
        let config = write_config(tmp.path(), yaml);
        let run_dir = tmp.path().join("run1");

        let err = run_capture(&config, &run_dir, "fixture").unwrap_err();

        assert!(matches!(err, CaptureCliError::Adapter(_)));
    }

    // --- dispatch: the exit-code mapping itself -----------------------------

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

    #[test]
    fn dispatch_init_exits_0_on_a_fresh_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".plumb");

        let code = dispatch(Command::Init { dir });

        assert_eq!(code, 0);
    }
}
