//! Aggregation: folding each project's validated manifest and its
//! adapters into one cross-project view. Deliberately infallible — an
//! adapter that fails degrades its own source and leaves the rest
//! intact, because a blank view is a worse failure than a number
//! labelled stale.

use crate::adapters::artifact::{Artifact, ArtifactAdapter};
use crate::adapters::session::{Session, SessionAdapter};
use crate::adapters::verification::{VerificationAdapter, VerificationStatus};
use crate::adapters::work::{WorkAdapter, WorkSnapshot};
use crate::adapters::{AdapterError, ProjectContext};
use crate::autonomy::{resolve, Resolution};
use crate::freshness::Observed;
use crate::validate::Validated;
use std::time::SystemTime;

/// The adapters serving one project. Every family is optional, because
/// partial support is normal.
#[derive(Default)]
pub struct ProjectAdapters {
    /// The work feed, when declared.
    pub work: Option<Box<dyn WorkAdapter>>,
    /// One adapter per declared verification check.
    pub verification: Vec<Box<dyn VerificationAdapter>>,
    /// One adapter per declared artifact feed.
    pub artifacts: Vec<Box<dyn ArtifactAdapter>>,
    /// The session feed, when declared.
    pub sessions: Option<Box<dyn SessionAdapter>>,
}

impl ProjectAdapters {
    /// A project with no adapters registered.
    pub fn new() -> Self {
        Self::default()
    }
}

/// One work item's labels, projected onto the normalized axes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemAutonomy {
    /// The item's number in its repository.
    pub number: u64,
    /// What its labels resolved to.
    pub resolution: Resolution,
}

/// A source that could not be read this cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degradation {
    /// The adapter's `source_name`.
    pub source: String,
    /// Why it could not be read, in one sentence.
    pub reason: String,
}

/// Everything the platform currently knows about one project.
#[derive(Debug, Default)]
pub struct ProjectState {
    /// The project's short name.
    pub name: String,
    /// Its declared methodology. **Display only — never branched on.**
    pub methodology: Option<String>,
    /// Its primary language, for display.
    pub language: Option<String>,
    /// The most recent work snapshot, when the feed was reachable.
    pub work: Option<Observed<WorkSnapshot>>,
    /// Each work item's projected autonomy, in snapshot order.
    pub autonomy: Vec<ItemAutonomy>,
    /// Labels seen on work items that the manifest does not declare,
    /// deduplicated and sorted. Not an error — a prompt to extend the
    /// map, or evidence a label is doing nothing.
    pub unmapped_labels: Vec<String>,
    /// Each declared verification check's standing.
    pub verification: Vec<Observed<VerificationStatus>>,
    /// Each declared artifact feed's contents.
    pub artifacts: Vec<Observed<Vec<Artifact>>>,
    /// The session feed's contents, when declared and reachable.
    pub sessions: Option<Observed<Vec<Session>>>,
    /// Sources that failed this cycle.
    pub degradations: Vec<Degradation>,
}

/// Every registered project's state.
#[derive(Debug, Default)]
pub struct PlatformState {
    /// One entry per registered project, in registration order.
    pub projects: Vec<ProjectState>,
}

impl PlatformState {
    /// Finds a project by name.
    pub fn project(&self, name: &str) -> Option<&ProjectState> {
        self.projects.iter().find(|p| p.name == name)
    }
}

/// Builds the adapter context from a validated manifest.
fn context(validated: &Validated) -> ProjectContext {
    let manifest = validated.manifest();
    let root = manifest.project.root.clone().unwrap_or_default();
    let mut ctx = ProjectContext::new(manifest.project.name.clone(), root);
    if let Some(work) = &manifest.work {
        ctx = ctx.with_repo(work.repo.clone());
    }
    ctx
}

/// Records an adapter failure against the project rather than
/// propagating it.
fn degrade(state: &mut ProjectState, source: String, error: AdapterError) {
    state.degradations.push(Degradation {
        source,
        reason: error.to_string(),
    });
}

