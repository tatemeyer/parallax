//! Both real manifests, replayed end to end through aggregation against
//! recorded fixtures. No network, no TTY, no wall clock.

use parallax_baseline::adapters::artifact::{
    ArtifactDetail, CaptureArtifactAdapter, MetricsArtifactAdapter,
};
use parallax_baseline::adapters::http::FixtureTransport;
use parallax_baseline::adapters::session::FilesystemSessionAdapter;
use parallax_baseline::adapters::verification::{
    CommandOutput, CommandVerificationAdapter, PlumbVerificationAdapter, ScriptedRunner,
    VerificationOutcome,
};
use parallax_baseline::adapters::work::{check_runs_url, issues_url, pulls_url, GithubWorkAdapter};
use parallax_baseline::autonomy::{Implement, Merge, Readiness};
use parallax_baseline::freshness::Freshness;
use parallax_baseline::manifest::parse_manifest_file;
use parallax_baseline::state::{aggregate, aggregate_project, ProjectAdapters};
use parallax_baseline::validate::{validate, Validated};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const REPO: &str = "tatemeyer/ttui";
const HEAD_142: &str = "1a7d51c9f0e2b3a4d5c6e7f8091a2b3c4d5e6f70";
const HEAD_143: &str = "0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4";

fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn fixture(relative: &str) -> PathBuf {
    crate_dir().join("tests/fixtures").join(relative)
}

fn manifest(name: &str) -> PathBuf {
    crate_dir().parent().unwrap().join("manifests").join(name)
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// Loads a real manifest and repoints `project.root` at a temp tree, so
/// filesystem-backed adapters read fixtures instead of the developer's
/// actual checkout.
fn load_rooted(name: &str, root: &Path) -> Validated {
    let mut parsed = parse_manifest_file(&manifest(name)).expect("parses");
    parsed.project.root = Some(root.to_path_buf());
    validate(parsed).expect("validates")
}

fn github_transport() -> FixtureTransport {
    let mut t = FixtureTransport::new();
    t.insert_from_file(
        issues_url(REPO),
        &fixture("github/issues.json"),
        Some("W/\"i1\""),
    )
    .unwrap();
    t.insert_from_file(
        pulls_url(REPO),
        &fixture("github/pulls.json"),
        Some("W/\"p1\""),
    )
    .unwrap();
    t.insert_from_file(
        check_runs_url(REPO, HEAD_142),
        &fixture("github/check-runs.json"),
        None,
    )
    .unwrap();
    t.insert(check_runs_url(REPO, HEAD_143), r#"{"check_runs":[]}"#, None);
    t
}

/// A TTUI-shaped tree: one completed Plumb run and two worktrees.
///
/// The run carries the `lenses/` and `merge/` subdirectories a real run
/// holds per Plumb's evidence contract, so the capture feed is exercised
/// against the layout it will actually meet rather than a flat one.
fn ttui_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path().join(".plumb/runs/20260814T112200Z");
    std::fs::create_dir_all(run.join("lenses/breakage.omnitrix-dial-rotate")).unwrap();
    std::fs::create_dir_all(run.join("merge")).unwrap();
    std::fs::write(
        run.join("lenses/breakage.omnitrix-dial-rotate/prompt.txt"),
        "as dispatched
",
    )
    .unwrap();
    std::fs::write(
        run.join("merge/survivors.json"),
        "[]
",
    )
    .unwrap();
    std::fs::copy(fixture("plumb/verdict-no-go.md"), run.join("verdict.md")).unwrap();
    for worktree in ["parallax-baseline", "widget-audit"] {
        let path = dir
            .path()
            .join(".claude/worktrees")
            .join(worktree)
            .join("src");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("lib.rs"), "// work in progress\n").unwrap();
    }
    dir
}

/// A Model-Experiments-shaped tree: one metrics feed, no sessions.
fn me_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let results = dir.path().join("projects/spectral/results/run7");
    std::fs::create_dir_all(&results).unwrap();
    std::fs::copy(fixture("metrics/loss.jsonl"), results.join("loss.jsonl")).unwrap();
    dir
}

