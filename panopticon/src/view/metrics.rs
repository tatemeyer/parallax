//! The metrics pane: what the experiments measured.
//!
//! The first pane in this cockpit that is not about software being
//! built. Every other one answers a question about development — what
//! is in flight, what is green, what an agent is doing. This one shows
//! a project whose *output* is the point.
//!
//! **A metric has two ages, and both belong on screen.** How long ago
//! this machine read the feed, and how long ago the producer last
//! wrote it. A curve read two seconds ago from a run that died an hour
//! back is fresh and stalled at once, and a pane showing one age cannot
//! say so. See `parallax_baseline::wire::ArtifactWire` for why the
//! second one is trustworthy across machines.
//!
//! **A curve is a claim, and this pane does not get to make it.**
//! Whether points may be drawn left to right is decided where the feed
//! is read, and arrives as `SeriesOrder`. Three seeds of one sweep cell
//! are not a curve; drawing them as one asserts a progression that
//! nothing measured.

use parallax_baseline::adapters::artifact::{ArtifactDetail, Series, SeriesOrder};
use parallax_baseline::state::ProjectState;
use std::time::{Duration, SystemTime};

/// How one row's points may be shown.
#[derive(Debug, Clone, PartialEq)]
pub enum RowShape {
    /// Ordered along `by`: successive points are successive values of
    /// it, so a curve is a claim the feed supports.
    Curve {
        /// The field that orders them.
        by: String,
        /// The points, in that order.
        points: Vec<f64>,
    },
    /// Repeated measurements of one configuration. Nothing orders
    /// them, so what can honestly be shown is where they fell.
    ///
    /// A non-effect is invisible without this: when the spread within a
    /// configuration is as wide as the difference between
    /// configurations, the levers did nothing — which is exactly what
    /// this platform's first real consumer concluded.
    Spread {
        /// The smallest measurement.
        min: f64,
        /// A measurement in the middle. **Always one that was actually
        /// taken** — for an even count this is the lower of the two
        /// middle values rather than their mean, because every number
        /// on this pane should be one somebody measured.
        median: f64,
        /// The largest measurement.
        max: f64,
    },
    /// Exactly one measurement.
    ///
    /// Deliberately not a one-point curve and not a zero-width spread.
    /// A flat line says "measured repeatedly, did not change", which is
    /// a far stronger claim than "measured once".
    Single(f64),
    /// The series parsed and held no points.
    ///
    /// Not zero. Zero is a measurement.
    Empty,
}

/// One row: a single series within a metric.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricRow {
    /// What distinguishes this row from others of the same metric —
    /// `variant=full`. Empty when the feed had no dimensions to give.
    pub label: String,
    /// How its points may be shown.
    pub shape: RowShape,
    /// How many points it holds.
    ///
    /// On screen beside the row because a sparkline over four points
    /// and one over four thousand look identical and are not.
    pub points: usize,
}

/// Every series that measured one metric.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricGroup {
    /// The metric's name.
    pub name: String,
    /// Its series, in the order the feed reported them.
    pub rows: Vec<MetricRow>,
    /// The scale every row in this group is drawn against, as
    /// `(min, max)` over all of their points.
    ///
    /// **Per metric, never across.** Overlap between two variants is
    /// only readable on a shared scale, and `effective_rank` (~2.5)
    /// sharing one with `embedding_std` (~0.5) would rebuild in the
    /// renderer the same defect the parser was fixed for.
    ///
    /// `None` when the group holds no points to scale.
    pub axis: Option<(f64, f64)>,
}

/// One metrics file, and what it measured.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricFeed {
    /// The file's name.
    pub name: String,
    /// How long ago **this machine** read it.
    pub read: Duration,
    /// How long ago the **producer** last wrote it, or `None` when
    /// nobody could say.
    pub produced: Option<Duration>,
    /// One group per metric, sorted by name.
    pub groups: Vec<MetricGroup>,
}

/// Every metrics feed this project declared, and what it holds.
pub fn metric_feeds(project: &ProjectState, now: SystemTime) -> Vec<MetricFeed> {
    let mut feeds = Vec::new();
    for observed in &project.artifacts {
        let read = observed.age(now);
        for artifact in &observed.value {
            let ArtifactDetail::Metrics { series } = &artifact.detail else {
                continue;
            };
            feeds.push(MetricFeed {
                name: artifact
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                read,
                produced: artifact
                    .modified
                    .and_then(|modified| now.duration_since(modified).ok()),
                groups: group(series),
            });
        }
    }
    feeds
}