/// Polls every adapter a project declares and folds the results into one
/// state. Never fails: a failing source becomes a `Degradation`.
pub fn aggregate_project(
    validated: &Validated,
    adapters: &mut ProjectAdapters,
    now: SystemTime,
) -> ProjectState {
    let manifest = validated.manifest();
    let ctx = context(validated);
    let mut state = ProjectState {
        name: manifest.project.name.clone(),
        methodology: manifest.project.methodology.clone(),
        language: manifest.project.language.clone(),
        ..Default::default()
    };

    if let Some(adapter) = adapters.work.as_mut() {
        let source = adapter.source_name();
        match adapter.poll(&ctx, now) {
            Ok(observed) => {
                let empty = Default::default();
                let map = manifest.work.as_ref().map(|w| &w.autonomy_map);
                let map = map.unwrap_or(&empty);
                let mut unmapped: Vec<String> = Vec::new();
                for item in &observed.value.items {
                    let resolution = resolve(map, &item.labels);
                    for label in &resolution.unmapped {
                        if !unmapped.contains(label) {
                            unmapped.push(label.clone());
                        }
                    }
                    state.autonomy.push(ItemAutonomy {
                        number: item.number,
                        resolution,
                    });
                }
                unmapped.sort();
                state.unmapped_labels = unmapped;
                state.work = Some(observed);
            }
            Err(e) => degrade(&mut state, source, e),
        }
    }

    for adapter in adapters.verification.iter_mut() {
        let source = adapter.source_name();
        match adapter.check(&ctx, now) {
            Ok(observed) => state.verification.push(observed),
            Err(e) => degrade(&mut state, source, e),
        }
    }

    for adapter in adapters.artifacts.iter_mut() {
        let source = adapter.source_name();
        match adapter.scan(&ctx, now) {
            Ok(observed) => state.artifacts.push(observed),
            Err(e) => degrade(&mut state, source, e),
        }
    }

    if let Some(adapter) = adapters.sessions.as_mut() {
        let source = adapter.source_name();
        match adapter.scan(&ctx, now) {
            Ok(observed) => state.sessions = Some(observed),
            Err(e) => degrade(&mut state, source, e),
        }
    }

    state
}

