//! The normalized autonomy vocabulary: three orthogonal axes that each
//! project's native labels project onto. Deliberately not a single
//! ladder — the two consumer repos each collapse two or three
//! independent axes into one label, and separating them is what makes
//! their schemes comparable at all.

use serde::{Deserialize, Serialize};

/// Who may do the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Implement {
    /// An agent may implement this unit of work.
    Agent,
    /// Reserved from the agent; a human implements it.
    HumanOnly,
}

/// What it takes for the work to land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Merge {
    /// Straight to the default branch, bypassing review.
    DirectPush,
    /// Merges once objective checks are green; no human wait.
    OnChecks,
    /// Requires explicit human sign-off beyond green checks.
    HumanApproval,
}

/// Whether "done" is even defined for this unit of work yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Readiness {
    /// A machine-checkable success criterion exists.
    #[default]
    Verifiable,
    /// The criterion cannot be written yet; intent must be settled first.
    NeedsIntent,
}

/// A native label projected onto the three normalized axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Autonomy {
    /// Who may implement. `None` means the label makes no claim.
    pub implement: Option<Implement>,
    /// What it takes to land. `None` means the label makes no claim.
    pub merge: Option<Merge>,
    /// Whether "done" is defined. Defaults to `Verifiable`.
    pub readiness: Readiness,
}

/// An `Autonomy` asserting nothing on either optional axis.
pub fn no_claim() -> Autonomy {
    Autonomy::default()
}

use std::collections::BTreeMap;

/// One `autonomy_map` entry: what a native label claims on each axis.
/// Every field is optional — an omitted field is "no claim", not a
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyEntry {
    /// Who may implement work carrying this label.
    #[serde(default)]
    pub implement: Option<Implement>,
    /// What it takes for work carrying this label to land.
    #[serde(default)]
    pub merge: Option<Merge>,
    /// Whether "done" is defined for work carrying this label.
    #[serde(default)]
    pub readiness: Option<Readiness>,
}

/// A project's native autonomy labels and what each projects onto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AutonomyMap {
    entries: BTreeMap<String, AutonomyEntry>,
}

impl AutonomyMap {
    /// Builds a map from native label to its projection.
    pub fn new(entries: BTreeMap<String, AutonomyEntry>) -> Self {
        Self { entries }
    }

    /// The raw entry for a native label, if declared.
    pub fn entry(&self, label: &str) -> Option<&AutonomyEntry> {
        self.entries.get(label)
    }

    /// Every native label this project declares, in sorted order.
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Whether the project declares no autonomy labels at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Projects one native label onto the normalized axes. Returns `None`
/// for a label the manifest does not declare — an ordinary issue label
/// is not an autonomy statement and must not be treated as an error.
pub fn project(map: &AutonomyMap, label: &str) -> Option<Autonomy> {
    map.entry(label).map(|e| Autonomy {
        implement: e.implement,
        merge: e.merge,
        readiness: e.readiness.unwrap_or_default(),
    })
}

/// The outcome of projecting every label on one work item.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolution {
    /// The combined projection across every mapped label.
    pub autonomy: Autonomy,
    /// The labels the manifest declares, in the order given.
    pub matched: Vec<String>,
    /// Labels the manifest does not declare, in the order given.
    pub unmapped: Vec<String>,
}

/// Resolves every label on one work item into a single `Autonomy`.
///
/// Per axis: a stated claim always beats "no claim", and when two
/// labels both state a claim the more restrictive one wins. Order
/// independent by construction.
pub fn resolve(map: &AutonomyMap, labels: &[String]) -> Resolution {
    let mut out = Resolution::default();
    for label in labels {
        match project(map, label) {
            Some(a) => {
                out.matched.push(label.clone());
                out.autonomy.implement = most_restrictive(out.autonomy.implement, a.implement);
                out.autonomy.merge = most_restrictive(out.autonomy.merge, a.merge);
                out.autonomy.readiness = out.autonomy.readiness.max(a.readiness);
            }
            None => out.unmapped.push(label.clone()),
        }
    }
    out
}

