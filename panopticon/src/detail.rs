//! The Detail screen: one project's full state, opened with `Enter` and
//! closed with `Esc`. Rendering is a pure function of `&ProjectState`,
//! so it tests headlessly, exactly like `overview` -- `run` (via `App`)
//! is the only code in this crate that touches a terminal.

use crate::fmt::{family_degraded, format_duration, sanitize, DASH};
use parallax_baseline::autonomy::{Implement, Merge, Readiness};
use parallax_baseline::freshness::Freshness;
use parallax_baseline::state::{ProjectState, SourceStatus};
use std::time::SystemTime;
use ttui::buffer::Buffer;
use ttui::layout::Rect;
use ttui::widgets::text::Text;

/// Ranks a freshness for "worst first" ordering -- mirrors
/// `ProjectState::stalest`'s own ranking, duplicated here rather than
/// exposed by Baseline because it is display policy, not core state.
fn severity(freshness: &Freshness) -> u8 {
    match freshness {
        Freshness::Live => 0,
        Freshness::Fresh { .. } => 1,
        Freshness::Stale { .. } => 2,
        Freshness::Unavailable { .. } => 3,
    }
}

/// A source's freshness, spelled out in full -- the Detail screen has
/// room a table column doesn't, so a degraded source reads as the word
/// "Unavailable" plus its reason, not just a glyph.
fn freshness_text(freshness: &Freshness) -> String {
    match freshness {
        Freshness::Live => "live".to_string(),
        Freshness::Fresh { age } => format_duration(*age),
        Freshness::Stale { age, overdue } => {
            format!(
                "{} (stale, {} overdue)",
                format_duration(*age),
                format_duration(*overdue)
            )
        }
        Freshness::Unavailable { reason, .. } => format!("Unavailable: {}", sanitize(reason)),
    }
}

fn implement_text(v: Option<Implement>) -> String {
    match v {
        Some(Implement::Agent) => "agent".to_string(),
        Some(Implement::HumanOnly) => "human-only".to_string(),
        None => DASH.to_string(),
    }
}

fn merge_text(v: Option<Merge>) -> String {
    match v {
        Some(Merge::DirectPush) => "direct-push".to_string(),
        Some(Merge::OnChecks) => "on-checks".to_string(),
        Some(Merge::HumanApproval) => "human-approval".to_string(),
        None => DASH.to_string(),
    }
}

fn readiness_text(v: Readiness) -> String {
    match v {
        Readiness::Verifiable => "verifiable".to_string(),
        Readiness::NeedsIntent => "needs-intent".to_string(),
    }
}

/// Not-declared-vs-Unavailable for a family with no successful reads:
/// the same three-state distinction `overview`'s `absent_cell` makes,
/// spelled out for a section body rather than a table cell.
fn absent_line(state: &ProjectState, family_prefix: &str) -> String {
    if family_degraded(state, family_prefix) {
        "Unavailable".to_string()
    } else {
        format!("{DASH} not declared")
    }
}

fn sources_lines(state: &ProjectState, now: SystemTime) -> Vec<String> {
    let mut lines = vec!["SOURCES (worst first)".to_string()];
    let mut sources: Vec<SourceStatus> = state.sources(now);
    // Stable sort: ties keep Baseline's own reporting order.
    sources.sort_by(|a, b| severity(&b.freshness).cmp(&severity(&a.freshness)));
    if sources.is_empty() {
        lines.push(format!("  {DASH} no sources declared"));
    } else {
        for s in &sources {
            lines.push(format!("  {}: {}", s.label, freshness_text(&s.freshness)));
        }
    }
    lines
}

fn work_lines(state: &ProjectState) -> Vec<String> {
    let mut lines = vec!["WORK IN FLIGHT".to_string()];
    match &state.work {
        Some(observed) if observed.value.items.is_empty() => {
            lines.push("  0 items".to_string());
        }
        Some(observed) => {
            for item in &observed.value.items {
                let autonomy = state.autonomy.iter().find(|a| a.number == item.number);
                let (implement, merge, readiness) = match autonomy {
                    Some(a) => (
                        implement_text(a.resolution.autonomy.implement),
                        merge_text(a.resolution.autonomy.merge),
                        readiness_text(a.resolution.autonomy.readiness),
                    ),
                    None => (
                        DASH.to_string(),
                        DASH.to_string(),
                        readiness_text(Readiness::default()),
                    ),
                };
                lines.push(format!(
                    "  #{} {}  implement={implement} merge={merge} readiness={readiness}",
                    item.number,
                    sanitize(&item.title)
                ));
            }
        }
        None => lines.push(format!("  {}", absent_line(state, "work"))),
    }
    lines
}

