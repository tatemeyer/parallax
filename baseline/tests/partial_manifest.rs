//! Partial manifests are normal, not an error path: "a project that
//! satisfies only the work adapter still shows up, just with less
//! detail."

use parallax_baseline::manifest::parse_manifest;
use parallax_baseline::validate::{validate, Family, Validated};

fn validated(yaml: &str) -> Validated {
    validate(parse_manifest(yaml).expect("parses")).expect("validates")
}

const WORK_ONLY: &str = r#"
apiVersion: parallax/v1
project:
  name: work-only
  root: <projects-root>/AnotherProject
work:
  adapter: github
  repo: tatemeyer/work-only
  autonomy_map:
    gated: { implement: agent, merge: on-checks }
"#;

#[test]
fn a_work_only_manifest_produces_a_valid_reduced_view() {
    let v = validated(WORK_ONLY);
    assert!(v.declares(Family::Work));
    assert!(!v.declares(Family::Verification));
    assert!(!v.declares(Family::Artifact));
    assert!(!v.declares(Family::Session));
    assert!(v.manifest().verification.is_empty());
    assert!(v.manifest().artifacts.is_empty());
    assert!(v.manifest().sessions.is_none());
}

#[test]
fn a_project_only_manifest_is_valid_too() {
    let v = validated("project:\n  name: nothing-declared\n");
    for family in [
        Family::Work,
        Family::Verification,
        Family::Artifact,
        Family::Session,
    ] {
        assert!(!v.declares(family));
    }
}

#[test]
fn each_family_can_be_declared_alone() {
    let cases = [
        (
            "verification only",
            "project:\n  name: p\nverification:\n  - kind: tests\n    adapter: command\n    command: pytest\n",
            Family::Verification,
        ),
        (
            "artifacts only",
            "project:\n  name: p\nartifacts:\n  - kind: figure\n    watch: 'out/**/*.png'\n",
            Family::Artifact,
        ),
        (
            "sessions only",
            "project:\n  name: p\nsessions:\n  watch: '.claude/worktrees/*'\n",
            Family::Session,
        ),
    ];
    for (name, yaml, family) in cases {
        let v = validated(yaml);
        assert!(v.declares(family), "{name}: its own family");
        for other in [
            Family::Work,
            Family::Verification,
            Family::Artifact,
            Family::Session,
        ] {
            if other != family {
                assert!(!v.declares(other), "{name}: declares nothing else");
            }
        }
    }
}

/// The spec's Model-Experiments manifest is itself partial. Its absences
/// are asserted here as well as in `real_manifests.rs`, because this is
/// the file a future task will read when it wonders whether a missing
/// section is legal.
#[test]
fn an_absent_section_is_never_an_error_regardless_of_which_one() {
    for yaml in [
        "project:\n  name: p\n",
        "apiVersion: parallax/v1\nproject:\n  name: p\n",
        "project:\n  name: p\n  language: rust\n",
        "project:\n  name: p\n  methodology: outcome-first\n",
    ] {
        assert!(
            validate(parse_manifest(yaml).expect("parses")).is_ok(),
            "{yaml:?}"
        );
    }
}
