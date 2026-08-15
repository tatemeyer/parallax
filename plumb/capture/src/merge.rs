//! Collapses duplicate findings raised by more than one lens into a
//! single merged finding, keyed by a stable fingerprint. Deliberately
//! excludes the lens from the fingerprint: including it would let the
//! same observation, raised again by a second lens, dodge a ruling
//! already made against the first.

use sha2::{Digest, Sha256};

use crate::finding::{Finding, Lens};

/// Lowercases, replaces every non-alphanumeric character with a space,
/// and collapses runs of whitespace. Used on both the region and the
/// claim before hashing, so trivial wording differences between lenses
/// don't produce different fingerprints.
pub fn normalize_claim(claim: &str) -> String {
    let lowered = claim.to_lowercase();
    let spaced: String = lowered
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The stable identity a ruling suppresses against: the first 16 hex
/// characters of `sha256(scenario \n normalized_region \n
/// normalized_claim)`. Deliberately excludes the lens — otherwise the
/// same observation raised by a second lens would evade a ruling
/// already made against the first.
pub fn fingerprint(scenario: &str, region: &str, claim: &str) -> String {
    let normalized_region = normalize_claim(region);
    let normalized_claim = normalize_claim(claim);
    let input = format!("{scenario}\n{normalized_region}\n{normalized_claim}");

    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();

    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// One surviving finding after dedup, with every other lens that
/// raised the same fingerprint. Nothing is discarded: a duplicate's
/// only trace of having been dropped as a standalone finding is that
/// its lens now appears here instead of owning its own `MergedFinding`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedFinding {
    /// The most severe of the duplicate reports; ties go to a
    /// blocker-capable lens so a suppressible advisory report never
    /// masks a blocker-capable one.
    pub finding: Finding,
    /// Every other lens that raised the same fingerprint, in the order
    /// they appeared in the input.
    pub also_raised_by: Vec<Lens>,
    /// The fingerprint every member of this group shares.
    pub fingerprint: String,
}

/// Groups findings by fingerprint, collapsing duplicates raised by more
/// than one lens into a single [`MergedFinding`] that keeps the most
/// severe report. Output is sorted most severe first (ties broken by
/// scenario, then region, then claim) for a stable, deterministic order
/// that never depends on input/subagent completion order.
pub fn merge(findings: Vec<Finding>) -> Vec<MergedFinding> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<Finding>> =
        std::collections::HashMap::new();

    for finding in findings {
        let fp = fingerprint(&finding.scenario, &finding.region, &finding.claim);
        groups.entry(fp.clone()).or_insert_with(|| {
            order.push(fp.clone());
            Vec::new()
        });
        groups.get_mut(&fp).unwrap().push(finding);
    }

    let mut merged: Vec<MergedFinding> = order
        .into_iter()
        .map(|fp| {
            let mut members = groups.remove(&fp).expect("grouped by this fingerprint");

            // Most severe wins; ties go to a blocker-capable lens; further
            // ties broken by lens name so the choice is deterministic.
            let rep_idx = members
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| {
                    a.severity
                        .cmp(&b.severity)
                        .then_with(|| {
                            a.lens
                                .is_blocker_capable()
                                .cmp(&b.lens.is_blocker_capable())
                        })
                        .then_with(|| a.lens.agent_name().cmp(b.lens.agent_name()))
                })
                .map(|(i, _)| i)
                .expect("non-empty group");

            let representative = members.remove(rep_idx);
            let also_raised_by: Vec<Lens> = members.iter().map(|m| m.lens).collect();

            MergedFinding {
                finding: representative,
                also_raised_by,
                fingerprint: fp,
            }
        })
        .collect();

    merged.sort_by(|a, b| {
        b.finding
            .severity
            .cmp(&a.finding.severity)
            .then_with(|| a.finding.scenario.cmp(&b.finding.scenario))
            .then_with(|| a.finding.region.cmp(&b.finding.region))
            .then_with(|| a.finding.claim.cmp(&b.finding.claim))
    });

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Confidence, Finding, Lens, Severity};

    fn f(lens: Lens, sev: Severity, region: &str, claim: &str) -> Finding {
        Finding {
            lens,
            scenario: "dial".into(),
            severity: sev,
            region: region.into(),
            claim: claim.into(),
            evidence: "e".into(),
            confidence: Confidence::High,
        }
    }

    #[test]
    fn normalization_ignores_case_punctuation_and_spacing() {
        assert_eq!(
            normalize_claim("The  border does NOT close."),
            normalize_claim("the border does not close")
        );
    }

    #[test]
    fn a_fingerprint_is_sixteen_stable_hex_characters() {
        let a = fingerprint("dial", "upper right", "the border does not close");
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            a,
            fingerprint("dial", "upper right", "The border does not close!")
        );
    }

    #[test]
    fn different_scenarios_fingerprint_differently() {
        assert_ne!(fingerprint("a", "r", "c"), fingerprint("b", "r", "c"));
    }

    #[test]
    fn two_lenses_raising_the_same_thing_merge_into_one_finding() {
        let merged = merge(vec![
            f(
                Lens::Breakage,
                Severity::Major,
                "upper right",
                "the border does not close",
            ),
            f(
                Lens::Design,
                Severity::Minor,
                "upper right",
                "The border does not close.",
            ),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].also_raised_by, vec![Lens::Design]);
    }

    #[test]
    fn a_merged_finding_keeps_the_most_severe_report() {
        let merged = merge(vec![
            f(Lens::Design, Severity::Minor, "r", "c"),
            f(Lens::Breakage, Severity::Blocker, "r", "c"),
        ]);
        assert_eq!(merged[0].finding.severity, Severity::Blocker);
        assert_eq!(
            merged[0].finding.lens,
            Lens::Breakage,
            "the severest report owns it"
        );
    }

    #[test]
    fn distinct_regions_do_not_merge() {
        let merged = merge(vec![
            f(Lens::Breakage, Severity::Major, "upper right", "c"),
            f(Lens::Breakage, Severity::Major, "lower left", "c"),
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn output_is_sorted_most_severe_first() {
        let merged = merge(vec![
            f(Lens::Breakage, Severity::Nit, "a", "x"),
            f(Lens::Breakage, Severity::Blocker, "b", "y"),
            f(Lens::Breakage, Severity::Minor, "c", "z"),
        ]);
        let sevs: Vec<_> = merged.iter().map(|m| m.finding.severity).collect();
        assert_eq!(
            sevs,
            vec![Severity::Blocker, Severity::Minor, Severity::Nit]
        );
    }

    #[test]
    fn merging_an_empty_list_yields_an_empty_list() {
        assert!(merge(Vec::new()).is_empty());
    }

    #[test]
    fn ties_on_severity_scenario_and_region_break_by_claim() {
        // Fed in descending claim order; a sort key that stops at region
        // would leave a stable sort's pre-sort (input) order untouched.
        let merged = merge(vec![
            f(Lens::Breakage, Severity::Major, "r", "the second claim"),
            f(Lens::Breakage, Severity::Major, "r", "the first claim"),
        ]);
        let claims: Vec<_> = merged.iter().map(|m| m.finding.claim.clone()).collect();
        assert_eq!(claims, vec!["the first claim", "the second claim"]);
    }

    #[test]
    fn ties_on_severity_and_blocker_class_break_by_lens_agent_name() {
        // Intent is listed first here. Iterator::max_by returns the last
        // of equally-maximum elements, so if arbitration relied on that
        // instead of comparing agent_name explicitly, this input order
        // would incorrectly pick Breakage (last in the vec) as the
        // representative instead of Intent ("critic-intent" > "critic-breakage").
        let merged = merge(vec![
            f(Lens::Intent, Severity::Blocker, "r", "c"),
            f(Lens::Breakage, Severity::Blocker, "r", "c"),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].finding.lens,
            Lens::Intent,
            "critic-intent sorts after critic-breakage"
        );
        assert_eq!(merged[0].also_raised_by, vec![Lens::Breakage]);
    }
}
