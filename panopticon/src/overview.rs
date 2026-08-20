//! The Overview screen: one row per registered project. Rendering is a
//! pure function of `&PlatformState`, so it tests headlessly, exactly
//! as Baseline's adapters test without a network -- `run` is the only
//! code in this crate that touches a terminal.

use parallax_baseline::adapters::verification::VerificationOutcome;
use parallax_baseline::autonomy::Merge;
use parallax_baseline::freshness::Freshness;
use parallax_baseline::state::{PlatformState, ProjectState};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};
use ttui::buffer::Buffer;
use ttui::layout::{Constraint, Rect};
use ttui::widgets::table::Table;

const HEADERS: [&str; 7] = [
    "PROJECT",
    "WORK",
    "CHECKS",
    "ARTIFACTS",
    "SESSIONS",
    "AUTONOMY",
    "OLDEST SOURCE",
];

/// Six narrow fixed columns and one `Fill(1)` -- the exact shape
/// `ttui` 2.0's `Table::widths` was released for (ttui#170).
const WIDTHS: [Constraint; 7] = [
    Constraint::Fixed(14), // PROJECT
    Constraint::Fixed(6),  // WORK
    Constraint::Fixed(8),  // CHECKS
    Constraint::Fixed(11), // ARTIFACTS
    Constraint::Fixed(10), // SESSIONS
    Constraint::Fixed(12), // AUTONOMY
    Constraint::Fill(1),   // OLDEST SOURCE -- takes the rest
];

const DASH: &str = "\u{2014}"; // — : not declared
const ALERT: &str = "!"; // fetch failed

/// Whether a `Degradation` belonging to `family_prefix` (Baseline names
/// sources `"<family>:<detail>"`, e.g. `work:github`) exists for this
/// project -- what distinguishes "never declared" from "declared but
/// this cycle's fetch failed" for a family whose field is absent.
fn family_degraded(state: &ProjectState, family_prefix: &str) -> bool {
    state
        .degradations
        .iter()
        .any(|d| d.source.starts_with(family_prefix))
}

/// The hard requirement's three states, for a family represented by an
/// absent value: `—` when nothing was ever declared for it, `!` when it
/// was declared but this cycle could not read it. Collapsing the two
/// would make "no CI configured" indistinguishable from "CI is down".
fn absent_cell(state: &ProjectState, family_prefix: &str) -> String {
    if family_degraded(state, family_prefix) {
        ALERT.to_string()
    } else {
        DASH.to_string()
    }
}

fn work_cell(state: &ProjectState) -> String {
    match &state.work {
        Some(observed) => observed.value.items.len().to_string(),
        None => absent_cell(state, "work"),
    }
}

fn checks_cell(state: &ProjectState) -> String {
    if state.verification.is_empty() {
        absent_cell(state, "verification")
    } else {
        let total = state.verification.len();
        let passed = state
            .verification
            .iter()
            .filter(|o| o.value.outcome == VerificationOutcome::Pass)
            .count();
        format!("{passed}/{total}")
    }
}

fn artifacts_cell(state: &ProjectState) -> String {
    if state.artifacts.is_empty() {
        absent_cell(state, "artifact")
    } else {
        let total: usize = state.artifacts.iter().map(|o| o.value.len()).sum();
        total.to_string()
    }
}

fn sessions_cell(state: &ProjectState) -> String {
    match &state.sessions {
        Some(observed) => observed.value.len().to_string(),
        None => absent_cell(state, "session"),
    }
}

/// Abbreviates the merge axis to one letter -- the axis that decides
/// whether work can land at all, and so the one worth compressing into
/// this column's width. `n` covers "no claim": items whose matched
/// labels asserted nothing on this axis, including items that matched
/// no autonomy label at all.
fn merge_letter(merge: Option<Merge>) -> char {
    match merge {
        Some(Merge::DirectPush) => 'd',
        Some(Merge::OnChecks) => 'c',
        Some(Merge::HumanApproval) => 'h',
        None => 'n',
    }
}

