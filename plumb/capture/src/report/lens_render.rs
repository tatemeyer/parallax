//! One lens's evidence and findings, and how that section of the
//! report renders. Split out of `render.rs` on line-count grounds
//! (soft ceiling 500 lines/file): document/scenario assembly and
//! per-lens rendering are independently readable concerns, and this
//! module owns the second one exclusively — it renders no `<html>`,
//! `<head>`, or top-level `<main>` wrapper of its own.

use crate::evidence::Evidence;
use crate::finding::{ClampRecord, Confidence, Finding, Lens, Severity};
use crate::report::render::{html_escape, render_text_evidence};

/// One lens's evidence and findings for one scenario: what it was
/// asked, what it returned, what survived to the verdict, and
/// everything discarded along the way.
pub struct LensReport {
    /// Which lens this section is about.
    pub lens: Lens,
    /// Whether this lens's enforcement outcome (`parsed.json`) was
    /// itself readable. Surfaced separately from `findings` — which is
    /// `parsed.json`'s content minus anything a ruling later
    /// suppressed — because an empty `findings` list must stay
    /// visibly distinct from "parsed.json missing or corrupt"; the
    /// same "absence never reads as success" rule this task's
    /// load-bearing test enforces for dropped/clamped findings applies
    /// here too, one level up.
    pub parsed: Evidence<()>,
    /// Findings that survived every stage — enforcement and
    /// suppression — and therefore actually reached the verdict.
    pub findings: Vec<RenderedFinding>,
    /// Findings dropped for naming no region.
    pub dropped: Evidence<Vec<Finding>>,
    /// Findings whose severity was clamped to this lens's ceiling.
    pub clamped: Evidence<Vec<ClampRecord>>,
    /// Findings a prior ruling overruled after merge — a whole-run
    /// artifact filtered to this lens, so `Missing`/`Unparseable` here
    /// reflect the shared `merge/suppressed.json` file's own state.
    pub suppressed: Evidence<Vec<Finding>>,
    /// The prompt this lens was dispatched with.
    pub prompt: Evidence<String>,
    /// Every raw reply this lens returned, oldest attempt first.
    pub replies: Vec<(u32, String)>,
}

/// A finding paired with the crop of the frame its region names, when
/// that region resolved to one.
pub struct RenderedFinding {
    /// The finding as it reached the verdict.
    pub finding: Finding,
    /// A `data:image/png;base64,…` crop of the named frame, or `None`
    /// when the region did not unambiguously identify one — in that
    /// case the full contact sheet, already shown once for the
    /// scenario, stands in for it.
    pub crop_uri: Option<String>,
}

/// `Lens`'s presentation label, matching `finding::Lens`'s serde
/// spelling. `Lens` has no `Display` impl (only a `serde` rename), so
/// this is this module's own mapping — the same pattern
/// `verdict::render`'s private `lens_name` already uses.
fn lens_label(lens: Lens) -> &'static str {
    match lens {
        Lens::Breakage => "breakage",
        Lens::Intent => "intent",
        Lens::Design => "design",
        Lens::Motion => "motion",
    }
}

fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Blocker => "blocker",
        Severity::Major => "major",
        Severity::Minor => "minor",
        Severity::Nit => "nit",
    }
}

fn confidence_label(c: Confidence) -> &'static str {
    match c {
        Confidence::High => "high",
        Confidence::Medium => "medium",
        Confidence::Low => "low",
    }
}

/// A discard/clamp count for the attrition chain: a real number when
/// the underlying evidence was `Present`, unknown otherwise. An
/// unknown count must never render as `0` — that would make an
/// unreadable artifact look like a healthy "found nothing" result, in
/// the one line a hurried reader is most likely to read instead of
/// the discard sections beside it.
enum Count {
    Known(usize),
    Unknown,
}

impl std::fmt::Display for Count {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Count::Known(n) => write!(f, "{n}"),
            Count::Unknown => write!(f, "?"),
        }
    }
}

