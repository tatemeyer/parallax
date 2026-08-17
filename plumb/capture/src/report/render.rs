//! Renders a [`RunReport`] into a single self-contained HTML document:
//! the audit trail a human reads to see *why* Plumb reached GO / NO-GO
//! / HOLD, not just the word itself. Every byte that came from a model
//! (a lens's prompt, its raw reply, a finding's claim) is arbitrary
//! text and is routed through [`html_escape`] before it reaches the
//! page, since none of it can be trusted to already be safe HTML.
//! Deliberately does not reference any external resource — this
//! document gets attached to a PR or dropped in a docs folder, so it
//! has to still render, unbroken, when opened somewhere with no
//! network access.
//!
//! `report::mod` assembles a [`RunReport`] from a run directory; this
//! module only turns one into HTML text and never touches the
//! filesystem itself. Per-lens section rendering (findings, discards,
//! the attrition chain) lives in the sibling `lens_render` module —
//! split out on line-count grounds; this module keeps only the
//! document skeleton and the scenario level.

use crate::evidence::Evidence;
use crate::report::lens_render::{render_lens, LensReport};

/// One captured run's evidence, as the top-level input to
/// [`render_report`].
pub struct RunReport {
    /// The run's identifier, as recorded in its manifest.
    pub run_id: String,
    /// The evidence-contract version the run's manifest declared, if
    /// any. `None` renders as "unknown" rather than a blank field, so
    /// a missing version reads as missing evidence, not a rendering
    /// bug.
    pub contract_version: Option<u32>,
    /// The run's `verdict.md`, rendered once above every scenario
    /// section rather than repeated per scenario — a run carries
    /// exactly one verdict even when it captured several scenarios.
    pub verdict: Evidence<String>,
    /// Every scenario captured in this run.
    pub scenarios: Vec<ScenarioReport>,
}

/// One scenario's evidence within a run: its contact sheet and each
/// lens's findings.
pub struct ScenarioReport {
    /// The scenario's name — read from the manifest when it parsed, or
    /// derived from the manifest filename when it did not (see
    /// `manifest`).
    pub scenario: String,
    /// Whether this scenario's own manifest was itself readable.
    /// `Missing`/`Unparseable` here means every other field is a
    /// stand-in (no sheet, no frames, no lenses) — the same "absence
    /// must never read as success" rule `LensReport::parsed` enforces
    /// one level down, applied here to a whole scenario: a corrupt
    /// manifest must render as a visible marker, not silently drop its
    /// scenario from the report.
    pub manifest: Evidence<()>,
    /// A `data:image/png;base64,…` encoding of the scenario's capture
    /// (the tiled contact sheet for a multi-frame capture, or the bare
    /// image for a single frame), or `None` when the image could not
    /// be read or encoded — the report still renders in that case, it
    /// just shows no sheet.
    pub sheet_uri: Option<String>,
    /// How many frames the capture holds, as recorded in the manifest.
    pub frame_count: usize,
    /// This scenario's evidence from each of the four lenses.
    pub lenses: Vec<LensReport>,
}

/// Escapes `&`, `<`, `>`, and `"` for safe interpolation into HTML text
/// or a double-quoted attribute. `&` is escaped first, deliberately —
/// escaping any of the other three characters first would then have
/// its own inserted `&` re-escaped by the `&` pass, corrupting the
/// entity. `pub(crate)` (not private): `lens_render` renders arbitrary
/// model output too (claims, evidence text, raw replies) and must
/// route through the exact same escaping, not a second copy of it.
pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Renders an `Evidence<String>`'s three states identically wherever a
/// bare piece of text (a prompt, a verdict) needs to show them:
/// `Missing` as an explicit "not persisted" marker, `Unparseable` as
/// "present but unparseable" with the raw text still shown, `Present`
/// as the text itself — always inside a `<pre>` so whitespace and line
/// breaks survive. `pub(crate)`: shared with `lens_render`'s prompt
/// rendering so both use one implementation of the three-state rule.
pub(crate) fn render_text_evidence(label: &str, evidence: &Evidence<String>) -> String {
    match evidence {
        Evidence::Missing => format!("<p class=\"empty\">{label}: not persisted</p>\n"),
        Evidence::Unparseable(raw) => format!(
            "<p>{label}: present but unparseable</p>\n<pre>{}</pre>\n",
            html_escape(raw)
        ),
        Evidence::Present(text) => format!(
            "<details><summary>{label}</summary>\n<pre>{}</pre>\n</details>\n",
            html_escape(text)
        ),
    }
}