/// Aggregates every registered project.
pub fn aggregate(inputs: &mut [(Validated, ProjectAdapters)], now: SystemTime) -> PlatformState {
    PlatformState {
        projects: inputs
            .iter_mut()
            .map(|(validated, adapters)| aggregate_project(validated, adapters, now))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::verification::VerificationOutcome;
    use crate::adapters::work::{ChecksSummary, WorkItem, WorkKind, WorkState};
    use crate::autonomy::Merge;
    use crate::manifest::parse_manifest;
    use crate::validate::validate;
    use std::time::Duration;

    pub(super) fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    fn item(number: u64, labels: &[&str]) -> WorkItem {
        WorkItem {
            number,
            title: format!("item {number}"),
            kind: WorkKind::Issue,
            state: WorkState::Open,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            checks: ChecksSummary::none(),
            url: String::new(),
            updated_at: "2026-08-14T00:00:00Z".into(),
        }
    }

    pub(super) struct StubWork {
        pub(super) result: Option<WorkSnapshot>,
    }

    impl WorkAdapter for StubWork {
        fn source_name(&self) -> String {
            "work:stub".into()
        }
        fn poll(
            &mut self,
            _ctx: &ProjectContext,
            now: SystemTime,
        ) -> Result<Observed<WorkSnapshot>, AdapterError> {
            match self.result.clone() {
                Some(s) => Ok(Observed::polled(s, now, Duration::from_secs(30))),
                None => Err(AdapterError::Http {
                    status: 403,
                    message: "rate limited".into(),
                }),
            }
        }
    }

    pub(super) struct StubVerification(pub(super) VerificationOutcome);

    impl VerificationAdapter for StubVerification {
        fn source_name(&self) -> String {
            "verification:stub".into()
        }
        fn check(
            &mut self,
            _ctx: &ProjectContext,
            now: SystemTime,
        ) -> Result<Observed<VerificationStatus>, AdapterError> {
            Ok(Observed::watched(
                VerificationStatus {
                    kind: "tests".into(),
                    outcome: self.0,
                    detail: None,
                },
                now,
            ))
        }
    }

    const TTUI_YAML: &str = r#"
project:
  name: ttui
  root: <projects-root>/TTUI
work:
  adapter: github
  repo: tatemeyer/ttui
  autonomy_map:
    direct: { implement: agent, merge: direct-push }
    gated:  { implement: agent, merge: on-checks }
"#;

    pub(super) fn ttui() -> crate::validate::Validated {
        validate(parse_manifest(TTUI_YAML).unwrap()).unwrap()
    }

    #[test]
    fn a_projects_identity_comes_from_its_manifest() {
        let mut adapters = ProjectAdapters::new();
        let state = aggregate_project(&ttui(), &mut adapters, at(0));
        assert_eq!(state.name, "ttui");
    }

    #[test]
    fn work_items_are_carried_through_and_their_labels_projected() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork {
            result: Some(WorkSnapshot {
                items: vec![item(1, &["gated"]), item(2, &["direct", "bug"])],
            }),
        }));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));

        assert_eq!(state.work.as_ref().unwrap().value.items.len(), 2);
        assert_eq!(state.autonomy.len(), 2);
        assert_eq!(state.autonomy[0].number, 1);
        assert_eq!(
            state.autonomy[0].resolution.autonomy.merge,
            Some(Merge::OnChecks)
        );
        assert_eq!(
            state.autonomy[1].resolution.autonomy.merge,
            Some(Merge::DirectPush)
        );
    }

    #[test]
    fn labels_the_manifest_never_declared_are_collected_once_and_deduplicated() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork {
            result: Some(WorkSnapshot {
                items: vec![item(1, &["bug", "gated"]), item(2, &["bug", "docs"])],
            }),
        }));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));
        assert_eq!(
            state.unmapped_labels,
            vec!["bug".to_string(), "docs".to_string()]
        );
    }

    /// The spec's headline partial case, at the aggregation layer.
    #[test]
    fn a_manifest_declaring_only_work_aggregates_to_a_valid_reduced_view() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork {
            result: Some(WorkSnapshot {
                items: vec![item(1, &["gated"])],
            }),
        }));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));
        assert!(state.work.is_some());
        assert!(state.verification.is_empty());
        assert!(state.artifacts.is_empty());
        assert!(state.sessions.is_none());
        assert!(state.degradations.is_empty(), "absent is not degraded");
    }

    /// A failing adapter must not blank the rest of the view.
    #[test]
    fn a_failing_work_adapter_degrades_only_its_own_source() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork { result: None }));
        adapters
            .verification
            .push(Box::new(StubVerification(VerificationOutcome::Pass)));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));

        assert!(state.work.is_none());
        assert_eq!(state.degradations.len(), 1);
        assert_eq!(state.degradations[0].source, "work:stub");
        assert!(state.degradations[0].reason.contains("rate limited"));
        assert_eq!(state.verification.len(), 1, "verification survived");
        assert_eq!(
            state.verification[0].value.outcome,
            VerificationOutcome::Pass
        );
    }

    #[test]
    fn aggregate_folds_several_projects_and_finds_them_by_name() {
        let me_yaml = "project:\n  name: model-experiments\n  root: /tmp/me\n";
        let me = validate(parse_manifest(me_yaml).unwrap()).unwrap();
        let mut inputs = vec![
            (ttui(), ProjectAdapters::new()),
            (me, ProjectAdapters::new()),
        ];
        let platform = aggregate(&mut inputs, at(0));
        assert_eq!(platform.projects.len(), 2);
        assert!(platform.project("ttui").is_some());
        assert!(platform.project("model-experiments").is_some());
        assert!(platform.project("nonexistent").is_none());
    }

    #[test]
    fn every_observation_carries_the_now_it_was_aggregated_at() {
        let mut adapters = ProjectAdapters::new();
        adapters
            .verification
            .push(Box::new(StubVerification(VerificationOutcome::Pass)));
        let state = aggregate_project(&ttui(), &mut adapters, at(42));
        assert_eq!(state.verification[0].observed_at, at(42));
    }
}
