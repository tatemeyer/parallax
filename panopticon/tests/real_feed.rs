//! The metrics pane against the producer it was built for.
//!
//! Everything else that exercises this pane hands it series written to
//! exercise it — three variants, one dimension each, nine points. Two
//! defects lived through all of that and died the first time the pane
//! was pointed at `Model-Experiments`' real feed, which is recorded
//! whole in `fixtures/model-experiments/`:
//!
//! - a row's label was every dimension the record carried, joined, which
//!   ran to 197 characters and pushed the point count, the band and the
//!   numbers off the right-hand edge;
//! - `detail_len` counted *feeds*, of which there is one, so `j` could
//!   not move — and the detail list draws from the top, so 88 of the 113
//!   lines were unreachable and nothing said so.
//!
//! These assertions are against the recording, so they are facts rather
//! than a taste in fixtures. If that repository's feed changes shape,
//! this file is where it is meant to fail.

use panopticon::fixtures;
use panopticon::view::metrics::{metric_feeds, metric_lines};
use panopticon::view::model::Declared;
use panopticon::view::render::{render, Frame, Tab};
use parallax_baseline::state::{aggregate, PlatformState, ProjectState};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use ttui::buffer::Buffer;
use ttui::layout::Rect;

/// The cockpit's declared capture size, and the one every Plumb
/// scenario runs at.
const WIDTH: u16 = 120;
/// See [`WIDTH`].
const HEIGHT: u16 = 30;

