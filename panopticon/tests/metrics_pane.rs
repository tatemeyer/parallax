//! The metrics pane, rendered into a `Buffer` and asserted on cells.
//!
//! The model tests in `view::metrics` decide what a row *is*; these
//! decide what reaches the screen, which is where a shape claim either
//! survives or quietly becomes a curve again.

use panopticon::view::model::Declared;
use panopticon::view::render::{render, Frame, Tab};
use parallax_baseline::adapters::artifact::{Artifact, ArtifactDetail, Series};
use parallax_baseline::freshness::Observed;
use parallax_baseline::manifest::ArtifactKind;
use parallax_baseline::state::{PlatformState, ProjectState};
use std::time::{Duration, SystemTime};
use ttui::buffer::Buffer;
use ttui::layout::Rect;

/// The eight glyphs a sparkline is drawn from.
const RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// A project whose only artifact is a metrics feed, last written
/// `modified`, observed at `at(0)`.
fn platform_of(series: Vec<Series>, modified: Option<SystemTime>) -> PlatformState {
    let mut project = ProjectState {
        name: "model-experiments".into(),
        ..Default::default()
    };
    project.artifacts.push(Observed::watched(
        vec![Artifact {
            path: std::path::PathBuf::from("/tmp/results/run7/results.jsonl"),
            kind: ArtifactKind::Metrics,
            modified,
            detail: ArtifactDetail::Metrics { series },
        }],
        at(0),
    ));
    PlatformState {
        projects: vec![project],
    }
}