/// The count an `Evidence<Vec<T>>` carries for the attrition chain:
/// [`Count::Known`] for `Present`, [`Count::Unknown`] otherwise — the
/// discard section rendered alongside it still shows the marker text,
/// so the unreadable state itself is never hidden, only kept out of
/// the arithmetic.
fn evidence_count<T>(evidence: &Evidence<Vec<T>>) -> Count {
    match evidence {
        Evidence::Present(items) => Count::Known(items.len()),
        Evidence::Missing | Evidence::Unparseable(_) => Count::Unknown,
    }
}

/// The attrition chain: how many findings this lens originally
/// reported, how many were dropped, clamped, and suppressed, and how
/// many survived. `raw` is derived (dropped + survivors + suppressed)
/// rather than stored, and is itself unknown whenever `dropped`,
/// `suppressed`, or `survivors` is. `clamped` is a subset of
/// `survivors`, not a term in that sum, so its own `?` never corrupts
/// `raw`. `survivors` is gated on `lens.parsed`'s own state exactly
/// like every other term here is gated on its own evidence source —
/// `lens.findings.len()` is only trustworthy when `parsed.json` itself
/// was actually readable; a corrupt `parsed.json` empties `findings`
/// (see `assemble::build_lens_report`), and a bare `usize` derived
/// from that would silently read as a known, healthy zero instead of
/// the unread artifact it actually is.
fn render_attrition(lens: &LensReport) -> String {
    let dropped = evidence_count(&lens.dropped);
    let clamped = evidence_count(&lens.clamped);
    let suppressed = evidence_count(&lens.suppressed);
    let survivors = match &lens.parsed {
        Evidence::Present(()) => Count::Known(lens.findings.len()),
        Evidence::Missing | Evidence::Unparseable(_) => Count::Unknown,
    };
    let raw = match (&dropped, &survivors, &suppressed) {
        (Count::Known(d), Count::Known(sv), Count::Known(s)) => Count::Known(d + sv + s),
        _ => Count::Unknown,
    };
    format!(
        "<p class=\"attrition\">{raw} raw \u{2192} {dropped} dropped \u{2192} {clamped} clamped \u{2192} {suppressed} suppressed \u{2192} {survivors} survivors</p>\n"
    )
}

/// Renders the findings that reached the verdict: each with its region
/// crop when one resolved, or standing against the scenario's full
/// sheet (already rendered above) when it did not.
fn render_findings(findings: &[RenderedFinding]) -> String {
    if findings.is_empty() {
        return "<p class=\"empty\">no findings</p>\n".to_string();
    }
    let mut out = String::new();
    for rf in findings {
        let f = &rf.finding;
        out.push_str("<div class=\"finding\">\n");
        out.push_str(&format!(
            "<p><strong>[{}]</strong> {} \u{2014} {}</p>\n",
            html_escape(severity_label(f.severity)),
            html_escape(&f.region),
            html_escape(&f.claim)
        ));
        out.push_str(&format!(
            "<p class=\"evidence\">evidence: {} (confidence: {})</p>\n",
            html_escape(&f.evidence),
            html_escape(confidence_label(f.confidence))
        ));
        match &rf.crop_uri {
            Some(uri) => out.push_str(&format!(
                "<img class=\"crop\" src=\"{}\" alt=\"cropped frame for: {}\">\n",
                html_escape(uri),
                html_escape(&f.region)
            )),
            // A declined region (ambiguous, or naming no frame at all)
            // must say so explicitly — rendering nothing here is its
            // own small instance of this whole task's rule, since a
            // reader can't tell "declined, see the full sheet above"
            // from "this finding has no visual evidence at all".
            None => out.push_str(
                "<p class=\"empty\">region not resolved to a frame (see the full contact sheet above)</p>\n",
            ),
        }
        out.push_str("</div>\n");
    }
    out
}

/// Whether this lens's `parsed.json` was itself readable, rendered as
/// the same three-state marker every other artifact uses.
fn render_parsed_state(parsed: &Evidence<()>) -> String {
    match parsed {
        Evidence::Missing => "<p class=\"empty\">findings: not persisted</p>\n".to_string(),
        Evidence::Unparseable(raw) => format!(
            "<p>findings: present but unparseable</p>\n<pre>{}</pre>\n",
            html_escape(raw)
        ),
        Evidence::Present(()) => String::new(),
    }
}

