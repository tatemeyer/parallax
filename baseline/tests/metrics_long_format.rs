//! A long-format metrics feed — one record per *measurement* rather
//! than per timestep — and the ordering claim that decides whether its
//! points may be drawn as a curve.
//!
//! The fixture is real: 27 rows of `projects/jepa/results.csv` from
//! Model-Experiments, the checked-in record Arc 1 concluded in. See
//! `fixtures/metrics/README.md`.

use parallax_baseline::adapters::artifact::{parse_metrics, Series, SeriesOrder};
use std::path::{Path, PathBuf};

fn sweep() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/metrics/sweep.jsonl")
}

fn parsed() -> Vec<Series> {
    parse_metrics(&std::fs::read_to_string(sweep()).unwrap())
}

fn find<'a>(series: &'a [Series], name: &str, variant: &str) -> &'a Series {
    series
        .iter()
        .find(|s| {
            s.name == name && s.dimensions().get("variant").map(String::as_str) == Some(variant)
        })
        .unwrap_or_else(|| panic!("no series {name} for variant {variant}"))
}

/// The series are the metrics the experiment measured, one per variant
/// — not the columns of the file that happened to hold numbers.
#[test]
fn a_long_format_record_is_keyed_by_the_metric_it_names() {
    let series = parsed();
    assert_eq!(series.len(), 9, "3 metrics x 3 variants");
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert!(names.iter().all(|n| n.starts_with("effective_rank")
        || n.starts_with("embedding_std")
        || n.starts_with("probe_r2")));
}

/// The two failures that made this slice necessary, asserted as
/// absences: an issue number and a seed index are identifiers, and a
/// pane that charts them is charting the file rather than the
/// experiment.
#[test]
fn identifiers_alongside_a_named_measurement_are_not_charted() {
    let series = parsed();
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert!(!names.contains(&"issue"));
    assert!(!names.contains(&"seed"));
    assert!(!names.contains(&"value"), "the column, not a measurement");
}

/// Each series holds only its own metric. Previously `effective_rank`
/// and `embedding_std` shared one series and one axis, which rendered
/// as a cliff between two things that were never comparable.
#[test]
fn unrelated_metrics_are_not_concatenated() {
    let series = parsed();
    for s in &series {
        assert_eq!(s.points.len(), 3, "{} carries its three seeds", s.name);
    }
    let rank = find(&series, "effective_rank", "full");
    assert!(rank.points.iter().all(|&p| (2.0..3.0).contains(&p)));
    let std = find(&series, "embedding_std", "full");
    assert!(std.points.iter().all(|&p| (0.4..0.6).contains(&p)));
}

/// A string field beside a named measurement is a dimension, and it is
/// what tells two series of the same metric apart.
#[test]
fn string_fields_become_the_dimensions_that_distinguish_series() {
    let series = parsed();
    let variants: std::collections::BTreeSet<&str> = series
        .iter()
        .filter(|s| s.name == "effective_rank")
        .filter_map(|s| s.dimensions().get("variant").map(String::as_str))
        .collect();
    assert_eq!(
        variants.into_iter().collect::<Vec<_>>(),
        vec!["full", "no_ema", "random_init"]
    );
}

/// The rule the slice exists for. Three seeds of one cell arrived in
/// some order; nothing about that order is a measurement.
#[test]
fn a_long_format_feed_never_claims_its_points_are_a_curve() {
    assert!(parsed()
        .iter()
        .all(|s| *s.order() == SeriesOrder::Unordered));
}