/// Rendered twelve seconds after the feed was read.
fn screen_of(platform: &PlatformState) -> String {
    let frame = Frame {
        platform,
        selected: 0,
        tab: Tab::Metrics,
        declared: Declared {
            work: true,
            verification: true,
            artifacts: true,
            sessions: true,
        },
        pending_checks: &[],
        now: at(12),
        detail_selected: 0,
        log: &[],
        question: None,
        alarm: false,
    };
    let area = Rect {
        x: 0,
        y: 0,
        width: 120,
        height: 30,
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

/// Three seeds of one sweep cell.
fn seeds(name: &str, variant: &str, points: Vec<f64>) -> Series {
    Series::unordered(name, points).with_dimensions([("variant", variant)])
}

/// Both ages, distinctly labelled. A feed read twelve seconds ago from
/// a run that stopped an hour back is fresh and stalled at once, and
/// one age cannot say so.
#[test]
fn the_pane_shows_the_read_age_and_the_producer_age_separately() {
    let screen = screen_of(&platform_of(
        vec![seeds("effective_rank", "full", vec![2.352, 2.791])],
        Some(at(0) - Duration::from_secs(3600)),
    ));

    assert!(screen.contains("read 12s ago"), "{screen}");
    assert!(screen.contains("produced 3612s ago"), "{screen}");
}

/// A producer age nobody could supply says so. Not `0s`, which reads as
/// the freshest thing on screen, and not a date in 1970.
#[test]
fn a_feed_whose_producer_age_is_unknown_says_unknown() {
    let screen = screen_of(&platform_of(
        vec![seeds("effective_rank", "full", vec![2.352, 2.791])],
        None,
    ));

    assert!(screen.contains("produced unknown"), "{screen}");
    assert!(!screen.contains("produced 0s"), "{screen}");
}

/// Arc 1's finding, on screen: three variants of one metric, each
/// showing where its seeds fell, against one shared scale.
#[test]
fn the_three_variants_of_a_metric_render_as_comparable_spreads() {
    let screen = screen_of(&platform_of(
        vec![
            seeds("effective_rank", "full", vec![2.352, 2.791]),
            seeds("effective_rank", "no_ema", vec![1.250, 1.459]),
            seeds("effective_rank", "random_init", vec![2.437, 2.934]),
        ],
        Some(at(0)),
    ));

    assert!(screen.contains("effective_rank"), "{screen}");
    for variant in ["variant=full", "variant=no_ema", "variant=random_init"] {
        assert!(screen.contains(variant), "missing {variant} in {screen}");
    }
    assert!(screen.contains("2.3520"), "full's floor: {screen}");
    assert!(screen.contains("2.9340"), "random_init's ceiling: {screen}");
}

/// The columns one row's band occupies, as `(left, right)`.
fn band_of(screen: &str, variant: &str) -> (usize, usize) {
    let line = screen
        .lines()
        .find(|line| line.contains(variant))
        .unwrap_or_else(|| panic!("no row for {variant}"));
    let left = line.find('├').unwrap_or_else(|| panic!("no band: {line}"));
    let right = line.rfind('┤').unwrap_or_else(|| panic!("no band: {line}"));
    (left, right)
}

/// Arc 1's null result, asserted as **geometry** rather than as
/// arithmetic a reader has to do.
///
/// `full` and `random_init` must occupy overlapping columns — the
/// trained model is not distinguishable from an untrained one on this
/// metric — and `no_ema` must sit clear of both. If the bands ever stop
/// being drawn on one shared scale this fails, which is the point.
#[test]
fn the_overlap_that_is_the_finding_is_visible_as_overlapping_bands() {
    let screen = screen_of(&platform_of(
        vec![
            seeds("effective_rank", "full", vec![2.352, 2.779, 2.791]),
            seeds("effective_rank", "no_ema", vec![1.250, 1.389, 1.459]),
            seeds("effective_rank", "random_init", vec![2.437, 2.461, 2.934]),
        ],
        Some(at(0)),
    ));

    let full = band_of(&screen, "variant=full");
    let no_ema = band_of(&screen, "variant=no_ema");
    let random = band_of(&screen, "variant=random_init");

    assert!(
        full.0 <= random.1 && random.0 <= full.1,
        "full {full:?} and random_init {random:?} must overlap on screen:\n{screen}"
    );
    assert!(
        no_ema.1 < full.0,
        "no_ema {no_ema:?} must sit clear of full {full:?}:\n{screen}"
    );
}

/// The rule the arc turns on, asserted at the cell level: unordered
/// points get no sparkline. A curve across three seeds would claim a
/// progression nothing measured.
#[test]
fn an_unordered_series_is_never_drawn_as_a_sparkline() {
    let screen = screen_of(&platform_of(
        vec![seeds("effective_rank", "full", vec![2.352, 2.791, 2.779])],
        Some(at(0)),
    ));

    for block in RAMP {
        assert!(
            !screen.contains(block),
            "a spread was drawn as a curve: {screen}"
        );
    }
}

/// An ordered feed does get one, so the absence above is a decision
/// rather than a pane that cannot draw.
#[test]
fn an_ordered_series_is_drawn_as_a_sparkline() {
    let screen = screen_of(&platform_of(
        vec![Series::ordered("loss", vec![2.7, 2.1, 1.6, 1.2], "step")],
        Some(at(0)),
    ));

    assert!(
        RAMP.iter().any(|block| screen.contains(*block)),
        "an ordered series drew no curve: {screen}"
    );
    assert!(screen.contains("by step"), "and names what orders it");
}

/// One point is a value, not a flat line — a flat line claims something
/// was measured repeatedly and did not change.
#[test]
fn a_single_point_draws_no_line_at_all() {
    let screen = screen_of(&platform_of(
        vec![Series::ordered("loss", vec![1.5], "step")],
        Some(at(0)),
    ));

    assert!(screen.contains("1.5000"), "{screen}");
    for block in RAMP {
        assert!(
            !screen.contains(block),
            "one point became a curve: {screen}"
        );
    }
}

/// An empty series says so rather than rendering a zero.
#[test]
fn an_empty_series_renders_as_words_not_as_zero() {
    let screen = screen_of(&platform_of(
        vec![Series::unordered("loss", Vec::new())],
        Some(at(0)),
    ));

    assert!(screen.contains("parsed, no points"), "{screen}");
}

/// A project declaring no metrics feed gets a sentence, not an empty
/// pane that reads as a broken one.
#[test]
fn a_project_with_no_metrics_feed_says_so() {
    let platform = PlatformState {
        projects: vec![ProjectState {
            name: "ttui".into(),
            ..Default::default()
        }],
    };

    assert!(
        screen_of(&platform).contains("no metrics feeds"),
        "{}",
        screen_of(&platform)
    );
}
