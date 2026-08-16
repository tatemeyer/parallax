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
    /// Every scenario captured in this run.
    pub scenarios: Vec<ScenarioReport>,
}

/// One scenario's evidence within a run. Stub for this task — Task 8
/// fills it with the scenario's captured frames, each lens's prompt
/// and raw reply, and its findings. Only `scenario` exists here so
/// [`render_report`] has something to name per scenario; Task 8 adds
/// fields rather than reshaping this one, so nothing here should need
/// to change to accommodate it.
pub struct ScenarioReport {
    /// The scenario's name, as recorded in the manifest.
    pub scenario: String,
}

/// Escapes `&`, `<`, `>`, and `"` for safe interpolation into HTML text
/// or a double-quoted attribute. `&` is escaped first, deliberately —
/// escaping any of the other three characters first would then have
/// its own inserted `&` re-escaped by the `&` pass, corrupting the
/// entity.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
.empty {
  color: #555;
  font-style: italic;
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

/// Renders one scenario's evidence block. Task 8 replaces this body
/// with the scenario's frames, prompts, replies, and findings; for
/// this task's stub `ScenarioReport` it renders only the escaped
/// scenario name.
fn render_scenario(scenario: &ScenarioReport) -> String {
    format!(
        "<section class=\"scenario\">\n<h2>{}</h2>\n</section>\n",
        html_escape(&scenario.scenario)
    )
}

/// Renders `run` as a complete, self-contained HTML document: no
/// `<link>`, no `http://`/`https://` reference, no relative image
/// path — every image Task 8 embeds arrives pre-encoded as a `data:`
/// URI, and every other resource (styles, fonts) is inlined here.
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
{body}\
</main>\n\
</body>\n\
</html>\n",
        run_id = html_escape(&run.run_id),
        style = STYLE,
        version = html_escape(&version),
        scenario_count = run.scenarios.len(),
        body = body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rendered_report_references_no_external_resource() {
        let html = render_report(&RunReport {
            run_id: "r".into(),
            contract_version: Some(1),
            scenarios: vec![],
        });
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
        let html = render_report(&RunReport {
            run_id: "r".into(),
            contract_version: None,
            scenarios: vec![ScenarioReport {
                scenario: "<script>alert(1)</script> & friends".into(),
            }],
        });
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "raw script tag must not reach the document: {html}"
        );
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; friends"));
    }

    #[test]
    fn a_missing_contract_version_renders_as_unknown_not_blank() {
        let html = render_report(&RunReport {
            run_id: "r".into(),
            contract_version: None,
            scenarios: vec![],
        });
        assert!(html.contains("unknown"));
    }

    #[test]
    fn an_empty_run_still_reports_zero_scenarios_explicitly() {
        let html = render_report(&RunReport {
            run_id: "r".into(),
            contract_version: Some(3),
            scenarios: vec![],
        });
        assert!(html.contains("No scenarios recorded for this run."));
    }
}