/// The distribution of a project's work items across the merge axis,
/// most common first. A count is more useful than a percentage at this
/// width, and does not overstate its own precision when `n` is small.
/// `—` when there are no work items to distribute at all (work not
/// declared, or declared with none open) -- distinct from a real
/// distribution, never a bare `0` bucket standing in for "no data".
fn autonomy_cell(state: &ProjectState) -> String {
    if state.autonomy.is_empty() {
        return DASH.to_string();
    }
    let mut counts: BTreeMap<char, usize> = BTreeMap::new();
    for item in &state.autonomy {
        *counts
            .entry(merge_letter(item.resolution.autonomy.merge))
            .or_insert(0) += 1;
    }
    let mut pairs: Vec<(char, usize)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    pairs
        .iter()
        .map(|(letter, n)| format!("{n}{letter}"))
        .collect::<Vec<_>>()
        .join("/")
}

/// A compact age string: seconds/minutes/hours/days, whichever is the
/// coarsest unit that keeps the number small -- this column has no room
/// for a full duration.
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// `stalest(now)` rendered as `"<label> <age>"` -- one number that
/// answers "how much of this screen should I believe". `—` when the
/// project declares no sources at all.
fn oldest_source_cell(state: &ProjectState, now: SystemTime) -> String {
    match state.stalest(now) {
        None => DASH.to_string(),
        Some(source) => {
            let age = match source.freshness {
                Freshness::Live => "live".to_string(),
                Freshness::Fresh { age } | Freshness::Stale { age, .. } => format_duration(age),
                Freshness::Unavailable { .. } => ALERT.to_string(),
            };
            format!("{} {}", source.label, age)
        }
    }
}

fn project_row(state: &ProjectState, now: SystemTime) -> Vec<String> {
    vec![
        state.name.clone(),
        work_cell(state),
        checks_cell(state),
        artifacts_cell(state),
        sessions_cell(state),
        autonomy_cell(state),
        oldest_source_cell(state, now),
    ]
}

