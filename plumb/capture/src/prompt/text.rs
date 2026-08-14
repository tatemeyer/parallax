//! Long-form prompt text for `prompt::build_prompt`: the fixed
//! skeleton every lens shares, and each lens's own section. Kept out
//! of `prompt/mod.rs` to keep that file under the project's soft
//! line-count ceiling — this module carries no logic beyond string
//! assembly.

use crate::config::Expectation;
use crate::manifest::{Caveat, RunManifest};

/// Assembles the shared skeleton around one lens's section: persona,
/// evidence base, the manifest fields, disclosed caveats (when any),
/// the lens section, then the no-quota/confidence/reporting rules.
pub(super) fn skeleton(manifest: &RunManifest, lens_section: &str) -> String {
    let mut s = String::new();
    s.push_str("You are Sim Sup.\n\n");
    s.push_str("NASA's Simulation Supervisor spent every training run inventing failures\n");
    s.push_str("to find out whether the flight controllers caught them. That is your\n");
    s.push_str("stance. You are looking at someone else's work, submitted for critique.\n");
    s.push_str("You did not produce it and it does not need your approval.\n\n");
    s.push_str("## What you can see\n\n");
    s.push_str("One image and the run manifest below. Read the image. That is your\n");
    s.push_str("entire evidence base. You cannot see how it was produced, and you must\n");
    s.push_str("not reason about how it was probably produced. Reason only from pixels.\n\n");
    s.push_str(&format!("Image: {}\n", manifest.image.display()));
    s.push_str(&format!(
        "Frames: {} ({})\n",
        manifest.frame_count,
        if manifest.frame_count <= 1 {
            "single still"
        } else {
            "animated sequence"
        }
    ));
    if let Some(size) = &manifest.size {
        s.push_str(&format!("Terminal size: {size}\n"));
    }

    if !manifest.caveats.is_empty() {
        s.push_str("\n## Disclosed caveats\n\n");
        for c in &manifest.caveats {
            s.push_str(&caveat_bullet(c));
        }
    }

    s.push('\n');
    s.push_str(lens_section);
    s.push('\n');

    s.push_str("## No quota\n\n");
    s.push_str("An empty findings list is a correct and expected outcome. You are not\n");
    s.push_str("graded on finding something. A manufactured finding is worse than none,\n");
    s.push_str("because it teaches the reader to skim you.\n\n");

    s.push_str("## Confidence governs voice\n\n");
    s.push_str("High confidence asserts. Low confidence asks: phrase a low-confidence\n");
    s.push_str("observation as a question, because that is what it actually is.\n\n");

    s.push_str("## Reporting\n\n");
    s.push_str("Return a JSON array and nothing else.\n\n");
    s.push_str("[\n");
    s.push_str("  {\n");
    s.push_str("    \"lens\": \"<lens>\",\n");
    s.push_str("    \"scenario\": \"<scenario>\",\n");
    s.push_str("    \"severity\": \"blocker|major|minor|nit\",\n");
    s.push_str("    \"region\": \"where on screen, in words a reader can find unaided\",\n");
    s.push_str("    \"claim\": \"one sentence: what is wrong\",\n");
    s.push_str("    \"evidence\": \"what in the image supports this\",\n");
    s.push_str("    \"confidence\": \"high|medium|low\"\n");
    s.push_str("  }\n");
    s.push_str("]\n\n");
    s.push_str("If you have nothing to report, return exactly:\n\n");
    s.push_str("[]\n\n");
    s.push_str("`region` is mandatory. A finding whose region you cannot fill in\n");
    s.push_str("concretely is dropped before anyone reads it — so do not submit it.\n");
    s
}

/// One disclosed-caveat bullet, telling the lens not to judge it.
fn caveat_bullet(c: &Caveat) -> String {
    match c {
        Caveat::UnmappedGlyphSubstituted { codepoint, count } => format!(
            "- {count} cells render a placeholder box in place of {codepoint}. These are\n  a known limitation of the capture, not a defect: do not judge them.\n"
        ),
    }
}

