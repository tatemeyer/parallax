//! Tests for `prompt`: the 17 behavioral tests from the Task 8 brief,
//! plus coverage added on review for the four authored prose bodies,
//! the `cap.max(1)` guard, and the taste-`None` path — split into its
//! own file to keep `prompt/mod.rs` under the line-count ceiling.

use super::*;
use crate::config::Expectation;
use crate::manifest::{Caveat, RunManifest};

fn m(frames: usize, intent: Option<&str>, expects: Vec<Expectation>) -> RunManifest {
    RunManifest {
        run_id: "r".into(),
        scenario: "falcon-glitch-burst".into(),
        adapter: "command".into(),
        image: "falcon-glitch-burst.gif".into(),
        animation: None,
        frame_count: frames,
        size: Some("120x40".into()),
        intent: intent.map(String::from),
        expects,
        caveats: Vec::new(),
    }
}

// --- applicability -------------------------------------------------

#[test]
fn breakage_always_applies() {
    let (apply, _) = applicable_lenses(&m(1, None, vec![]), false);
    assert!(apply.contains(&Lens::Breakage));
}

#[test]
fn intent_is_skipped_with_a_notice_when_no_intent_is_declared() {
    let (apply, skipped) = applicable_lenses(&m(1, None, vec![]), true);
    assert!(!apply.contains(&Lens::Intent));
    assert!(skipped.contains(&(Lens::Intent, Skip::NoIntentDeclared)));
}

#[test]
fn design_is_skipped_with_a_notice_when_no_taste_profile_exists() {
    // A generic aesthetic opinion is worse than none.
    let (apply, skipped) = applicable_lenses(&m(1, Some("i"), vec![]), false);
    assert!(!apply.contains(&Lens::Design));
    assert!(skipped.contains(&(Lens::Design, Skip::NoTasteProfile)));
}

#[test]
fn motion_is_skipped_with_a_notice_on_a_single_frame_capture() {
    let (apply, skipped) = applicable_lenses(&m(1, Some("i"), vec![]), true);
    assert!(!apply.contains(&Lens::Motion));
    assert!(skipped.contains(&(Lens::Motion, Skip::SingleFrame)));
}

#[test]
fn all_four_apply_on_a_multiframe_capture_with_intent_and_taste() {
    let (apply, skipped) = applicable_lenses(&m(5, Some("i"), vec![]), true);
    assert_eq!(apply.len(), 4);
    assert!(skipped.is_empty());
}

// --- blinding ------------------------------------------------------

/// The single most important test in this crate.
#[test]
fn no_prompt_carries_a_diff_source_authorship_or_change_framing() {
    for lens in [Lens::Breakage, Lens::Intent, Lens::Design, Lens::Motion] {
        let manifest = m(5, Some("The panel stays legible."), vec![]);
        let p = build_prompt(&LensInputs {
            lens,
            manifest: &manifest,
            taste: Some("Loud in colour, disciplined in structure."),
            taste_override: None,
        })
        .to_lowercase();
        for forbidden in [
            "diff",
            "git ",
            "commit",
            "source code",
            "the code",
            "your change",
            "you changed",
            "verify",
            "confirm this",
            "looks right",
            "regression",
            "src/",
            "examples/",
            "cargo",
            "--example",
            "touches",
            // Change-implying synonyms.
            "update",
            "modified",
            "revision",
            "previously",
            "recent",
            "before",
            "after",
            // Authorship words.
            "developer",
            "author",
            "the team",
            // App/tooling identity.
            "ttui",
            "omnitrix",
            "visual-snapshot",
        ] {
            assert!(
                !p.contains(forbidden),
                "{lens:?} prompt leaked {forbidden:?}"
            );
        }
    }
}

#[test]
fn every_prompt_frames_the_work_as_someone_elses() {
    for lens in [Lens::Breakage, Lens::Intent, Lens::Design, Lens::Motion] {
        let manifest = m(5, Some("i"), vec![]);
        let p = build_prompt(&LensInputs {
            lens,
            manifest: &manifest,
            taste: Some("t"),
            taste_override: None,
        });
        assert!(p.contains("Sim Sup"), "{lens:?} must carry the persona");
        assert!(
            p.contains("someone else"),
            "{lens:?} must use third-party framing"
        );
    }
}

