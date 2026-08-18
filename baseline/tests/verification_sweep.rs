//! The platform spec's Verification section, executable. One test per
//! bullet, in the spec's own order.
//!
//! - `cargo test` / `clippy` / `fmt --check` clean — enforced by CI and
//!   by every task's commit gate, not assertable from inside a test.
//! - Both real manifests parse, validate, and project — below.
//! - Adapter fixtures replay to correct aggregated state, including the
//!   partial-support case — below.
//! - Confirmation-required actions refuse to execute without explicit
//!   confirmation — below.

use parallax_baseline::actions::{
    authorize, Action, ActionError, ActionExecutor, Confirmation, RecordingExecutor, Reversibility,
};
use parallax_baseline::autonomy::{project, Implement, Merge, Readiness};
use parallax_baseline::manifest::parse_manifest_file;
use parallax_baseline::state::{aggregate_project, ProjectAdapters};
use parallax_baseline::validate::{validate, Validated};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// Bullet 2, first half: both real manifests parse and validate.
#[test]
fn both_real_manifests_parse_and_validate() {
    assert_eq!(load("ttui.yaml").manifest().project.name, "ttui");
    assert_eq!(
        load("model-experiments.yaml").manifest().project.name,
        "model-experiments"
    );
}

/// Bullet 2, second half: every row of the spec's projection table,
/// against the real files, in the spec's own order.
#[test]
fn every_row_of_the_projection_table_holds_for_the_real_manifests() {
    let ttui = load("ttui.yaml");
    let ttui_map = &ttui.manifest().work.as_ref().unwrap().autonomy_map;
    let me = load("model-experiments.yaml");
    let me_map = &me.manifest().work.as_ref().unwrap().autonomy_map;

    type Row = (&'static str, Option<Implement>, Option<Merge>, Readiness);
    let ttui_rows: Vec<Row> = vec![
        (
            "direct",
            Some(Implement::Agent),
            Some(Merge::DirectPush),
            Readiness::Verifiable,
        ),
        (
            "gated",
            Some(Implement::Agent),
            Some(Merge::OnChecks),
            Readiness::Verifiable,
        ),
        (
            "human",
            Some(Implement::Agent),
            Some(Merge::HumanApproval),
            Readiness::Verifiable,
        ),
    ];
    let me_rows: Vec<Row> = vec![
        (
            "autonomy:safe",
            Some(Implement::Agent),
            Some(Merge::OnChecks),
            Readiness::Verifiable,
        ),
        (
            "autonomy:review",
            Some(Implement::Agent),
            Some(Merge::HumanApproval),
            Readiness::Verifiable,
        ),
        (
            "autonomy:human",
            Some(Implement::HumanOnly),
            None,
            Readiness::Verifiable,
        ),
        ("needs-intent", None, None, Readiness::NeedsIntent),
    ];

    for (map, rows) in [(ttui_map, ttui_rows), (me_map, me_rows)] {
        for (label, implement, merge, readiness) in rows {
            let a = project(map, label).unwrap_or_else(|| panic!("`{label}` is declared"));
            assert_eq!(a.implement, implement, "{label}: implement");
            assert_eq!(a.merge, merge, "{label}: merge");
            assert_eq!(a.readiness, readiness, "{label}: readiness");
        }
    }
}

/// Bullet 3's named case: a manifest declaring only `work:` produces a
/// valid, reduced view rather than an error.
#[test]
fn a_work_only_manifest_produces_a_valid_reduced_view_rather_than_an_error() {
    let mut parsed = parse_manifest_file(&manifest("ttui.yaml")).unwrap();
    parsed.verification.clear();
    parsed.artifacts.clear();
    parsed.sessions = None;
    parsed.project.root = Some(std::env::temp_dir());
    let validated = validate(parsed).expect("a work-only manifest is valid");

    let state = aggregate_project(&validated, &mut ProjectAdapters::new(), at(0));
    assert_eq!(state.name, "ttui");
    assert!(state.verification.is_empty());
    assert!(state.artifacts.is_empty());
    assert!(state.sessions.is_none());
    assert!(
        state.degradations.is_empty(),
        "an undeclared source is not a degraded one"
    );
}

/// Bullet 4: confirmation-required actions refuse to execute without
/// explicit confirmation.
#[test]
fn confirmation_required_actions_refuse_to_execute_unconfirmed() {
    let mut executor = RecordingExecutor::new();
    let irreversible = [
        Action::StopAgentRun {
            project: "ttui".into(),
            session: "s".into(),
        },
        Action::MergePullRequest {
            project: "ttui".into(),
            number: 142,
        },
        Action::Push {
            project: "ttui".into(),
            branch: "main".into(),
        },
    ];
    for action in &irreversible {
        assert_eq!(action.reversibility(), Reversibility::ConfirmationRequired);
        assert!(matches!(
            authorize(action, None),
            Err(ActionError::ConfirmationRequired { .. })
        ));
    }
    assert!(
        executor.executed().is_empty(),
        "nothing reached the executor"
    );

    // And they do execute once confirmed, so the refusal is a gate
    // rather than a wall.
    for action in &irreversible {
        let authorized = authorize(action, Some(&Confirmation::of(action))).expect("authorizes");
        executor.execute(authorized).expect("executes");
    }
    assert_eq!(executor.executed().len(), 3);
}

/// A constraint with no spec bullet of its own, asserted anyway because
/// it is the one thing that would quietly couple two sub-projects.
#[test]
fn nothing_in_this_crate_links_plumb() {
    let cargo_toml =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("the crate has a manifest");
    assert!(
        !cargo_toml.contains("parallax-plumb"),
        "Baseline consumes Plumb's output as files, never as a crate"
    );
}
