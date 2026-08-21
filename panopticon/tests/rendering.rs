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
        log: &[],
        question: None,
        alarm: false,
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

// --- What a blinded perceptual review found on screen (run
// 20260820T020000Z) and these tests now hold in place ---

/// Two lenses, independently: a title clipped flush against the border
/// is indistinguishable from one that ended there. Row #141's
/// "…do with a singl" read as a complete title to a reader who could
/// only look.
#[test]
fn an_over_wide_line_is_marked_rather_than_clipped_silently() {
    let long = "a title far longer than the pane it is being asked to fit inside of, by a lot";
    let mut project = ttui_state();
    let work = project.work.as_mut().expect("work feed");
    work.value.items = vec![item(1, long, &["gated"])];
    let platform = PlatformState {
        projects: vec![project],
    };

    let mut buf = Buffer::new(60, 20);
    render(&frame(&platform, Tab::Work, &[]), area(60, 20), &mut buf);
    let screen = text_of(&buf);

    assert!(
        screen.contains("..."),
        "a truncated title says nothing about being truncated:
{screen}"
    );
    assert!(
        !screen.contains("by a lot"),
        "the elision kept text it had no room for:
{screen}"
    );
}

/// A line that fits is left exactly alone — the marker is evidence of
/// truncation, so a spurious one is a lie in the other direction.
#[test]
fn a_line_that_fits_is_untouched() {
    let mut buf = Buffer::new(160, 20);
    let platform = PlatformState {
        projects: vec![ttui_state()],
    };
    render(&frame(&platform, Tab::Work, &[]), area(160, 20), &mut buf);
    assert!(!text_of(&buf).contains("..."));
}

/// The bell rings for the platform while the footer box holds the
/// *selected* project's sources. A banner over a healthy project with
/// nothing on screen to account for it is a question, not a warning.
#[test]
fn the_blocker_banner_names_whose_blocker_it_is() {
    let healthy = ProjectState {
        name: "sesh".to_string(),
        ..ProjectState::default()
    };
    let broken = ttui_state(); // its artifact source is degraded
    let platform = PlatformState {
        projects: vec![healthy, broken],
    };
    let mut f = frame(&platform, Tab::Work, &[]);
    f.alarm = true;
    f.selected = 0; // looking at sesh while ttui is the one on fire

    let mut buf = Buffer::new(120, 20);
    render(&f, area(120, 20), &mut buf);
    let screen = text_of(&buf);
    assert!(
        screen.contains("BLOCKER: ttui"),
        "the banner does not say whose blocker it is:
{screen}"
    );
}

