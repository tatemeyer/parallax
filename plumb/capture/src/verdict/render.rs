//! Renders a [`VerdictInput`] into `verdict.md`'s Markdown text: the
//! header verdict word, the per-scenario/per-lens poll, capture
//! failures, findings, and the accounting lines that name everything
//! that could not be checked. Kept apart from `mod.rs` so aggregation
//! logic and presentation formatting stay independently readable.

use super::{verdict_for, LensOutcome, LensReport, Verdict, VerdictInput};
use crate::finding::{Confidence, Lens, Severity};
use crate::prompt::Skip;

/// The exact word rendered in the header: `GO` / `NO-GO` / `HOLD`.
fn verdict_word(v: Verdict) -> &'static str {
    match v {
        Verdict::Go => "GO",
        Verdict::NoGo => "NO-GO",
        Verdict::Hold => "HOLD",
    }
}

/// Lowercase lens name, matching `finding::Lens`'s serde spelling.
/// `Lens` has no `Display` impl (only a `serde` rename), so this is
/// this module's own presentation mapping.
fn lens_name(lens: Lens) -> &'static str {
    match lens {
        Lens::Breakage => "breakage",
        Lens::Intent => "intent",
        Lens::Design => "design",
        Lens::Motion => "motion",
    }
}

/// A fixed display order for the poll table: breakage, intent, design,
/// motion. Keeps `verdict.md` byte-stable across runs regardless of
/// subagent completion order — the same determinism concern Task 11
/// already fixed for `merge`'s output.
fn lens_rank(lens: Lens) -> u8 {
    match lens {
        Lens::Breakage => 0,
        Lens::Intent => 1,
        Lens::Design => 2,
        Lens::Motion => 3,
    }
}

fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Blocker => "blocker",
        Severity::Major => "major",
        Severity::Minor => "minor",
        Severity::Nit => "nit",
    }
}

fn confidence_name(c: Confidence) -> &'static str {
    match c {
        Confidence::High => "high",
        Confidence::Medium => "medium",
        Confidence::Low => "low",
    }
}

/// `Skip`'s rendered reason, exactly as the brief specifies: "no intent
/// declared", "no taste.md", "single-frame capture".
fn skip_reason(s: Skip) -> &'static str {
    match s {
        Skip::NoIntentDeclared => "no intent declared",
        Skip::NoTasteProfile => "no taste.md",
        Skip::SingleFrame => "single-frame capture",
    }
}

fn poll_line(r: &LensReport) -> String {
    match &r.outcome {
        LensOutcome::Reported => format!("- {} / {}: reported", r.scenario, lens_name(r.lens)),
        LensOutcome::Skipped(s) => format!(
            "- {} / {}: skipped — {}",
            r.scenario,
            lens_name(r.lens),
            skip_reason(*s)
        ),
        LensOutcome::Held(why) => {
            format!("- {} / {}: HOLD — {}", r.scenario, lens_name(r.lens), why)
        }
    }
}

/// Renders the full `verdict.md` text. Order: a header line carrying
/// the run id and the overall verdict in the exact words `GO` /
/// `NO-GO` / `HOLD`; the per-scenario, per-lens poll table; capture
/// failures with their adapter errors; findings sorted most-severe-
/// first (order preserved exactly as `merge` produced it — Task 11
/// already made that deterministic); then the accounting lines.
pub fn render_verdict(input: &VerdictInput) -> String {
    let verdict = verdict_for(input);
    let mut out = String::new();

    out.push_str(&format!(
        "# Plumb verdict: {} (run {})\n\n",
        verdict_word(verdict),
        input.run_id
    ));

    out.push_str("## Lens poll\n");
    let mut reports: Vec<&LensReport> = input.reports.iter().collect();
    reports.sort_by(|a, b| {
        a.scenario
            .cmp(&b.scenario)
            .then_with(|| lens_rank(a.lens).cmp(&lens_rank(b.lens)))
    });
    if reports.is_empty() {
        out.push_str("(no lens reported)\n");
    } else {
        for r in reports {
            out.push_str(&poll_line(r));
            out.push('\n');
        }
    }
    out.push('\n');

    if !input.capture_failures.is_empty() {
        out.push_str("## Capture failures\n");
        let mut failures = input.capture_failures.clone();
        failures.sort();
        for (scenario, err) in failures {
            out.push_str(&format!("- {scenario}: HOLD — {err}\n"));
        }
        out.push('\n');
    }

    out.push_str(&format!("## Findings ({})\n", input.findings.len()));
    if input.findings.is_empty() {
        out.push_str("(none)\n");
    } else {
        for m in &input.findings {
            let f = &m.finding;
            out.push_str(&format!(
                "- [{}] {} / {} — {}\n",
                severity_name(f.severity).to_uppercase(),
                lens_name(f.lens),
                f.region,
                f.claim
            ));
            out.push_str(&format!("  scenario: {}\n", f.scenario));
            out.push_str(&format!("  evidence: {}\n", f.evidence));
            out.push_str(&format!(
                "  confidence: {}\n",
                confidence_name(f.confidence)
            ));
            if !m.also_raised_by.is_empty() {
                let names: Vec<&str> = m.also_raised_by.iter().map(|l| lens_name(*l)).collect();
                out.push_str(&format!("  also raised by: {}\n", names.join(", ")));
            }
        }
    }
    out.push('\n');

    out.push_str("## Accounting\n");
    out.push_str(&format!(
        "previously overruled ({})\n",
        input.suppressed.len()
    ));
    out.push_str(&format!(
        "{} finding(s) dropped for naming no region\n",
        input.dropped_no_region
    ));
    let deferred = if input.deferred.is_empty() {
        "none".to_string()
    } else {
        input.deferred.join(", ")
    };
    out.push_str(&format!("deferred to a later batch: {deferred}\n"));
    let stale = if input.stale_rulings.is_empty() {
        "none".to_string()
    } else {
        input.stale_rulings.join(", ")
    };
    out.push_str(&format!("stale ruling(s) needing re-validation: {stale}\n"));

    out
}
