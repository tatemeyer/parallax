//! Building a project's adapters from its validated manifest.
//!
//! The manifest's meaning lives here and nowhere else: `adapter: github`
//! means "poll GitHub with conditional requests at the configured
//! interval," and that sentence has to have exactly one implementation
//! or the manifest stops being a specification. A frontend that
//! translates manifests owns part of the schema.

use super::artifact::{
    CaptureArtifactAdapter, CsvMetricsArtifactAdapter, FigureArtifactAdapter,
    MetricsArtifactAdapter,
};
use super::http::{HttpTransport, UreqTransport};
use super::session::FilesystemSessionAdapter;
use super::verification::{
    CommandRunner, CommandVerificationAdapter, PlumbVerificationAdapter, ProcessRunner,
};
use super::work::GithubWorkAdapter;
use crate::actions::{GithubWorkControl, LocalExecutor, LocalProcessControl};
use crate::freshness::DEFAULT_POLL_INTERVAL;
use crate::manifest::{
    ArtifactAdapterKind, VerificationAdapterKind, VerificationEntry, WorkAdapterKind,
};
use crate::state::ProjectAdapters;
use crate::validate::Validated;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the built-in adapters need that the manifest does not say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterConfig {
    /// How often polled sources are refreshed.
    pub poll_interval: Duration,
    /// The token the GitHub adapter authenticates with, when there is
    /// one. **Passed in, never read from the environment** — a library
    /// that reaches for `GITHUB_TOKEN` cannot be tested twice on the
    /// same machine with different answers.
    pub github_token: Option<String>,
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            poll_interval: DEFAULT_POLL_INTERVAL,
            github_token: None,
        }
    }
}

/// Where a `plumb` entry's runs live: `<config parent>/runs`, resolved
/// against the project root.
///
/// The manifest declares a config path, not a runs path — TTUI writes
/// `config: .plumb/config.yaml` and Plumb writes its runs to
/// `.plumb/runs/`. This is a convention rather than a declaration, and
/// the day a project disagrees the escape hatch is a `runs:` key on the
/// entry, deliberately not added while nothing needs it.
fn plumb_runs_dir(validated: &Validated, entry: &VerificationEntry) -> PathBuf {
    let config = validated.plumb_config(entry);
    let parent = config.parent().unwrap_or_else(|| Path::new(""));
    let root = validated
        .manifest()
        .project
        .root
        .clone()
        .unwrap_or_default();
    root.join(parent).join("runs")
}

/// Builds a project's adapters from its validated manifest, taking the
/// transport and runner from factories so each adapter owns its own.
///
/// Cannot fail: `validate` has already rejected everything that would
/// make an adapter unconstructible.
pub fn from_manifest_with<T, R>(
    validated: &Validated,
    config: &AdapterConfig,
    transport: impl Fn() -> T,
    runner: impl Fn() -> R,
) -> ProjectAdapters
where
    T: HttpTransport + Send + 'static,
    R: CommandRunner + Send + 'static,
{
    let manifest = validated.manifest();
    let mut adapters = ProjectAdapters::new();

    if let Some(work) = &manifest.work {
        // Exhaustive with one arm on purpose: adding a second work
        // adapter must not compile until this line chooses between them.
        adapters.work = Some(match work.adapter {
            WorkAdapterKind::Github => {
                Box::new(GithubWorkAdapter::new(transport()).with_interval(config.poll_interval))
            }
        });
    }

    for entry in &manifest.verification {
        adapters.verification.push(match entry.adapter {
            VerificationAdapterKind::Command => Box::new(CommandVerificationAdapter::new(
                entry.kind.clone(),
                entry
                    .command
                    .clone()
                    .expect("validation rejects a command adapter with no command"),
                runner(),
            )),
            VerificationAdapterKind::Plumb => Box::new(PlumbVerificationAdapter::new(
                entry.kind.clone(),
                plumb_runs_dir(validated, entry),
            )),
        });
    }

    for entry in &manifest.artifacts {
        let watch = entry.watch.clone();
        adapters
            .artifacts
            .push(match validated.artifact_adapter(entry) {
                ArtifactAdapterKind::Figure => Box::new(FigureArtifactAdapter::new(watch)),
                ArtifactAdapterKind::Metrics => Box::new(MetricsArtifactAdapter::new(watch)),
                ArtifactAdapterKind::Csv => Box::new(CsvMetricsArtifactAdapter::new(
                    watch,
                    entry.identifiers.clone(),
                )),
                ArtifactAdapterKind::Capture => Box::new(CaptureArtifactAdapter::new(watch)),
            });
    }

    if let Some(sessions) = &manifest.sessions {
        adapters.sessions = Some(Box::new(FilesystemSessionAdapter::new(
            sessions.watch.clone(),
        )));
    }

    adapters
}

