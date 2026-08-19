//! Two runs, identical frames.
//!
//! This is what makes a Plumb NO-GO mean "the layout is wrong" rather
//! than "time passed". If it fails, the cause is a `SystemTime::now()`
//! that escaped, and the fix is to find it — not to loosen the
//! assertion.

use panopticon::fixtures;
use panopticon::view::model::Declared;
use panopticon::view::render::{render, Frame, Tab};
use parallax_baseline::state::{aggregate, PlatformState};
use std::path::{Path, PathBuf};
use ttui::buffer::{Buffer, Cell};
use ttui::layout::Rect;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Loads the fixture set and aggregates it, exactly as the cockpit does
/// on its first refresh — including taking the build checks out first,
/// through the same `split_by_cost` the refresh thread uses. Skipping
/// that would show `cargo test` as passing in a demo that never ran it.
fn aggregated() -> (PlatformState, std::time::SystemTime) {
    let set = match fixtures::load(&fixture_dir()) {
        Ok(set) => set,
        Err(e) => panic!("the shipped fixture set must load: {e}"),
    };
    let now = set.now;
    let mut projects = set.projects;
    for (_, adapters) in projects.iter_mut() {
        let _held_back = panopticon::refresh::split_by_cost(adapters);
    }
    (aggregate(&mut projects, now), now)
}

/// The fixture registry is scanned, so projects arrive sorted by
/// directory name rather than in any order this test chose.
fn index_of(platform: &PlatformState, name: &str) -> usize {
    platform
        .projects
        .iter()
        .position(|p| p.name == name)
        .unwrap_or_else(|| panic!("{name} is in the fixture set"))
}

fn draw(platform: &PlatformState, tab: Tab, w: u16, h: u16, now: std::time::SystemTime) -> Buffer {
    let frame = Frame {
        platform,
        selected: index_of(platform, "ttui"),
        tab,
        declared: Declared {
            work: true,
            verification: true,
            artifacts: true,
            sessions: true,
        },
        pending_checks: &["lint".to_string(), "tests".to_string()],
        now,
        detail_selected: 0,
        alarm: false,
    };
    let mut buf = Buffer::new(w, h);
    render(
        &frame,
        Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        },
        &mut buf,
    );
    buf
}

/// Every cell, not just the glyphs: a colour that drifts run to run
/// would fail a perceptual review while every assertion on text passed.
fn assert_same(a: &Buffer, b: &Buffer, what: &str) {
    assert_eq!((a.width, a.height), (b.width, b.height), "{what}: size");
    for y in 0..a.height {
        for x in 0..a.width {
            let (l, r): (&Cell, &Cell) = (a.get(x, y), b.get(x, y));
            assert_eq!(l.symbol, r.symbol, "{what}: glyph at {x},{y}");
            assert_eq!(l.fg, r.fg, "{what}: fg at {x},{y}");
            assert_eq!(l.bg, r.bg, "{what}: bg at {x},{y}");
            assert_eq!(l.style, r.style, "{what}: style at {x},{y}");
        }
    }
}

#[test]
fn two_runs_of_the_fixture_set_render_identical_frames() {
    let (first, now_a) = aggregated();
    let (second, now_b) = aggregated();
    assert_eq!(now_a, now_b, "the clock is frozen, not sampled");

    for tab in Tab::ALL {
        assert_same(
            &draw(&first, tab, 120, 30, now_a),
            &draw(&second, tab, 120, 30, now_b),
            tab.label(),
        );
    }
}

/// Determinism must not be an accident of one geometry — Plumb captures
/// at whatever size the scenario declares.
#[test]
fn the_same_holds_at_a_second_size() {
    let (first, now) = aggregated();
    let (second, _) = aggregated();
    assert_same(
        &draw(&first, Tab::Work, 80, 24, now),
        &draw(&second, Tab::Work, 80, 24, now),
        "80x24",
    );
}

/// The fixture set is not an empty screen pretending to be a cockpit:
/// it has work, a verdict, and a session in it.
#[test]
fn the_fixture_set_actually_shows_something() {
    let (platform, now) = aggregated();
    let text: String = {
        let buf = draw(&platform, Tab::Work, 140, 30, now);
        (0..buf.height)
            .map(|y| {
                (0..buf.width)
                    .map(|x| buf.get(x, y).symbol)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(text.contains("ttui"), "the rail names the project:\n{text}");
    assert!(text.contains("sesh"), "and the second one");
    assert!(
        text.contains("Sparkline") && text.contains("Gauge widget"),
        "the work pane holds recorded issues:\n{text}"
    );
}

/// The plumb verdict in the fixture tree is a NO-GO, and it must reach
/// the artifacts pane in Plumb's own words.
#[test]
fn the_recorded_verdict_reaches_the_artifacts_pane() {
    let (platform, now) = aggregated();
    let buf = draw(&platform, Tab::Artifacts, 140, 30, now);
    let text: String = (0..buf.height)
        .flat_map(|y| (0..buf.width).map(move |x| (x, y)))
        .map(|(x, y)| buf.get(x, y).symbol)
        .collect();
    assert!(text.contains("NO-GO"), "got:\n{text}");
}