fn verification_lines(state: &ProjectState) -> Vec<String> {
    let mut lines = vec!["VERIFICATION".to_string()];
    if state.verification.is_empty() {
        lines.push(format!("  {}", absent_line(state, "verification")));
    } else {
        for v in &state.verification {
            let detail = match &v.value.detail {
                Some(d) if !d.is_empty() => format!(" -- {}", sanitize(d)),
                _ => String::new(),
            };
            lines.push(format!("  {}: {:?}{detail}", v.value.kind, v.value.outcome));
        }
    }
    lines
}

fn artifacts_lines(state: &ProjectState) -> Vec<String> {
    let mut lines = vec!["ARTIFACTS".to_string()];
    if state.artifacts.is_empty() {
        lines.push(format!("  {}", absent_line(state, "artifact")));
    } else {
        let total: usize = state.artifacts.iter().map(|o| o.value.len()).sum();
        let most_recent = state
            .artifacts
            .iter()
            .flat_map(|o| o.value.iter())
            .max_by_key(|a| a.modified);
        match most_recent {
            Some(a) => lines.push(format!(
                "  {total} total, most recent: {}",
                sanitize(&a.path.display().to_string())
            )),
            None => lines.push(format!("  {total} total")),
        }
    }
    lines
}

fn sessions_lines(state: &ProjectState) -> Vec<String> {
    let mut lines = vec!["SESSIONS".to_string()];
    match &state.sessions {
        Some(observed) if observed.value.is_empty() => lines.push("  0".to_string()),
        Some(observed) => {
            let total = observed.value.len();
            let most_recent = observed.value.iter().max_by_key(|s| s.last_activity);
            match most_recent {
                Some(s) => lines.push(format!(
                    "  {total} total, most recent: {}",
                    sanitize(&s.name)
                )),
                None => lines.push(format!("  {total} total")),
            }
        }
        None => lines.push(format!("  {}", absent_line(state, "session"))),
    }
    lines
}

fn unmapped_lines(state: &ProjectState) -> Vec<String> {
    if state.unmapped_labels.is_empty() {
        return Vec::new();
    }
    let labels: Vec<String> = state.unmapped_labels.iter().map(|l| sanitize(l)).collect();
    vec![
        "UNMAPPED LABELS".to_string(),
        format!("  {}", labels.join(", ")),
    ]
}

/// Every line the Detail screen draws, top to bottom: title, then each
/// section from the design (Sources worst-first, Work in flight with
/// projected autonomy, Verification, Artifacts, Sessions, and Unmapped
/// labels when any). A blank line separates sections. Pure and
/// independent of any buffer size, so it tests without one.
fn detail_lines(state: &ProjectState, now: SystemTime) -> Vec<String> {
    let mut lines = vec![state.name.clone(), String::new()];
    lines.extend(sources_lines(state, now));
    lines.push(String::new());
    lines.extend(work_lines(state));
    lines.push(String::new());
    lines.extend(verification_lines(state));
    lines.push(String::new());
    lines.extend(artifacts_lines(state));
    lines.push(String::new());
    lines.extend(sessions_lines(state));
    let unmapped = unmapped_lines(state);
    if !unmapped.is_empty() {
        lines.push(String::new());
        lines.extend(unmapped);
    }
    lines
}

