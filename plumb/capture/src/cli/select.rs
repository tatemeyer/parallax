//! `plumb select`: chooses which scenarios a change warrants
//! reviewing, by loading the config and delegating to
//! `select::select_by_name`/`select_by_paths`. The empty-selection
//! outcome is not an error — it is a legitimate `Selection` `dispatch`
//! inspects and reports (exit 3), never widened to "review everything."

use super::IoFailure;
use parallax_plumb::{config, select};
use std::path::Path;

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
pub(super) enum SelectCliError {
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
/// given.
pub(super) fn run_select(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
}
