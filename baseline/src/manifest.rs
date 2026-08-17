//! The `parallax.yaml` schema and its parser. A project joins the
//! platform by dropping one of these in its root. Deliberately tolerant
//! of missing sections — partial support is normal, not an error path —
//! and deliberately intolerant of unknown keys, so a typo'd section
//! never silently vanishes.

use crate::autonomy::AutonomyMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One project's declared references.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    /// Schema version, e.g. `parallax/v1`. Optional.
    #[serde(default)]
    pub api_version: Option<String>,
    /// Who this project is.
    pub project: Project,
    /// The work feed, if declared.
    #[serde(default)]
    pub work: Option<Work>,
    /// Declared verification checks. Empty when none.
    #[serde(default)]
    pub verification: Vec<VerificationEntry>,
    /// Declared artifact feeds. Empty when none.
    #[serde(default)]
    pub artifacts: Vec<ArtifactEntry>,
    /// The agent-session feed, if declared.
    #[serde(default)]
    pub sessions: Option<Sessions>,
}

/// Project identity. `methodology` is informational metadata only —
/// nothing in this crate branches on it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// The project's short name, unique across the platform.
    pub name: String,
    /// Absolute path to the project root. Defaults to the manifest's
    /// own directory when parsed from a file.
    #[serde(default)]
    pub root: Option<PathBuf>,
    /// Primary language, for display only.
    #[serde(default)]
    pub language: Option<String>,
    /// Declared development methodology. **Informational only.**
    #[serde(default)]
    pub methodology: Option<String>,
}

/// The work feed: issues, pull requests, and their autonomy labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    /// Which work adapter serves this project.
    pub adapter: WorkAdapterKind,
    /// The adapter's repository argument, `owner/name`.
    pub repo: String,
    /// This project's native labels and what each projects onto.
    #[serde(default)]
    pub autonomy_map: AutonomyMap,
}

/// Built-in work adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkAdapterKind {
    /// Issues, pull requests, labels, and check runs from GitHub.
    Github,
}

/// One declared verification check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationEntry {
    /// A display label such as `lint`, `tests`, or `perceptual`.
    /// Never a dispatch key — `adapter` is.
    pub kind: String,
    /// Which verification adapter runs this check.
    pub adapter: VerificationAdapterKind,
    /// The shell command, for the `command` adapter.
    #[serde(default)]
    pub command: Option<String>,
    /// A config path, for the `plumb` adapter.
    #[serde(default)]
    pub config: Option<PathBuf>,
}

/// Built-in verification adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerificationAdapterKind {
    /// Runs a shell command and reads its exit status.
    Command,
    /// Reads a Plumb `verdict.md` from disk. Does not link Plumb.
    Plumb,
}

/// One declared artifact feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactEntry {
    /// What kind of artifact this feed produces.
    pub kind: ArtifactKind,
    /// Which artifact adapter reads it. Defaults from `kind`.
    #[serde(default)]
    pub adapter: Option<ArtifactAdapterKind>,
    /// A glob, relative to the project root.
    pub watch: String,
}

/// What an artifact feed produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    /// Pre-rendered images.
    Figure,
    /// Scalar series.
    Metrics,
    /// Terminal captures with their verdicts.
    Capture,
}

/// Built-in artifact adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactAdapterKind {
    /// Image files: path, size, modification time.
    Figure,
    /// JSONL scalar series. Also spelled `jsonl` in a manifest.
    #[serde(alias = "jsonl")]
    Metrics,
    /// Plumb run directories: run id plus verdict.
    Capture,
}

/// The agent-session feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sessions {
    /// A glob of session directories, relative to the project root.
    pub watch: String,
}

/// Failure reading or parsing a manifest.
#[derive(Debug)]
pub enum ManifestError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// Not valid YAML, or not this schema.
    Yaml(serde_yaml::Error),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Io(e) => write!(f, "reading manifest: {e}"),
            ManifestError::Yaml(e) => write!(f, "parsing manifest: {e}"),
        }
    }
}
impl std::error::Error for ManifestError {}

/// Parses a manifest from YAML text.
pub fn parse_manifest(yaml: &str) -> Result<Manifest, ManifestError> {
    serde_yaml::from_str(yaml).map_err(ManifestError::Yaml)
}

