//! Turning this machine's registry into an envelope.
//!
//! The probe invents no aggregation of its own: it calls the same
//! `from_manifest` and `aggregate_project` a local frontend calls, and
//! serializes the result. Every hard question it might have had was
//! answered when those were written.

use parallax_baseline::adapters::factory::{from_manifest, AdapterConfig};
use parallax_baseline::adapters::verification::{
    CheckCost, VerificationAdapter, VerificationOutcome, VerificationStatus,
};
use parallax_baseline::adapters::{AdapterError, ProjectContext};
use parallax_baseline::freshness::Observed;
use parallax_baseline::registry::Registry;
use parallax_baseline::state::{aggregate_project, split_by_cost, Degradation, ProjectState};
use parallax_baseline::wire::StateEnvelope;
use std::time::SystemTime;

/// Stands in for a check the probe declines to run.
///
/// `split_by_cost` removes the checks that produce state by running
/// something. Removing them entirely would make a declared check vanish
/// from the envelope, which reads as "this project has no tests" rather
/// than "nobody has run them". So each one is replaced by this, which
/// reports [`VerificationOutcome::NotRun`] and costs nothing to call.
struct Deferred {
    kind: String,
    command: String,
}

impl VerificationAdapter for Deferred {
    fn source_name(&self) -> String {
        format!("verification:deferred:{}", self.kind)
    }

    fn check(
        &mut self,
        _ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<VerificationStatus>, AdapterError> {
        Ok(Observed::watched(
            VerificationStatus {
                kind: self.kind.clone(),
                outcome: VerificationOutcome::NotRun,
                detail: Some(format!(
                    "not run by the probe: `{}` runs a build",
                    self.command
                )),
            },
            now,
        ))
    }

    /// Reading a constant costs nothing. The point of this type is that
    /// it is safe on any cadence, which is what the real one was not.
    fn cost(&self) -> CheckCost {
        CheckCost::Read
    }
}

/// `verification:command:lint` -> `lint`.
fn kind_of(source_name: &str) -> String {
    source_name
        .rsplit(':')
        .next()
        .unwrap_or(source_name)
        .to_string()
}

/// Every registered project on this machine, aggregated and ready to
/// serve.
///
/// **Runs no build.** The rule lives in `split_by_cost`, in baseline,
/// shared with the cockpit's refresh thread — a probe with its own idea
/// of which checks are safe to poll is how three machines end up able
/// to trigger `cargo test` on a fourth.
///
/// Deferred checks are appended after the ones that were actually read,
/// rather than held in manifest position. A check nobody ran sorting
/// below the ones that did is the more useful reading order anyway.
pub fn envelope(
    registry: &Registry,
    config: &AdapterConfig,
    peer: &str,
    now: SystemTime,
) -> StateEnvelope {
    let mut projects = Vec::new();

    for project in registry.projects() {
        let mut adapters = from_manifest(&project.manifest, config);
        for held in split_by_cost(&mut adapters) {
            let kind = kind_of(&held.source_name());
            let command = project
                .manifest
                .manifest()
                .verification
                .iter()
                .find(|e| e.kind == kind)
                .and_then(|e| e.command.clone())
                .unwrap_or_else(|| kind.clone());
            adapters
                .verification
                .push(Box::new(Deferred { kind, command }));
        }
        projects.push(aggregate_project(&project.manifest, &mut adapters, now));
    }

    // A project the registry could not load is a degraded row, not a
    // missing one: the rule `aggregate` already follows for a failing
    // adapter, applied one level up.
    for failure in registry.failures() {
        projects.push(ProjectState {
            name: failure
                .source
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "unknown".to_string()),
            degradations: vec![Degradation {
                source: "registry".into(),
                reason: failure.problem.clone(),
            }],
            ..Default::default()
        });
    }

    StateEnvelope::send(peer, now, projects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn project(dir: &Path, name: &str, manifest: &str) {
        let root = dir.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("parallax.yaml"), manifest).unwrap();
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + secs)
    }

    const WITH_TESTS: &str = "project:\n  name: ttui\n  language: rust\nverification:\n  - kind: tests\n    adapter: command\n    command: cargo test\n";

    #[test]
    fn kind_is_taken_from_the_tail_of_a_source_name() {
        assert_eq!(kind_of("verification:command:lint"), "lint");
        assert_eq!(kind_of("bare"), "bare");
    }

    /// The property the whole cost model exists for, now with a network
    /// in front of it. If this regresses, a cockpit refresh on any of
    /// three machines runs `cargo test` on this one.
    #[test]
    fn building_an_envelope_never_runs_a_declared_command() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), "ttui", WITH_TESTS);
        let registry = Registry::scan(dir.path());
        assert_eq!(registry.projects().len(), 1);

        // `cargo test` inside a temp dir would fail loudly and slowly.
        // It reports NotRun instead, which is only possible if it was
        // never spawned.
        let envelope = envelope(&registry, &AdapterConfig::default(), "test", at(0));
        let project = &envelope.projects[0];
        assert_eq!(project.verification.len(), 1);
        assert_eq!(
            project.verification[0].value.outcome,
            VerificationOutcome::NotRun
        );
    }

    #[test]
    fn a_deferred_check_names_the_command_that_would_run_it() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), "ttui", WITH_TESTS);
        let registry = Registry::scan(dir.path());

        let envelope = envelope(&registry, &AdapterConfig::default(), "test", at(0));
        let detail = envelope.projects[0].verification[0]
            .value
            .detail
            .clone()
            .expect("a deferred check with no explanation teaches nothing");
        assert!(detail.contains("cargo test"), "got {detail}");
    }

    #[test]
    fn the_envelope_carries_the_peer_name_and_the_version() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), "ttui", "project:\n  name: ttui\n");
        let registry = Registry::scan(dir.path());

        let envelope = envelope(&registry, &AdapterConfig::default(), "pi5", at(7));
        assert_eq!(envelope.peer, "pi5");
        assert_eq!(envelope.now, at(7));
        assert_eq!(
            envelope.api_version,
            parallax_baseline::wire::WIRE_API_VERSION
        );
    }

    /// One unreadable project must not blank the machine.
    #[test]
    fn a_project_that_will_not_load_is_a_degraded_row_rather_than_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), "good", "project:\n  name: good\n");
        project(dir.path(), "broken", "project:\n  name: [unclosed\n");
        let registry = Registry::scan(dir.path());

        let envelope = envelope(&registry, &AdapterConfig::default(), "test", at(0));
        assert_eq!(envelope.projects.len(), 2, "the broken one vanished");
        let broken = envelope
            .projects
            .iter()
            .find(|p| p.name == "broken")
            .expect("no row for the project that failed to load");
        assert_eq!(broken.degradations.len(), 1);
        assert_eq!(broken.degradations[0].source, "registry");
    }

    #[test]
    fn an_empty_machine_serves_an_empty_envelope_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::scan(dir.path());
        let envelope = envelope(&registry, &AdapterConfig::default(), "test", at(0));
        assert!(envelope.projects.is_empty());
    }
}
