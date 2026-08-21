//! CHARACTERIZATION — what `parse_metrics` does to a long-format feed
//! *today*, before Slice 1 fixes it.
//!
//! Every assertion in this file is a bug. They are written down so the
//! commit that fixes them shows the change rather than looking like a
//! refactor, and this whole file is deleted in the same commit that
//! makes it false.
//!
//! The fixture is real: 27 rows of `projects/jepa/results.csv` from
//! Model-Experiments, the record Arc 1 concluded in.

use parallax_baseline::adapters::artifact::parse_metrics;
use std::path::{Path, PathBuf};

fn sweep() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/metrics/sweep.jsonl")
}

fn parsed() -> Vec<parallax_baseline::adapters::artifact::Series> {
    parse_metrics(&std::fs::read_to_string(sweep()).unwrap())
}

/// The whole defect in one assertion: three series, none of which is a
/// metric this experiment measured.
#[test]
fn today_the_series_are_two_identifiers_and_one_undifferentiated_heap() {
    let series = parsed();
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["issue", "seed", "value"]);
}

/// An issue number, charted. It renders as a flat line at 69 — which
/// under the spec's own rule is the strong claim that something was
/// measured repeatedly and did not change.
#[test]
fn today_an_issue_number_is_charted_as_a_measurement() {
    let issue = parsed().into_iter().find(|s| s.name == "issue").unwrap();
    assert_eq!(issue.points.len(), 27);
    assert!(issue.points.iter().all(|&p| p == 69.0));
}

/// A seed index, charted. It renders as a sawtooth.
#[test]
fn today_a_seed_index_is_charted_as_a_measurement() {
    let seed = parsed().into_iter().find(|s| s.name == "seed").unwrap();
    assert_eq!(&seed.points[..6], &[0.0, 1.0, 2.0, 0.0, 1.0, 2.0]);
}

/// The costly one. `effective_rank` (1.25..2.93), `embedding_std`
/// (0.49..0.53) and `probe_r2_superseded_104` land in one series on one
/// axis, in the order the writing loop happened to nest. It renders as a
/// metric that fell off a cliff at point 9. There is no cliff.
#[test]
fn today_unrelated_metrics_are_concatenated_into_one_series() {
    let value = parsed().into_iter().find(|s| s.name == "value").unwrap();
    assert_eq!(value.points.len(), 27, "every measurement, heaped together");

    // Points 0..9 are effective_rank; 9..18 are embedding_std. The step
    // between them is an artifact of the loop, not of anything measured.
    assert_eq!(value.points[8], 2.437, "last effective_rank");
    assert_eq!(value.points[9], 0.4951, "first embedding_std — the 'cliff'");
}

/// The two columns the finding actually lives in are dropped, because
/// they are strings. Without `metric` there is nothing to group by, and
/// without `variant` the comparison Arc 1 concluded from is not
/// expressible at all.
#[test]
fn today_the_dimensions_that_carry_the_finding_are_discarded() {
    let series = parsed();
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert!(!names.contains(&"metric"));
    assert!(!names.contains(&"variant"));
    assert!(!names.contains(&"effective_rank"));
}
