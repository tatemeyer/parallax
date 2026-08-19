//! Both real manifests, built into adapters by the factory rather than
//! by hand. Reaches only the public API, which is what a frontend has.

use parallax_baseline::adapters::factory::{from_manifest, from_manifest_with, AdapterConfig};
use parallax_baseline::adapters::http::FixtureTransport;
use parallax_baseline::adapters::verification::{CheckCost, ScriptedRunner};
use parallax_baseline::manifest::parse_manifest_file;
use parallax_baseline::state::ProjectAdapters;
use parallax_baseline::validate::{validate, Validated};
use std::path::{Path, PathBuf};

fn manifest(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("manifests")
        .join(name)
}

fn load(name: &str) -> Validated {
    validate(parse_manifest_file(&manifest(name)).expect("parses")).expect("validates")
}

fn built(name: &str) -> ProjectAdapters {
    from_manifest_with(
        &load(name),
        &AdapterConfig::default(),
        FixtureTransport::new,
        ScriptedRunner::new,
    )
}

fn shape(a: &ProjectAdapters) -> (usize, usize, usize, usize) {
    (
        a.work.iter().count(),
        a.verification.len(),
        a.artifacts.len(),
        a.sessions.iter().count(),
    )
}

#[test]
fn ttui_declares_one_of_every_family() {
    assert_eq!(shape(&built("ttui.yaml")), (1, 3, 1, 1));
}

/// Model-Experiments declares no `sessions:`, so no session adapter is
/// built — absent is not degraded, and it must not become an adapter
/// that scans nothing.
#[test]
fn model_experiments_builds_no_session_adapter_because_it_declares_none() {
    let a = built("model-experiments.yaml");
    assert_eq!(shape(&a), (1, 2, 2, 0));
    assert!(a.sessions.is_none());
}

#[test]
fn both_manifests_name_their_adapters_the_way_their_declarations_read() {
    let ttui = built("ttui.yaml");
    let names: Vec<String> = ttui.verification.iter().map(|v| v.source_name()).collect();
    assert_eq!(
        names,
        vec![
            "verification:command:lint".to_string(),
            "verification:command:tests".to_string(),
            "verification:plumb:perceptual".to_string(),
        ]
    );

    let me = built("model-experiments.yaml");
    let names: Vec<String> = me.artifacts.iter().map(|a| a.source_name()).collect();
    assert_eq!(
        names,
        vec![
            "artifact:figure".to_string(),
            "artifact:metrics".to_string()
        ],
        "`adapter: jsonl` selects the metrics adapter"
    );
}

/// The partition a scheduler needs: TTUI declares two checks that run a
/// build and one that reads a file off disk.
#[test]
fn a_scheduler_can_split_the_built_checks_by_what_they_cost() {
    let ttui = built("ttui.yaml");
    let (reads, runs): (Vec<_>, Vec<_>) = ttui
        .verification
        .iter()
        .partition(|v| v.cost() == CheckCost::Read);
    assert_eq!(runs.len(), 2, "cargo clippy and cargo test");
    assert_eq!(reads.len(), 1, "the plumb verdict");
}

/// The spec's headline partial case, at the factory layer.
#[test]
fn a_work_only_manifest_builds_exactly_one_adapter() {
    let mut parsed = parse_manifest_file(&manifest("ttui.yaml")).unwrap();
    parsed.verification.clear();
    parsed.artifacts.clear();
    parsed.sessions = None;
    let validated = validate(parsed).unwrap();

    let a = from_manifest_with(
        &validated,
        &AdapterConfig::default(),
        FixtureTransport::new,
        ScriptedRunner::new,
    );
    assert_eq!(shape(&a), (1, 0, 0, 0));
}

/// The live wrapper builds the same shape. It is not polled here —
/// naming `UreqTransport` is as far as a test goes toward the network.
#[test]
fn the_live_wrapper_builds_the_same_shape_for_a_real_manifest() {
    let live = from_manifest(&load("ttui.yaml"), &AdapterConfig::default());
    assert_eq!(shape(&live), shape(&built("ttui.yaml")));
}
