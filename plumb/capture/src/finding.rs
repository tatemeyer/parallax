//! The finding contract every lens agent reports against, plus the two
//! rules the orchestrator enforces on the way in: a finding that cannot
//! name where on screen it lives is dropped, and an advisory lens's
//! severity is clamped to its ceiling regardless of what it claimed.

use serde::{Deserialize, Serialize};

/// One of the four review lenses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lens {
    /// Rendering corruption, clipping, overlap, misalignment.
    Breakage,
    /// Conformance to the scenario's declared intent.
    Intent,
    /// Conformance to the project's taste profile.
    Design,
    /// Pacing, continuity, and readability across frames.
    Motion,
}

impl Lens {
    /// The agent definition file's `name` this lens dispatches to.
    pub fn agent_name(self) -> &'static str {
        match self {
            Lens::Breakage => "critic-breakage",
            Lens::Intent => "critic-intent",
            Lens::Design => "critic-design",
            Lens::Motion => "critic-motion",
        }
    }

    /// The most severe finding this lens is permitted to report.
    pub fn max_severity(self) -> Severity {
        match self {
            Lens::Breakage | Lens::Intent => Severity::Blocker,
            Lens::Design | Lens::Motion => Severity::Major,
        }
    }

    /// Whether an unresolved finding from this lens can hold the run.
    pub fn is_blocker_capable(self) -> bool {
        self.max_severity() == Severity::Blocker
    }
}

/// How bad a finding is. Ordered: `Blocker` is the most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Least severe.
    Nit,
    /// Small, worth knowing.
    Minor,
    /// Substantial, not run-holding.
    Major,
    /// Holds the run.
    Blocker,
}

/// How sure the lens is. Governs voice, not weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Assert it.
    High,
    /// State it plainly.
    Medium,
    /// Phrase it as a question.
    Low,
}

/// One reported observation about one scenario's capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Which lens reported it.
    pub lens: Lens,
    /// Which scenario it is about.
    pub scenario: String,
    /// How bad it is, after clamping.
    pub severity: Severity,
    /// Where on screen it lives. Mandatory and load-bearing.
    pub region: String,
    /// One sentence: what is wrong.
    pub claim: String,
    /// What in the image supports the claim.
    pub evidence: String,
    /// How sure the lens is.
    pub confidence: Confidence,
}

/// The result of ingesting one lens's report, with what was discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFindings {
    /// Findings that survived enforcement.
    pub kept: Vec<Finding>,
    /// How many were dropped for naming no region.
    pub dropped_no_region: usize,
    /// How many had their severity clamped to the lens's ceiling.
    pub clamped: usize,
}

/// A lens report that could not be read as the finding schema.
#[derive(Debug)]
pub struct FindingParseError(pub String);

impl std::fmt::Display for FindingParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lens report was not a JSON finding array: {}", self.0)
    }
}
impl std::error::Error for FindingParseError {}

/// Extracts the outermost `[...]` from text a model may have padded
/// with prose or a fenced code block. One recovery attempt only.
fn extract_array(text: &str) -> Option<&str> {
    let start = text.find('[')?;
    let end = text.rfind(']')?;
    if end > start {
        Some(&text[start..=end])
    } else {
        None
    }
}