/// Builds a project's adapters against the live world: `UreqTransport`
/// for work, `ProcessRunner` for `command` verification.
///
/// This is the only place those two are named, which keeps the network
/// and process seams countable.
pub fn from_manifest(validated: &Validated, config: &AdapterConfig) -> ProjectAdapters {
    let token = config.github_token.clone();
    from_manifest_with(
        validated,
        config,
        move || match &token {
            Some(t) => UreqTransport::with_token(t.clone()),
            None => UreqTransport::new(),
        },
        || ProcessRunner,
    )
}

/// The executor for one project: GitHub for work, the local shell for
/// processes, rulings appended beside the project's Plumb state.
///
/// `None` when the project declares no work feed — there is nothing for
/// a work action to address, and an executor that cannot name a
/// repository would fail at the point of use instead of at the point of
/// construction.
pub fn executor_for(
    validated: &Validated,
    config: &AdapterConfig,
) -> Option<LocalExecutor<GithubWorkControl<UreqTransport>, LocalProcessControl<ProcessRunner>>> {
    let work = validated.manifest().work.as_ref()?;
    let transport = match &config.github_token {
        Some(t) => UreqTransport::with_token(t.clone()),
        None => UreqTransport::new(),
    };
    // The manifest's declared root, which the registry has already
    // overridden with where the project was actually found.
    let root: PathBuf = validated
        .manifest()
        .project
        .root
        .clone()
        .unwrap_or_default();
    Some(LocalExecutor::new(
        work.repo.clone(),
        root.join(".plumb").join("rulings.jsonl"),
        GithubWorkControl::new(transport),
        LocalProcessControl::new(ProcessRunner, root),
    ))
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn the_default_config_polls_at_the_crate_default_and_carries_no_token() {
        let c = AdapterConfig::default();
        assert_eq!(c.poll_interval, DEFAULT_POLL_INTERVAL);
        assert_eq!(c.github_token, None);
    }
}

#[cfg(test)]
mod translation_tests {
    use super::*;
    use crate::adapters::http::FixtureTransport;
    use crate::adapters::verification::{CheckCost, ScriptedRunner};
    use crate::manifest::parse_manifest;
    use crate::validate::validate;

    fn built(yaml: &str) -> ProjectAdapters {
        let validated = validate(parse_manifest(yaml).expect("parses")).expect("validates");
        from_manifest_with(
            &validated,
            &AdapterConfig::default(),
            FixtureTransport::new,
            ScriptedRunner::new,
        )
    }

    fn validated_of(yaml: &str) -> Validated {
        validate(parse_manifest(yaml).expect("parses")).expect("validates")
    }

    const TTUI_LIKE: &str = r#"
apiVersion: parallax/v1
project:
  name: ttui
  root: /tmp/p
  language: rust
  methodology: methodology-first
work:
  adapter: github
  repo: tatemeyer/ttui
  autonomy_map:
    gated: { implement: agent, merge: on-checks }
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
    fn a_declared_github_work_feed_becomes_a_github_adapter() {
        let a = built(
            "project:
  name: p
  root: /tmp/p
work:
  adapter: github
  repo: a/b
  autonomy_map: {}
",
        );
        assert_eq!(
            a.work.as_ref().map(|w| w.source_name()),
            Some("work:github".to_string())
        );
    }

    #[test]
    fn a_manifest_with_no_feeds_builds_no_adapters() {
        let a = built(
            "project:
  name: p
  root: /tmp/p
",
        );
        assert!(a.work.is_none());
        assert!(a.verification.is_empty());
        assert!(a.artifacts.is_empty());
        assert!(a.sessions.is_none());
    }

    #[test]
    fn a_declared_session_feed_becomes_a_filesystem_session_adapter() {
        let a = built(
            "project:
  name: p
  root: /tmp/p
sessions:
  watch: '.claude/worktrees/*'
",
        );
        assert_eq!(
            a.sessions.as_ref().map(|s| s.source_name()),
            Some("session:filesystem".to_string())
        );
    }

    #[test]
    fn each_verification_entry_becomes_its_declared_adapter_in_order() {
        let a = built(TTUI_LIKE);
        let names: Vec<String> = a.verification.iter().map(|v| v.source_name()).collect();
        assert_eq!(
            names,
            vec![
                "verification:command:lint".to_string(),
                "verification:command:tests".to_string(),
                "verification:plumb:perceptual".to_string(),
            ]
        );
    }