/// Collects series into one group per metric, preserving feed order
/// within a group. `parse_metrics` already sorts by name then
/// dimensions, so variants of one metric arrive adjacent.
fn group(series: &[Series]) -> Vec<MetricGroup> {
    let mut groups: Vec<MetricGroup> = Vec::new();
    for s in series {
        let row = row_of(s);
        match groups.last_mut() {
            Some(group) if group.name == s.name => group.rows.push(row),
            _ => groups.push(MetricGroup {
                name: s.name.clone(),
                rows: vec![row],
                axis: None,
            }),
        }
    }
    for group in &mut groups {
        group.axis = axis_of(group);
    }
    groups
}

/// The shared scale for one group: the extent of every point in it.
fn axis_of(group: &MetricGroup) -> Option<(f64, f64)> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for row in &group.rows {
        for point in row_points(&row.shape) {
            min = min.min(point);
            max = max.max(point);
        }
    }
    (min <= max).then_some((min, max))
}

/// Every value a row holds, whatever shape it took.
fn row_points(shape: &RowShape) -> Vec<f64> {
    match shape {
        RowShape::Curve { points, .. } => points.clone(),
        RowShape::Spread { min, median, max } => vec![*min, *median, *max],
        RowShape::Single(value) => vec![*value],
        RowShape::Empty => Vec::new(),
    }
}

/// Chooses a row's shape from what the feed was willing to claim.
fn row_of(series: &Series) -> MetricRow {
    let label = series
        .dimensions()
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" ");

    let shape = match (series.points.len(), series.order()) {
        (0, _) => RowShape::Empty,
        (1, _) => RowShape::Single(series.points[0]),
        (_, SeriesOrder::By(by)) => RowShape::Curve {
            by: by.clone(),
            points: series.points.clone(),
        },
        (_, SeriesOrder::Unordered) => spread(&series.points),
    };

    MetricRow {
        label,
        shape,
        points: series.points.len(),
    }
}