/// A stated claim beats no claim; between two claims the higher
/// (more restrictive) variant wins.
fn most_restrictive<T: Ord>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axes_serialize_with_the_spec_s_kebab_case_wire_names() {
        assert_eq!(
            serde_yaml::to_string(&Implement::HumanOnly).unwrap().trim(),
            "human-only"
        );
        assert_eq!(
            serde_yaml::to_string(&Merge::OnChecks).unwrap().trim(),
            "on-checks"
        );
        assert_eq!(
            serde_yaml::to_string(&Merge::DirectPush).unwrap().trim(),
            "direct-push"
        );
        assert_eq!(
            serde_yaml::to_string(&Merge::HumanApproval).unwrap().trim(),
            "human-approval"
        );
        assert_eq!(
            serde_yaml::to_string(&Readiness::NeedsIntent)
                .unwrap()
                .trim(),
            "needs-intent"
        );
    }

    #[test]
    fn readiness_defaults_to_verifiable() {
        assert_eq!(Readiness::default(), Readiness::Verifiable);
        assert_eq!(no_claim().readiness, Readiness::Verifiable);
    }

    #[test]
    fn no_claim_asserts_nothing_on_the_two_optional_axes() {
        let a = no_claim();
        assert_eq!(a.implement, None);
        assert_eq!(a.merge, None);
    }

    /// Restrictiveness ordering is what multi-label resolution (Task 4)
    /// is built on, so it is pinned here rather than left implicit.
    #[test]
    fn the_axes_order_least_to_most_restrictive() {
        assert!(Implement::Agent < Implement::HumanOnly);
        assert!(Merge::DirectPush < Merge::OnChecks);
        assert!(Merge::OnChecks < Merge::HumanApproval);
        assert!(Readiness::Verifiable < Readiness::NeedsIntent);
    }
}

#[cfg(test)]
mod projection_tests_support {
    use super::*;
    use std::collections::BTreeMap;

    pub(super) fn entry(
        implement: Option<Implement>,
        merge: Option<Merge>,
        readiness: Option<Readiness>,
    ) -> AutonomyEntry {
        AutonomyEntry {
            implement,
            merge,
            readiness,
        }
    }

    /// TTUI's three tiers, exactly as `manifests/ttui.yaml` declares them.
    pub(super) fn ttui_map() -> AutonomyMap {
        let mut m = BTreeMap::new();
        m.insert(
            "direct".to_string(),
            entry(Some(Implement::Agent), Some(Merge::DirectPush), None),
        );
        m.insert(
            "gated".to_string(),
            entry(Some(Implement::Agent), Some(Merge::OnChecks), None),
        );
        m.insert(
            "human".to_string(),
            entry(Some(Implement::Agent), Some(Merge::HumanApproval), None),
        );
        AutonomyMap::new(m)
    }

    /// Model-Experiments' four labels, exactly as its manifest declares them.
    pub(super) fn me_map() -> AutonomyMap {
        let mut m = BTreeMap::new();
        m.insert(
            "autonomy:safe".to_string(),
            entry(Some(Implement::Agent), Some(Merge::OnChecks), None),
        );
        m.insert(
            "autonomy:review".to_string(),
            entry(Some(Implement::Agent), Some(Merge::HumanApproval), None),
        );
        m.insert(
            "autonomy:human".to_string(),
            entry(Some(Implement::HumanOnly), None, None),
        );
        m.insert(
            "needs-intent".to_string(),
            entry(None, None, Some(Readiness::NeedsIntent)),
        );
        AutonomyMap::new(m)
    }
}

#[cfg(test)]
mod projection_tests {
    use super::projection_tests_support::{me_map, ttui_map};
    use super::*;

    // --- The spec's projection table. One test per row. ---

    #[test]
    fn row_ttui_direct() {
        assert_eq!(
            project(&ttui_map(), "direct").unwrap(),
            Autonomy {
                implement: Some(Implement::Agent),
                merge: Some(Merge::DirectPush),
                readiness: Readiness::Verifiable
            }
        );
    }

    #[test]
    fn row_ttui_gated() {
        assert_eq!(
            project(&ttui_map(), "gated").unwrap(),
            Autonomy {
                implement: Some(Implement::Agent),
                merge: Some(Merge::OnChecks),
                readiness: Readiness::Verifiable
            }
        );
    }

    #[test]
    fn row_ttui_human() {
        assert_eq!(
            project(&ttui_map(), "human").unwrap(),
            Autonomy {
                implement: Some(Implement::Agent),
                merge: Some(Merge::HumanApproval),
                readiness: Readiness::Verifiable
            }
        );
    }

    #[test]
    fn row_me_autonomy_safe() {
        assert_eq!(
            project(&me_map(), "autonomy:safe").unwrap(),
            Autonomy {
                implement: Some(Implement::Agent),
                merge: Some(Merge::OnChecks),
                readiness: Readiness::Verifiable
            }
        );
    }

    #[test]
    fn row_me_autonomy_review() {
        assert_eq!(
            project(&me_map(), "autonomy:review").unwrap(),
            Autonomy {
                implement: Some(Implement::Agent),
                merge: Some(Merge::HumanApproval),
                readiness: Readiness::Verifiable
            }
        );
    }

    /// The `—` in the merge column is None: a human doing the work makes
    /// no claim about what it takes to land.
    #[test]
    fn row_me_autonomy_human() {
        assert_eq!(
            project(&me_map(), "autonomy:human").unwrap(),
            Autonomy {
                implement: Some(Implement::HumanOnly),
                merge: None,
                readiness: Readiness::Verifiable
            }
        );
    }

