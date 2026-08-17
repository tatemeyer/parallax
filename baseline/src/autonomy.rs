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