/// Arc 1's conclusion, asserted by value: `full` sits almost entirely
/// inside `random_init` — the trained model is not distinguishable from
/// an untrained one on this metric — while `no_ema` separates cleanly
/// below. This is the finding the pane has to make legible, so a
/// regression that flattens the grouping fails here rather than
/// rendering a plausible picture.
#[test]
fn the_bands_that_carry_arc_ones_null_result_survive_parsing() {
    let series = parsed();
    let band = |variant: &str| {
        let points = &find(&series, "effective_rank", variant).points;
        let lo = points.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = points.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (lo, hi)
    };

    let (full_lo, full_hi) = band("full");
    let (random_lo, random_hi) = band("random_init");
    let (no_ema_lo, no_ema_hi) = band("no_ema");

    assert_eq!((full_lo, full_hi), (2.352, 2.791));
    assert_eq!((random_lo, random_hi), (2.437, 2.934));
    assert_eq!((no_ema_lo, no_ema_hi), (1.25, 1.459));

    assert!(
        full_hi > random_lo && random_hi > full_lo,
        "the null result: they overlap"
    );
    assert!(no_ema_hi < full_lo, "and stop-gradient's absence does not");
}

// ---------------------------------------------------------------------
// The wide shape, and the ordering claim it can support.
// ---------------------------------------------------------------------

#[test]
fn a_wide_feed_with_a_monotonic_step_field_is_ordered_by_it() {
    let text = "{\"step\": 0, \"loss\": 2.0}\n{\"step\": 1, \"loss\": 1.0}\n";
    let series = parse_metrics(text);
    assert!(series
        .iter()
        .all(|s| *s.order() == SeriesOrder::By("step".into())));
}

/// A field missing from one record in the file cannot order the file.
/// The claim is that successive points are successive steps, and a gap
/// is exactly the case where that stops being true.
#[test]
fn an_ordering_field_absent_from_any_record_orders_nothing() {
    let text = "{\"step\": 0, \"loss\": 2.0}\n{\"loss\": 1.5}\n{\"step\": 2, \"loss\": 1.0}\n";
    let series = parse_metrics(text);
    assert!(series.iter().all(|s| *s.order() == SeriesOrder::Unordered));
}

#[test]
fn a_step_field_that_goes_backwards_orders_nothing() {
    let text = "{\"step\": 0, \"loss\": 2.0}\n{\"step\": 5, \"loss\": 1.5}\n{\"step\": 2, \"loss\": 1.0}\n";
    let series = parse_metrics(text);
    assert!(series.iter().all(|s| *s.order() == SeriesOrder::Unordered));
}

/// One record cannot establish an order — there is no "successive"
/// with nothing to succeed. It would otherwise pass the monotonic
/// check vacuously.
#[test]
fn a_single_record_claims_no_ordering() {
    let series = parse_metrics("{\"step\": 0, \"loss\": 2.0}\n");
    assert!(series.iter().all(|s| *s.order() == SeriesOrder::Unordered));
}

/// A wide feed with no ordering field at all is a bag of numbers that
/// arrived. That is not a curve either.
#[test]
fn a_wide_feed_without_an_ordering_field_claims_nothing() {
    let text = "{\"loss\": 2.0}\n{\"loss\": 1.0}\n";
    let series = parse_metrics(text);
    assert!(series.iter().all(|s| *s.order() == SeriesOrder::Unordered));
}

/// `epoch` and `iteration` are the other two names a real producer
/// uses for the same thing.
#[test]
fn epoch_and_iteration_order_a_feed_the_same_way_step_does() {
    for field in ["epoch", "iteration"] {
        let text =
            format!("{{\"{field}\": 0, \"loss\": 2.0}}\n{{\"{field}\": 1, \"loss\": 1.0}}\n");
        let series = parse_metrics(&text);
        assert!(series
            .iter()
            .all(|s| *s.order() == SeriesOrder::By(field.into())));
    }
}

/// A wide feed has no dimensions to express, and must not grow empty
/// ones that a pane would then render as a blank label.
#[test]
fn a_wide_feed_carries_no_dimensions() {
    let series = parse_metrics("{\"step\": 0, \"loss\": 2.0}\n{\"step\": 1, \"loss\": 1.0}\n");
    assert!(series.iter().all(|s| s.dimensions().is_empty()));
}