#[test]
fn every_prompt_states_that_an_empty_list_is_a_correct_outcome() {
    let manifest = m(5, Some("i"), vec![]);
    let p = build_prompt(&LensInputs {
        lens: Lens::Breakage,
        manifest: &manifest,
        taste: None,
        taste_override: None,
    });
    assert!(p.contains("[]"));
    assert!(p.to_lowercase().contains("expected outcome"));
}

// --- per-lens payloads ---------------------------------------------

#[test]
fn only_the_intent_lens_receives_the_declared_intent() {
    let manifest = m(5, Some("THE DIAL ROTATES"), vec![]);
    let intent_prompt = build_prompt(&LensInputs {
        lens: Lens::Intent,
        manifest: &manifest,
        taste: Some("t"),
        taste_override: None,
    });
    assert!(intent_prompt.contains("THE DIAL ROTATES"));
    for lens in [Lens::Breakage, Lens::Design, Lens::Motion] {
        let p = build_prompt(&LensInputs {
            lens,
            manifest: &manifest,
            taste: Some("t"),
            taste_override: None,
        });
        assert!(
            !p.contains("THE DIAL ROTATES"),
            "{lens:?} must not see the intent"
        );
    }
}

#[test]
fn only_the_design_lens_receives_the_taste_profile_and_its_override() {
    let manifest = m(5, Some("i"), vec![]);
    let design = build_prompt(&LensInputs {
        lens: Lens::Design,
        manifest: &manifest,
        taste: Some("DENSITY IS INTENTIONAL"),
        taste_override: Some("SCRUFFIER THAN THE HOUSE GRAMMAR"),
    });
    assert!(design.contains("DENSITY IS INTENTIONAL"));
    assert!(design.contains("SCRUFFIER THAN THE HOUSE GRAMMAR"));
    for lens in [Lens::Breakage, Lens::Intent, Lens::Motion] {
        let p = build_prompt(&LensInputs {
            lens,
            manifest: &manifest,
            taste: Some("DENSITY IS INTENTIONAL"),
            taste_override: Some("SCRUFFIER THAN THE HOUSE GRAMMAR"),
        });
        assert!(
            !p.contains("DENSITY IS INTENTIONAL"),
            "{lens:?} must not see taste.md"
        );
    }
}

// --- intentional distortion ----------------------------------------

#[test]
fn declared_visual_corruption_reaches_the_breakage_lens_as_an_exemption() {
    let manifest = m(5, Some("i"), vec![Expectation::VisualCorruption]);
    let p = build_prompt(&LensInputs {
        lens: Lens::Breakage,
        manifest: &manifest,
        taste: None,
        taste_override: None,
    });
    assert!(p.contains("visual-corruption"));
    assert!(p.contains("Do not raise findings for it"));
    // Bound 1: a category, not a region.
    assert!(p.contains("does not excuse a panel that failed to draw"));
    // Bound 2: still bound by legibility.
    assert!(p.contains("permanently destroys a reading"));
}

#[test]
fn an_undeclared_scenario_gets_the_default_garbling_is_a_defect_treatment() {
    let manifest = m(5, Some("i"), vec![]);
    let p = build_prompt(&LensInputs {
        lens: Lens::Breakage,
        manifest: &manifest,
        taste: None,
        taste_override: None,
    });
    assert!(!p.contains("visual-corruption"));
    assert!(p.contains("This scenario declares no intentional distortion"));
}

#[test]
fn expects_is_a_breakage_lens_input_only() {
    let manifest = m(5, Some("i"), vec![Expectation::VisualCorruption]);
    for lens in [Lens::Intent, Lens::Design, Lens::Motion] {
        let p = build_prompt(&LensInputs {
            lens,
            manifest: &manifest,
            taste: Some("t"),
            taste_override: None,
        });
        assert!(
            !p.contains("visual-corruption"),
            "{lens:?} must not receive expects"
        );
    }
}

// --- caveats and batching -------------------------------------------

#[test]
fn a_disclosed_caveat_reaches_every_lens() {
    let mut manifest = m(5, Some("i"), vec![]);
    manifest.caveats = vec![Caveat::UnmappedGlyphSubstituted {
        codepoint: "U+2726".into(),
        count: 3,
    }];
    for lens in [Lens::Breakage, Lens::Intent, Lens::Design, Lens::Motion] {
        let p = build_prompt(&LensInputs {
            lens,
            manifest: &manifest,
            taste: Some("t"),
            taste_override: None,
        });
        assert!(
            p.contains("U+2726"),
            "{lens:?} must be told about placeholders"
        );
        assert!(
            p.contains("do not judge"),
            "{lens:?} must be told not to judge them"
        );
    }
}

