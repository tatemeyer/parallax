//! The freshness footer.
//!
//! This is the pane that makes the rest of the screen trustworthy, so
//! it renders `ProjectState::sources(now)` verbatim and adds only
//! formatting. A degraded source keeps its reason — truncated when it
//! must be, never dropped.

use parallax_baseline::freshness::Freshness;
use parallax_baseline::state::ProjectState;
use std::time::SystemTime;

/// One source's label and how current it is, formatted for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCell {
    /// The source's stable label, e.g. `verification:lint`.
    pub label: String,
    /// Its age: `live`, `12s`, `45s !`, or why it could not be read.
    pub age: String,
    /// Whether a reader should be looking at this one.
    pub alarming: bool,
}

/// Every declared source, in the order `parallax-baseline` reports it.
pub fn footer(project: &ProjectState, now: SystemTime) -> Vec<SourceCell> {
    project
        .sources(now)
        .into_iter()
        .map(|source| {
            let (age, alarming) = match &source.freshness {
                Freshness::Live => ("live".to_string(), false),
                Freshness::Fresh { age } => (format!("{}s", age.as_secs()), false),
                Freshness::Stale { age, .. } => (format!("{}s !", age.as_secs()), true),
                // The reason is the whole value of the row. A cockpit
                // that renders "unavailable" and drops the why has taken
                // a fact and returned a shrug.
                Freshness::Unavailable { reason, .. } => (reason.clone(), true),
            };
            SourceCell {
                label: source.label,
                age,
                alarming,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::test_support::*;
    use parallax_baseline::adapters::verification::VerificationOutcome;

    #[test]
    fn a_project_with_no_sources_has_an_empty_footer_rather_than_a_fabricated_row() {
        assert!(footer(&bare_project("p"), at(0)).is_empty());
    }

    #[test]
    fn a_filesystem_backed_source_reads_live() {
        let p = project_with(|p| {
            p.verification.push(watched(
                check("perceptual", VerificationOutcome::Pass),
                at(0),
            ));
        });
        let cells = footer(&p, at(9999));
        assert_eq!(cells[0].label, "verification:perceptual");
        assert_eq!(cells[0].age, "live");
        assert!(!cells[0].alarming);
    }

    #[test]
    fn a_polled_source_inside_its_interval_reads_its_age_in_seconds() {
        let p = project_with(|p| p.work = Some(polled(work_snapshot(&[]), at(0))));
        let cells = footer(&p, at(12));
        assert_eq!(cells[0].label, "work");
        assert_eq!(cells[0].age, "12s");
        assert!(!cells[0].alarming);
    }

    #[test]
    fn a_stale_source_is_marked() {
        let p = project_with(|p| p.work = Some(polled(work_snapshot(&[]), at(0))));
        let cells = footer(&p, at(45));
        assert_eq!(cells[0].age, "45s !");
        assert!(cells[0].alarming);
    }

    /// The property the spec names twice: a degraded source appears with
    /// its reason, never as an empty pane.
    #[test]
    fn an_unavailable_source_carries_its_reason_and_is_alarming() {
        let p = project_with(|p| {
            p.degradations.push(degradation(
                "work:github",
                "http 403: API rate limit exceeded",
            ));
        });
        let cells = footer(&p, at(0));
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].label, "work:github");
        assert!(cells[0].age.contains("rate limit"), "got {}", cells[0].age);
        assert!(cells[0].alarming);
    }

    /// A degraded source is listed *alongside* the ones that worked, not
    /// instead of them.
    #[test]
    fn a_degraded_source_does_not_displace_the_ones_that_reported() {
        let p = project_with(|p| {
            p.verification
                .push(watched(check("tests", VerificationOutcome::Pass), at(0)));
            p.degradations
                .push(degradation("work:github", "http 403: rate limited"));
        });
        let labels: Vec<String> = footer(&p, at(0)).into_iter().map(|c| c.label).collect();
        assert_eq!(labels, vec!["verification:tests", "work:github"]);
    }
}
