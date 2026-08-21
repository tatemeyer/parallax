//! A long-format **CSV** metrics feed, and the one thing the format
//! change costs.
//!
//! `fixtures/metrics/sweep.csv` and `fixtures/metrics/sweep.jsonl` are
//! the *same 27 measurements* — the JSONL is a projection of the CSV,
//! made by the repository that owns them because until this adapter
//! existed a CSV could not be declared. So the test that matters here
//! is that the two read the same, which is what lets that repository
//! delete the projection.

use parallax_baseline::adapters::artifact::{
    parse_metrics, parse_metrics_csv, Series, SeriesOrder,
};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/metrics")
        .join(name)
}

fn read(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).unwrap()
}

/// `seed` is the column that enumerates the repeats of a cell. Dropping
/// it is what collapses three runs into one series carrying their
/// spread.
fn seed() -> Vec<String> {
    vec!["seed".to_string()]
}

fn csv() -> Vec<Series> {
    parse_metrics_csv(&read("sweep.csv"), &seed()).expect("the fixture is long-format csv")
}

/// A series' identity for comparison across the two formats: what was
/// measured, and the measurements. Deliberately **not** the dimensions —
/// see `the_dimensions_differ_and_that_is_the_cost`.
fn content(series: &[Series]) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = series
        .iter()
        .map(|s| {
            let mut points: Vec<String> = s.points.iter().map(|p| format!("{p:.6}")).collect();
            points.sort();
            (s.name.clone(), points)
        })
        .collect();
    out.sort();
    out
}

/// **The reason this adapter exists.** The CSV and the projection made
/// from it carry the same measurements, so a producer keeping tidy long
/// CSV can declare the file it already has instead of maintaining a
/// second copy of facts it already had.
#[test]
fn a_csv_feed_and_the_jsonl_projection_of_it_read_the_same() {
    let from_csv = csv();
    let from_jsonl = parse_metrics(&read("sweep.jsonl"));
    assert_eq!(from_csv.len(), 9, "3 metrics x 3 variants");
    assert_eq!(content(&from_csv), content(&from_jsonl));
}

/// The cost, stated rather than hidden. The projection explodes the
/// CSV's `params` column into one field per parameter and drops `date`;
/// read straight, those are one opaque dimension and one more. The
/// *grouping* is identical — `params` partitions exactly as the fields
/// it encodes do — but a row's label is not.
///
/// A producer that wants the tidier labels flattens its CSV. That is a
/// choice it can now make, which is the whole difference from before.
#[test]
fn the_dimensions_differ_and_that_is_the_cost() {
    let by_variant = |series: &[Series], variant: &str| -> Vec<String> {
        series
            .iter()
            .find(|s| {
                s.name == "effective_rank"
                    && s.dimensions().get("variant").map(String::as_str) == Some(variant)
            })
            .expect("a row for this variant")
            .dimensions()
            .keys()
            .cloned()
            .collect()
    };
    let from_csv = by_variant(&csv(), "full");
    let from_jsonl = by_variant(&parse_metrics(&read("sweep.jsonl")), "full");

    assert!(from_csv.contains(&"params".to_string()));
    assert!(from_csv.contains(&"date".to_string()));
    assert!(
        !from_jsonl.contains(&"params".to_string()),
        "the projection exploded it"
    );
    assert_ne!(from_csv, from_jsonl, "the dimensions are not the same set");
}

/// **The rule the manifest exists to state.** Undeclared, `seed` is
/// just another column, and every measurement becomes its own series —
/// 27 one-point series where there were 9 bands. Nothing in the file
/// says which of those is right.
#[test]
fn an_undeclared_identifier_column_shatters_every_cell() {
    let shattered = parse_metrics_csv(&read("sweep.csv"), &[]).unwrap();
    assert_eq!(shattered.len(), 27, "one series per row");
    assert!(
        shattered.iter().all(|s| s.points.len() == 1),
        "a one-point series cannot show a spread"
    );
    assert_eq!(csv().len(), 9, "declared, the repeats collapse");
}