/// A project whose every **observed** field is packed with escape
/// sequences and raw control characters.
///
/// Deliberately not a plausible one: every string here is the sort of
/// thing a real tool emits, all at once. `pytest`, `cargo` and `npm`
/// colour their output by default, a branch name can hold anything a
/// person typed, and an adapter's error carries whatever the process
/// said on the way down.
fn hostile_project() -> ProjectState {
    use parallax_baseline::adapters::artifact::{Artifact, ArtifactDetail, RunFinding};
    use parallax_baseline::adapters::session::Session;
    use parallax_baseline::manifest::ArtifactKind;
    use std::path::PathBuf;

    let mut p = ProjectState {
        name: "ttui".to_string(),
        ..Default::default()
    };

    // The line from the issue, verbatim.
    p.verification.push(Observed::watched(
        VerificationStatus {
            kind: "tests".into(),
            outcome: VerificationOutcome::Fail,
            detail: Some(
                "\u{1b}[33m======= \u{1b}[33mno tests ran\u{1b}[0m\u{1b}[33m in 0.01s\u{1b}[0m"
                    .into(),
            ),
        },
        at(0),
    ));

    p.work = Some(Observed::polled(
        WorkSnapshot {
            items: vec![WorkItem {
                number: 1,
                title: "\u{1b}[1;31mfix:\u{1b}[0m a title that\u{1b}[2J clears the screen".into(),
                kind: WorkKind::PullRequest,
                state: WorkState::Open,
                labels: vec![],
                checks: ChecksSummary::none(),
                url: String::new(),
                updated_at: "2026-08-19T00:00:00Z".into(),
            }],
        },
        at(0),
        DEFAULT_POLL_INTERVAL,
    ));

    p.sessions = Some(Observed::watched(
        vec![Session {
            // A directory name really can hold an escape.
            name: "agent\u{1b}[1;1H-run\ttwo".into(),
            path: PathBuf::from("/tmp/agent"),
            last_activity: at(0),
        }],
        at(0),
    ));

    p.artifacts.push(Observed::watched(
        vec![Artifact {
            path: PathBuf::from("/tmp/\u{1b}[31mrun-01"),
            kind: ArtifactKind::Capture,
            modified: Some(at(0)),
            detail: ArtifactDetail::Capture {
                run_id: "\u{1b}]0;retitled\u{7}run-01".into(),
                outcome: VerificationOutcome::Fail,
                findings: vec![RunFinding {
                    fingerprint: "abc123".into(),
                    lens: "design".into(),
                    severity: "major".into(),
                    claim: "the dial\u{1b}[5m blinks\r\nand wraps".into(),
                }],
            },
        }],
        at(0),
    ));

    p.degradations.push(Degradation {
        source: "artifacts".into(),
        reason: "could not read \u{1b}[31m/tmp/x\u{1b}[0m: denied".into(),
    });

    p
}

/// **The guarantee, asserted where it actually matters.** Sanitising
/// happens at three call sites, and three call sites is a habit rather
/// than a property — so this checks the end of it instead: whatever path
/// drew a cell, no cell holds anything a terminal would act on.
///
/// Every pane, because each reaches the buffer through different code,
/// and a pane added later that forgets to elide fails here.
#[test]
fn no_pane_lets_captured_text_put_a_control_character_on_screen() {
    let platform = PlatformState {
        projects: vec![hostile_project()],
    };
    for tab in Tab::ALL {
        let mut f = frame(&platform, tab, &[]);
        f.question = Some("merge \u{1b}[31m#1\u{1b}[0m — type 1");
        f.log = &[];
        let buf = draw(&f, 120, 30);
        for y in 0..buf.height {
            for x in 0..buf.width {
                let symbol = buf.get(x, y).symbol;
                assert!(
                    !symbol.is_control(),
                    "{tab:?} put {symbol:?} at ({x},{y}) — observed data reached the terminal"
                );
            }
        }
    }
}

/// The other half, and the one that guards the *fix* rather than the
/// bug: a stripper that took the words with the escapes would pass every
/// assertion above and leave the pane empty. This fails only if the
/// stripping is too eager, which is why it is here and why it would pass
/// against the old unsanitised code too.
#[test]
fn the_words_the_escapes_were_hiding_are_still_on_screen() {
    let platform = PlatformState {
        projects: vec![hostile_project()],
    };
    let buf = draw(&frame(&platform, Tab::Verification, &[]), 120, 30);
    let screen = text_of(&buf);
    assert!(
        screen.contains("no tests ran"),
        "the detail was lost with its escapes:
{screen}"
    );
}

/// An escape is several `char`s and no display columns, so a line
/// measured before stripping is truncated against a length that has
/// nothing to do with what is visible — the row reads as full while
/// showing almost nothing.
#[test]
fn a_line_is_truncated_against_what_is_visible_not_what_was_captured() {
    let platform = PlatformState {
        projects: vec![hostile_project()],
    };
    // Narrow on purpose. With its column padding this row is about 63
    // visible columns and about 86 raw ones, so the width sits between
    // them: measured correctly the whole line fits, and measured against
    // its escape bytes it is elided and loses its last words.
    let buf = draw(&frame(&platform, Tab::Verification, &[]), 90, 30);
    let screen = text_of(&buf);
    assert!(
        screen.contains("in 0.01s"),
        "the line was cut against its escape bytes:
{screen}"
    );
}
