//! The run manifest: everything a lens agent is permitted to know about
//! a capture, and nothing else. Deliberately carries no command line,
//! no source paths, and no statement that anything changed — see the
//! blinding constraint in the Plumb design.

use crate::config::Expectation;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A limitation of the capture, disclosed to the lens agents so they
/// do not report it as a defect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Caveat {
    /// Cells rendering this codepoint show a placeholder box instead.
    UnmappedGlyphSubstituted {
        /// The codepoint, as `U+XXXX`.
        codepoint: String,
        /// How many cells were substituted.
        count: usize,
    },
}

/// One captured scenario, as described to the reviewer. This struct is
/// the blinding boundary: the only per-run data a lens agent ever sees.
/// It deliberately has no `args` field and no `touches` field — the
/// adapter's command line names source paths and tools, and `touches`
/// is a list of source files; neither may ever reach a prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunManifest {
    /// The run's timestamp id, shared by every scenario in the run.
    pub run_id: String,
    /// The scenario's name.
    pub scenario: String,
    /// Which adapter produced the image (`pty`/`window`/`command`).
    pub adapter: String,
    /// The captured image, relative to the run directory.
    pub image: PathBuf,
    /// 1 for a PNG, 2+ for an animated GIF.
    pub frame_count: usize,
    /// Terminal size as `COLSxROWS`, when the adapter knows it.
    pub size: Option<String>,
    /// The scenario's declared intent, for the intent lens.
    pub intent: Option<String>,
    /// Distortion this scenario declares intentional.
    pub expects: Vec<Expectation>,
    /// Disclosed limitations of this capture.
    pub caveats: Vec<Caveat>,
}

/// An I/O failure reading a manifest, together with the path that
/// caused it — kept as a single field so `ManifestError::Io(_)` stays
/// an opaque, one-wildcard match for callers that only care *that* it
/// failed, not why. Mirrors `config::IoFailure`.
#[derive(Debug)]
pub struct IoFailure {
    /// The path `read_manifest` was asked to read.
    pub path: PathBuf,
    /// The underlying I/O error.
    pub source: std::io::Error,
}

/// A JSON parse failure, together with the path that caused it — kept
/// as a single field for the same reason as [`IoFailure`]: it keeps
/// `ManifestError::Json(_)` an opaque match, which is what lets
/// `serde_json` stay swappable behind this module alone. Mirrors
/// `config::YamlFailure`.
#[derive(Debug)]
pub struct JsonFailure {
    /// The path `read_manifest` was asked to read.
    pub path: PathBuf,
    /// The underlying JSON error.
    pub source: serde_json::Error,
}

/// Failure reading or parsing a manifest.
#[derive(Debug)]
pub enum ManifestError {
    /// Filesystem failure reading the file, and the path that caused it.
    Io(IoFailure),
    /// Not valid JSON, or not this schema; and the path that caused it.
    Json(JsonFailure),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Io(e) => write!(f, "reading {}: {}", e.path.display(), e.source),
            ManifestError::Json(e) => write!(f, "parsing {}: {}", e.path.display(), e.source),
        }
    }
}
impl std::error::Error for ManifestError {}

/// A sortable UTC run id: `YYYYMMDDTHHMMSSZ`.
pub fn new_run_id() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Writes `m` to `<dir>/<scenario>.manifest.json`.
pub fn write_manifest(m: &RunManifest, dir: &Path) -> std::io::Result<PathBuf> {
    let path = dir.join(format!("{}.manifest.json", m.scenario));
    let json = serde_json::to_string_pretty(m)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Reads a manifest written by `write_manifest`.
pub fn read_manifest(path: &Path) -> Result<RunManifest, ManifestError> {
    let text = std::fs::read_to_string(path).map_err(|source| {
        ManifestError::Io(IoFailure {
            path: path.to_path_buf(),
            source,
        })
    })?;
    serde_json::from_str(&text).map_err(|source| {
        ManifestError::Json(JsonFailure {
            path: path.to_path_buf(),
            source,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Expectation;

    fn sample() -> RunManifest {
        RunManifest {
            run_id: "20260814T101500Z".into(),
            scenario: "omnitrix-dial-rotate".into(),
            adapter: "command".into(),
            image: std::path::PathBuf::from("omnitrix-dial-rotate.gif"),
            frame_count: 5,
            size: Some("120x40".into()),
            intent: Some("The dial rotates through four alien modes.".into()),
            expects: vec![Expectation::VisualCorruption],
            caveats: vec![Caveat::UnmappedGlyphSubstituted {
                codepoint: "U+2726".into(),
                count: 3,
            }],
        }
    }

    #[test]
    fn round_trips_through_json_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(&sample(), dir.path()).unwrap();
        let back = read_manifest(&path).unwrap();
        assert_eq!(back.scenario, "omnitrix-dial-rotate");
        assert_eq!(back.frame_count, 5);
        assert_eq!(back.expects, vec![Expectation::VisualCorruption]);
        assert_eq!(back.caveats.len(), 1);
    }

    #[test]
    fn manifest_lands_beside_the_images_as_manifest_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(&sample(), dir.path()).unwrap();
        assert_eq!(
            path.file_name().unwrap(),
            "omnitrix-dial-rotate.manifest.json"
        );
    }

    #[test]
    fn a_run_id_is_a_sortable_utc_timestamp() {
        let id = new_run_id();
        assert_eq!(id.len(), 16, "YYYYMMDDTHHMMSSZ");
        assert!(id.ends_with('Z') && id.contains('T'));
    }

    /// Guards the blinding boundary: the serialized manifest must not
    /// contain the adapter's command line or any source path. This is
    /// asserted on the serialized form, not just the struct, because
    /// the serialized form is what a lens agent literally reads.
    #[test]
    fn serialized_manifest_carries_no_command_line_and_no_source_paths() {
        let json = serde_json::to_string(&sample()).unwrap();
        for forbidden in ["cargo", "--example", "src/", "examples/", "touches", "args"] {
            assert!(
                !json.contains(forbidden),
                "manifest leaked {forbidden:?} to the reviewer: {json}"
            );
        }
    }

    /// A `ManifestError` is a user interface: a run directory holds one
    /// manifest per scenario, so its message must name *which path*
    /// failed to parse — not just that some manifest did.
    #[test]
    fn read_manifest_reports_the_offending_path_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-json.manifest.json");
        std::fs::write(&path, "not json").unwrap();
        let err = read_manifest(&path).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(&path.display().to_string()),
            "error message should name the offending path {path:?}: {msg}"
        );
    }

    /// Same as above for the I/O variant: the message must name the
    /// specific missing path, not just that reading failed.
    #[test]
    fn read_manifest_missing_file_names_the_failure_kind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.manifest.json");
        let err = read_manifest(&path).unwrap_err();
        assert!(matches!(err, ManifestError::Io(_)));
        let msg = err.to_string();
        assert!(
            msg.contains(&path.display().to_string()),
            "error message should name the offending path {path:?}: {msg}"
        );
    }
}
