//! The two real consumers' manifests must parse, validate, and project
//! their native autonomy labels onto the normalized axes.

use parallax_baseline::autonomy::{project, Implement, Merge, Readiness};
use parallax_baseline::manifest::{parse_manifest_file, ArtifactKind, VerificationAdapterKind};
use parallax_baseline::validate::{validate, Family, Validated};
use std::path::{Path, PathBuf};

/// `manifests/` sits at the workspace root, one level above this crate.
fn manifest_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("baseline/ has a parent")
        .join("manifests")
        .join(name)
}

fn load(name: &str) -> Validated {
    let parsed = parse_manifest_file(&manifest_path(name)).expect("manifest parses");
    validate(parsed).expect("manifest validates")
}

#[test]
fn ttui_manifest_parses_and_validates() {
    let v = load("ttui.yaml");
    let m = v.manifest();
    assert_eq!(m.api_version.as_deref(), Some("parallax/v1"));
    assert_eq!(m.project.name, "ttui");
    assert_eq!(m.project.language.as_deref(), Some("rust"));
    assert_eq!(m.project.methodology.as_deref(), Some("methodology-first"));
    assert_eq!(m.work.as_ref().unwrap().repo, "tatemeyer/ttui");
    assert_eq!(m.verification.len(), 3);
    assert_eq!(m.verification[2].adapter, VerificationAdapterKind::Plumb);
    assert_eq!(m.artifacts[0].kind, ArtifactKind::Capture);
    assert_eq!(m.sessions.as_ref().unwrap().watch, ".claude/worktrees/*");
    for family in [
        Family::Work,
        Family::Verification,
        Family::Artifact,
        Family::Session,
    ] {
        assert!(v.declares(family), "ttui declares all four families");
    }
}

#[test]
fn ttui_labels_project_onto_the_spec_s_table() {
    let v = load("ttui.yaml");
    let map = &v.manifest().work.as_ref().unwrap().autonomy_map;

    let direct = project(map, "direct").expect("direct is declared");
    assert_eq!(direct.implement, Some(Implement::Agent));
    assert_eq!(direct.merge, Some(Merge::DirectPush));
    assert_eq!(direct.readiness, Readiness::Verifiable);

    let gated = project(map, "gated").expect("gated is declared");
    assert_eq!(gated.implement, Some(Implement::Agent));
    assert_eq!(gated.merge, Some(Merge::OnChecks));
    assert_eq!(gated.readiness, Readiness::Verifiable);

    let human = project(map, "human").expect("human is declared");
    assert_eq!(human.implement, Some(Implement::Agent));
    assert_eq!(human.merge, Some(Merge::HumanApproval));
    assert_eq!(human.readiness, Readiness::Verifiable);
}

#[test]
fn model_experiments_manifest_parses_and_validates() {
    let v = load("model-experiments.yaml");
    let m = v.manifest();
    assert_eq!(m.project.name, "model-experiments");
    assert_eq!(m.project.language.as_deref(), Some("python"));
    assert_eq!(m.project.methodology.as_deref(), Some("outcome-first"));
    assert_eq!(m.work.as_ref().unwrap().repo, "tatemeyer/Model-Experiments");
    assert_eq!(m.verification.len(), 2);
    assert_eq!(m.artifacts.len(), 2);
    assert_eq!(m.artifacts[0].kind, ArtifactKind::Figure);
    assert_eq!(m.artifacts[1].kind, ArtifactKind::Metrics);
    assert!(
        !v.declares(Family::Session),
        "Model-Experiments declares no session feed"
    );
}

#[test]
fn model_experiments_labels_project_onto_the_spec_s_table() {
    let v = load("model-experiments.yaml");
    let map = &v.manifest().work.as_ref().unwrap().autonomy_map;

    let safe = project(map, "autonomy:safe").expect("declared");
    assert_eq!(safe.implement, Some(Implement::Agent));
    assert_eq!(safe.merge, Some(Merge::OnChecks));
    assert_eq!(safe.readiness, Readiness::Verifiable);

    let review = project(map, "autonomy:review").expect("declared");
    assert_eq!(review.implement, Some(Implement::Agent));
    assert_eq!(review.merge, Some(Merge::HumanApproval));
    assert_eq!(review.readiness, Readiness::Verifiable);

    let human = project(map, "autonomy:human").expect("declared");
    assert_eq!(human.implement, Some(Implement::HumanOnly));
    assert_eq!(human.merge, None, "the spec's table has a dash here");
    assert_eq!(human.readiness, Readiness::Verifiable);

    let intent = project(map, "needs-intent").expect("declared");
    assert_eq!(intent.implement, None, "the spec's table has a dash here");
    assert_eq!(intent.merge, None, "the spec's table has a dash here");
    assert_eq!(intent.readiness, Readiness::NeedsIntent);
}

/// The two asymmetries the shared vocabulary exists to surface, asserted
/// against the real files rather than test fixtures.
#[test]
fn the_two_asymmetries_hold_for_the_real_manifests() {
    let ttui = load("ttui.yaml");
    let ttui_map = &ttui.manifest().work.as_ref().unwrap().autonomy_map;
    assert!(
        ttui_map
            .labels()
            .all(|l| project(ttui_map, l).unwrap().implement != Some(Implement::HumanOnly)),
        "TTUI reserves no work from the agent"
    );

    let me = load("model-experiments.yaml");
    let me_map = &me.manifest().work.as_ref().unwrap().autonomy_map;
    assert!(
        me_map
            .labels()
            .all(|l| project(me_map, l).unwrap().merge != Some(Merge::DirectPush)),
        "nothing in Model-Experiments bypasses CI"
    );
}