/// Renders the Overview screen into a fresh `width`x`height` buffer:
/// one row per registered project, in registration order, with `selected`
/// highlighted. A pure function of the platform's current state -- no
/// terminal involved, so it tests without one.
pub fn render_overview(
    platform: &PlatformState,
    selected: usize,
    now: SystemTime,
    width: u16,
    height: u16,
) -> Buffer {
    let mut buf = Buffer::new(width, height);
    let area = Rect {
        x: 0,
        y: 0,
        width,
        height,
    };
    let headers: Vec<String> = HEADERS.iter().map(|s| s.to_string()).collect();
    let rows: Vec<Vec<String>> = platform
        .projects
        .iter()
        .map(|p| project_row(p, now))
        .collect();
    Table::new(&headers, &rows, selected)
        .widths(&WIDTHS)
        .spacing(1)
        .render(area, &mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_baseline::adapters::artifact::{Artifact, ArtifactDetail};
    use parallax_baseline::adapters::session::Session;
    use parallax_baseline::adapters::verification::VerificationStatus;
    use parallax_baseline::adapters::work::{WorkItem, WorkKind, WorkSnapshot, WorkState};
    use parallax_baseline::autonomy::{Autonomy, Resolution};
    use parallax_baseline::freshness::Observed;
    use parallax_baseline::manifest::ArtifactKind;
    use parallax_baseline::state::{Degradation, ItemAutonomy};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    fn project(name: &str) -> ProjectState {
        ProjectState {
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn degraded(mut p: ProjectState, source: &str) -> ProjectState {
        p.degradations.push(Degradation {
            source: source.to_string(),
            reason: "unreachable".into(),
        });
        p
    }

    fn item(number: u64) -> WorkItem {
        WorkItem {
            number,
            title: String::new(),
            kind: WorkKind::Issue,
            state: WorkState::Open,
            labels: vec![],
            checks: Default::default(),
            url: String::new(),
            updated_at: String::new(),
        }
    }

    // --- WORK column: the three states ---

    #[test]
    fn work_not_declared_renders_a_dash() {
        assert_eq!(work_cell(&project("x")), DASH);
    }

    #[test]
    fn work_fetch_failed_renders_an_alert_mark() {
        let p = degraded(project("x"), "work:github");
        assert_eq!(work_cell(&p), ALERT);
    }

    #[test]
    fn work_fetched_empty_renders_zero() {
        let mut p = project("x");
        p.work = Some(Observed::polled(
            WorkSnapshot { items: vec![] },
            at(0),
            Duration::from_secs(30),
        ));
        assert_eq!(work_cell(&p), "0");
    }

    #[test]
    fn work_fetched_nonempty_renders_the_count() {
        let mut p = project("x");
        p.work = Some(Observed::polled(
            WorkSnapshot {
                items: vec![item(1), item(2)],
            },
            at(0),
            Duration::from_secs(30),
        ));
        assert_eq!(work_cell(&p), "2");
    }

    #[test]
    fn the_three_work_states_are_pairwise_distinct() {
        let not_declared = work_cell(&project("x"));
        let fetch_failed = work_cell(&degraded(project("x"), "work:github"));
        let mut fetched = project("x");
        fetched.work = Some(Observed::polled(
            WorkSnapshot { items: vec![] },
            at(0),
            Duration::from_secs(30),
        ));
        let fetched_empty = work_cell(&fetched);
        assert_ne!(not_declared, fetch_failed);
        assert_ne!(not_declared, fetched_empty);
        assert_ne!(fetch_failed, fetched_empty);
    }

    // --- CHECKS column: the three states ---

    #[test]
    fn checks_not_declared_renders_a_dash() {
        assert_eq!(checks_cell(&project("x")), DASH);
    }

    #[test]
    fn checks_fetch_failed_renders_an_alert_mark() {
        let p = degraded(project("x"), "verification:command:lint");
        assert_eq!(checks_cell(&p), ALERT);
    }

    #[test]
    fn checks_fetched_renders_passed_over_total() {
        let mut p = project("x");
        p.verification = vec![
            Observed::watched(
                VerificationStatus {
                    kind: "lint".into(),
                    outcome: VerificationOutcome::Pass,
                    detail: None,
                },
                at(0),
            ),
            Observed::watched(
                VerificationStatus {
                    kind: "tests".into(),
                    outcome: VerificationOutcome::Fail,
                    detail: None,
                },
                at(0),
            ),
        ];
        assert_eq!(checks_cell(&p), "1/2");
    }

    // --- ARTIFACTS column: the three states ---

    #[test]
    fn artifacts_not_declared_renders_a_dash() {
        assert_eq!(artifacts_cell(&project("x")), DASH);
    }

    #[test]
    fn artifacts_fetch_failed_renders_an_alert_mark() {
        let p = degraded(project("x"), "artifact:capture");
        assert_eq!(artifacts_cell(&p), ALERT);
    }

    #[test]
    fn artifacts_fetched_empty_renders_zero() {
        let mut p = project("x");
        p.artifacts = vec![Observed::watched(vec![], at(0))];
        assert_eq!(artifacts_cell(&p), "0");
    }

    #[test]
    fn artifacts_fetched_nonempty_renders_the_total_across_feeds() {
        let mut p = project("x");
        let a = Artifact {
            path: "x.png".into(),
            kind: ArtifactKind::Figure,
            modified: at(0),
            detail: ArtifactDetail::Figure { bytes: 1 },
        };
        p.artifacts = vec![
            Observed::watched(vec![a.clone()], at(0)),
            Observed::watched(vec![a.clone(), a], at(0)),
        ];
        assert_eq!(artifacts_cell(&p), "3");
    }

    // --- SESSIONS column: the three states ---

    #[test]
    fn sessions_not_declared_renders_a_dash() {
        assert_eq!(sessions_cell(&project("x")), DASH);
    }

    #[test]
    fn sessions_fetch_failed_renders_an_alert_mark() {
        let p = degraded(project("x"), "session:filesystem");
        assert_eq!(sessions_cell(&p), ALERT);
    }

    #[test]
    fn sessions_fetched_empty_renders_zero() {
        let mut p = project("x");
        p.sessions = Some(Observed::watched(vec![], at(0)));
        assert_eq!(sessions_cell(&p), "0");
    }

    #[test]
    fn sessions_fetched_nonempty_renders_the_count() {
        let mut p = project("x");
        p.sessions = Some(Observed::watched(
            vec![Session {
                name: "s".into(),
                path: "s".into(),
                last_activity: at(0),
            }],
            at(0),
        ));
        assert_eq!(sessions_cell(&p), "1");
    }

    // --- AUTONOMY column ---

    #[test]
    fn autonomy_with_no_work_items_renders_a_dash_not_a_claim() {
        assert_eq!(autonomy_cell(&project("x")), DASH);
    }

    #[test]
    fn autonomy_distinguishes_a_dash_from_a_real_distribution() {
        let mut p = project("x");
        p.autonomy = vec![ItemAutonomy {
            number: 1,
            resolution: Resolution {
                autonomy: Autonomy {
                    merge: Some(Merge::OnChecks),
                    ..Default::default()
                },
                ..Default::default()
            },
        }];
        assert_ne!(autonomy_cell(&p), DASH);
        assert_eq!(autonomy_cell(&p), "1c");
    }

    #[test]
    fn autonomy_groups_and_sorts_by_count_descending() {
        let mut p = project("x");
        let claim = |m: Option<Merge>| ItemAutonomy {
            number: 1,
            resolution: Resolution {
                autonomy: Autonomy {
                    merge: m,
                    ..Default::default()
                },
                ..Default::default()
            },
        };
        p.autonomy = vec![
            claim(Some(Merge::OnChecks)),
            claim(Some(Merge::OnChecks)),
            claim(Some(Merge::HumanApproval)),
            claim(None),
        ];
        assert_eq!(autonomy_cell(&p), "2c/1h/1n");
    }

    // --- OLDEST SOURCE column ---

    #[test]
    fn oldest_source_with_no_sources_renders_a_dash() {
        assert_eq!(oldest_source_cell(&project("x"), at(100)), DASH);
    }

    #[test]
    fn oldest_source_reports_the_worst_labelled_source_and_its_age() {
        let mut p = project("x");
        p.verification = vec![Observed::watched(
            VerificationStatus {
                kind: "fmt".into(),
                outcome: VerificationOutcome::Pass,
                detail: None,
            },
            at(0),
        )];
        // Watched sources are always Live, so this exercises the "live"
        // formatting branch specifically.
        assert_eq!(oldest_source_cell(&p, at(300)), "verification:fmt live");
    }

    // --- The spec's headline partial case ---

    #[test]
    fn a_manifest_declaring_only_a_project_renders_as_a_normal_row_of_dashes_not_an_error() {
        let platform = PlatformState {
            projects: vec![project("minimal")],
        };
        let buf = render_overview(&platform, 0, at(0), 100, 5);

        // Row 1 (first data row): name then six dashes/counts, none of
        // them an error marker.
        assert_eq!(buf.get(0, 1).symbol, 'm'); // "minimal"
        let work_col_x = 15; // PROJECT (14) + spacing(1)
        assert_eq!(buf.get(work_col_x, 1).symbol, '\u{2014}');
    }

    // --- The hard requirement, asserted against the actual rendered buffer ---

    #[test]
    fn the_three_states_render_distinctly_in_the_actual_buffer() {
        let not_declared = project("no-ci");
        let fetch_failed = degraded(project("ci-down"), "verification:command:lint");
        let mut fetched_empty = project("ci-clean");
        fetched_empty.verification = vec![Observed::watched(
            VerificationStatus {
                kind: "lint".into(),
                outcome: VerificationOutcome::Pass,
                detail: None,
            },
            at(0),
        )];
        let platform = PlatformState {
            projects: vec![not_declared, fetch_failed, fetched_empty],
        };

        let buf = render_overview(&platform, 0, at(0), 100, 5);

        // CHECKS column starts at PROJECT(14) + gap(1) + WORK(6) + gap(1) = 22.
        let checks_col_x = 22;
        assert_eq!(buf.get(checks_col_x, 1).symbol, '\u{2014}', "not declared");
        assert_eq!(buf.get(checks_col_x, 2).symbol, '!', "fetch failed");
        assert_eq!(buf.get(checks_col_x, 3).symbol, '1', "fetched (1/1)");
    }

    // --- Narrow terminal: Fill(1) collapsing toward zero must not panic ---

    #[test]
    fn a_narrow_terminal_does_not_panic() {
        let platform = PlatformState {
            projects: vec![project("x")],
        };
        // Narrower than the sum of the fixed columns alone.
        let buf = render_overview(&platform, 0, at(0), 10, 5);
        assert_eq!(buf.width, 10);
    }

    #[test]
    fn a_zero_width_terminal_does_not_panic() {
        let platform = PlatformState {
            projects: vec![project("x")],
        };
        let buf = render_overview(&platform, 0, at(0), 0, 5);
        assert_eq!(buf.width, 0);
    }

    #[test]
    fn a_zero_height_terminal_does_not_panic() {
        let platform = PlatformState {
            projects: vec![project("x")],
        };
        let buf = render_overview(&platform, 0, at(0), 100, 0);
        assert_eq!(buf.height, 0);
    }

    #[test]
    fn an_empty_platform_renders_only_the_header_without_panicking() {
        let platform = PlatformState { projects: vec![] };
        let buf = render_overview(&platform, 0, at(0), 100, 5);
        assert_eq!(buf.get(0, 0).symbol, 'P'); // "PROJECT" header
    }
}