/// Builds TTUI's adapters exactly as its manifest declares them.
fn ttui_adapters(root: &Path) -> ProjectAdapters {
    let mut a = ProjectAdapters::new();
    a.work = Some(Box::new(GithubWorkAdapter::new(github_transport())));
    let mut lint = ScriptedRunner::new();
    lint.push(CommandOutput {
        status: 0,
        stdout: String::new(),
        stderr: String::new(),
    });
    a.verification
        .push(Box::new(CommandVerificationAdapter::new(
            "lint",
            "cargo clippy --all-targets -- -D warnings",
            lint,
        )));
    let mut tests = ScriptedRunner::new();
    tests.push(CommandOutput {
        status: 101,
        stdout: String::new(),
        stderr: "test result: FAILED. 1 failed".into(),
    });
    a.verification
        .push(Box::new(CommandVerificationAdapter::new(
            "tests",
            "cargo test",
            tests,
        )));
    a.verification.push(Box::new(PlumbVerificationAdapter::new(
        "perceptual",
        root.join(".plumb/runs"),
    )));
    a.artifacts
        .push(Box::new(CaptureArtifactAdapter::new(".plumb/runs/**")));
    a.sessions = Some(Box::new(FilesystemSessionAdapter::new(
        ".claude/worktrees/*",
    )));
    a
}

#[test]
fn ttui_aggregates_every_declared_family_from_fixtures() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    assert!(
        state.degradations.is_empty(),
        "no source failed: {:?}",
        state.degradations
    );
    assert_eq!(state.work.as_ref().unwrap().value.items.len(), 6);
    assert_eq!(state.verification.len(), 3);
    assert_eq!(
        state.verification[0].value.outcome,
        VerificationOutcome::Pass,
        "lint"
    );
    assert_eq!(
        state.verification[1].value.outcome,
        VerificationOutcome::Fail,
        "tests"
    );
    assert_eq!(
        state.verification[2].value.outcome,
        VerificationOutcome::Fail,
        "perceptual NO-GO"
    );
    assert_eq!(state.artifacts.len(), 1);
    assert_eq!(state.sessions.as_ref().unwrap().value.len(), 2);
}

#[test]
fn ttui_work_items_project_onto_the_normalized_axes() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    let by_number = |n: u64| {
        state
            .autonomy
            .iter()
            .find(|a| a.number == n)
            .unwrap_or_else(|| panic!("item {n}"))
    };
    // Issue 134 is `gated`; issue 140 is `direct`; PR 142 is `gated`.
    assert_eq!(
        by_number(134).resolution.autonomy.merge,
        Some(Merge::OnChecks)
    );
    assert_eq!(
        by_number(140).resolution.autonomy.merge,
        Some(Merge::DirectPush)
    );
    assert_eq!(
        by_number(142).resolution.autonomy.merge,
        Some(Merge::OnChecks)
    );
    assert_eq!(
        by_number(134).resolution.autonomy.implement,
        Some(Implement::Agent)
    );
}

/// `needs-intent` is in Model-Experiments' map, not TTUI's. Issue 141
/// carries it anyway, so it must land in `unmapped_labels` rather than
/// silently projecting or erroring.
#[test]
fn a_label_ttui_does_not_declare_is_reported_as_unmapped() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    assert!(state.unmapped_labels.contains(&"needs-intent".to_string()));
    assert!(state.unmapped_labels.contains(&"semver:minor".to_string()));
    let item_141 = state.autonomy.iter().find(|a| a.number == 141).unwrap();
    assert!(item_141.resolution.matched.is_empty());
    assert_eq!(
        item_141.resolution.autonomy.readiness,
        Readiness::Verifiable
    );
}

#[test]
fn ttui_capture_artifacts_carry_the_run_s_verdict() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    let artifacts = &state.artifacts[0].value;
    assert_eq!(artifacts.len(), 1);
    assert_eq!(
        artifacts[0].detail,
        ArtifactDetail::Capture {
            run_id: "20260814T112200Z".into(),
            outcome: VerificationOutcome::Fail,
        }
    );
}

#[test]
fn ttui_source_freshness_distinguishes_the_polled_feed_from_the_watched_ones() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    let sources = state.sources(at(45));
    let work = sources.iter().find(|s| s.label == "work").unwrap();
    assert!(work.freshness.is_stale(), "45s past a 30s interval");
    for label in [
        "verification:lint",
        "verification:tests",
        "verification:perceptual",
        "sessions",
    ] {
        let source = sources
            .iter()
            .find(|s| s.label == label)
            .unwrap_or_else(|| panic!("{label}"));
        assert_eq!(
            source.freshness,
            Freshness::Live,
            "{label} is filesystem-backed"
        );
    }
}