/// The report's inline stylesheet. No `@import`, no `url(...)` to an
/// external font or asset — plain system fonts and monospace stacks
/// only, so the page renders identically with or without network
/// access. Sets an explicit background and text colour rather than
/// inheriting, since the page opens in a plain browser with no theme
/// system to fall back on. Wide content (long prompt/reply lines)
/// scrolls inside its own container instead of forcing the whole page
/// to scroll sideways.
const STYLE: &str = r#"
:root {
  color-scheme: light;
}
body {
  background: #ffffff;
  color: #1a1a1a;
  font-family: -apple-system, "Segoe UI", Helvetica, Arial, sans-serif;
  max-width: 72rem;
  margin: 0 auto;
  padding: 1.5rem 2rem 4rem;
  line-height: 1.5;
}
header {
  border-bottom: 2px solid #1a1a1a;
  margin-bottom: 1.5rem;
  padding-bottom: 1rem;
}
h1 {
  margin: 0 0 0.5rem;
  font-size: 1.5rem;
}
h2 {
  font-size: 1.15rem;
  border-top: 1px solid #ccc;
  padding-top: 1rem;
  margin-top: 2rem;
}
h3 {
  font-size: 1rem;
  margin-top: 1.5rem;
}
dl.meta {
  display: grid;
  grid-template-columns: max-content 1fr;
  column-gap: 0.75rem;
  row-gap: 0.25rem;
  margin: 0;
}
dl.meta dt {
  font-weight: 600;
  color: #444;
}
dl.meta dd {
  margin: 0;
}
.scenario {
  border: 1px solid #ccc;
  border-radius: 4px;
  padding: 1rem;
  margin-bottom: 1.5rem;
}
.lens {
  border-top: 1px dashed #ccc;
  padding-top: 0.75rem;
  margin-top: 1rem;
}
.finding {
  border-left: 3px solid #888;
  padding: 0.25rem 0 0.25rem 0.75rem;
  margin: 0.5rem 0;
}
.empty {
  color: #555;
  font-style: italic;
}
.evidence {
  color: #444;
}
img.sheet, img.crop {
  max-width: 100%;
  height: auto;
  border: 1px solid #ccc;
  display: block;
  margin: 0.5rem 0;
}
details {
  margin: 0.35rem 0;
}
summary {
  cursor: pointer;
}
pre, code {
  font-family: ui-monospace, SFMono-Regular, Consolas, "Liberation Mono", Menlo, monospace;
  font-size: 0.85rem;
}
pre {
  background: #f4f4f4;
  border: 1px solid #ddd;
  border-radius: 4px;
  padding: 0.75rem;
  white-space: pre-wrap;
  word-break: break-word;
  overflow-x: auto;
  max-width: 100%;
}
"#;

/// Renders the run's `verdict.md` once, above every scenario section —
/// always expanded, never behind a collapsed `<details>`. The verdict
/// is the report's headline (the first thing a reader checks, "what
/// was claimed"); collapse-by-default is reserved for prompts and raw
/// replies, which `render_text_evidence` still handles that way.
fn render_verdict(verdict: &Evidence<String>) -> String {
    let body = match verdict {
        Evidence::Missing => "<p class=\"empty\">verdict.md: not persisted</p>\n".to_string(),
        Evidence::Unparseable(raw) => format!(
            "<p>verdict.md: present but unparseable</p>\n<pre>{}</pre>\n",
            html_escape(raw)
        ),
        Evidence::Present(text) => format!("<pre>{}</pre>\n", html_escape(text)),
    };
    format!("<section class=\"verdict\">\n<h2>Verdict</h2>\n{body}</section>\n")
}

