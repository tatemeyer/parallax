//! `plumb plan`: reads every `*.manifest.json` in a run directory and
//! prints the resulting `DispatchPlan` as JSON, for the orchestrating
//! skill (Task 13) to dispatch. No `--config` flag exists yet, so a
//! scenario's `taste_override` (declared in `config.yaml`, not in the
//! manifest) has no path to reach here — every plan is built with an
//! empty override map until a later task wires that flag in.

use super::IoFailure;
use parallax_plumb::evidence::{self, EvidenceError};
use parallax_plumb::finding::Lens;
use parallax_plumb::manifest::{self, RunManifest};
use parallax_plumb::prompt::{self, Dispatch, DispatchPlan, Skip};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Failure planning a run's dispatch.
#[derive(Debug)]
pub(super) enum PlanCliError {
    /// The run directory could not be listed, or `--taste` could not
    /// be read.
    Io(IoFailure),
    /// A manifest in the run directory failed to read or parse.
    Manifest(manifest::ManifestError),
    /// A dispatched prompt could not be persisted as evidence.
    Evidence(EvidenceError),
}

impl std::fmt::Display for PlanCliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanCliError::Io(e) => write!(f, "{e}"),
            PlanCliError::Manifest(e) => write!(f, "{e}"),
            PlanCliError::Evidence(e) => write!(f, "{e}"),
        }
    }
}

fn lens_name(lens: Lens) -> &'static str {
    match lens {
        Lens::Breakage => "breakage",
        Lens::Intent => "intent",
        Lens::Design => "design",
        Lens::Motion => "motion",
    }
}

fn skip_reason(s: Skip) -> &'static str {
    match s {
        Skip::NoIntentDeclared => "no intent declared",
        Skip::NoTasteProfile => "no taste.md",
        Skip::SingleFrame => "single-frame capture",
    }
}

/// JSON shape of one `Dispatch`, for the skill to consume. `prompt::
/// Dispatch` itself derives no `Serialize` (Task 8 never needed one),
/// so this is a local mirror rather than a change to that module.
#[derive(Serialize)]
struct DispatchOut {
    lens: &'static str,
    agent: String,
    scenario: String,
    image: PathBuf,
    prompt: String,
}

impl From<&Dispatch> for DispatchOut {
    fn from(d: &Dispatch) -> Self {
        DispatchOut {
            lens: lens_name(d.lens),
            agent: d.agent.clone(),
            scenario: d.scenario.clone(),
            image: d.image.clone(),
            prompt: d.prompt.clone(),
        }
    }
}

/// JSON shape of one skipped lens.
#[derive(Serialize)]
struct SkipOut {
    scenario: String,
    lens: &'static str,
    reason: &'static str,
}

/// JSON shape of a whole `DispatchPlan`.
#[derive(Serialize)]
struct PlanOut {
    batches: Vec<Vec<DispatchOut>>,
    skipped: Vec<SkipOut>,
    cap: usize,
}

impl From<DispatchPlan> for PlanOut {
    fn from(p: DispatchPlan) -> Self {
        PlanOut {
            batches: p
                .batches
                .iter()
                .map(|b| b.iter().map(DispatchOut::from).collect())
                .collect(),
            skipped: p
                .skipped
                .into_iter()
                .map(|(scenario, lens, reason)| SkipOut {
                    scenario,
                    lens: lens_name(lens),
                    reason: skip_reason(reason),
                })
                .collect(),
            cap: p.cap,
        }
    }
}

/// Reads every `*.manifest.json` in `run_dir`, sorted by filename for a
/// deterministic plan.
fn read_manifests(run_dir: &Path) -> Result<Vec<RunManifest>, PlanCliError> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(run_dir)
        .map_err(|source| {
            PlanCliError::Io(IoFailure {
                path: run_dir.to_path_buf(),
                source,
            })
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".manifest.json"))
        })
        .collect();
    paths.sort();

    paths
        .into_iter()
        .map(|p| manifest::read_manifest(&p).map_err(PlanCliError::Manifest))
        .collect()
}

/// Builds the run's `DispatchPlan` from every manifest in `run_dir`,
/// batched to `cap` dispatches per batch, with `taste`'s text (if
/// given) reaching the design lens.
pub(super) fn run_plan(
    run_dir: &Path,
    taste: Option<&Path>,
    cap: usize,
) -> Result<DispatchPlan, PlanCliError> {
    let manifests = read_manifests(run_dir)?;
    let taste_text = taste
        .map(|p| {
            std::fs::read_to_string(p).map_err(|source| {
                PlanCliError::Io(IoFailure {
                    path: p.to_path_buf(),
                    source,
                })
            })
        })
        .transpose()?;
    let overrides: HashMap<String, String> = HashMap::new();
    let plan = prompt::plan_dispatch(&manifests, taste_text.as_deref(), &overrides, cap);

    // Persisted only after `build_prompt` (inside `plan_dispatch`) has
    // already returned, and never read back here — evidence must never
    // become a channel back into a prompt.
    for batch in &plan.batches {
        for d in batch {
            persist_prompt(run_dir, d)?;
        }
    }

    Ok(plan)
}