#[test]
fn dispatch_batches_at_the_concurrency_cap_and_reports_the_cap() {
    let manifests: Vec<_> = (0..3)
        .map(|i| {
            let mut mm = m(5, Some("i"), vec![]);
            mm.scenario = format!("s{i}");
            mm
        })
        .collect();
    // 3 scenarios x 4 applicable lenses = 12 dispatches, cap 8.
    let plan = plan_dispatch(&manifests, Some("t"), &Default::default(), 8);
    assert_eq!(plan.batches.len(), 2);
    assert_eq!(plan.batches[0].len(), 8);
    assert_eq!(plan.batches[1].len(), 4);
    assert_eq!(plan.cap, 8);
}

#[test]
fn the_default_concurrency_cap_is_eight() {
    assert_eq!(DEFAULT_CONCURRENCY_CAP, 8);
}

#[test]
fn a_taste_override_is_matched_to_its_scenario_only() {
    let manifests = vec![m(5, Some("i"), vec![])];
    let mut overrides = std::collections::HashMap::new();
    overrides.insert(
        "some-other-scenario".to_string(),
        "NOT THIS ONE".to_string(),
    );
    let plan = plan_dispatch(&manifests, Some("t"), &overrides, 8);
    for d in plan.batches.iter().flatten() {
        assert!(!d.prompt.contains("NOT THIS ONE"));
    }
}

// --- added on review: Finding 1, design with no taste profile ------

#[test]
fn design_with_no_taste_profile_renders_an_explicit_abstention_not_a_generic_critique() {
    let manifest = m(5, Some("i"), vec![]);
    let p = build_prompt(&LensInputs {
        lens: Lens::Design,
        manifest: &manifest,
        taste: None,
        taste_override: None,
    })
    .to_lowercase();
    assert!(
        p.contains("no taste profile is declared"),
        "must state the abstention explicitly, not render a blank profile"
    );
    assert!(
        p.contains("report nothing on this lens"),
        "must instruct the lens to abstain, not critique generically"
    );
    assert!(
        !p.contains("this project's declared taste"),
        "must not render the taste-profile framing with nothing to fill it"
    );
}

// --- added on review: Finding 2, design/motion scope and abstention -

#[test]
fn design_declares_abstention_when_the_profile_is_silent_on_a_specific_point() {
    let manifest = m(5, Some("i"), vec![]);
    let p = build_prompt(&LensInputs {
        lens: Lens::Design,
        manifest: &manifest,
        taste: Some("Loud in colour, disciplined in structure."),
        taste_override: None,
    });
    assert!(
        p.contains("no standard to judge against"),
        "design must say to abstain, not substitute stock UI advice, where the profile is silent"
    );
}

#[test]
fn design_carries_the_low_confidence_exemplar_from_the_spec() {
    let manifest = m(5, Some("i"), vec![]);
    let p = build_prompt(&LensInputs {
        lens: Lens::Design,
        manifest: &manifest,
        taste: Some("t"),
        taste_override: None,
    });
    assert!(p.contains("is the mode label meant to overlap the frame corner?"));
}

// --- added on review: Task 14a follow-up, contact-sheet wording -----

#[test]
fn multi_frame_frames_line_describes_a_contact_sheet_not_an_animation() {
    let manifest = m(5, Some("i"), vec![]);
    let p = build_prompt(&LensInputs {
        lens: Lens::Breakage,
        manifest: &manifest,
        taste: None,
        taste_override: None,
    });
    assert!(
        p.contains("Frames: 5 (contact sheet in reading order, separated by gutters)"),
        "multi-frame Frames line must describe a contact sheet, not an animation: {p}"
    );
    assert!(
        !p.to_lowercase().contains("animated sequence"),
        "the agent receives a contact sheet, never an animation, and must not be told otherwise"
    );
}