/// Renders one scenario's evidence block: a manifest marker when the
/// manifest itself was not readable, otherwise its contact sheet then
/// each lens's section in turn (via `lens_render::render_lens`).
fn render_scenario(scenario: &ScenarioReport) -> String {
    let mut out = format!(
        "<section class=\"scenario\">\n<h2>{}</h2>\n",
        html_escape(&scenario.scenario)
    );

    match &scenario.manifest {
        Evidence::Missing => {
            out.push_str("<p class=\"empty\">manifest: not persisted</p>\n");
        }
        Evidence::Unparseable(raw) => {
            out.push_str(&format!(
                "<p>manifest: present but unparseable</p>\n<pre>{}</pre>\n",
                html_escape(raw)
            ));
        }
        Evidence::Present(()) => {
            out.push_str(&format!("<p>Frames: {}</p>\n", scenario.frame_count));
            match &scenario.sheet_uri {
                Some(uri) => out.push_str(&format!(
                    "<img class=\"sheet\" src=\"{}\" alt=\"contact sheet\">\n",
                    html_escape(uri)
                )),
                None => out.push_str("<p class=\"empty\">contact sheet not available</p>\n"),
            }
            for lens in &scenario.lenses {
                out.push_str(&render_lens(lens));
            }
        }
    }

    out.push_str("</section>\n");
    out
}