/// Renders one discard section (dropped or suppressed findings): a
/// visible summary line per the artifact's own three-state marker,
/// then one collapsed `<details>` per discarded finding whose
/// `<summary>` stays visible even collapsed and whose body carries the
/// claim and evidence text — the property the load-bearing test
/// depends on: the text is present in the document regardless of
/// whether a reader ever expands it.
fn render_finding_discards(title: &str, evidence: &Evidence<Vec<Finding>>) -> String {
    match evidence {
        Evidence::Missing => format!("<p class=\"empty\">{title}: not persisted</p>\n"),
        Evidence::Unparseable(raw) => format!(
            "<p>{title}: present but unparseable</p>\n<pre>{}</pre>\n",
            html_escape(raw)
        ),
        Evidence::Present(items) => {
            let mut out = format!("<p>{title} ({})</p>\n", items.len());
            for f in items {
                out.push_str(&format!(
                    "<details><summary>[{}] {}</summary>\n<p>{}</p>\n<p class=\"evidence\">evidence: {}</p>\n</details>\n",
                    html_escape(severity_label(f.severity)),
                    html_escape(&f.region),
                    html_escape(&f.claim),
                    html_escape(&f.evidence),
                ));
            }
            out
        }
    }
}

/// Renders the clamped-findings discard section. Kept apart from
/// [`render_finding_discards`] because a `ClampRecord` names both the
/// severity a finding survived at and the severity it was lowered
/// from, which a bare `Finding` cannot express.
fn render_clamp_discards(evidence: &Evidence<Vec<ClampRecord>>) -> String {
    match evidence {
        Evidence::Missing => "<p class=\"empty\">Clamped: not persisted</p>\n".to_string(),
        Evidence::Unparseable(raw) => format!(
            "<p>Clamped: present but unparseable</p>\n<pre>{}</pre>\n",
            html_escape(raw)
        ),
        Evidence::Present(items) => {
            let mut out = format!("<p>Clamped ({})</p>\n", items.len());
            for c in items {
                let f = &c.finding;
                out.push_str(&format!(
                    "<details><summary>[{} \u{2192} {}] {}</summary>\n<p>{}</p>\n<p class=\"evidence\">evidence: {}</p>\n</details>\n",
                    html_escape(severity_label(c.from)),
                    html_escape(severity_label(f.severity)),
                    html_escape(&f.region),
                    html_escape(&f.claim),
                    html_escape(&f.evidence),
                ));
            }
            out
        }
    }
}

/// Renders every raw reply this lens returned. An empty list means no
/// reply was ever persisted for this lens/scenario and must say so
/// explicitly — never silently render as a lens that returned zero
/// findings, which is a materially different fact about the run.
fn render_replies(replies: &[(u32, String)]) -> String {
    if replies.is_empty() {
        return "<p class=\"empty\">no reply persisted</p>\n".to_string();
    }
    let mut out = String::new();
    for (attempt, text) in replies {
        out.push_str(&format!(
            "<details><summary>Reply, attempt {attempt}</summary>\n<pre>{}</pre>\n</details>\n",
            html_escape(text)
        ));
    }
    out
}

/// Renders one lens's whole section: its findings, every discard, the
/// attrition chain accounting for all of them, then the prompt and
/// every reply. Called by `render::render_scenario`, one per lens.
pub(crate) fn render_lens(lens: &LensReport) -> String {
    let mut out = format!(
        "<article class=\"lens\">\n<h3>{}</h3>\n",
        html_escape(lens_label(lens.lens))
    );

    out.push_str(&render_parsed_state(&lens.parsed));
    out.push_str(&render_findings(&lens.findings));
    out.push_str(&render_finding_discards(
        "Dropped (no region named)",
        &lens.dropped,
    ));
    out.push_str(&render_clamp_discards(&lens.clamped));
    out.push_str(&render_finding_discards(
        "Suppressed (overruled by a prior ruling)",
        &lens.suppressed,
    ));
    out.push_str(&render_attrition(lens));
    out.push_str(&render_text_evidence("Prompt", &lens.prompt));
    out.push_str(&render_replies(&lens.replies));

    out.push_str("</article>\n");
    out
}

#[cfg(test)]
mod tests;