/// Model-Experiments' manifest declares no `sessions:`. Its reduced view
/// is the spec's partial-support case, proved against the real file.
#[test]
fn model_experiments_aggregates_to_a_reduced_view_with_no_session_source() {
    let tree = me_tree();
    let validated = load_rooted("model-experiments.yaml", tree.path());
    let mut adapters = ProjectAdapters::new();
    adapters
        .artifacts
        .push(Box::new(MetricsArtifactAdapter::new(
            "projects/*/results/**/*.jsonl",
        )));
    let state = aggregate_project(&validated, &mut adapters, at(0));

    assert!(state.sessions.is_none(), "no session feed declared");
    assert!(
        state.work.is_none(),
        "no work adapter registered in this test"
    );
    assert!(state.degradations.is_empty(), "absent is not degraded");
    assert_eq!(state.artifacts.len(), 1);
    match &state.artifacts[0].value[0].detail {
        ArtifactDetail::Metrics { series } => {
            assert!(series.iter().any(|s| s.name == "loss"));
        }
        other => panic!("expected metrics, got {other:?}"),
    }
}

#[test]
fn a_work_only_registration_still_produces_a_valid_project_state() {
    let tree = tempfile::tempdir().unwrap();
    let mut parsed = parse_manifest_file(&manifest("ttui.yaml")).unwrap();
    parsed.project.root = Some(tree.path().to_path_buf());
    parsed.verification.clear();
    parsed.artifacts.clear();
    parsed.sessions = None;
    let validated = validate(parsed).unwrap();

    let mut adapters = ProjectAdapters::new();
    adapters.work = Some(Box::new(GithubWorkAdapter::new(github_transport())));
    let state = aggregate_project(&validated, &mut adapters, at(0));

    assert_eq!(state.work.as_ref().unwrap().value.items.len(), 6);
    assert!(state.verification.is_empty());
    assert!(state.artifacts.is_empty());
    assert!(state.sessions.is_none());
    assert!(state.degradations.is_empty());
    assert_eq!(
        state.sources(at(0)).len(),
        1,
        "one declared source, one reported"
    );
}

#[test]
fn both_projects_aggregate_into_one_platform_state() {
    let ttui = ttui_tree();
    let me = me_tree();
    let mut me_adapters = ProjectAdapters::new();
    me_adapters
        .artifacts
        .push(Box::new(MetricsArtifactAdapter::new(
            "projects/*/results/**/*.jsonl",
        )));

    let mut inputs = vec![
        (
            load_rooted("ttui.yaml", ttui.path()),
            ttui_adapters(ttui.path()),
        ),
        (
            load_rooted("model-experiments.yaml", me.path()),
            me_adapters,
        ),
    ];
    let platform = aggregate(&mut inputs, at(0));

    assert_eq!(platform.projects.len(), 2);
    assert_eq!(
        platform.project("ttui").unwrap().methodology.as_deref(),
        Some("methodology-first")
    );
    assert_eq!(
        platform
            .project("model-experiments")
            .unwrap()
            .methodology
            .as_deref(),
        Some("outcome-first")
    );
    assert!(platform.degraded().is_empty());
}

/// The spec: "`methodology:` appears in the manifest as informational
/// metadata only — nothing in the platform branches on it." Two
/// registrations identical but for that field must aggregate to
/// identical state.
#[test]
fn methodology_changes_nothing_about_the_aggregated_state() {
    let tree = me_tree();

    let mut a = parse_manifest_file(&manifest("model-experiments.yaml")).unwrap();
    a.project.root = Some(tree.path().to_path_buf());
    let mut b = a.clone();
    a.project.methodology = Some("outcome-first".into());
    b.project.methodology = Some("methodology-first".into());

    let build = |manifest| {
        let validated = validate(manifest).unwrap();
        let mut adapters = ProjectAdapters::new();
        adapters
            .artifacts
            .push(Box::new(MetricsArtifactAdapter::new(
                "projects/*/results/**/*.jsonl",
            )));
        let state = aggregate_project(&validated, &mut adapters, at(0));
        (
            state.artifacts.len(),
            state.artifacts[0].value.len(),
            state.sources(at(0)).len(),
            state.degradations.len(),
            state.work.is_some(),
            state.sessions.is_some(),
        )
    };

    assert_eq!(
        build(a),
        build(b),
        "methodology must not reach any behaviour"
    );
}
