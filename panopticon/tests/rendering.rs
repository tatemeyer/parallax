//! Frames rendered into a `Buffer` and asserted on cells. No terminal
//! anywhere: TTUI's `Buffer` is inspectable in-process, which is the
//! same property TTUI uses to test its own widgets.

use panopticon::view::model::Declared;
use panopticon::view::render::{render, Frame, Tab};
use parallax_baseline::adapters::verification::{VerificationOutcome, VerificationStatus};
use parallax_baseline::adapters::work::{
    ChecksSummary, WorkItem, WorkKind, WorkSnapshot, WorkState,
};
use parallax_baseline::autonomy::resolve;
use parallax_baseline::autonomy::{AutonomyEntry, AutonomyMap, Implement, Merge};
use parallax_baseline::freshness::{Observed, DEFAULT_POLL_INTERVAL};
use parallax_baseline::state::{Degradation, ItemAutonomy, PlatformState, ProjectState};
use std::collections::BTreeMap;
use std::time::SystemTime;
use ttui::buffer::Buffer;
use ttui::layout::Rect;

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + secs)
}

fn area(w: u16, h: u16) -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: w,
        height: h,
    }
}

/// Every glyph in the buffer, row by row, so a test can ask what is on
/// screen without caring where.
fn text_of(buf: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.height {
        for x in 0..buf.width {
            out.push(buf.get(x, y).symbol);
        }
        out.push('\n');
    }
    out
}

/// The glyphs of one row.
fn row_of(buf: &Buffer, y: u16) -> String {
    (0..buf.width).map(|x| buf.get(x, y).symbol).collect()
}

fn gated_map() -> AutonomyMap {
    let mut m = BTreeMap::new();
    m.insert(
        "gated".to_string(),
        AutonomyEntry {
            implement: Some(Implement::Agent),
            merge: Some(Merge::OnChecks),
            readiness: None,
        },
    );
    AutonomyMap::new(m)
}

fn item(number: u64, title: &str, labels: &[&str]) -> WorkItem {
    WorkItem {
        number,
        title: title.to_string(),
        kind: WorkKind::Issue,
        state: WorkState::Open,
        labels: labels.iter().map(|s| s.to_string()).collect(),
        checks: ChecksSummary::none(),
        url: String::new(),
        updated_at: "2026-08-19T00:00:00Z".into(),
    }
}

/// A project with a work feed, a passing check, a plumb check that has
/// never run, and a degraded artifact source.
fn ttui_state() -> ProjectState {
    let items = vec![
        item(
            165,
            "tardis-console-idle claims coverage it does not have",
            &["gated"],
        ),
        item(
            83,
            "housekeeping: retire examples/demo.rs",
            &["semver:patch"],
        ),
    ];
    let map = gated_map();
    let mut p = ProjectState {
        name: "ttui".to_string(),
        language: Some("rust".into()),
        ..Default::default()
    };
    for i in &items {
        p.autonomy.push(ItemAutonomy {
            number: i.number,
            resolution: resolve(&map, &i.labels),
        });
    }
    p.unmapped_labels = vec!["semver:patch".to_string()];
    p.work = Some(Observed::polled(
        WorkSnapshot { items },
        at(0),
        DEFAULT_POLL_INTERVAL,
    ));
    p.verification.push(Observed::watched(
        VerificationStatus {
            kind: "perceptual".into(),
            outcome: VerificationOutcome::NotRun,
            detail: None,
        },
        at(0),
    ));
    p.degradations.push(Degradation {
        source: "artifact:capture".into(),
        reason: "reading source: permission denied".into(),
    });
    p
}

fn platform() -> PlatformState {
    let mut state = PlatformState::default();
    state.projects.push(ttui_state());
    let mut sesh = ProjectState {
        name: "sesh".to_string(),
        ..Default::default()
    };
    sesh.verification.push(Observed::watched(
        VerificationStatus {
            kind: "tests".into(),
            outcome: VerificationOutcome::Pass,
            detail: None,
        },
        at(0),
    ));
    state.projects.push(sesh);
    state
}

fn frame<'a>(platform: &'a PlatformState, tab: Tab, pending: &'a [String]) -> Frame<'a> {
    Frame {
        platform,
        selected: 0,
        tab,
        declared: Declared {
            work: true,
            verification: true,
            artifacts: true,
            sessions: true,
        },
        pending_checks: pending,
        now: at(12),
        detail_selected: 0,
    }
}

fn draw(f: &Frame<'_>, w: u16, h: u16) -> Buffer {
    let mut buf = Buffer::new(w, h);
    render(f, area(w, h), &mut buf);
    buf
}

#[test]
fn the_rail_lists_every_project_on_the_left() {
    let platform = platform();
    let buf = draw(&frame(&platform, Tab::Work, &[]), 100, 30);
    let text = text_of(&buf);
    assert!(text.contains("PROJECTS"), "the rail is titled");
    assert!(text.contains("ttui"));
    assert!(text.contains("sesh"));
}