/// Writes one dispatch's already-built prompt to the run's evidence
/// directory, mapping any failure into [`PlanCliError`] so the message
/// still names the file that could not be written.
fn persist_prompt(run_dir: &Path, d: &Dispatch) -> Result<(), PlanCliError> {
    evidence::write_prompt(run_dir, d.lens, &d.scenario, &d.prompt).map_err(PlanCliError::Evidence)
}

/// Serializes a `DispatchPlan` to pretty JSON for the skill to consume.
pub(super) fn render_plan(plan: DispatchPlan) -> String {
    serde_json::to_string_pretty(&PlanOut::from(plan)).expect("PlanOut serializes infallibly")
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_plumb::manifest::write_manifest;

    fn sample_manifest(scenario: &str, intent: Option<&str>, frame_count: usize) -> RunManifest {
        RunManifest {
            run_id: "20260814T101500Z".into(),
            scenario: scenario.into(),
            adapter: "command".into(),
            image: PathBuf::from(format!("{scenario}.png")),
            animation: None,
            frame_count,
            size: Some("80x24".into()),
            intent: intent.map(String::from),
            expects: Vec::new(),
            caveats: Vec::new(),
        }
    }

    #[test]
    fn plan_reads_every_manifest_in_the_run_dir() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(&sample_manifest("dial", Some("spins"), 3), tmp.path()).unwrap();
        write_manifest(&sample_manifest("crabs", None, 1), tmp.path()).unwrap();

        let plan = run_plan(tmp.path(), None, 8).unwrap();

        let dispatched: usize = plan.batches.iter().map(|b| b.len()).sum();
        // dial: breakage + intent + motion = 3 (no taste -> design skipped)
        // crabs: breakage only = 1 (no intent, no taste, single frame)
        assert_eq!(dispatched, 4);
        assert!(!plan.skipped.is_empty());
    }

    #[test]
    fn plan_batches_to_the_given_cap() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(&sample_manifest("dial", Some("spins"), 3), tmp.path()).unwrap();

        let plan = run_plan(tmp.path(), None, 1).unwrap();

        assert!(plan.batches.iter().all(|b| b.len() <= 1));
        assert_eq!(plan.cap, 1);
    }

    #[test]
    fn plan_reads_taste_text_when_given() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(&sample_manifest("dial", None, 1), tmp.path()).unwrap();
        let taste = tmp.path().join("taste.md");
        std::fs::write(&taste, "Prefer sharp corners.").unwrap();

        let plan = run_plan(tmp.path(), Some(&taste), 8).unwrap();

        let dispatched: usize = plan.batches.iter().map(|b| b.len()).sum();
        // breakage + design (taste present) = 2
        assert_eq!(dispatched, 2);
    }

    #[test]
    fn plan_errors_on_a_missing_run_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");

        let result = run_plan(&missing, None, 8);

        assert!(matches!(result, Err(PlanCliError::Io(_))));
    }

    #[test]
    fn render_plan_produces_valid_json_naming_the_lens_as_lowercase_text() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(&sample_manifest("dial", Some("spins"), 1), tmp.path()).unwrap();
        let plan = run_plan(tmp.path(), None, 8).unwrap();

        let json = render_plan(plan);

        assert!(json.contains("\"breakage\""));
        assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
    }

    /// Review Finding 4: the earlier test only ever exercised
    /// `"breakage"`, so a swapped `design`/`motion` lens name or a
    /// swapped skip-reason string in the local `lens_name`/`skip_reason`
    /// mirrors in this file would have passed every prior test. A
    /// single-frame, no-intent, no-taste manifest skips all three
    /// non-breakage lenses for three different reasons, which pins all
    /// six strings at once.
    /// Task 3: `run_plan` must persist each dispatched prompt verbatim
    /// to `lenses/<lens>.<scenario>/prompt.txt`, so a human auditing a
    /// verdict later can see exactly what a lens agent was told. The
    /// brief's sample used a `write_test_manifest` helper that does not
    /// exist in this module; this module's existing tests stage a
    /// manifest via `sample_manifest` + `write_manifest` instead, so
    /// this test uses that pair, keeping the assertion identical.
    #[test]
    fn planning_persists_each_dispatched_prompt_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        let run = tmp.path();
        write_manifest(&sample_manifest("omni", Some("spins"), 6), run).unwrap();

        run_plan(run, None, 8).expect("plan succeeds");

        let p = std::fs::read_to_string(run.join("lenses/breakage.omni/prompt.txt"))
            .expect("prompt persisted");
        assert!(p.contains("Sim Sup"), "prompt body was written verbatim");
    }

    #[test]
    fn render_plan_json_names_every_non_breakage_lens_and_its_own_skip_reason() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(&sample_manifest("crabs", None, 1), tmp.path()).unwrap();

        let plan = run_plan(tmp.path(), None, 8).unwrap();
        let json = render_plan(plan);

        assert!(json.contains("\"lens\": \"intent\""), "{json}");
        assert!(json.contains("\"lens\": \"design\""), "{json}");
        assert!(json.contains("\"lens\": \"motion\""), "{json}");
        assert!(
            json.contains("\"reason\": \"no intent declared\""),
            "{json}"
        );
        assert!(json.contains("\"reason\": \"no taste.md\""), "{json}");
        assert!(
            json.contains("\"reason\": \"single-frame capture\""),
            "{json}"
        );
    }
}