#[test]
fn single_frame_frames_line_is_unchanged() {
    let manifest = m(1, Some("i"), vec![]);
    let p = build_prompt(&LensInputs {
        lens: Lens::Breakage,
        manifest: &manifest,
        taste: None,
        taste_override: None,
    });
    assert!(p.contains("Frames: 1 (single still)"));
}

#[test]
fn motion_scopes_itself_to_what_only_a_sequence_reveals() {
    let manifest = m(5, Some("i"), vec![]);
    let p = build_prompt(&LensInputs {
        lens: Lens::Motion,
        manifest: &manifest,
        taste: None,
        taste_override: None,
    });
    assert!(p.contains("pacing"));
    assert!(
        p.contains("belongs to breakage"),
        "motion must not restate a single-frame defect breakage already owns"
    );
}

// --- added on review: Finding 3, image path leak --------------------

#[test]
fn image_path_is_rendered_as_a_file_name_only_never_a_full_or_absolute_path() {
    let mut manifest = m(1, None, vec![]);
    manifest.image = std::path::PathBuf::from(
        "D:/Dev/Projects/TTUI/.plumb/runs/20260101T000000Z/falcon-glitch-burst.gif",
    );
    let p = build_prompt(&LensInputs {
        lens: Lens::Breakage,
        manifest: &manifest,
        taste: None,
        taste_override: None,
    });
    assert!(p.contains("falcon-glitch-burst.gif"));
    for leaked in [
        "TTUI",
        "Dev",
        "Projects",
        ".plumb/runs",
        "20260101T000000Z",
        "D:/",
    ] {
        assert!(!p.contains(leaked), "leaked path component {leaked:?}: {p}");
    }
}

// --- added on review: Finding 4, untested authored behavior ---------

#[test]
fn dispatch_plan_skipped_carries_the_scenario_name() {
    // No intent, no taste, single frame: every non-breakage lens skips,
    // each tagged with the scenario that produced it.
    let manifests = vec![m(1, None, vec![])];
    let plan = plan_dispatch(&manifests, None, &Default::default(), 8);
    assert!(plan.skipped.contains(&(
        "falcon-glitch-burst".to_string(),
        Lens::Intent,
        Skip::NoIntentDeclared
    )));
    assert!(plan.skipped.contains(&(
        "falcon-glitch-burst".to_string(),
        Lens::Design,
        Skip::NoTasteProfile
    )));
    assert!(plan.skipped.contains(&(
        "falcon-glitch-burst".to_string(),
        Lens::Motion,
        Skip::SingleFrame
    )));
}

#[test]
fn dispatch_carries_the_agent_name_and_the_image_path() {
    let mut manifest = m(1, None, vec![]);
    manifest.image = std::path::PathBuf::from("falcon-glitch-burst.gif");
    let plan = plan_dispatch(&[manifest], None, &Default::default(), 8);
    let d = &plan.batches[0][0];
    assert_eq!(d.lens, Lens::Breakage);
    assert_eq!(d.agent, "critic-breakage");
    assert_eq!(d.image, std::path::PathBuf::from("falcon-glitch-burst.gif"));
}

#[test]
fn every_lens_states_its_own_severity_ceiling() {
    let manifest = m(5, Some("i"), vec![]);
    let cases = [
        (Lens::Breakage, "Severity ceiling: blocker"),
        (Lens::Intent, "Severity ceiling: blocker"),
        (Lens::Design, "Severity ceiling: major"),
        (Lens::Motion, "Severity ceiling: major"),
    ];
    for (lens, ceiling) in cases {
        let p = build_prompt(&LensInputs {
            lens,
            manifest: &manifest,
            taste: Some("t"),
            taste_override: None,
        });
        assert!(p.contains(ceiling), "{lens:?} must state {ceiling:?}");
    }
}

#[test]
fn plan_dispatch_treats_a_zero_cap_as_one_dispatch_per_batch() {
    // The chunking guard (cap.max(1)) must not panic on chunks(0), and
    // must not silently widen the caller's reported cap.
    let manifests = vec![m(5, Some("i"), vec![])];
    let plan = plan_dispatch(&manifests, Some("t"), &Default::default(), 0);
    assert_eq!(
        plan.cap, 0,
        "the reported cap must be exactly what was passed"
    );
    assert_eq!(plan.batches.len(), 4, "4 applicable lenses, one per batch");
    for batch in &plan.batches {
        assert_eq!(batch.len(), 1);
    }
}
