//! Semantic validation of a parsed manifest: the rules serde cannot
//! express. Resolves adapter defaults, checks cross-field consistency,
//! and reports every problem at once rather than the first. Produces a
//! `Validated` whose private field means nothing downstream can
//! aggregate an unchecked manifest.

use crate::manifest::{
    ArtifactAdapterKind, ArtifactEntry, ArtifactKind, Manifest, VerificationAdapterKind,
    VerificationEntry,
};
use std::path::PathBuf;

/// One of the four adapter families a manifest may declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Issues and pull requests.
    Work,
    /// Checks that decide whether work is done.
    Verification,
    /// Files a run produced.
    Artifact,
    /// Agent working directories.
    Session,
}

/// One thing wrong with a manifest, located by field path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Dotted path to the offending field, e.g. `verification[0].command`.
    pub field: String,
    /// What is wrong with it, in one sentence.
    pub problem: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.problem)
    }
}
impl std::error::Error for ValidationError {}

/// Which artifact adapter serves an entry, defaulting from its kind.
///
/// A free function because `validate` has to answer this before a
/// [`Validated`] exists — the rule about `identifiers` is a rule about
/// *which reader* an entry gets, so it cannot wait for the type that
/// says the manifest is already fine.
///
/// A `metrics` feed defaults to the JSONL reader, so `csv` is only ever
/// chosen by a manifest that says `csv`, and every manifest written
/// before that adapter existed keeps the reader it had.
pub fn artifact_adapter_of(entry: &ArtifactEntry) -> ArtifactAdapterKind {
    entry.adapter.unwrap_or(match entry.kind {
        ArtifactKind::Figure => ArtifactAdapterKind::Figure,
        ArtifactKind::Metrics => ArtifactAdapterKind::Metrics,
        ArtifactKind::Capture => ArtifactAdapterKind::Capture,
    })
}

/// A manifest that passed validation. Only `validate` can build one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated {
    manifest: Manifest,
}

impl Validated {
    /// The manifest inside.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Consumes this wrapper, returning the manifest.
    pub fn into_manifest(self) -> Manifest {
        self.manifest
    }

    /// Whether the manifest declares this adapter family at all.
    pub fn declares(&self, family: Family) -> bool {
        match family {
            Family::Work => self.manifest.work.is_some(),
            Family::Verification => !self.manifest.verification.is_empty(),
            Family::Artifact => !self.manifest.artifacts.is_empty(),
            Family::Session => self.manifest.sessions.is_some(),
        }
    }

    /// Which artifact adapter serves an entry, defaulting from its kind.
    pub fn artifact_adapter(&self, entry: &ArtifactEntry) -> ArtifactAdapterKind {
        artifact_adapter_of(entry)
    }

    /// The Plumb config path for an entry, defaulting to
    /// `.plumb/config.yaml`.
    pub fn plumb_config(&self, entry: &VerificationEntry) -> PathBuf {
        entry
            .config
            .clone()
            .unwrap_or_else(|| PathBuf::from(".plumb/config.yaml"))
    }
}

