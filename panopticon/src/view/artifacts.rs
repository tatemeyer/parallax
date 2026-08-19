//! The artifacts pane: what runs produced.

use parallax_baseline::adapters::artifact::ArtifactDetail;
use parallax_baseline::adapters::verification::VerificationOutcome;
use parallax_baseline::state::ProjectState;

/// One row of the artifacts pane.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactRow {
    /// The artifact's file or directory name.
    pub name: String,
    /// What kind of thing it is, for the row's first column.
    pub kind: &'static str,
    /// A one-line summary: a verdict, a size, or the series found.
    pub summary: String,
    /// A metric series to sparkline, when this row has one.
    pub series: Vec<f32>,
}

/// Every artifact every declared feed reported.
pub fn artifact_rows(project: &ProjectState) -> Vec<ArtifactRow> {
    project
        .artifacts
        .iter()
        .flat_map(|feed| feed.value.iter())
        .map(|artifact| {
            let name = artifact
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match &artifact.detail {
                ArtifactDetail::Figure { bytes } => ArtifactRow {
                    name,
                    kind: "figure",
                    summary: format!("{bytes} bytes"),
                    series: Vec::new(),
                },
                ArtifactDetail::Capture { run_id, outcome } => ArtifactRow {
                    name,
                    kind: "capture",
                    summary: format!("{run_id} — {}", verdict_word(*outcome)),
                    series: Vec::new(),
                },
                ArtifactDetail::Metrics { series } => {
                    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
                    // The first series gets the sparkline; the rest are
                    // named, so nothing is silently dropped.
                    let points = series
                        .first()
                        .map(|s| s.points.iter().map(|p| *p as f32).collect())
                        .unwrap_or_default();
                    ArtifactRow {
                        name,
                        kind: "metrics",
                        summary: names.join(", "),
                        series: points,
                    }
                }
            }
        })
        .collect()
}

/// Plumb's own words, carried through rather than reworded.
fn verdict_word(outcome: VerificationOutcome) -> &'static str {
    match outcome {
        VerificationOutcome::Pass => "GO",
        VerificationOutcome::Fail => "NO-GO",
        VerificationOutcome::Hold => "HOLD",
        VerificationOutcome::NotRun => "in progress",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::test_support::*;
    use parallax_baseline::adapters::artifact::{Artifact, Series};
    use parallax_baseline::manifest::ArtifactKind;

    fn artifact(name: &str, kind: ArtifactKind, detail: ArtifactDetail) -> Artifact {
        Artifact {
            path: std::path::PathBuf::from("/tmp").join(name),
            kind,
            modified: at(0),
            detail,
        }
    }

    #[test]
    fn a_project_with_no_artifact_feeds_has_no_rows() {
        assert!(artifact_rows(&bare_project("p")).is_empty());
    }

    /// Plumb's verdict words survive the trip. A cockpit rendering
    /// "failed" where Plumb said NO-GO would be inventing vocabulary the
    /// rest of the platform does not use.
    #[test]
    fn a_capture_row_carries_the_run_id_and_the_verdict_word() {
        let p = project_with(|p| {
            p.artifacts.push(watched(
                vec![artifact(
                    "20260814T112200Z",
                    ArtifactKind::Capture,
                    ArtifactDetail::Capture {
                        run_id: "20260814T112200Z".into(),
                        outcome: VerificationOutcome::Fail,
                    },
                )],
                at(0),
            ));
        });
        let rows = artifact_rows(&p);
        assert_eq!(rows[0].kind, "capture");
        assert!(rows[0].summary.contains("NO-GO"), "got {}", rows[0].summary);
    }

    #[test]
    fn a_run_with_no_verdict_yet_reads_as_in_progress() {
        let p = project_with(|p| {
            p.artifacts.push(watched(
                vec![artifact(
                    "20260819T090000Z",
                    ArtifactKind::Capture,
                    ArtifactDetail::Capture {
                        run_id: "20260819T090000Z".into(),
                        outcome: VerificationOutcome::NotRun,
                    },
                )],
                at(0),
            ));
        });
        assert!(artifact_rows(&p)[0].summary.contains("in progress"));
    }

    #[test]
    fn a_metrics_row_names_every_series_and_sparklines_the_first() {
        let p = project_with(|p| {
            p.artifacts.push(watched(
                vec![artifact(
                    "loss.jsonl",
                    ArtifactKind::Metrics,
                    ArtifactDetail::Metrics {
                        series: vec![
                            Series {
                                name: "loss".into(),
                                points: vec![2.7, 2.1, 1.6],
                            },
                            Series {
                                name: "probe_acc".into(),
                                points: vec![0.1, 0.2],
                            },
                        ],
                    },
                )],
                at(0),
            ));
        });
        let rows = artifact_rows(&p);
        assert_eq!(
            rows[0].summary, "loss, probe_acc",
            "nothing silently dropped"
        );
        assert_eq!(rows[0].series.len(), 3);
    }

    #[test]
    fn every_declared_feed_contributes_its_rows() {
        let p = project_with(|p| {
            p.artifacts.push(watched(
                vec![artifact(
                    "a.png",
                    ArtifactKind::Figure,
                    ArtifactDetail::Figure { bytes: 18 },
                )],
                at(0),
            ));
            p.artifacts.push(watched(
                vec![artifact(
                    "b.png",
                    ArtifactKind::Figure,
                    ArtifactDetail::Figure { bytes: 24 },
                )],
                at(0),
            ));
        });
        assert_eq!(artifact_rows(&p).len(), 2);
    }
}
