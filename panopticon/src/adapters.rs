//! Wires a validated manifest's declarations to Baseline's real
//! (network/filesystem/process) adapter implementations. Baseline
//! defines the adapter traits and resolves per-project context
//! automatically during aggregation; picking *which* concrete adapter
//! serves each manifest entry is deliberately left to a frontend, since
//! that choice is policy Baseline stays agnostic about.

use parallax_baseline::adapters::artifact::{
    ArtifactAdapter, CaptureArtifactAdapter, FigureArtifactAdapter, MetricsArtifactAdapter,
};
use parallax_baseline::adapters::http::UreqTransport;
use parallax_baseline::adapters::session::FilesystemSessionAdapter;
use parallax_baseline::adapters::verification::{
    CommandVerificationAdapter, PlumbVerificationAdapter, ProcessRunner, VerificationAdapter,
};
use parallax_baseline::adapters::work::GithubWorkAdapter;
use parallax_baseline::manifest::{ArtifactAdapterKind, VerificationAdapterKind, WorkAdapterKind};
use parallax_baseline::state::ProjectAdapters;
use parallax_baseline::validate::Validated;
use std::path::Path;

/// Builds the real adapters a validated manifest declares. A GitHub
/// token, when given, authenticates the work feed; its absence is a
/// supported path -- an unauthenticated `UreqTransport` still works, at
/// GitHub's lower unauthenticated rate limit, and with no network at
/// all the adapter simply degrades on its first poll rather than
/// failing to construct.
pub fn build_adapters(validated: &Validated, github_token: Option<&str>) -> ProjectAdapters {
    let manifest = validated.manifest();
    let mut adapters = ProjectAdapters::new();

    if let Some(work) = &manifest.work {
        match work.adapter {
            WorkAdapterKind::Github => {
                let transport = match github_token {
                    Some(token) => UreqTransport::with_token(token),
                    None => UreqTransport::new(),
                };
                adapters.work = Some(Box::new(GithubWorkAdapter::new(transport)));
            }
        }
    }

    for entry in &manifest.verification {
        let adapter: Option<Box<dyn VerificationAdapter>> = match entry.adapter {
            VerificationAdapterKind::Command => entry.command.as_ref().map(|command| {
                let boxed: Box<dyn VerificationAdapter> =
                    Box::new(CommandVerificationAdapter::new(
                        entry.kind.clone(),
                        command.clone(),
                        ProcessRunner,
                    ));
                boxed
            }),
            VerificationAdapterKind::Plumb => {
                // `PlumbVerificationAdapter` reads its runs directory
                // directly, ignoring the `ProjectContext` aggregation
                // supplies at poll time -- so unlike every other
                // adapter here, its path must be resolved against the
                // project root up front, at construction.
                let root = manifest.project.root.clone().unwrap_or_default();
                let config = validated.plumb_config(entry);
                let runs_dir = root
                    .join(config.parent().unwrap_or_else(|| Path::new(".plumb")))
                    .join("runs");
                Some(Box::new(PlumbVerificationAdapter::new(
                    entry.kind.clone(),
                    runs_dir,
                )))
            }
        };
        if let Some(adapter) = adapter {
            adapters.verification.push(adapter);
        }
    }

    for entry in &manifest.artifacts {
        let adapter: Box<dyn ArtifactAdapter> = match validated.artifact_adapter(entry) {
            ArtifactAdapterKind::Figure => {
                Box::new(FigureArtifactAdapter::new(entry.watch.clone()))
            }
            ArtifactAdapterKind::Metrics => {
                Box::new(MetricsArtifactAdapter::new(entry.watch.clone()))
            }
            ArtifactAdapterKind::Capture => {
                Box::new(CaptureArtifactAdapter::new(entry.watch.clone()))
            }
        };
        adapters.artifacts.push(adapter);
    }

    if let Some(sessions) = &manifest.sessions {
        adapters.sessions = Some(Box::new(FilesystemSessionAdapter::new(
            sessions.watch.clone(),
        )));
    }

    adapters
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_baseline::manifest::parse_manifest;
    use parallax_baseline::validate::validate;

    fn validated(yaml: &str) -> Validated {
        validate(parse_manifest(yaml).unwrap()).expect("should validate")
    }

    #[test]
    fn a_project_with_no_declarations_gets_no_adapters() {
        let v = validated("project:\n  name: bare\n");
        let adapters = build_adapters(&v, None);
        assert!(adapters.work.is_none());
        assert!(adapters.verification.is_empty());
        assert!(adapters.artifacts.is_empty());
        assert!(adapters.sessions.is_none());
    }

    #[test]
    fn a_github_work_declaration_gets_a_work_adapter_regardless_of_token() {
        let yaml =
            "project:\n  name: x\nwork:\n  adapter: github\n  repo: a/b\n  autonomy_map: {}\n";
        let v = validated(yaml);
        assert!(build_adapters(&v, None).work.is_some());
        assert!(build_adapters(&v, Some("tok")).work.is_some());
    }

    #[test]
    fn each_declared_verification_entry_gets_one_adapter() {
        let yaml = "project:\n  name: x\nverification:\n  - kind: tests\n    adapter: command\n    command: echo ok\n  - kind: perceptual\n    adapter: plumb\n";
        let v = validated(yaml);
        assert_eq!(build_adapters(&v, None).verification.len(), 2);
    }

    #[test]
    fn each_declared_artifact_feed_gets_one_adapter() {
        let yaml = "project:\n  name: x\nartifacts:\n  - kind: figure\n    watch: 'out/*.png'\n  - kind: metrics\n    watch: 'out/*.jsonl'\n";
        let v = validated(yaml);
        assert_eq!(build_adapters(&v, None).artifacts.len(), 2);
    }

    #[test]
    fn a_sessions_declaration_gets_a_session_adapter() {
        let yaml = "project:\n  name: x\nsessions:\n  watch: '.claude/worktrees/*'\n";
        let v = validated(yaml);
        assert!(build_adapters(&v, None).sessions.is_some());
    }
}