/// The property the spec names first: every source's age is on screen.
#[test]
fn every_source_appears_in_the_footer_with_its_age() {
    let platform = platform();
    let buf = draw(&frame(&platform, Tab::Work, &[]), 120, 30);
    let footer = (27..30).map(|y| row_of(&buf, y)).collect::<String>();
    assert!(footer.contains("work"), "the polled feed: {footer}");
    assert!(footer.contains("12s"), "and its age");
    assert!(
        footer.contains("verification:perceptual"),
        "the plumb check"
    );
    assert!(footer.contains("live"), "which is filesystem-backed");
}

/// The property the spec names twice: a degraded source is visible
/// *with its reason*, never as an empty pane.
#[test]
fn a_degraded_source_shows_its_reason_on_screen() {
    let platform = platform();
    let buf = draw(&frame(&platform, Tab::Work, &[]), 140, 30);
    let text = text_of(&buf);
    assert!(
        text.contains("permission denied"),
        "the reason must reach the screen:\n{text}"
    );
}

/// The footer never silently drops a source: if they do not fit, it says
/// how many are missing.
#[test]
fn a_footer_too_narrow_for_every_source_says_how_many_it_hid() {
    let platform = platform();
    let buf = draw(&frame(&platform, Tab::Work, &[]), 44, 20);
    let text = text_of(&buf);
    assert!(text.contains("(+"), "hidden sources are counted:\n{text}");
}

#[test]
fn the_work_pane_renders_the_projection_and_the_unmapped_labels() {
    let platform = platform();
    let buf = draw(&frame(&platform, Tab::Work, &[]), 140, 30);
    let text = text_of(&buf);
    assert!(text.contains("agent"), "the mapped item claims agent");
    assert!(text.contains("on-checks"));
    assert!(
        text.contains("—"),
        "the unmapped item claims nothing, and shows it"
    );
    assert!(
        text.contains("unmapped: semver:patch"),
        "labels the manifest never declared are surfaced"
    );
}

#[test]
fn a_check_that_has_not_been_asked_for_says_so_rather_than_showing_green() {
    let platform = platform();
    let pending = vec!["tests".to_string()];
    let buf = draw(&frame(&platform, Tab::Verification, &pending), 120, 30);
    let text = text_of(&buf);
    assert!(text.contains("not run this session"), "{text}");
    assert!(
        text.contains("never run"),
        "and plumb's own never-run: {text}"
    );
}

#[test]
fn the_tab_strip_marks_the_pane_that_is_showing() {
    let platform = platform();
    let buf = draw(&frame(&platform, Tab::Sessions, &[]), 120, 30);
    let text = text_of(&buf);
    assert!(text.contains("[SESSIONS]"), "{text}");
    assert!(text.contains(" WORK "), "the others are listed unmarked");
}

#[test]
fn an_undeclared_family_says_not_declared_rather_than_rendering_empty() {
    let platform = platform();
    let mut f = frame(&platform, Tab::Sessions, &[]);
    f.declared.sessions = false;
    let buf = draw(&f, 120, 30);
    assert!(text_of(&buf).contains("not declared"));
}

#[test]
fn an_empty_platform_says_what_to_do_about_it() {
    let empty = PlatformState::default();
    let buf = draw(&frame(&empty, Tab::Work, &[]), 100, 20);
    let text = text_of(&buf);
    assert!(text.contains("no projects registered"), "{text}");
    assert!(text.contains("--projects-root"), "and how to fix it");
}

/// A narrow terminal drops the rail rather than rendering two useless
/// columns.
#[test]
fn a_narrow_frame_drops_the_rail_and_keeps_the_detail() {
    let platform = platform();
    let buf = draw(&frame(&platform, Tab::Work, &[]), 30, 20);
    let text = text_of(&buf);
    assert!(!text.contains("PROJECTS"), "no rail at 30 columns");
    assert!(text.contains("WORK"), "the detail survives");
}

#[test]
fn a_frame_too_small_for_anything_says_so_instead_of_rendering_nothing() {
    let platform = platform();
    let buf = draw(&frame(&platform, Tab::Work, &[]), 12, 4);
    assert!(text_of(&buf).contains("too small"));
}

/// Rendering is a pure function of the frame: same input, same cells.
#[test]
fn the_same_frame_renders_identically_twice() {
    let platform = platform();
    let f = frame(&platform, Tab::Work, &[]);
    let a = draw(&f, 100, 30);
    let b = draw(&f, 100, 30);
    for y in 0..30 {
        for x in 0..100 {
            assert_eq!(a.get(x, y).symbol, b.get(x, y).symbol, "cell {x},{y}");
        }
    }
}