/// Renders the Detail screen into a fresh `width`x`height` buffer, one
/// line per row, clipped to `height`. A pure function of the project's
/// current state -- no terminal involved, so it tests without one.
pub fn render_detail(state: &ProjectState, now: SystemTime, width: u16, height: u16) -> Buffer {
    let mut buf = Buffer::new(width, height);
    for (y, line) in detail_lines(state, now)
        .iter()
        .take(height as usize)
        .enumerate()
    {
        Text::new(line).render(
            Rect {
                x: 0,
                y: y as u16,
                width,
                height: 1,
            },
            &mut buf,
        );
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_baseline::adapters::artifact::{Artifact, ArtifactDetail};
    use parallax_baseline::adapters::session::Session;
    use parallax_baseline::adapters::verification::{VerificationOutcome, VerificationStatus};
    use parallax_baseline::adapters::work::{WorkItem, WorkKind, WorkSnapshot, WorkState};
    use parallax_baseline::autonomy::{Autonomy, Resolution};
    use parallax_baseline::freshness::Observed;
    use parallax_baseline::manifest::ArtifactKind;
    use parallax_baseline::state::{Degradation, ItemAutonomy};
    use std::time::Duration;

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

    fn item(number: u64, title: &str) -> WorkItem {
        WorkItem {
            number,
            title: title.to_string(),
            kind: WorkKind::Issue,
            state: WorkState::Open,
            labels: vec![],
            checks: Default::default(),
            url: String::new(),
            updated_at: String::new(),
        }
    }

    fn buf_text(buf: &Buffer, y: u16) -> String {
        (0..buf.width).map(|x| buf.get(x, y).symbol).collect()
    }

    // --- Sources: worst first, and a failed source never disappears ---

    #[test]
    fn a_project_with_no_sources_says_so_plainly() {
        let lines = sources_lines(&project("x"), at(0));
        assert!(lines[1].contains("no sources declared"));
    }

    #[test]
    fn a_degraded_source_stays_listed_as_unavailable() {
        let p = degraded(project("x"), "work:github");
        let lines = sources_lines(&p, at(0));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("work:github") && l.contains("Unavailable")),
            "{lines:?}"
        );
    }

    #[test]
    fn an_unavailable_reason_carrying_ansi_escapes_renders_without_them() {
        let text = freshness_text(&Freshness::Unavailable {
            reason: "\x1b[31mrate limited\x1b[0m".into(),
            since: None,
        });
        assert!(!text.contains('\x1b'), "{text}");
        assert!(text.contains("rate limited"), "{text}");
    }

    #[test]
    fn sources_are_ordered_worst_first() {
        let mut p = degraded(project("x"), "work:github");
        p.verification = vec![Observed::watched(
            VerificationStatus {
                kind: "lint".into(),
                outcome: VerificationOutcome::Pass,
                detail: None,
            },
            at(0),
        )];
        let lines = sources_lines(&p, at(0));
        // First data row (after the header) is the degraded one, since
        // Unavailable outranks Live.
        assert!(lines[1].contains("Unavailable"), "{lines:?}");
    }

    // --- Work in flight: `—` means no claim, never a default ---

    #[test]
    fn work_not_declared_says_so_and_is_distinct_from_degraded() {
        let lines = work_lines(&project("x"));
        assert!(lines[1].contains("not declared"));
    }

    #[test]
    fn work_degraded_reads_unavailable_not_a_dash() {
        let p = degraded(project("x"), "work:github");
        let lines = work_lines(&p);
        assert_eq!(lines[1].trim(), "Unavailable");
    }

    #[test]
    fn a_work_item_title_carrying_ansi_escapes_renders_without_them() {
        let mut p = project("x");
        p.work = Some(Observed::polled(
            WorkSnapshot {
                items: vec![item(1, "\x1b[31murgent\x1b[0m fix")],
            },
            at(0),
            Duration::from_secs(30),
        ));
        let lines = work_lines(&p);
        assert!(!lines[1].contains('\x1b'), "{}", lines[1]);
        assert!(lines[1].contains("urgent fix"), "{}", lines[1]);
    }

    #[test]
    fn a_work_item_with_no_matching_autonomy_claim_renders_dashes() {
        let mut p = project("x");
        p.work = Some(Observed::polled(
            WorkSnapshot {
                items: vec![item(1, "fix the thing")],
            },
            at(0),
            Duration::from_secs(30),
        ));
        // Deliberately no `p.autonomy` entry for item 1.
        let lines = work_lines(&p);
        assert!(lines[1].contains("implement=\u{2014}"), "{}", lines[1]);
        assert!(lines[1].contains("merge=\u{2014}"), "{}", lines[1]);
    }

    #[test]
    fn a_work_item_with_a_resolved_claim_spells_out_every_axis() {
        let mut p = project("x");
        p.work = Some(Observed::polled(
            WorkSnapshot {
                items: vec![item(1, "fix the thing")],
            },
            at(0),
            Duration::from_secs(30),
        ));
        p.autonomy = vec![ItemAutonomy {
            number: 1,
            resolution: Resolution {
                autonomy: Autonomy {
                    implement: Some(Implement::Agent),
                    merge: Some(Merge::OnChecks),
                    readiness: Readiness::Verifiable,
                },
                ..Default::default()
            },
        }];
        let lines = work_lines(&p);
        assert!(lines[1].contains("implement=agent"), "{}", lines[1]);
        assert!(lines[1].contains("merge=on-checks"), "{}", lines[1]);
        assert!(lines[1].contains("readiness=verifiable"), "{}", lines[1]);
    }

    #[test]
    fn a_none_merge_claim_is_distinct_from_a_real_value_never_a_default() {
        // The `—` in the merge column is `None`: a human doing the work
        // makes no claim about what it takes to land. It must render as
        // `—`, never silently coerce to some default axis value.
        let mut p = project("x");
        p.work = Some(Observed::polled(
            WorkSnapshot {
                items: vec![item(1, "human review only")],
            },
            at(0),
            Duration::from_secs(30),
        ));
        p.autonomy = vec![ItemAutonomy {
            number: 1,
            resolution: Resolution {
                autonomy: Autonomy {
                    implement: Some(Implement::HumanOnly),
                    merge: None,
                    readiness: Readiness::Verifiable,
                },
                ..Default::default()
            },
        }];
        let lines = work_lines(&p);
        assert!(lines[1].contains("implement=human-only"), "{}", lines[1]);
        assert!(lines[1].contains("merge=\u{2014}"), "{}", lines[1]);
        assert!(!lines[1].contains("merge=on-checks"));
        assert!(!lines[1].contains("merge=direct-push"));
        assert!(!lines[1].contains("merge=human-approval"));
    }

    // --- Verification ---

    #[test]
    fn verification_not_declared_says_so() {
        let lines = verification_lines(&project("x"));
        assert!(lines[1].contains("not declared"));
    }

    #[test]
    fn verification_degraded_reads_unavailable() {
        let p = degraded(project("x"), "verification:command:lint");
        let lines = verification_lines(&p);
        assert_eq!(lines[1].trim(), "Unavailable");
    }

    #[test]
    fn verification_detail_strips_ansi_escapes_and_keeps_the_message() {
        // The actual repro: pytest's own coloured "no tests ran" summary,
        // captured verbatim and handed to us as `detail`.
        let mut p = project("x");
        p.verification = vec![Observed::watched(
            VerificationStatus {
                kind: "tests".into(),
                outcome: VerificationOutcome::Fail,
                detail: Some("\x1b[33m====== no tests ran \x1b[0min 0.01s\x1b[0m ======".into()),
            },
            at(0),
        )];
        let lines = verification_lines(&p);
        assert!(!lines[1].contains('\x1b'), "{}", lines[1]);
        assert!(!lines[1].contains("[33m"), "{}", lines[1]);
        assert!(!lines[1].contains("[0m"), "{}", lines[1]);
        assert!(lines[1].contains("no tests ran"), "{}", lines[1]);
        assert!(lines[1].contains("in 0.01s"), "{}", lines[1]);
    }

    #[test]
    fn verification_lists_each_checks_outcome() {
        let mut p = project("x");
        p.verification = vec![Observed::watched(
            VerificationStatus {
                kind: "lint".into(),
                outcome: VerificationOutcome::Fail,
                detail: Some("2 errors".into()),
            },
            at(0),
        )];
        let lines = verification_lines(&p);
        assert!(lines[1].contains("lint"));
        assert!(lines[1].contains("Fail"));
        assert!(lines[1].contains("2 errors"));
    }

    // --- Artifacts / Sessions: counts and most recent ---

    #[test]
    fn artifacts_not_declared_says_so() {
        let lines = artifacts_lines(&project("x"));
        assert!(lines[1].contains("not declared"));
    }

    #[test]
    fn artifacts_reports_the_total_and_the_most_recent() {
        let mut p = project("x");
        let older = Artifact {
            path: "old.png".into(),
            kind: ArtifactKind::Figure,
            modified: at(0),
            detail: ArtifactDetail::Figure { bytes: 1 },
        };
        let newer = Artifact {
            path: "new.png".into(),
            kind: ArtifactKind::Figure,
            modified: at(100),
            detail: ArtifactDetail::Figure { bytes: 1 },
        };
        p.artifacts = vec![Observed::watched(vec![older, newer], at(0))];
        let lines = artifacts_lines(&p);
        assert!(lines[1].contains('2'));
        assert!(lines[1].contains("new.png"));
    }

    #[test]
    fn an_artifact_path_carrying_ansi_escapes_renders_without_them() {
        let mut p = project("x");
        let a = Artifact {
            path: "\x1b[31mnew\x1b[0m.png".into(),
            kind: ArtifactKind::Figure,
            modified: at(0),
            detail: ArtifactDetail::Figure { bytes: 1 },
        };
        p.artifacts = vec![Observed::watched(vec![a], at(0))];
        let lines = artifacts_lines(&p);
        assert!(!lines[1].contains('\x1b'), "{}", lines[1]);
        assert!(lines[1].contains("new.png"), "{}", lines[1]);
    }

    #[test]
    fn sessions_not_declared_says_so() {
        let lines = sessions_lines(&project("x"));
        assert!(lines[1].contains("not declared"));
    }

    #[test]
    fn sessions_reports_the_total_and_the_most_recent() {
        let mut p = project("x");
        p.sessions = Some(Observed::watched(
            vec![Session {
                name: "s1".into(),
                path: "s1".into(),
                last_activity: at(0),
            }],
            at(0),
        ));
        let lines = sessions_lines(&p);
        assert!(lines[1].contains('1'));
        assert!(lines[1].contains("s1"));
    }

    #[test]
    fn a_session_name_carrying_ansi_escapes_renders_without_them() {
        let mut p = project("x");
        p.sessions = Some(Observed::watched(
            vec![Session {
                name: "\x1b[35mweird\x1b[0m-session".into(),
                path: "s1".into(),
                last_activity: at(0),
            }],
            at(0),
        ));
        let lines = sessions_lines(&p);
        assert!(!lines[1].contains('\x1b'), "{}", lines[1]);
        assert!(lines[1].contains("weird-session"), "{}", lines[1]);
    }

    // --- Unmapped labels: present only when there are any ---

    #[test]
    fn unmapped_labels_are_absent_when_there_are_none() {
        assert!(unmapped_lines(&project("x")).is_empty());
    }

    #[test]
    fn unmapped_labels_are_listed_when_present() {
        let mut p = project("x");
        p.unmapped_labels = vec!["bug".to_string(), "docs".to_string()];
        let lines = unmapped_lines(&p);
        assert_eq!(lines[0], "UNMAPPED LABELS");
        assert!(lines[1].contains("bug"));
        assert!(lines[1].contains("docs"));
    }

    // --- Whole-screen rendering ---

    #[test]
    fn render_detail_writes_the_project_name_on_the_first_row() {
        let buf = render_detail(&project("ttui"), at(0), 40, 30);
        assert_eq!(buf.get(0, 0).symbol, 't');
    }

    #[test]
    fn render_detail_includes_every_section_header() {
        let buf = render_detail(&project("x"), at(0), 40, 30);
        let all: String = (0..30)
            .map(|y| buf_text(&buf, y))
            .collect::<Vec<_>>()
            .join("\n");
        for header in [
            "SOURCES",
            "WORK IN FLIGHT",
            "VERIFICATION",
            "ARTIFACTS",
            "SESSIONS",
        ] {
            assert!(all.contains(header), "missing {header}\n{all}");
        }
    }

    #[test]
    fn a_short_terminal_clips_without_panicking() {
        let buf = render_detail(&project("x"), at(0), 40, 2);
        assert_eq!(buf.height, 2);
    }

    #[test]
    fn a_narrow_terminal_does_not_panic() {
        let buf = render_detail(&project("x"), at(0), 3, 30);
        assert_eq!(buf.width, 3);
    }

    #[test]
    fn a_zero_size_terminal_does_not_panic() {
        let buf = render_detail(&project("x"), at(0), 0, 0);
        assert_eq!((buf.width, buf.height), (0, 0));
    }

    #[test]
    fn a_partial_manifest_renders_as_a_normal_screen_of_dashes_not_an_error() {
        let buf = render_detail(&project("minimal"), at(0), 60, 30);
        let all: String = (0..30)
            .map(|y| buf_text(&buf, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("not declared"));
        assert!(!all.to_lowercase().contains("error"));
        assert!(!all.contains("panic"));
    }
}
