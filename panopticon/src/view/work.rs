//! The work pane: what is in flight, and what each item's labels
//! project onto.

use parallax_baseline::adapters::work::{WorkKind, WorkState};
use parallax_baseline::autonomy::{Autonomy, Implement, Merge, Readiness};
use parallax_baseline::state::ProjectState;

/// One row of the work pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkRow {
    /// The item's number in its repository.
    pub number: u64,
    /// `#` for an issue, `>` for a pull request.
    pub kind: char,
    /// Where it stands.
    pub state: &'static str,
    /// Who may implement it, or `—` when nothing claims.
    pub implement: &'static str,
    /// What it takes to land, or `—`.
    pub merge: &'static str,
    /// Whether "done" is defined.
    pub readiness: &'static str,
    /// Its checks, as `3/0/1` — passed, failed, pending — or blank when
    /// nothing reported any.
    pub checks: String,
    /// Its title.
    pub title: String,
}

/// Every in-flight work item, in the order the source returned them.
///
/// Closed and merged items are dropped: the pane answers "what is in
/// flight", and a repository's history is overwhelmingly finished work.
pub fn work_rows(project: &ProjectState) -> Vec<WorkRow> {
    let Some(work) = &project.work else {
        return Vec::new();
    };
    work.value
        .items
        .iter()
        .filter(|i| matches!(i.state, WorkState::Open | WorkState::Draft))
        .map(|item| {
            let autonomy = project
                .autonomy
                .iter()
                .find(|a| a.number == item.number)
                .map(|a| a.resolution.autonomy)
                .unwrap_or_default();
            WorkRow {
                number: item.number,
                kind: match item.kind {
                    WorkKind::Issue => '#',
                    WorkKind::PullRequest => '>',
                },
                state: match item.state {
                    WorkState::Open => "open",
                    WorkState::Draft => "draft",
                    WorkState::Closed => "closed",
                    WorkState::Merged => "merged",
                },
                implement: implement_of(&autonomy),
                merge: merge_of(&autonomy),
                readiness: readiness_of(&autonomy),
                checks: checks_of(item.checks),
                title: item.title.clone(),
            }
        })
        .collect()
}

/// An axis with no claim renders as an em dash, never as a default.
/// "Nothing said" and "said agent" are different facts.
fn implement_of(a: &Autonomy) -> &'static str {
    match a.implement {
        Some(Implement::Agent) => "agent",
        Some(Implement::HumanOnly) => "human-only",
        None => "—",
    }
}

fn merge_of(a: &Autonomy) -> &'static str {
    match a.merge {
        Some(Merge::DirectPush) => "direct-push",
        Some(Merge::OnChecks) => "on-checks",
        Some(Merge::HumanApproval) => "human-approval",
        None => "—",
    }
}

fn readiness_of(a: &Autonomy) -> &'static str {
    match a.readiness {
        Readiness::Verifiable => "verifiable",
        Readiness::NeedsIntent => "needs-intent",
    }
}

fn checks_of(c: parallax_baseline::adapters::work::ChecksSummary) -> String {
    if c.total() == 0 {
        String::new()
    } else {
        format!("{}/{}/{}", c.passed, c.failed, c.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::test_support::*;
    use parallax_baseline::adapters::work::ChecksSummary;
    use parallax_baseline::autonomy::{
        resolve, AutonomyEntry, AutonomyMap, Implement as I, Merge as M,
    };
    use parallax_baseline::state::ItemAutonomy;
    use std::collections::BTreeMap;

    fn ttui_map() -> AutonomyMap {
        let mut m = BTreeMap::new();
        m.insert(
            "gated".to_string(),
            AutonomyEntry {
                implement: Some(I::Agent),
                merge: Some(M::OnChecks),
                readiness: None,
            },
        );
        AutonomyMap::new(m)
    }

    /// Builds a project whose work feed holds `items`, with each item's
    /// labels projected the way aggregation projects them.
    fn with_work(items: Vec<parallax_baseline::adapters::work::WorkItem>) -> ProjectState {
        let map = ttui_map();
        project_with(|p| {
            for item in &items {
                p.autonomy.push(ItemAutonomy {
                    number: item.number,
                    resolution: resolve(&map, &item.labels),
                });
            }
            p.work = Some(polled(work_snapshot(&items), at(0)));
        })
    }

    #[test]
    fn a_project_with_no_work_feed_has_no_rows() {
        assert!(work_rows(&bare_project("p")).is_empty());
    }

    #[test]
    fn a_mapped_label_renders_its_projection() {
        let rows = work_rows(&with_work(vec![issue(1, "do the thing", &["gated"])]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].implement, "agent");
        assert_eq!(rows[0].merge, "on-checks");
        assert_eq!(rows[0].readiness, "verifiable");
    }

    /// The case every TTUI issue is actually in today (see Parallax
    /// issue #16): labels the manifest never declared. Nothing claims,
    /// so nothing is shown — rather than a default that reads as a claim.
    #[test]
    fn an_item_whose_labels_are_all_unmapped_renders_dashes() {
        let rows = work_rows(&with_work(vec![issue(
            2,
            "retire demo.rs",
            &["semver:patch"],
        )]));
        assert_eq!(rows[0].implement, "—");
        assert_eq!(rows[0].merge, "—");
        assert_eq!(
            rows[0].readiness, "verifiable",
            "readiness always lands somewhere"
        );
    }

    #[test]
    fn finished_work_is_not_in_flight() {
        let mut merged = pull(3, "already landed", ChecksSummary::none());
        merged.state = parallax_baseline::adapters::work::WorkState::Merged;
        let rows = work_rows(&with_work(vec![issue(1, "open", &[]), merged]));
        assert_eq!(rows.len(), 1, "the merged pull is not in flight");
        assert_eq!(rows[0].number, 1);
    }

    #[test]
    fn checks_render_as_counts_and_blank_when_nothing_reported() {
        let rows = work_rows(&with_work(vec![
            pull(
                4,
                "has checks",
                ChecksSummary {
                    passed: 3,
                    failed: 0,
                    pending: 1,
                },
            ),
            issue(5, "has none", &[]),
        ]));
        assert_eq!(rows[0].checks, "3/0/1");
        assert_eq!(
            rows[1].checks, "",
            "an issue has no checks, and says nothing"
        );
    }

    #[test]
    fn issues_and_pull_requests_are_distinguishable_at_a_glance() {
        let rows = work_rows(&with_work(vec![
            issue(1, "i", &[]),
            pull(2, "p", ChecksSummary::none()),
        ]));
        assert_eq!(rows[0].kind, '#');
        assert_eq!(rows[1].kind, '>');
    }
}