/// Parses one lens's report, enforcing the mandatory `region` and the
/// lens's severity ceiling, and forcing `lens`/`scenario` to what was
/// actually dispatched rather than what the agent claimed.
pub fn parse_findings(
    lens: Lens,
    scenario: &str,
    json: &str,
) -> Result<ParsedFindings, FindingParseError> {
    let array =
        extract_array(json).ok_or_else(|| FindingParseError(json.chars().take(200).collect()))?;
    let raw: Vec<Finding> =
        serde_json::from_str(array).map_err(|e| FindingParseError(e.to_string()))?;

    let ceiling = lens.max_severity();
    let mut kept = Vec::new();
    let mut dropped_no_region = 0;
    let mut clamped = 0;
    for mut f in raw {
        if f.region.trim().is_empty() {
            dropped_no_region += 1;
            continue;
        }
        f.lens = lens;
        f.scenario = scenario.to_string();
        if f.severity > ceiling {
            f.severity = ceiling;
            clamped += 1;
        }
        kept.push(f);
    }
    Ok(ParsedFindings {
        kept,
        dropped_no_region,
        clamped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"[{"lens":"breakage","scenario":"dial","severity":"blocker",
      "region":"upper-right quadrant","claim":"the border does not close",
      "evidence":"the top-right corner glyph is a space","confidence":"high"}]"#;

    #[test]
    fn parses_a_well_formed_finding() {
        let p = parse_findings(Lens::Breakage, "dial", ONE).unwrap();
        assert_eq!(p.kept.len(), 1);
        assert_eq!(p.kept[0].severity, Severity::Blocker);
        assert_eq!(p.kept[0].confidence, Confidence::High);
        assert_eq!(p.dropped_no_region, 0);
    }

    #[test]
    fn an_empty_array_is_a_legitimate_result_not_an_error() {
        // No quota: finding nothing is expected and must never be an error.
        let p = parse_findings(Lens::Breakage, "dial", "[]").unwrap();
        assert!(p.kept.is_empty());
    }

    #[test]
    fn a_finding_with_no_region_is_dropped_and_counted() {
        let json = r#"[{"lens":"design","scenario":"dial","severity":"minor","region":"",
          "claim":"the layout feels unbalanced","evidence":"vibes","confidence":"low"}]"#;
        let p = parse_findings(Lens::Design, "dial", json).unwrap();
        assert!(p.kept.is_empty());
        assert_eq!(p.dropped_no_region, 1);
    }

    #[test]
    fn a_whitespace_only_region_is_also_dropped() {
        let json = ONE.replace("upper-right quadrant", "   ");
        let p = parse_findings(Lens::Breakage, "dial", &json).unwrap();
        assert_eq!(p.dropped_no_region, 1);
    }

    #[test]
    fn an_advisory_lens_cannot_emit_a_blocker() {
        // design/motion are capped at major, whatever they claim.
        let json = ONE.replace("breakage", "design");
        let p = parse_findings(Lens::Design, "dial", &json).unwrap();
        assert_eq!(p.kept[0].severity, Severity::Major);
        assert_eq!(p.clamped, 1);
    }

    #[test]
    fn a_blocker_capable_lens_keeps_its_blocker() {
        let p = parse_findings(Lens::Intent, "dial", &ONE.replace("breakage", "intent")).unwrap();
        assert_eq!(p.kept[0].severity, Severity::Blocker);
        assert_eq!(p.clamped, 0);
    }

    #[test]
    fn the_scenario_is_forced_to_the_one_actually_dispatched() {
        // An agent that mislabels its scenario must not corrupt the merge.
        let p = parse_findings(
            Lens::Breakage,
            "actual",
            &ONE.replace("dial", "hallucinated"),
        )
        .unwrap();
        assert_eq!(p.kept[0].scenario, "actual");
    }

    #[test]
    fn unparseable_output_is_an_error_the_caller_can_retry_on() {
        assert!(parse_findings(Lens::Breakage, "dial", "I looked at it and it's fine!").is_err());
    }

    #[test]
    fn prose_wrapped_around_a_json_array_is_recovered() {
        // Models pad. One recovery attempt is cheaper than a HOLD.
        let padded = format!("Here is my report:\n```json\n{ONE}\n```\n");
        assert_eq!(
            parse_findings(Lens::Breakage, "dial", &padded)
                .unwrap()
                .kept
                .len(),
            1
        );
    }

    #[test]
    fn severity_orders_blocker_above_nit() {
        assert!(Severity::Blocker > Severity::Major);
        assert!(Severity::Major > Severity::Minor);
        assert!(Severity::Minor > Severity::Nit);
    }

    #[test]
    fn only_breakage_and_intent_are_blocker_capable() {
        assert!(Lens::Breakage.is_blocker_capable());
        assert!(Lens::Intent.is_blocker_capable());
        assert!(!Lens::Design.is_blocker_capable());
        assert!(!Lens::Motion.is_blocker_capable());
    }
}