/// Where a set of unordered measurements fell.
fn spread(points: &[f64]) -> RowShape {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    RowShape::Spread {
        min: sorted[0],
        // The lower of the two middles on an even count. See
        // `RowShape::Spread::median`.
        median: sorted[(sorted.len() - 1) / 2],
        max: sorted[sorted.len() - 1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::test_support::*;
    use parallax_baseline::adapters::artifact::Artifact;
    use parallax_baseline::manifest::ArtifactKind;

    fn feed(series: Vec<Series>, modified: Option<SystemTime>) -> Artifact {
        Artifact {
            path: std::path::PathBuf::from("/tmp/results/run7/loss.jsonl"),
            kind: ArtifactKind::Metrics,
            modified,
            detail: ArtifactDetail::Metrics { series },
        }
    }

    fn project(series: Vec<Series>, modified: Option<SystemTime>) -> ProjectState {
        project_with(|p| {
            p.artifacts
                .push(watched(vec![feed(series, modified)], at(0)));
        })
    }

    /// Three seeds of one sweep cell, the shape Model-Experiments'
    /// results actually take.
    fn seeds(name: &str, variant: &str, points: Vec<f64>) -> Series {
        Series::unordered(name, points).with_dimensions([("variant", variant)])
    }

    #[test]
    fn a_project_with_no_artifact_feeds_has_no_metrics() {
        assert!(metric_feeds(&bare_project("p"), at(0)).is_empty());
    }

    /// A figure or a capture is not a metrics feed and must not become
    /// an empty one.
    #[test]
    fn a_non_metrics_artifact_contributes_nothing() {
        let p = project_with(|p| {
            p.artifacts.push(watched(
                vec![Artifact {
                    path: std::path::PathBuf::from("/tmp/loss.png"),
                    kind: ArtifactKind::Figure,
                    modified: Some(at(0)),
                    detail: ArtifactDetail::Figure { bytes: 12 },
                }],
                at(0),
            ));
        });
        assert!(metric_feeds(&p, at(0)).is_empty());
    }

    // -----------------------------------------------------------------
    // The two ages.
    // -----------------------------------------------------------------

    /// The pane the rule exists for: a feed read two seconds ago whose
    /// producer stopped an hour back is fresh and stalled at once.
    #[test]
    fn a_feed_read_recently_from_a_run_that_stopped_reports_both_ages() {
        let p = project(
            vec![Series::ordered("loss", vec![2.0, 1.0], "step")],
            Some(at(0) - Duration::from_secs(3600)),
        );
        let feeds = metric_feeds(&p, at(2));

        assert_eq!(feeds[0].read, Duration::from_secs(2));
        assert_eq!(feeds[0].produced, Some(Duration::from_secs(3602)));
    }

    /// A producer that could not say is rendered as unknown, not as
    /// zero and not as 1970.
    #[test]
    fn a_producer_age_nobody_could_supply_is_unknown() {
        let p = project(vec![Series::ordered("loss", vec![2.0, 1.0], "step")], None);
        assert_eq!(metric_feeds(&p, at(0))[0].produced, None);
    }

    // -----------------------------------------------------------------
    // Row shapes.
    // -----------------------------------------------------------------

    #[test]
    fn an_ordered_series_may_be_drawn_as_a_curve() {
        let p = project(
            vec![Series::ordered("loss", vec![2.0, 1.5, 1.0], "step")],
            None,
        );
        let row = &metric_feeds(&p, at(0))[0].groups[0].rows[0];
        assert_eq!(
            row.shape,
            RowShape::Curve {
                by: "step".into(),
                points: vec![2.0, 1.5, 1.0]
            }
        );
        assert_eq!(row.points, 3);
    }

    /// The rule the whole arc turns on. Three seeds arrived in some
    /// order; drawing them left to right would assert a progression
    /// nothing measured.
    #[test]
    fn an_unordered_series_becomes_a_spread_not_a_curve() {
        let p = project(
            vec![seeds("effective_rank", "full", vec![2.779, 2.352, 2.791])],
            None,
        );
        let row = &metric_feeds(&p, at(0))[0].groups[0].rows[0];
        assert_eq!(
            row.shape,
            RowShape::Spread {
                min: 2.352,
                median: 2.779,
                max: 2.791
            }
        );
    }

    /// Every number shown is one that was measured — no invented mean
    /// on an even count.
    #[test]
    fn a_median_is_always_a_measurement_that_was_taken() {
        let p = project(vec![seeds("r", "v", vec![4.0, 1.0, 3.0, 2.0])], None);
        let RowShape::Spread { median, .. } = metric_feeds(&p, at(0))[0].groups[0].rows[0].shape
        else {
            panic!("expected a spread");
        };
        assert!(
            [1.0, 2.0, 3.0, 4.0].contains(&median),
            "{median} was never measured"
        );
        assert_eq!(median, 2.0, "the lower middle, not 2.5");
    }

    /// One point is not a curve: a flat line claims something was
    /// measured repeatedly and did not change.
    #[test]
    fn a_single_point_is_a_value_rather_than_a_flat_line() {
        let p = project(vec![Series::ordered("loss", vec![1.5], "step")], None);
        assert_eq!(
            metric_feeds(&p, at(0))[0].groups[0].rows[0].shape,
            RowShape::Single(1.5)
        );
    }

    /// An empty series is not zero. Zero is a measurement.
    #[test]
    fn an_empty_series_says_so_rather_than_rendering_zero() {
        let p = project(vec![Series::unordered("loss", Vec::new())], None);
        let row = &metric_feeds(&p, at(0))[0].groups[0].rows[0];
        assert_eq!(row.shape, RowShape::Empty);
        assert_eq!(row.points, 0);
        assert_eq!(
            metric_feeds(&p, at(0))[0].groups[0].axis,
            None,
            "nothing to scale, and zero is not a floor"
        );
    }

    // -----------------------------------------------------------------
    // Grouping and the shared axis.
    // -----------------------------------------------------------------

    #[test]
    fn series_of_one_metric_group_together_and_keep_their_labels() {
        let p = project(
            vec![
                seeds("effective_rank", "full", vec![2.352, 2.791]),
                seeds("effective_rank", "no_ema", vec![1.250, 1.459]),
            ],
            None,
        );
        let groups = &metric_feeds(&p, at(0))[0].groups;
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "effective_rank");
        assert_eq!(groups[0].rows[0].label, "variant=full");
        assert_eq!(groups[0].rows[1].label, "variant=no_ema");
    }

    /// The finding, made legible. `full` overlaps `random_init` and
    /// `no_ema` does not — visible only because all three are drawn
    /// against one scale.
    #[test]
    fn a_groups_axis_spans_every_row_in_it() {
        let p = project(
            vec![
                seeds("effective_rank", "full", vec![2.352, 2.791]),
                seeds("effective_rank", "no_ema", vec![1.250, 1.459]),
                seeds("effective_rank", "random_init", vec![2.437, 2.934]),
            ],
            None,
        );
        assert_eq!(
            metric_feeds(&p, at(0))[0].groups[0].axis,
            Some((1.250, 2.934))
        );
    }

    /// Two metrics on different scales never share one. This is the
    /// concatenation defect the parser was fixed for, and it must not
    /// come back in the renderer.
    #[test]
    fn two_metrics_on_different_scales_get_their_own_axes() {
        let p = project(
            vec![
                seeds("effective_rank", "full", vec![2.352, 2.791]),
                seeds("embedding_std", "full", vec![0.4933, 0.5311]),
            ],
            None,
        );
        let groups = &metric_feeds(&p, at(0))[0].groups;
        assert_eq!(groups.len(), 2, "two metrics, two groups");
        assert_eq!(groups[0].axis, Some((2.352, 2.791)));
        assert_eq!(groups[1].axis, Some((0.4933, 0.5311)));
    }
}