    /// The cost hint from Arc 1, carried through the factory: a
    /// scheduler must be able to partition what it just built.
    #[test]
    fn the_built_adapters_carry_their_cost() {
        let a = built(TTUI_LIKE);
        assert_eq!(a.verification[0].cost(), CheckCost::Execute);
        assert_eq!(a.verification[1].cost(), CheckCost::Execute);
        assert_eq!(a.verification[2].cost(), CheckCost::Read);
    }

    #[test]
    fn the_plumb_runs_directory_is_the_config_s_parent_plus_runs() {
        let v = validated_of(TTUI_LIKE);
        let entry = &v.manifest().verification[2];
        assert_eq!(
            plumb_runs_dir(&v, entry),
            PathBuf::from("/tmp/p/.plumb/runs")
        );
    }

    /// An entry that declares no config still gets the default
    /// `.plumb/config.yaml`, and therefore the same runs directory.
    #[test]
    fn an_entry_with_no_declared_config_still_resolves_to_dot_plumb_runs() {
        let v = validated_of(
            "project:
  name: p
  root: /tmp/p
verification:
  - kind: perceptual
    adapter: plumb
",
        );
        let entry = &v.manifest().verification[0];
        assert_eq!(
            plumb_runs_dir(&v, entry),
            PathBuf::from("/tmp/p/.plumb/runs")
        );
    }

    #[test]
    fn each_artifact_entry_becomes_the_adapter_its_kind_resolves_to() {
        let a = built(
            "project:
  name: p
  root: /tmp/p
artifacts:
  - kind: figure
    watch: 'out/**/*.png'
  - kind: metrics
    adapter: jsonl
    watch: 'r/**/*.jsonl'
  - kind: capture
    watch: '.plumb/runs/**'
",
        );
        let names: Vec<String> = a.artifacts.iter().map(|x| x.source_name()).collect();
        assert_eq!(
            names,
            vec![
                "artifact:figure".to_string(),
                "artifact:metrics".to_string(),
                "artifact:capture".to_string(),
            ]
        );
    }

    #[test]
    fn the_live_wrapper_builds_the_same_shape_as_the_generic_form() {
        let v = validated_of(TTUI_LIKE);
        let live = from_manifest(&v, &AdapterConfig::default());
        assert!(live.work.is_some());
        assert_eq!(live.verification.len(), 3);
        assert_eq!(live.artifacts.len(), 1);
        assert!(live.sessions.is_some());
    }

    /// The guard behind `from_manifest_with`'s one `expect`: if this
    /// ever fails, the factory panics on a manifest a caller was told
    /// was valid.
    #[test]
    fn validation_still_rejects_a_command_entry_with_no_command() {
        let m = parse_manifest(
            "project:
  name: p
verification:
  - kind: tests
    adapter: command
",
        )
        .unwrap();
        assert!(validate(m).is_err(), "the factory's expect depends on this");
    }
}

#[cfg(test)]
mod executor_tests {
    use super::*;

    fn validated(yaml: &str) -> Validated {
        crate::validate::validate(serde_yaml::from_str(yaml).expect("parses")).expect("validates")
    }

    const WITH_WORK: &str = "project:
  name: ttui
  root: D:/Dev/TTUI
work:
  adapter: github
  repo: tatemeyer/ttui
  autonomy_map: {}
";

    #[test]
    fn an_executor_addresses_the_repository_the_manifest_declares() {
        let v = validated(WITH_WORK);
        let e = executor_for(&v, &AdapterConfig::default()).expect("work is declared");
        assert_eq!(e.repo(), "tatemeyer/ttui");
    }

    /// Rulings live beside the project's other Plumb state, not in a
    /// directory the cockpit invents.
    #[test]
    fn rulings_are_appended_inside_the_project_s_plumb_directory() {
        let v = validated(WITH_WORK);
        let e = executor_for(&v, &AdapterConfig::default()).unwrap();
        assert_eq!(
            e.rulings_path(),
            Path::new("D:/Dev/TTUI")
                .join(".plumb")
                .join("rulings.jsonl")
        );
    }

    /// No work feed, no repository to address. Failing here beats
    /// failing at the moment the operator confirms a merge.
    #[test]
    fn a_project_with_no_work_feed_has_no_executor() {
        let v = validated(
            "project:
  name: bare
",
        );
        assert!(executor_for(&v, &AdapterConfig::default()).is_none());
    }
}