/// Validates a parsed manifest, reporting every problem found.
pub fn validate(manifest: Manifest) -> Result<Validated, Vec<ValidationError>> {
    let mut errors = Vec::new();

    if manifest.project.name.trim().is_empty() {
        errors.push(ValidationError {
            field: "project.name".into(),
            problem: "a project must have a non-empty name".into(),
        });
    }

    if let Some(work) = &manifest.work {
        let parts: Vec<&str> = work.repo.split('/').collect();
        if parts.len() != 2 || parts.iter().any(|p| p.trim().is_empty()) {
            errors.push(ValidationError {
                field: "work.repo".into(),
                problem: format!("expected `owner/name`, got `{}`", work.repo),
            });
        }
        for label in work.autonomy_map.labels() {
            let entry = work
                .autonomy_map
                .entry(label)
                .expect("label came from this map");
            if entry.implement.is_none() && entry.merge.is_none() && entry.readiness.is_none() {
                errors.push(ValidationError {
                    field: format!("work.autonomy_map.{label}"),
                    problem: "claims nothing on any axis, so the label carries no meaning".into(),
                });
            }
        }
    }

    for (i, entry) in manifest.verification.iter().enumerate() {
        if entry.adapter == VerificationAdapterKind::Command && entry.command.is_none() {
            errors.push(ValidationError {
                field: format!("verification[{i}].command"),
                problem: "the `command` adapter requires a command".into(),
            });
        }
        if entry.adapter == VerificationAdapterKind::Plumb && entry.command.is_some() {
            errors.push(ValidationError {
                field: format!("verification[{i}].command"),
                problem: "the `plumb` adapter reads a verdict file and takes no command".into(),
            });
        }
    }

    for (i, entry) in manifest.artifacts.iter().enumerate() {
        if let Err(e) = globset::Glob::new(&entry.watch) {
            errors.push(ValidationError {
                field: format!("artifacts[{i}].watch"),
                problem: format!("not a valid glob: {e}"),
            });
        }
        // `identifiers` answers a question only CSV asks — a JSONL
        // producer says the same thing by writing a column as a number.
        // On any other feed the key would be read by nobody and quietly
        // do nothing, which is a declaration nothing backs.
        if !entry.identifiers.is_empty() && artifact_adapter_of(entry) != ArtifactAdapterKind::Csv {
            errors.push(ValidationError {
                field: format!("artifacts[{i}].identifiers"),
                problem: "only a `csv` feed declares identifier columns; a JSONL producer \
                          says the same thing by writing them as numbers"
                    .into(),
            });
        }
        if entry
            .identifiers
            .iter()
            .any(|column| column.trim().is_empty())
        {
            errors.push(ValidationError {
                field: format!("artifacts[{i}].identifiers"),
                problem: "a column name here is blank".into(),
            });
        }
    }

    if let Some(sessions) = &manifest.sessions {
        if let Err(e) = globset::Glob::new(&sessions.watch) {
            errors.push(ValidationError {
                field: "sessions.watch".into(),
                problem: format!("not a valid glob: {e}"),
            });
        }
    }

    if errors.is_empty() {
        Ok(Validated { manifest })
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_manifest;

    fn valid_yaml() -> &'static str {
        r#"
project:
  name: ttui
  root: <projects-root>/TTUI
work:
  adapter: github
  repo: tatemeyer/ttui
  autonomy_map:
    gated: { implement: agent, merge: on-checks }
verification:
  - kind: tests
    adapter: command
    command: cargo test
  - kind: perceptual
    adapter: plumb
artifacts:
  - kind: metrics
    watch: results/**/*.jsonl
sessions:
  watch: .claude/worktrees/*
"#
    }

    fn validated(yaml: &str) -> Validated {
        validate(parse_manifest(yaml).unwrap()).expect("should validate")
    }

    #[test]
    fn a_complete_manifest_validates() {
        let v = validated(valid_yaml());
        assert!(v.declares(Family::Work));
        assert!(v.declares(Family::Verification));
        assert!(v.declares(Family::Artifact));
        assert!(v.declares(Family::Session));
    }

    #[test]
    fn an_omitted_artifact_adapter_defaults_from_its_kind() {
        let v = validated(valid_yaml());
        let entry = &v.manifest().artifacts[0];
        assert_eq!(v.artifact_adapter(entry), ArtifactAdapterKind::Metrics);
    }

    /// The thing that could not be said before. `ArtifactAdapterKind`
    /// is a closed enum, so a producer keeping tidy long-format CSV had
    /// no way to declare the file it already had.
    #[test]
    fn a_csv_metrics_feed_can_be_declared() {
        let yaml = "project:\n  name: x\nartifacts:\n  - kind: metrics\n    adapter: csv\n    watch: 'projects/*/results.csv'\n    identifiers: [seed]\n";
        let v = validate(parse_manifest(yaml).unwrap()).expect("a csv feed validates");
        let entry = &v.manifest().artifacts[0];
        assert_eq!(v.artifact_adapter(entry), ArtifactAdapterKind::Csv);
        assert_eq!(entry.identifiers, vec!["seed".to_string()]);
    }

    /// A `metrics` feed still means JSONL unless it says otherwise, so
    /// every manifest written before the CSV reader existed keeps the
    /// reader it had.
    #[test]
    fn a_metrics_feed_still_defaults_to_the_jsonl_reader() {
        let yaml = "project:\n  name: x\nartifacts:\n  - kind: metrics\n    watch: 'out/*.jsonl'\n";
        let v = validate(parse_manifest(yaml).unwrap()).unwrap();
        assert_eq!(
            v.artifact_adapter(&v.manifest().artifacts[0]),
            ArtifactAdapterKind::Metrics
        );
    }

    /// `identifiers` answers a question only CSV asks. On a JSONL feed
    /// nobody would read it, and a key nobody reads is a declaration
    /// nothing backs.
    #[test]
    fn identifier_columns_on_a_feed_that_has_types_are_refused() {
        let yaml = "project:\n  name: x\nartifacts:\n  - kind: metrics\n    adapter: jsonl\n    watch: 'out/*.jsonl'\n    identifiers: [seed]\n";
        let errors = validate(parse_manifest(yaml).unwrap()).expect_err("refused");
        assert!(
            errors
                .iter()
                .any(|e| e.field == "artifacts[0].identifiers" && e.problem.contains("csv")),
            "got {errors:?}"
        );
    }

    #[test]
    fn a_blank_identifier_column_is_refused() {
        let yaml = "project:\n  name: x\nartifacts:\n  - kind: metrics\n    adapter: csv\n    watch: 'out/*.csv'\n    identifiers: ['  ']\n";
        let errors = validate(parse_manifest(yaml).unwrap()).expect_err("refused");
        assert!(
            errors.iter().any(|e| e.field == "artifacts[0].identifiers"),
            "got {errors:?}"
        );
    }

    #[test]
    fn an_explicit_artifact_adapter_overrides_the_default_from_kind() {
        let yaml = "project:\n  name: x\nartifacts:\n  - kind: capture\n    adapter: figure\n    watch: 'runs/**'\n";
        let v = validated(yaml);
        assert_eq!(
            v.artifact_adapter(&v.manifest().artifacts[0]),
            ArtifactAdapterKind::Figure
        );
    }

    #[test]
    fn an_omitted_plumb_config_defaults_to_dot_plumb_config_yaml() {
        let v = validated(valid_yaml());
        let entry = &v.manifest().verification[1];
        assert_eq!(
            v.plumb_config(entry),
            std::path::PathBuf::from(".plumb/config.yaml")
        );
    }

    #[test]
    fn a_command_adapter_without_a_command_is_a_validation_error() {
        let yaml = "project:\n  name: x\nverification:\n  - kind: tests\n    adapter: command\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(
            errs[0].field.contains("verification[0].command"),
            "got {:?}",
            errs[0]
        );
    }

    #[test]
    fn an_empty_project_name_is_a_validation_error() {
        let errs = validate(parse_manifest("project:\n  name: ''\n").unwrap()).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "project.name"));
    }

    #[test]
    fn a_work_repo_that_is_not_owner_slash_name_is_a_validation_error() {
        let yaml =
            "project:\n  name: x\nwork:\n  adapter: github\n  repo: ttui\n  autonomy_map: {}\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "work.repo"));
    }

    #[test]
    fn an_unparseable_watch_glob_is_a_validation_error_naming_the_entry() {
        let yaml = "project:\n  name: x\nartifacts:\n  - kind: figure\n    watch: '['\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "artifacts[0].watch"));
    }

    /// Every problem is reported, not just the first — a manifest author
    /// should not have to fix one line, re-run, and find the next.
    #[test]
    fn every_problem_is_reported_at_once() {
        let yaml =
            "project:\n  name: ''\nwork:\n  adapter: github\n  repo: nope\n  autonomy_map: {}\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert_eq!(errs.len(), 2);
    }

    /// An autonomy_map entry that claims nothing at all is a mistake:
    /// the label would project onto an Autonomy indistinguishable from
    /// having no label.
    #[test]
    fn an_autonomy_map_entry_claiming_nothing_on_any_axis_is_an_error() {
        let yaml = "project:\n  name: x\nwork:\n  adapter: github\n  repo: a/b\n  autonomy_map:\n    weird: {}\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert!(errs.iter().any(|e| e.field.contains("weird")));
    }

    /// A declared work feed with no labels is legitimate: the project
    /// simply does not use autonomy labels yet.
    #[test]
    fn an_empty_autonomy_map_is_legitimate() {
        let yaml =
            "project:\n  name: x\nwork:\n  adapter: github\n  repo: a/b\n  autonomy_map: {}\n";
        assert!(validate(parse_manifest(yaml).unwrap()).is_ok());
    }

    #[test]
    fn methodology_is_never_validated_against_a_known_set() {
        let yaml = "project:\n  name: x\n  methodology: whatever-i-feel-like\n";
        assert!(validate(parse_manifest(yaml).unwrap()).is_ok());
    }
}