/// Parses a manifest from a file, defaulting `project.root` to the
/// manifest's own directory when it declares none.
pub fn parse_manifest_file(path: &Path) -> Result<Manifest, ManifestError> {
    let text = std::fs::read_to_string(path).map_err(ManifestError::Io)?;
    let mut manifest = parse_manifest(&text)?;
    if manifest.project.root.is_none() {
        manifest.project.root = path.parent().map(Path::to_path_buf);
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTUI: &str = r#"
apiVersion: parallax/v1
project:
  name: ttui
  root: <projects-root>/TTUI
  language: rust
  methodology: methodology-first
work:
  adapter: github
  repo: tatemeyer/ttui
  autonomy_map:
    direct: { implement: agent, merge: direct-push }
    gated:  { implement: agent, merge: on-checks }
    human:  { implement: agent, merge: human-approval }
verification:
  - kind: lint
    adapter: command
    command: cargo clippy --all-targets -- -D warnings
  - kind: tests
    adapter: command
    command: cargo test
  - kind: perceptual
    adapter: plumb
    config: .plumb/config.yaml
artifacts:
  - kind: capture
    watch: .plumb/runs/**
sessions:
  watch: .claude/worktrees/*
"#;

    #[test]
    fn parses_the_ttui_manifest_end_to_end() {
        let m = parse_manifest(TTUI).unwrap();
        assert_eq!(m.api_version.as_deref(), Some("parallax/v1"));
        assert_eq!(m.project.name, "ttui");
        assert_eq!(m.project.language.as_deref(), Some("rust"));
        assert_eq!(m.project.methodology.as_deref(), Some("methodology-first"));
        let work = m.work.unwrap();
        assert_eq!(work.adapter, WorkAdapterKind::Github);
        assert_eq!(work.repo, "tatemeyer/ttui");
        assert_eq!(
            work.autonomy_map.labels().collect::<Vec<_>>(),
            vec!["direct", "gated", "human"]
        );
        assert_eq!(m.verification.len(), 3);
        assert_eq!(m.verification[0].kind, "lint");
        assert_eq!(m.verification[0].adapter, VerificationAdapterKind::Command);
        assert_eq!(m.verification[2].adapter, VerificationAdapterKind::Plumb);
        assert_eq!(
            m.verification[2].config.as_deref(),
            Some(std::path::Path::new(".plumb/config.yaml"))
        );
        assert_eq!(m.artifacts.len(), 1);
        assert_eq!(m.artifacts[0].kind, ArtifactKind::Capture);
        assert_eq!(m.artifacts[0].watch, ".plumb/runs/**");
        assert_eq!(m.sessions.unwrap().watch, ".claude/worktrees/*");
    }

    /// Model-Experiments' manifest omits apiVersion and root, and writes
    /// `adapter: jsonl` for its metrics feed. All three must parse.
    #[test]
    fn parses_model_experiments_shape_including_the_jsonl_adapter_alias() {
        let yaml = r#"
project:
  name: model-experiments
  language: python
  methodology: outcome-first
work:
  adapter: github
  repo: tatemeyer/Model-Experiments
  autonomy_map:
    "autonomy:safe":   { implement: agent, merge: on-checks }
    "autonomy:human":  { implement: human-only }
    "needs-intent":    { readiness: needs-intent }
artifacts:
  - kind: figure
    watch: projects/*/results/**/*.png
  - kind: metrics
    adapter: jsonl
    watch: projects/*/results/**/*.jsonl
"#;
        let m = parse_manifest(yaml).unwrap();
        assert_eq!(m.api_version, None);
        assert_eq!(m.project.root, None);
        assert_eq!(m.artifacts[0].adapter, None, "figure declares no adapter");
        assert_eq!(
            m.artifacts[1].adapter,
            Some(ArtifactAdapterKind::Metrics),
            "`jsonl` selects the metrics adapter"
        );
        assert_eq!(m.sessions, None);
        assert!(m.verification.is_empty());
    }

    /// The spec's headline partial case.
    #[test]
    fn a_manifest_declaring_only_work_parses() {
        let yaml = r#"
project:
  name: minimal
work:
  adapter: github
  repo: tatemeyer/minimal
  autonomy_map:
    gated: { implement: agent, merge: on-checks }
"#;
        let m = parse_manifest(yaml).unwrap();
        assert!(m.work.is_some());
        assert!(m.verification.is_empty());
        assert!(m.artifacts.is_empty());
        assert_eq!(m.sessions, None);
    }

    #[test]
    fn a_manifest_declaring_only_a_project_parses() {
        let m = parse_manifest("project:\n  name: bare\n").unwrap();
        assert_eq!(m.project.name, "bare");
        assert!(m.work.is_none());
    }

    #[test]
    fn an_unknown_adapter_name_is_a_parse_error_naming_the_field() {
        let yaml =
            "project:\n  name: x\nwork:\n  adapter: gitlab\n  repo: a/b\n  autonomy_map: {}\n";
        let err = parse_manifest(yaml).unwrap_err().to_string();
        assert!(
            err.contains("gitlab"),
            "error should name the offending value: {err}"
        );
    }

    #[test]
    fn an_unknown_top_level_key_is_a_parse_error_rather_than_silently_ignored() {
        let yaml = "project:\n  name: x\nverifications:\n  - kind: tests\n";
        assert!(
            parse_manifest(yaml).is_err(),
            "a typo'd section must not vanish silently"
        );
    }

    #[test]
    fn parse_manifest_file_defaults_root_to_the_manifest_s_own_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parallax.yaml");
        std::fs::write(&path, "project:\n  name: local\n").unwrap();
        let m = parse_manifest_file(&path).unwrap();
        assert_eq!(m.project.root.as_deref(), Some(dir.path()));
    }

    #[test]
    fn an_explicit_root_is_not_overwritten_by_the_manifest_s_location() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parallax.yaml");
        std::fs::write(&path, "project:\n  name: local\n  root: D:/Elsewhere\n").unwrap();
        let m = parse_manifest_file(&path).unwrap();
        assert_eq!(
            m.project.root.as_deref(),
            Some(std::path::Path::new("D:/Elsewhere"))
        );
    }
}
