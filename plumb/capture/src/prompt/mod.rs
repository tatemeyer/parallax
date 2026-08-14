//! Builds the blinded prompt sent to each lens agent, and plans how a
//! run's dispatches batch under the concurrency cap. `build_prompt`'s
//! only inputs are the lens, the manifest, and the taste text — there
//! is no parameter through which a diff, a source path, or the
//! adapter's command line could arrive, which is what turns the
//! blinding contract into a unit test instead of a hope. Long-form
//! prompt text lives in the private `text` submodule to keep this file
//! under the project's soft line-count ceiling.

mod text;

use crate::finding::Lens;
use crate::manifest::RunManifest;
use std::collections::HashMap;
use std::path::PathBuf;

/// Everything `build_prompt` is allowed to see for one lens dispatch.
/// No field here can carry a diff, a source path, `args`, or `touches`.
pub struct LensInputs<'a> {
    /// Which lens this prompt is for.
    pub lens: Lens,
    /// The run manifest — the only per-scenario data every lens shares.
    pub manifest: &'a RunManifest,
    /// `taste.md` verbatim. Reaches the built prompt for `Lens::Design` only.
    pub taste: Option<&'a str>,
    /// Scenario-scoped addition to `taste`. Reaches `Lens::Design` only.
    pub taste_override: Option<&'a str>,
}

/// Why a lens did not apply to a capture. A design lens with no taste
/// profile is skipped with this, never run generically — a generic
/// aesthetic opinion is worse than none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// The scenario declares no `intent`.
    NoIntentDeclared,
    /// No `taste.md` exists for this run.
    NoTasteProfile,
    /// The capture is a single still; nothing moves to judge.
    SingleFrame,
}

/// Decides which lenses apply to a capture, and why any did not —
/// checked per run, never assumed.
pub fn applicable_lenses(m: &RunManifest, taste_present: bool) -> (Vec<Lens>, Vec<(Lens, Skip)>) {
    let mut apply = Vec::new();
    let mut skipped = Vec::new();

    apply.push(Lens::Breakage);

    if m.intent.is_some() {
        apply.push(Lens::Intent);
    } else {
        skipped.push((Lens::Intent, Skip::NoIntentDeclared));
    }

    if taste_present {
        apply.push(Lens::Design);
    } else {
        skipped.push((Lens::Design, Skip::NoTasteProfile));
    }

    if m.frame_count > 1 {
        apply.push(Lens::Motion);
    } else {
        skipped.push((Lens::Motion, Skip::SingleFrame));
    }

    (apply, skipped)
}

/// Builds the full text sent to one lens agent: the shared skeleton
/// wrapped around that lens's own section. Only `inputs` feeds this —
/// no diff, source path, or command line has a parameter to arrive on.
pub fn build_prompt(inputs: &LensInputs) -> String {
    let section = match inputs.lens {
        Lens::Breakage => text::breakage_section(&inputs.manifest.expects),
        Lens::Intent => text::intent_section(inputs.manifest.intent.as_deref().unwrap_or("")),
        Lens::Design => text::design_section(inputs.taste, inputs.taste_override),
        Lens::Motion => text::motion_section(inputs.manifest.frame_count),
    };
    text::skeleton(inputs.manifest, &section)
}

/// One lens dispatched against one scenario: which agent, which image,
/// and the fully built, blinded prompt.
pub struct Dispatch {
    /// Which lens this is.
    pub lens: Lens,
    /// The agent definition file's `name` this dispatches to.
    pub agent: String,
    /// The scenario under review.
    pub scenario: String,
    /// The captured image's path.
    pub image: PathBuf,
    /// The built, blinded prompt.
    pub prompt: String,
}

/// The default number of lens agents dispatched concurrently.
pub const DEFAULT_CONCURRENCY_CAP: usize = 8;

/// A whole run's dispatches, batched to the concurrency cap, with
/// every skipped lens and why.
pub struct DispatchPlan {
    /// Dispatches, chunked to at most `cap` per batch.
    pub batches: Vec<Vec<Dispatch>>,
    /// Scenario, lens, and reason for every lens that did not apply.
    pub skipped: Vec<(String, Lens, Skip)>,
    /// The concurrency cap this plan was built with.
    pub cap: usize,
}

/// Plans dispatch for a whole run: every applicable lens on every
/// manifest, with a `taste_override` matched to its own scenario only,
/// batched to `cap` dispatches at a time.
pub fn plan_dispatch(
    manifests: &[RunManifest],
    taste: Option<&str>,
    overrides: &HashMap<String, String>,
    cap: usize,
) -> DispatchPlan {
    let taste_present = taste.is_some();
    let mut flat = Vec::new();
    let mut skipped = Vec::new();

    for m in manifests {
        let (apply, skip) = applicable_lenses(m, taste_present);
        for (lens, reason) in skip {
            skipped.push((m.scenario.clone(), lens, reason));
        }
        let taste_override = overrides.get(&m.scenario).map(|s| s.as_str());
        for lens in apply {
            let prompt = build_prompt(&LensInputs {
                lens,
                manifest: m,
                taste,
                taste_override,
            });
            flat.push(Dispatch {
                lens,
                agent: lens.agent_name().to_string(),
                scenario: m.scenario.clone(),
                image: m.image.clone(),
                prompt,
            });
        }
    }

    let chunk_size = cap.max(1);
    let mut batches = Vec::new();
    let mut current = Vec::new();
    for d in flat {
        current.push(d);
        if current.len() == chunk_size {
            batches.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        batches.push(current);
    }

    DispatchPlan {
        batches,
        skipped,
        cap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Expectation;
    use crate::manifest::{Caveat, RunManifest};

    fn m(frames: usize, intent: Option<&str>, expects: Vec<Expectation>) -> RunManifest {
        RunManifest {
            run_id: "r".into(),
            scenario: "falcon-glitch-burst".into(),
            adapter: "command".into(),
            image: "falcon-glitch-burst.gif".into(),
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
}
