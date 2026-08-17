//! The metrics artifact adapter, replayed against a sample JSONL feed.

use parallax_baseline::adapters::artifact::{
    parse_metrics, ArtifactAdapter, ArtifactDetail, MetricsArtifactAdapter,
};
use parallax_baseline::adapters::ProjectContext;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/metrics/loss.jsonl")
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// Copies the fixture into a Model-Experiments-shaped project tree.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("projects/spectral/results/run7");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::copy(fixture(), target.join("loss.jsonl")).unwrap();
    dir
}

#[test]
fn every_numeric_key_becomes_a_named_series_sorted_by_name() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let series = parse_metrics(&text);
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["loss", "probe_acc", "spectral_err", "step"]);
}

/// The fixture's loss curve happens to pass near `e` and `√2`, which is
/// what `approx_constant` fires on — these are recorded sample data, not
/// a misspelled mathematical constant.
#[test]
#[allow(clippy::approx_constant)]
fn a_series_carries_its_points_in_record_order() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let series = parse_metrics(&text);
    let loss = series.iter().find(|s| s.name == "loss").unwrap();
    assert_eq!(loss.points, vec![2.7183, 2.1041, 1.6180, 1.4142, 1.2020]);
}

/// Real producers emit ragged records. A key appearing only in the last
/// line yields a one-point series rather than four fabricated zeros.
#[test]
fn a_key_present_in_only_some_records_yields_only_the_points_it_had() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let series = parse_metrics(&text);
    let spectral = series.iter().find(|s| s.name == "spectral_err").unwrap();
    assert_eq!(spectral.points, vec![0.008], "no interpolation, no padding");
}

#[test]
fn non_numeric_fields_are_skipped_rather_than_coerced_or_fatal() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let series = parse_metrics(&text);
    assert!(series.iter().all(|s| s.name != "note"));
}

#[test]
fn a_malformed_line_is_skipped_and_the_rest_of_the_file_still_parses() {
    let text = "{\"loss\": 1.0}\nnot json at all\n{\"loss\": 0.5}\n";
    let series = parse_metrics(text);
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].points, vec![1.0, 0.5]);
}

#[test]
fn blank_lines_are_ignored() {
    assert_eq!(parse_metrics("\n\n{\"a\": 1}\n\n").len(), 1);
}

#[test]
fn an_empty_file_yields_no_series_rather_than_an_error() {
    assert!(parse_metrics("").is_empty());
}

#[test]
fn the_adapter_finds_metrics_files_through_the_manifest_s_watch_glob() {
    let dir = project();
    let mut a = MetricsArtifactAdapter::new("projects/*/results/**/*.jsonl");
    let artifacts = a
        .scan(&ProjectContext::new("model-experiments", dir.path()), at(0))
        .unwrap()
        .value;
    assert_eq!(artifacts.len(), 1);
    match &artifacts[0].detail {
        ArtifactDetail::Metrics { series } => assert_eq!(series.len(), 4),
        other => panic!("expected metrics, got {other:?}"),
    }
}

#[test]
fn a_project_with_no_metrics_files_yields_an_empty_scan() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = MetricsArtifactAdapter::new("projects/*/results/**/*.jsonl");
    let artifacts = a
        .scan(&ProjectContext::new("model-experiments", dir.path()), at(0))
        .unwrap()
        .value;
    assert!(artifacts.is_empty());
}