/// Renders `run` as a complete, self-contained HTML document: no
/// `<link>`, no `http://`/`https://` reference, no relative image
/// path — every image `report::build_run_report` embeds arrives
/// pre-encoded as a `data:` URI, and every other resource (styles,
/// fonts) is inlined here.
pub fn render_report(run: &RunReport) -> String {
    let version = run
        .contract_version
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let body = if run.scenarios.is_empty() {
        "<p class=\"empty\">No scenarios recorded for this run.</p>\n".to_string()
    } else {
        run.scenarios.iter().map(render_scenario).collect()
    };

    format!(
        "<!doctype html>\n\
<html lang=\"en\">\n\
<head>\n\
<meta charset=\"utf-8\">\n\
<title>Plumb report — {run_id}</title>\n\
<style>{style}</style>\n\
</head>\n\
<body>\n\
<header>\n\
<h1>Plumb report</h1>\n\
<dl class=\"meta\">\n\
<dt>Run</dt><dd>{run_id}</dd>\n\
<dt>Contract version</dt><dd>{version}</dd>\n\
<dt>Scenarios</dt><dd>{scenario_count}</dd>\n\
</dl>\n\
</header>\n\
<main>\n\
{verdict}\
{body}\
</main>\n\
</body>\n\
</html>\n",
        run_id = html_escape(&run.run_id),
        style = STYLE,
        version = html_escape(&version),
        scenario_count = run.scenarios.len(),
        verdict = render_verdict(&run.verdict),
        body = body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_run() -> RunReport {
        RunReport {
            run_id: "r".into(),
            contract_version: Some(1),
            verdict: Evidence::Missing,
            scenarios: vec![],
        }
    }

    #[test]
    fn the_rendered_report_references_no_external_resource() {
        let html = render_report(&empty_run());
        assert!(!html.contains("http://"), "no external URL");
        assert!(!html.contains("https://"), "no external URL");
        assert!(!html.contains("<link"), "no external stylesheet or font");
        assert!(html.contains("<!doctype html>") || html.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn html_escape_neutralizes_tags_and_ampersands_in_that_order() {
        let escaped = html_escape("<script>alert(1)</script> & \"quoted\" <b>");
        assert!(!escaped.contains("<script>"), "tag must not survive raw");
        assert_eq!(
            escaped,
            "&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;quoted&quot; &lt;b&gt;"
        );
    }

    #[test]
    fn a_scenario_name_containing_a_script_tag_survives_escaped_in_the_report() {
        let mut run = empty_run();
        run.contract_version = None;
        run.scenarios.push(ScenarioReport {
            scenario: "<script>alert(1)</script> & friends".into(),
            manifest: Evidence::Present(()),
            sheet_uri: None,
            frame_count: 0,
            lenses: vec![],
        });
        let html = render_report(&run);
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "raw script tag must not reach the document: {html}"
        );
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; friends"));
    }

    #[test]
    fn a_missing_contract_version_renders_as_unknown_not_blank() {
        let mut run = empty_run();
        run.contract_version = None;
        let html = render_report(&run);
        assert!(html.contains("unknown"));
    }

    #[test]
    fn an_empty_run_still_reports_zero_scenarios_explicitly() {
        let html = render_report(&empty_run());
        assert!(html.contains("No scenarios recorded for this run."));
    }

    #[test]
    fn a_missing_verdict_says_not_persisted_rather_than_an_empty_section() {
        let html = render_report(&empty_run());
        assert!(html.contains("verdict.md: not persisted"));
    }

    #[test]
    fn an_unparseable_verdict_shows_the_raw_text() {
        let mut run = empty_run();
        run.verdict = Evidence::Unparseable("not actually markdown &".into());
        let html = render_report(&run);
        assert!(html.contains("present but unparseable"));
        assert!(html.contains("not actually markdown &amp;"));
    }

    #[test]
    fn a_present_verdict_renders_its_text() {
        let mut run = empty_run();
        run.verdict = Evidence::Present("# Plumb verdict: GO (run r)\n".into());
        let html = render_report(&run);
        assert!(html.contains("Plumb verdict: GO (run r)"));
    }

    #[test]
    fn the_verdict_renders_expanded_not_collapsed_behind_details() {
        // The verdict is the report's headline (spec question 1, "what
        // was claimed") — collapse-by-default is reserved for prompts
        // and raw replies. A reader opening the file to check a GO
        // must not have to click anything first.
        let mut run = empty_run();
        run.verdict = Evidence::Present("# Plumb verdict: GO (run r)\n".into());
        let html = render_report(&run);
        assert!(
            !html.contains("<details>"),
            "the verdict must not be wrapped in a collapsed <details>: {html}"
        );
    }

    #[test]
    fn a_scenario_with_no_sheet_says_so_rather_than_a_broken_image_tag() {
        let mut run = empty_run();
        run.scenarios.push(ScenarioReport {
            scenario: "s".into(),
            manifest: Evidence::Present(()),
            sheet_uri: None,
            frame_count: 1,
            lenses: vec![],
        });
        let html = render_report(&run);
        assert!(html.contains("contact sheet not available"));
    }

    #[test]
    fn a_scenario_with_a_sheet_embeds_it_as_a_data_uri() {
        let mut run = empty_run();
        run.scenarios.push(ScenarioReport {
            scenario: "s".into(),
            manifest: Evidence::Present(()),
            sheet_uri: Some("data:image/png;base64,AAAA".into()),
            frame_count: 8,
            lenses: vec![],
        });
        let html = render_report(&run);
        assert!(html.contains("data:image/png;base64,AAAA"));
    }

    #[test]
    fn a_missing_manifest_says_so_rather_than_a_blank_scenario() {
        let mut run = empty_run();
        run.scenarios.push(ScenarioReport {
            scenario: "unreadable".into(),
            manifest: Evidence::Missing,
            sheet_uri: None,
            frame_count: 0,
            lenses: vec![],
        });
        let html = render_report(&run);
        assert!(html.contains("manifest: not persisted"));
        assert!(
            !html.contains("Frames:"),
            "a scenario with no readable manifest must not report a frame count it never had"
        );
    }

    #[test]
    fn an_unparseable_manifest_shows_the_raw_text() {
        let mut run = empty_run();
        run.scenarios.push(ScenarioReport {
            scenario: "bad".into(),
            manifest: Evidence::Unparseable("not json at all".into()),
            sheet_uri: None,
            frame_count: 0,
            lenses: vec![],
        });
        let html = render_report(&run);
        assert!(html.contains("manifest: present but unparseable"));
        assert!(html.contains("not json at all"));
    }
}