/// The width the renderer gives a row's label before it elides.
const LABEL: usize = 42;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// The fixture set, aggregated exactly as the cockpit aggregates it on
/// its first refresh.
fn platform() -> (PlatformState, SystemTime) {
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

fn experiments(platform: &PlatformState) -> &ProjectState {
    platform
        .projects
        .iter()
        .find(|p| p.name == "model-experiments")
        .expect("model-experiments is in the fixture set")
}

/// The cockpit's metrics pane, as cells, with the selection where
/// `detail_selected` puts it.
fn screen(platform: &PlatformState, now: SystemTime, detail_selected: usize) -> String {
    let selected = platform
        .projects
        .iter()
        .position(|p| p.name == "model-experiments")
        .unwrap();
    let frame = Frame {
        platform,
        selected,
        tab: Tab::Metrics,
        declared: Declared {
            work: true,
            verification: true,
            artifacts: true,
            sessions: true,
        },
        pending_checks: &[],
        now,
        detail_selected,
        log: &[],
        question: None,
        alarm: false,
    };
    let area = Rect {
        x: 0,
        y: 0,
        width: WIDTH,
        height: HEIGHT,
    };
    let mut buf = Buffer::new(area.width, area.height);
    render(&frame, area, &mut buf);
    let mut out = String::new();
    for y in 0..buf.height {
        for x in 0..buf.width {
            out.push(buf.get(x, y).symbol);
        }
        out.push('\n');
    }
    out
}

/// Every row of the pane, in order.
fn rows(screen: &str) -> Vec<String> {
    screen
        .lines()
        .filter(|line| line.contains("variant="))
        .map(|line| line.to_string())
        .collect()
}

/// The columns one row's band occupies, as `(left, right)`.
fn band(row: &str) -> (usize, usize) {
    let left = row
        .find('\u{251c}')
        .unwrap_or_else(|| panic!("no band: {row}"));
    let right = row
        .rfind('\u{2524}')
        .unwrap_or_else(|| panic!("no band: {row}"));
    assert!(left < right, "not a band: {row}");
    (left, right)
}

/// The shape of the recording, stated once so the rest of this file can
/// be read against it. 318 records, six metrics, 106 series — and the
/// coverage is ragged, because a record carries whichever parameters its
/// own experiment varied.
#[test]
fn the_recorded_feed_is_the_messy_one() {
    let (platform, now) = platform();
    let feeds = metric_feeds(experiments(&platform), now);
    assert_eq!(feeds.len(), 1, "one metrics file");
    assert_eq!(feeds[0].name, "results.jsonl");
    assert_eq!(feeds[0].groups.len(), 6, "six metrics");
    assert_eq!(feeds[0].series(), 106);
    assert_eq!(
        metric_lines(&feeds).len(),
        113,
        "the lines `j` moves through"
    );

    let names: Vec<&str> = feeds[0].groups.iter().map(|g| g.name.as_str()).collect();
    assert!(names.contains(&"effective_rank"), "got {names:?}");
    assert!(
        !names.contains(&"issue") && !names.contains(&"seed"),
        "an identifier is being charted as a measurement: {names:?}"
    );
}

/// **The first defect.** Every row on the first screenful has to reach
/// its numbers; a row whose label ate the line says nothing at all.
#[test]
fn every_row_on_screen_shows_what_it_measured() {
    let (platform, now) = platform();
    let screen = screen(&platform, now, 0);
    let rows = rows(&screen);
    assert!(
        rows.len() > 15,
        "the pane should be full of rows:\n{screen}"
    );
    for row in &rows {
        assert!(
            // A band, or the single mark a spread narrower than one
            // cell gets. Both are marks; a row with neither has had its
            // measurements pushed off the edge by its own label.
            row.contains('\u{251c}') || row.contains('\u{253c}'),
            "no band reached the screen: {row}"
        );
        assert!(
            row.contains('.'),
            "no measurement reached the screen: {row}"
        );
    }
}

/// Arc 1's conclusion, read off the real feed rather than off numbers
/// typed into a test: `full` overlaps `random_init` — the trained model
/// is not distinguishable from an untrained one on `effective_rank` —
/// and `no_ema` sits clear below both.
///
/// This is the sentence the arc's Plumb scenario is judged on, asserted
/// here as geometry so that a NO-GO there is about the layout rather
/// than about the data.
#[test]
fn the_finding_is_visible_in_the_first_three_rows() {
    let (platform, now) = platform();
    let screen = screen(&platform, now, 0);
    let rows = rows(&screen);

    // The trailing space matters: `variant=full` is also a prefix of
    // `variant=full_m0`, which is a different experiment's cell.
    assert!(rows[0].contains("variant=full "), "got {}", rows[0]);
    assert!(rows[1].contains("variant=no_ema "), "got {}", rows[1]);
    assert!(rows[2].contains("variant=random_init "), "got {}", rows[2]);

    let (full, no_ema, random) = (band(&rows[0]), band(&rows[1]), band(&rows[2]));
    assert!(
        full.0 <= random.1 && random.0 <= full.1,
        "full {full:?} and random_init {random:?} must overlap:\n{screen}"
    );
    assert!(
        no_ema.1 < full.0,
        "no_ema {no_ema:?} must sit clear of full {full:?}:\n{screen}"
    );
}

/// A row is labelled by what tells it apart from its siblings, so no two
/// rows of one metric should read the same on screen.
///
/// **One pair per metric does**, and it is recorded here rather than
/// asserted away: `no_ema` at 3000 steps was run in both `001` and
/// `002`, so the only thing separating those two rows is a
/// thirty-character `experiment_slug` that does not fit the column.
/// Two of the six metrics cover both experiments. Everything else in
/// the feed is distinct, and if a change makes this worse the count
/// goes up and this fails.
#[test]
fn the_rows_of_a_metric_are_told_apart_by_their_labels() {
    let (platform, now) = platform();
    let feeds = metric_feeds(experiments(&platform), now);
    let mut collisions = 0;
    for group in &feeds[0].groups {
        let mut seen = std::collections::BTreeMap::new();
        for row in &group.rows {
            let visible: String = row.label.chars().take(LABEL).collect();
            *seen.entry(visible).or_insert(0usize) += 1;
        }
        collisions += seen.values().filter(|n| **n > 1).count();
    }
    assert_eq!(
        collisions, 2,
        "one pair in each of the two metrics that cover both experiments"
    );
}

/// **The second defect.** `j` has to be able to reach the last line, and
/// the line it reaches has to be on the screen.
#[test]
fn the_last_line_of_the_feed_can_be_reached_and_is_drawn() {
    let (platform, now) = platform();
    let feeds = metric_feeds(experiments(&platform), now);
    let last = metric_lines(&feeds).len() - 1;

    let top = screen(&platform, now, 0);
    let bottom = screen(&platform, now, last);
    assert_ne!(top, bottom, "the pane did not move");
    assert!(
        bottom.contains("probe_r2_superseded_104"),
        "the last metric never reaches the screen:\n{bottom}"
    );
    assert!(
        !top.contains("probe_r2_superseded_104"),
        "the fixture is too small to be testing this"
    );
}

/// The feed's header says how much of it there is, so a screenful of
/// rows cannot be mistaken for the whole feed.
#[test]
fn the_feeds_header_says_how_many_series_it_holds() {
    let (platform, now) = platform();
    let screen = screen(&platform, now, 0);
    assert!(
        screen.contains("6 metrics, 106 series"),
        "the pane does not say how much it is not showing:\n{screen}"
    );
}

/// A checked-in file's modification time is whenever the clone happened,
/// which is later than this fixture set's frozen clock and unknowable in
/// advance. The producer age is therefore **unknown** here — which is
/// the honest answer, and a rendering the pane has to be able to give.
/// `0s` would put a stalled producer at the top of the screen as the
/// freshest thing on it.
#[test]
fn a_recorded_feed_reports_its_producer_age_as_unknown() {
    let (platform, now) = platform();
    let screen = screen(&platform, now, 0);
    assert!(screen.contains("produced unknown"), "{screen}");
    assert!(!screen.contains("produced 0s ago"), "{screen}");
}

/// The order the rail puts projects in, which the recorded Plumb
/// scripts count `Tab` presses against.
///
/// A registry scan sorts by directory name, so adding a project can
/// move every row below it — and a script that lands one row off
/// photographs the wrong pane while still passing every test here. It
/// is asserted rather than remembered for that reason. Peers append
/// after the local projects once they answer, so `sesh@pi5` and
/// `ttui@tates-laptop` are rows three and four.
#[test]
fn the_rail_order_the_plumb_scripts_are_written_against() {
    let (platform, _) = platform();
    let names: Vec<&str> = platform.projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["model-experiments", "sesh", "ttui"]);
}