    /// Two `—` cells: "done" is not defined, so neither other axis is
    /// asserted.
    #[test]
    fn row_me_needs_intent() {
        assert_eq!(
            project(&me_map(), "needs-intent").unwrap(),
            Autonomy {
                implement: None,
                merge: None,
                readiness: Readiness::NeedsIntent
            }
        );
    }

    // --- The two asymmetries the shared vocabulary exists to surface ---

    #[test]
    fn model_experiments_has_no_direct_push_tier() {
        let map = me_map();
        assert!(
            map.labels()
                .all(|l| project(&map, l).unwrap().merge != Some(Merge::DirectPush)),
            "nothing in Model-Experiments bypasses CI"
        );
    }

    #[test]
    fn ttui_has_no_human_only_tier() {
        let map = ttui_map();
        assert!(
            map.labels()
                .all(|l| project(&map, l).unwrap().implement != Some(Implement::HumanOnly)),
            "no TTUI work is reserved from the agent"
        );
    }

    // --- Unmapped labels ---

    #[test]
    fn an_unmapped_label_projects_to_none_rather_than_erroring() {
        // A GitHub issue carries labels the manifest never mentions
        // ("bug", "documentation"). Those are not autonomy statements
        // and must not fail projection.
        assert_eq!(project(&ttui_map(), "bug"), None);
        assert_eq!(project(&me_map(), "good first issue"), None);
    }

    #[test]
    fn label_lookup_is_exact_and_case_sensitive() {
        assert_eq!(project(&ttui_map(), "Direct"), None);
        assert!(project(&ttui_map(), "direct").is_some());
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::projection_tests_support::{me_map, ttui_map};
    use super::*;

    fn labels(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_labels_at_all_claims_nothing() {
        let r = resolve(&ttui_map(), &[]);
        assert_eq!(r.autonomy, no_claim());
        assert!(r.matched.is_empty());
        assert!(r.unmapped.is_empty());
    }

    #[test]
    fn a_single_mapped_label_resolves_to_its_row() {
        let r = resolve(&ttui_map(), &labels(&["gated"]));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
        assert_eq!(r.matched, vec!["gated".to_string()]);
    }

    #[test]
    fn unmapped_labels_are_reported_not_dropped_and_not_fatal() {
        let r = resolve(&ttui_map(), &labels(&["bug", "gated", "documentation"]));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
        assert_eq!(r.matched, vec!["gated".to_string()]);
        assert_eq!(r.unmapped, labels(&["bug", "documentation"]));
    }

    /// The real combination: work that is agent-implementable and merges
    /// on checks, but whose success criterion is not written yet.
    #[test]
    fn needs_intent_combines_with_a_tier_rather_than_overriding_it() {
        let r = resolve(&me_map(), &labels(&["autonomy:safe", "needs-intent"]));
        assert_eq!(r.autonomy.implement, Some(Implement::Agent));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
        assert_eq!(r.autonomy.readiness, Readiness::NeedsIntent);
    }

    #[test]
    fn conflicting_labels_resolve_to_the_most_restrictive_value_per_axis() {
        let r = resolve(&me_map(), &labels(&["autonomy:safe", "autonomy:review"]));
        assert_eq!(
            r.autonomy.merge,
            Some(Merge::HumanApproval),
            "human-approval outranks on-checks"
        );

        let r = resolve(&me_map(), &labels(&["autonomy:safe", "autonomy:human"]));
        assert_eq!(
            r.autonomy.implement,
            Some(Implement::HumanOnly),
            "human-only outranks agent"
        );
    }

    /// "No claim" never beats a claim: a label that says nothing about
    /// an axis must not erase what another label said about it.
    #[test]
    fn a_label_making_no_claim_does_not_erase_another_label_s_claim() {
        let r = resolve(&me_map(), &labels(&["autonomy:safe", "needs-intent"]));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
        let r = resolve(&me_map(), &labels(&["autonomy:human", "autonomy:safe"]));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
    }

    #[test]
    fn resolution_is_order_independent() {
        let a = resolve(&me_map(), &labels(&["autonomy:review", "autonomy:safe"]));
        let b = resolve(&me_map(), &labels(&["autonomy:safe", "autonomy:review"]));
        assert_eq!(a.autonomy, b.autonomy);
    }

    #[test]
    fn matched_labels_are_reported_in_the_order_they_appeared() {
        let r = resolve(&me_map(), &labels(&["needs-intent", "autonomy:safe"]));
        assert_eq!(r.matched, labels(&["needs-intent", "autonomy:safe"]));
    }
}
