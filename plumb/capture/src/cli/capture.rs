//! `plumb capture`: runs one scenario's adapter and writes its run
//! manifest. `CaptureCliError::Adapter` is the one variant that means
//! the scenario itself failed to capture; every other variant means
//! capture never got a chance to run at all (bad config, unknown
//! scenario, an unwritable run directory) — `dispatch` treats those
//! differently (see `cli::dispatch`'s doc comment on the `Capture` arm).

use super::IoFailure;
use parallax_plumb::{adapter, config, manifest};
use std::path::{Path, PathBuf};

/// Failure running a capture.
#[derive(Debug)]
pub(super) enum CaptureCliError {
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
pub(super) fn run_capture(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(dir: &Path, yaml: &str) -> PathBuf {
        let path = dir.join("config.yaml");
        std::fs::write(&path, yaml).unwrap();
        path
    }

    const SAMPLE_CONFIG: &str = "scenarios:\n  - name: dial\n    adapter: command\n    args: 'x {out}.png'\n    touches: ['src/widgets/dial.rs']\n";

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

    #[test]
    fn a_missing_config_is_reported_as_capturecli_config_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing_config = tmp.path().join("does-not-exist.yaml");
        let run_dir = tmp.path().join("run1");

        let err = run_capture(&missing_config, &run_dir, "anything").unwrap_err();

        assert!(matches!(err, CaptureCliError::Config(_)));
    }

    #[test]
    fn an_uncreatable_run_dir_is_reported_as_capturecli_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), SAMPLE_CONFIG);
        // A regular file where a directory component is expected: no
        // platform can `create_dir_all` a path through a file.
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let run_dir = blocker.join("run1");

        let err = run_capture(&config, &run_dir, "dial").unwrap_err();

        assert!(matches!(err, CaptureCliError::Io(_)));
    }
}
