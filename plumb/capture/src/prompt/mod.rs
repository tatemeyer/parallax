//! Builds the blinded prompt sent to each lens agent, and plans how a
//! run's dispatches batch under the concurrency cap. `build_prompt`'s
//! only inputs are the lens, the manifest, and the taste text — there
//! is no parameter through which a diff, a source path, or the
//! adapter's command line could arrive, which is what turns the
//! blinding contract into a unit test instead of a hope. Long-form
//! prompt text lives in the private `text` submodule to keep this file
//! under the project's soft line-count ceiling.
//!
//! **Trust boundary:** `taste`, `taste_override`, and the manifest's
//! `intent` are the operator's own free text, rendered verbatim into
//! the prompt with no filtering. Blinding guarantees (no diff, no
//! source, no authorship framing) hold for everything *this module*
//! writes into the prompt; they do not extend to what a `taste.md` or
//! a scenario's `intent` string itself says. Filtering the operator's
//! own words would be the wrong fix for that — it is recorded here so
//! a future reader does not have to rediscover it.

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
mod tests;