/// A quoted cell holding commas is one value. The JEPA feed's `params`
/// column is embedded JSON in every row, so this is not a corner case
/// in the file this was written for.
#[test]
fn a_quoted_cell_holding_commas_is_one_dimension() {
    let params = csv()
        .iter()
        .find(|s| s.name == "effective_rank")
        .expect("a row")
        .dimensions()
        .get("params")
        .cloned()
        .expect("params survived as a dimension");
    assert!(params.starts_with('{') && params.ends_with('}'), "{params}");
    assert!(params.contains(','), "the commas are inside the value");
}

/// Coverage in a real sweep is ragged: a column belongs to the
/// experiment that varied it, and a CSV spells "not this row" with a
/// blank. `momentum=` is not a value anybody measured.
#[test]
fn an_empty_cell_is_an_absent_dimension_rather_than_an_empty_one() {
    let text = "variant,momentum,metric,value\na,0.9,r,1.0\nb,,r,2.0\n";
    let series = parse_metrics_csv(text, &[]).unwrap();
    assert_eq!(series.len(), 2);
    let b = series
        .iter()
        .find(|s| s.dimensions().get("variant").map(String::as_str) == Some("b"))
        .unwrap();
    assert!(
        !b.dimensions().contains_key("momentum"),
        "got {:?}",
        b.dimensions()
    );
}

/// One unusable row is not the file's fate — the same trade the JSONL
/// reader makes for an unparseable line.
#[test]
fn a_row_whose_value_is_not_a_number_is_skipped_rather_than_fatal() {
    let text = "variant,metric,value\na,r,1.0\nb,r,not-a-number\nc,r,3.0\n";
    let series = parse_metrics_csv(text, &[]).unwrap();
    let variants: Vec<&str> = series
        .iter()
        .filter_map(|s| s.dimensions().get("variant").map(String::as_str))
        .collect();
    assert_eq!(variants, ["a", "c"]);
}

/// A header that is not long-format is a **declaration that does not
/// match its file**, and an empty feed is the silence this adapter was
/// added to end. The complaint names the columns the file does have,
/// because "needs a `metric` column" is not actionable on its own.
#[test]
fn a_header_that_is_not_long_format_is_an_error_naming_what_it_has() {
    let text = "step,loss,probe_acc\n0,2.7,0.11\n1,2.1,0.19\n";
    let problem = parse_metrics_csv(text, &[])
        .expect_err("a wide csv is not a long-format feed")
        .to_string();
    assert!(problem.contains("metric"), "got {problem}");
    assert!(problem.contains("step, loss, probe_acc"), "got {problem}");
}

/// Record order in a CSV is the writing loop's nesting exactly as it is
/// in JSONL, and the reader gets there through the same grouping — so
/// the claim is the same one.
#[test]
fn a_long_format_csv_never_claims_its_points_are_a_curve() {
    assert!(csv().iter().all(|s| *s.order() == SeriesOrder::Unordered));
}

/// The bands Arc 1 concluded in, read off the CSV this time.
#[test]
fn the_bands_that_carry_arc_ones_null_result_survive_the_csv_reader() {
    let series = csv();
    let band = |variant: &str| -> (f64, f64) {
        let s = series
            .iter()
            .find(|s| {
                s.name == "effective_rank"
                    && s.dimensions().get("variant").map(String::as_str) == Some(variant)
            })
            .unwrap_or_else(|| panic!("no effective_rank for {variant}"));
        assert_eq!(s.points.len(), 3, "three seeds, one series");
        let lo = s.points.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = s.points.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (lo, hi)
    };
    let full = band("full");
    let random = band("random_init");
    let no_ema = band("no_ema");

    assert!(
        full.0 <= random.1 && random.0 <= full.1,
        "full {full:?} and random_init {random:?} overlap — that is the finding"
    );
    assert!(no_ema.1 < full.0, "no_ema {no_ema:?} separates below");
}
