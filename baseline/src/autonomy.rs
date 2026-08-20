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
mod projection_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn entry(
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
    fn ttui_map() -> AutonomyMap {
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
    fn me_map() -> AutonomyMap {
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
