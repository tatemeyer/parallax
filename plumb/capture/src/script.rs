//! Parses a snapshot script — a flat JSON array of wait/key/click steps
//! that a PTY-capture adapter drives a spawned target through.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// One step of a snapshot script: a real wall-clock pause, a named key
/// press, or a click, sent to the spawned target.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Step {
    /// Sleep `wait_ms` milliseconds of real time before the next step.
    Wait {
        /// Duration to sleep, in milliseconds.
        wait_ms: u64,
    },
    /// Send the named key (see `keys::encode_key`) to the spawned target.
    Key {
        /// The key's name, as written in the script (e.g. `"Right"`).
        key: String,
    },
    /// Send a left-button click at the given cell coordinates to the
    /// spawned target.
    Click {
        /// Column (0-indexed) to click.
        x: u16,
        /// Row (0-indexed) to click.
        y: u16,
    },
}

/// An I/O failure reading a script file, together with the path that
/// caused it — kept as a single field so `ScriptError::Io(_)` stays an
/// opaque, one-wildcard match for callers that only care *that* it
/// failed, not why. Mirrors `config::IoFailure`.
///
/// Deviation from `tools/visual-snapshot`'s `script.rs`: the source
/// wraps a bare `std::io::Error` via a blanket `From<std::io::Error>`
/// impl. Plumb's settled error convention removed blanket
/// `From<io::Error>` impls project-wide in favor of a path-carrying
/// wrapper constructed explicitly, so this struct and the explicit
/// `map_err` in `parse_script` below are a forced deviation, not a
/// stylistic choice.
#[derive(Debug)]
pub struct IoFailure {
    /// The path `parse_script` was asked to read.
    pub path: PathBuf,
    /// The underlying I/O error.
    pub source: std::io::Error,
}

/// A JSON parse failure, together with the path that caused it — kept
/// as a single field for the same reason as [`IoFailure`]: it keeps
/// `ScriptError::Json(_)` an opaque match. Same forced deviation as
/// [`IoFailure`]: the source used a bare `From<serde_json::Error>`.
#[derive(Debug)]
pub struct JsonFailure {
    /// The path `parse_script` was asked to read.
    pub path: PathBuf,
    /// The underlying JSON error.
    pub source: serde_json::Error,
}

/// Failure reading or parsing a snapshot script file.
#[derive(Debug)]
pub enum ScriptError {
    /// Underlying filesystem I/O failure reading the script file, and
    /// the path that caused it.
    Io(IoFailure),
    /// Failure parsing the file's contents as a script's JSON shape,
    /// and the path that caused it.
    Json(JsonFailure),
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::Io(e) => write!(f, "reading {}: {}", e.path.display(), e.source),
            ScriptError::Json(e) => write!(f, "parsing {}: {}", e.path.display(), e.source),
        }
    }
}
impl std::error::Error for ScriptError {}

/// Reads and parses a snapshot script: a flat JSON array of `{"wait_ms": N}`,
/// `{"key": "Name"}`, and `{"x": N, "y": N}` steps.
pub fn parse_script(path: &Path) -> Result<Vec<Step>, ScriptError> {
    let contents = std::fs::read_to_string(path).map_err(|source| {
        ScriptError::Io(IoFailure {
            path: path.to_path_buf(),
            source,
        })
    })?;
    let steps = serde_json::from_str(&contents).map_err(|source| {
        ScriptError::Json(JsonFailure {
            path: path.to_path_buf(),
            source,
        })
    })?;
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_mix_of_wait_and_key_steps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.json");
        std::fs::write(
            &path,
            r#"[{"wait_ms":16},{"key":"Right"},{"wait_ms":150},{"key":"Enter"}]"#,
        )
        .unwrap();

        let steps = parse_script(&path).unwrap();

        assert_eq!(
            steps,
            vec![
                Step::Wait { wait_ms: 16 },
                Step::Key {
                    key: "Right".to_string()
                },
                Step::Wait { wait_ms: 150 },
                Step::Key {
                    key: "Enter".to_string()
                },
            ]
        );
    }

    #[test]
    fn empty_script_parses_to_an_empty_vec() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.json");
        std::fs::write(&path, "[]").unwrap();

        assert_eq!(parse_script(&path).unwrap(), Vec::new());
    }

    #[test]
    fn malformed_json_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.json");
        std::fs::write(&path, "not json").unwrap();

        assert!(matches!(parse_script(&path), Err(ScriptError::Json(_))));
    }

    #[test]
    fn missing_file_is_an_error() {
        let missing = std::path::Path::new("/does/not/exist.json");
        assert!(matches!(parse_script(missing), Err(ScriptError::Io(_))));
    }

    #[test]
    fn parses_a_click_step() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.json");
        std::fs::write(&path, r#"[{"x":10,"y":5}]"#).unwrap();

        let steps = parse_script(&path).unwrap();

        assert_eq!(steps, vec![Step::Click { x: 10, y: 5 }]);
    }

    #[test]
    fn parses_a_mix_of_wait_key_and_click_steps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.json");
        std::fs::write(&path, r#"[{"wait_ms":16},{"key":"Enter"},{"x":10,"y":5}]"#).unwrap();

        let steps = parse_script(&path).unwrap();

        assert_eq!(
            steps,
            vec![
                Step::Wait { wait_ms: 16 },
                Step::Key {
                    key: "Enter".to_string()
                },
                Step::Click { x: 10, y: 5 },
            ]
        );
    }
}