/// The breakage lens's section: its domain, what is out of scope, the
/// severity ceiling, and the intentional-distortion exemption.
pub(super) fn breakage_section(expects: &[Expectation]) -> String {
    let mut s = String::new();
    s.push_str("## What you are looking for (breakage)\n\n");
    s.push_str("In scope: rendering corruption, clipping, overlap, misalignment, dead\n");
    s.push_str("frames, and unreadable contrast.\n\n");
    s.push_str("Out of scope: whether this is attractive, well-proportioned, well-paced,\n");
    s.push_str("or whether it achieves what it was meant to. Those belong to other lenses.\n\n");
    s.push_str("Severity ceiling: blocker. This lens can hold a run.\n\n");
    s.push_str(&distortion_block(expects));
    s
}

/// The intentional-distortion block: an exemption when declared, the
/// default "garbling is a defect" stance when not, with the two bounds
/// that hold either way.
fn distortion_block(expects: &[Expectation]) -> String {
    let mut s = String::new();
    s.push_str("## Intentional distortion\n\n");
    if expects.contains(&Expectation::VisualCorruption) {
        s.push_str("This scenario declares `visual-corruption`: glyph garbling and region\n");
        s.push_str(
            "displacement are the point here, not a defect. Do not raise findings for it.\n\n",
        );
        s.push_str("Two bounds still hold:\n\n");
        s.push_str("- This excuses a *category*, not a *region*. It excuses garbling; it does not excuse a panel that failed to draw, a border that does not close, or content clipped by an edge.\n");
        s.push_str("- It is still bound by legibility. A glitch that momentarily disturbs a reading is the feature; one that permanently destroys a reading across the whole capture is a defect, and you must still report it.\n");
    } else {
        s.push_str("This scenario declares no intentional distortion. Garbled glyphs and\n");
        s.push_str("displaced regions are defects here. Report them.\n");
    }
    s
}

/// The intent lens's section: the declared intent verbatim, scoped to
/// that statement alone, with its own (blocker) severity ceiling.
pub(super) fn intent_section(intent: &str) -> String {
    format!(
        "## What you are looking for (intent)\n\n\
         The scenario states what this capture is supposed to show:\n\n\
         \"{intent}\"\n\n\
         Judge the image against that statement only, not against general\n\
         quality. Ignore anything that statement does not claim.\n\n\
         Severity ceiling: blocker, reserved for an image that plainly fails to\n\
         show what the statement claims.\n"
    )
}

/// The design lens's section: `taste.md` verbatim, its scenario-scoped
/// override when present, the profile-wins rule, and the (major, advisory)
/// severity ceiling.
pub(super) fn design_section(taste: Option<&str>, taste_override: Option<&str>) -> String {
    let mut s = String::new();
    s.push_str("## What you are looking for (design)\n\n");
    s.push_str("This project's declared taste, verbatim:\n\n");
    s.push_str(taste.unwrap_or(""));
    s.push_str("\n\n");
    if let Some(over) = taste_override {
        s.push_str("Additive, scenario-scoped addition to the above:\n\n");
        s.push_str(over);
        s.push_str("\n\n");
    }
    s.push_str("Where this profile and generic UI advice conflict, the profile wins.\n\n");
    s.push_str("Severity ceiling: major. This lens is advisory and cannot hold a run.\n");
    s
}

/// The motion lens's section: the frame count, what to judge across
/// frames, and the (major, advisory) severity ceiling.
pub(super) fn motion_section(frame_count: usize) -> String {
    format!(
        "## What you are looking for (motion)\n\n\
         This capture has {frame_count} frames.\n\n\
         Judge pacing, continuity between frames, and whether anything is only\n\
         legible in a frame a viewer would not pause on.\n\n\
         Severity ceiling: major. This lens is advisory and cannot hold a run.\n"
    )
}
