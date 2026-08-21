//! The artifacts pane: what runs produced.

use parallax_baseline::adapters::artifact::ArtifactDetail;
use parallax_baseline::adapters::verification::VerificationOutcome;
use parallax_baseline::state::ProjectState;

/// What a row is, so a key that rules on a finding can tell whether it
/// is pointing at one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RowOf {
    /// An artifact: a figure, a metrics feed, a run.
    #[default]
    Artifact,
    /// A finding inside the run above it, addressed by fingerprint.
    Finding {
        /// The finding this row rules on.
        fingerprint: String,
    },
}

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
    /// Whether this row is an artifact or one of a run's findings.
    pub of: RowOf,
}

/// Every artifact every declared feed reported.
pub fn artifact_rows(project: &ProjectState) -> Vec<ArtifactRow> {
    project
        .artifacts
        .iter()
        .flat_map(|feed| feed.value.iter())
        .flat_map(|artifact| {
            let name = artifact
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match &artifact.detail {
                ArtifactDetail::Figure { bytes } => vec![ArtifactRow {
                    name,
                    kind: "figure",
                    summary: format!("{bytes} bytes"),
                    series: Vec::new(),
                    of: RowOf::Artifact,
                }],
                ArtifactDetail::Capture {
                    run_id,
                    outcome,
                    findings,
                } => {
                    // The run, then what it found. A finding is a row of
                    // its own because a ruling has to be able to point
                    // at one, and because a verdict word with the
                    // findings folded away is the summary that made
                    // this pane useless to act from.
                    let mut rows = vec![ArtifactRow {
                        name,
                        kind: "capture",
                        summary: format!("{run_id} — {}", verdict_word(*outcome)),
                        series: Vec::new(),
                        of: RowOf::Artifact,
                    }];
                    rows.extend(findings.iter().map(|f| ArtifactRow {
                        name: String::new(),
                        kind: "  finding",
                        summary: format!("{:<8} {:<9} {}", f.severity, f.lens, f.claim),
                        series: Vec::new(),
                        of: RowOf::Finding {
                            fingerprint: f.fingerprint.clone(),
                        },
                    }));
                    rows
                }
                ArtifactDetail::Metrics { series } => {
                    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
                    // The first series gets the sparkline; the rest are
                    // named, so nothing is silently dropped.
                    let points = series
                        .first()
                        .map(|s| s.points.iter().map(|p| *p as f32).collect())
                        .unwrap_or_default();
                    vec![ArtifactRow {
                        name,
                        kind: "metrics",
                        summary: names.join(", "),
                        series: points,
                        of: RowOf::Artifact,
                    }]
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
    use parallax_baseline::adapters::artifact::{Artifact, RunFinding, Series};
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
                        findings: Vec::new(),
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
                        findings: Vec::new(),
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
                            Series::ordered("loss", vec![2.7, 2.1, 1.6], "step"),
                            Series::ordered("probe_acc", vec![0.1, 0.2], "step"),
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

    fn finding(fingerprint: &str, lens: &str, severity: &str, claim: &str) -> RunFinding {
        RunFinding {
            fingerprint: fingerprint.into(),
            lens: lens.into(),
            severity: severity.into(),
            claim: claim.into(),
        }
    }

    fn run_with(findings: Vec<RunFinding>) -> ProjectState {
        project_with(|p| {
            p.artifacts.push(watched(
                vec![artifact(
                    "20260820T020000Z",
                    ArtifactKind::Capture,
                    ArtifactDetail::Capture {
                        run_id: "20260820T020000Z".into(),
                        outcome: VerificationOutcome::Pass,
                        findings: findings.clone(),
                    },
                )],
                at(0),
            ));
        })
    }

    /// Each finding is a row of its own, because a ruling has to point
    /// at one. A verdict word with the findings folded away is the
    /// summary that made this pane impossible to act from.
    #[test]
    fn a_runs_findings_are_rows_beneath_it() {
        let p = run_with(vec![
            finding(
                "abc123",
                "intent",
                "major",
                "only two em-dash columns appear",
            ),
            finding(
                "def456",
                "motion",
                "minor",
                "the last two frames are identical",
            ),
        ]);
        let rows = artifact_rows(&p);
        assert_eq!(rows.len(), 3, "the run and its two findings");
        assert_eq!(rows[0].of, RowOf::Artifact);
        assert_eq!(
            rows[1].of,
            RowOf::Finding {
                fingerprint: "abc123".into()
            }
        );
        assert!(rows[1].summary.contains("major"));
        assert!(rows[1].summary.contains("intent"));
        assert!(rows[1].summary.contains("em-dash"));
        assert_eq!(
            rows[2].of,
            RowOf::Finding {
                fingerprint: "def456".into()
            }
        );
    }

    /// A run that found nothing is one row. An empty findings list must
    /// not render as a finding with nothing in it.
    #[test]
    fn a_run_with_no_findings_is_one_row() {
        assert_eq!(artifact_rows(&run_with(Vec::new())).len(), 1);
    }

    /// The order is the run's. Plumb ranks findings by severity when it
    /// merges them, and re-sorting here would quietly disagree with the
    /// verdict file the operator can also read.
    #[test]
    fn findings_keep_the_order_the_run_ranked_them_in() {
        let p = run_with(vec![
            finding("1", "breakage", "minor", "first"),
            finding("2", "intent", "major", "second"),
        ]);
        let rows = artifact_rows(&p);
        assert!(rows[1].summary.contains("first"));
        assert!(rows[2].summary.contains("second"));
    }
}
