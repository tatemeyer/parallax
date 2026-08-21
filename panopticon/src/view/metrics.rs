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
use std::collections::{BTreeMap, BTreeSet};
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

/// One line of the metrics pane, in the order the pane draws them.
///
/// Every other pane in this cockpit is a list, so its model is a `Vec`
/// of rows and `j` moves through it by index. This one is a tree — a
/// feed, its metrics, and their series — and the tree was flattened in
/// the renderer, where nothing outside could count it. The result was a
/// pane whose selection could not move: `detail_len` reported the
/// number of *feeds*, which is one.
///
/// Flattened here instead, once, so the renderer is a map over it and
/// the count is a fact about the model rather than about a `for` loop
/// inside a `format!`.
pub enum MetricLine<'a> {
    /// A feed's header: what it is called, and its two ages.
    Feed(&'a MetricFeed),
    /// A metric's header.
    Group(&'a MetricGroup),
    /// One series, and the scale its group is drawn against.
    Row {
        /// The series.
        row: &'a MetricRow,
        /// Its group's shared scale. Carried on the line because a row
        /// is drawn against its metric's extent, never its own.
        axis: Option<(f64, f64)>,
    },
}

/// Every line the metrics pane draws, in order.
pub fn metric_lines(feeds: &[MetricFeed]) -> Vec<MetricLine<'_>> {
    let mut lines = Vec::new();
    for feed in feeds {
        lines.push(MetricLine::Feed(feed));
        for group in &feed.groups {
            lines.push(MetricLine::Group(group));
            for row in &group.rows {
                lines.push(MetricLine::Row {
                    row,
                    axis: group.axis,
                });
            }
        }
    }
    lines
}

impl MetricFeed {
    /// How many series this feed holds, across every metric in it.
    ///
    /// On screen in the feed's header. A pane twenty rows tall showing
    /// the first twenty of a hundred and six is fine; a pane that does
    /// not say which of those two it is showing is not.
    pub fn series(&self) -> usize {
        self.groups.iter().map(|g| g.rows.len()).sum()
    }
}

/// Collects series into one group per metric, preserving feed order
/// within a group. `parse_metrics` already sorts by name then
/// dimensions, so variants of one metric arrive adjacent.
///
/// A group is assembled whole rather than a row at a time because a
/// row's label is not a property of that row: it is what distinguishes
/// it from the others in its group, which cannot be known one series
/// at a time. See [`label_rows`].
fn group(series: &[Series]) -> Vec<MetricGroup> {
    let mut groups: Vec<MetricGroup> = Vec::new();
    let mut start = 0;
    while start < series.len() {
        let mut end = start + 1;
        while end < series.len() && series[end].name == series[start].name {
            end += 1;
        }
        let members = &series[start..end];
        let rows = label_rows(members)
            .into_iter()
            .zip(members)
            .map(|(label, s)| row_of(s, label))
            .collect();
        let mut group = MetricGroup {
            name: series[start].name.clone(),
            rows,
            axis: None,
        };
        group.axis = axis_of(&group);
        groups.push(group);
        start = end;
    }
    groups
}

/// Labels every series in one group by **what distinguishes it** from
/// the others, most distinguishing part first.
///
/// A dimension every series in the group carries with the same value
/// distinguishes nothing, so it is dropped. So is one that only repeats
/// a distinction already made: the label carries the **smallest** set of
/// dimensions that still tells every row of the group apart, taken most
/// distinguishing first, so the part that survives being cut off at the
/// pane's edge is the part that identifies the row.
///
/// Dropping the redundant ones is not tidying. In the JEPA sweep
/// `momentum` and `variant` say the same thing — `full_m0` *is*
/// `momentum=0` — so a label that spends its width on both leaves four
/// rows reading `variant=full_m0 momentum=0 ...` and differing only in
/// the `steps` that got cut. Four rows that look identical and are not
/// is a worse pane than a long label, not a tidier one.
///
/// **The rule exists because the real feed broke the old one.** A
/// record in `Model-Experiments`' JEPA sweep carries whichever
/// parameters its experiment varied, so a series arrives with seven to
/// thirteen dimensions on it and the flat `key=value` join ran 80 to
/// 197 characters. Every row of a metric began with the same characters,
/// and the numbers and the band were pushed off the right-hand edge —
/// a pane of identical prefixes and no measurements.
///
/// **Nothing here knows what a `variant` is.** Against that feed
/// `variant` does come first in every group, because it is the column
/// that varies most. It is not named by this code, by the manifest, or
/// by the pane. A producer whose sweep varies something else gets that
/// something else first, which is the only version of this rule that
/// survives a second producer.
///
/// An **absent** dimension counts as a value of its own: a series with
/// no `momentum` differs from one with `momentum=0`, and treating the
/// gap as nothing would drop the column that says so.
///
/// A group of one has no siblings, so nothing in it is redundant and
/// every dimension is kept — the rule is vacuous there, not restrictive.
fn label_rows(series: &[Series]) -> Vec<String> {
    let mut seen: BTreeMap<&str, BTreeSet<Option<&str>>> = BTreeMap::new();
    for s in series {
        for key in s.dimensions().keys() {
            seen.entry(key.as_str()).or_default();
        }
    }
    for (key, values) in seen.iter_mut() {
        for s in series {
            values.insert(s.dimensions().get(*key).map(String::as_str));
        }
    }

    // A group of one has no siblings. Nothing in it is redundant, so
    // the rule below is vacuous and every dimension is kept.
    if series.len() < 2 {
        let all: Vec<&str> = seen.keys().copied().collect();
        return render_labels(series, &all);
    }

    let mut candidates: Vec<&str> = seen
        .iter()
        .filter(|(_, values)| values.len() > 1)
        .map(|(key, _)| *key)
        .collect();
    // Most distinguishing first; by name where two are equally so, so
    // that two runs of the same feed label their rows identically.
    candidates.sort_by(|a, b| seen[b].len().cmp(&seen[a].len()).then_with(|| a.cmp(b)));

    let mut kept: Vec<&str> = Vec::new();
    let mut distinct = partitions(series, &kept);
    for key in candidates {
        if distinct == series.len() {
            break;
        }
        kept.push(key);
        let finer = partitions(series, &kept);
        if finer > distinct {
            distinct = finer;
        } else {
            kept.pop();
        }
    }
    render_labels(series, &kept)
}

/// How many series `keys` tells apart.
fn partitions(series: &[Series], keys: &[&str]) -> usize {
    series
        .iter()
        .map(|s| {
            keys.iter()
                .map(|key| s.dimensions().get(*key).map(String::as_str))
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>()
        .len()
}

/// `key=value` for each of `keys` the series carries, in that order.
fn render_labels(series: &[Series], keys: &[&str]) -> Vec<String> {
    series
        .iter()
        .map(|s| {
            keys.iter()
                .filter_map(|key| s.dimensions().get(*key).map(|v| format!("{key}={v}")))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
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
///
/// `label` comes from [`label_rows`], which needed the whole group to
/// work it out.
fn row_of(series: &Series, label: String) -> MetricRow {
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
    // Labels: what distinguishes a row from its siblings.
    //
    // Every one of these is a rule the real Model-Experiments feed
    // forced. Before it, a label was every dimension the record
    // carried, joined — which against that feed ran to 197 characters
    // and pushed every measurement off the screen.
    // -----------------------------------------------------------------

    /// A series built the way the JEPA feed's records arrive: a metric,
    /// a variant, and whatever parameters that experiment varied.
    fn cell(name: &str, dims: &[(&str, &str)], points: Vec<f64>) -> Series {
        Series::unordered(name, points).with_dimensions(dims.iter().copied())
    }

    fn labels(series: Vec<Series>) -> Vec<String> {
        let p = project(series, None);
        metric_feeds(&p, at(0))[0].groups[0]
            .rows
            .iter()
            .map(|r| r.label.clone())
            .collect()
    }

    /// The whole group carries `experiment_slug=001`, so it says nothing
    /// about which row this is.
    #[test]
    fn a_dimension_every_row_shares_is_not_part_of_a_label() {
        assert_eq!(
            labels(vec![
                cell(
                    "r",
                    &[("experiment_slug", "001"), ("variant", "full")],
                    vec![2.0, 3.0]
                ),
                cell(
                    "r",
                    &[("experiment_slug", "001"), ("variant", "no_ema")],
                    vec![1.0, 1.5]
                ),
            ]),
            ["variant=full", "variant=no_ema"]
        );
    }

    /// The one that matters most in practice. `full_m0` **is**
    /// `momentum=0`, so a label carrying both spends its width twice on
    /// the same fact and loses the `steps` that actually tells the four
    /// rows apart.
    #[test]
    fn a_dimension_that_only_repeats_a_distinction_already_made_is_dropped() {
        let rows = labels(vec![
            cell(
                "r",
                &[("variant", "full_m0"), ("momentum", "0"), ("steps", "300")],
                vec![1.0, 2.0],
            ),
            cell(
                "r",
                &[("variant", "full_m0"), ("momentum", "0"), ("steps", "3000")],
                vec![1.0, 2.0],
            ),
            cell(
                "r",
                &[
                    ("variant", "full_m9"),
                    ("momentum", "0.9"),
                    ("steps", "300"),
                ],
                vec![1.0, 2.0],
            ),
            cell(
                "r",
                &[
                    ("variant", "full_m9"),
                    ("momentum", "0.9"),
                    ("steps", "3000"),
                ],
                vec![1.0, 2.0],
            ),
        ]);
        for row in &rows {
            assert!(
                row.contains("variant=") != row.contains("momentum="),
                "one of the two redundant columns should be gone, not both or neither: {row}"
            );
            assert!(
                row.contains("steps="),
                "the column that actually tells them apart is gone: {row}"
            );
        }
        assert_eq!(
            rows.iter().collect::<std::collections::BTreeSet<_>>().len(),
            4,
            "four rows, four labels"
        );
        // Which of the two survives is decided by how much each varies,
        // and by name where — as here — they vary exactly as much. In
        // the real feed `variant` takes eighteen values to `momentum`'s
        // seven, so it is `momentum` that goes.
    }

    /// A record carries whichever parameters its experiment varied, so
    /// a dimension can be **absent** — and absence is a distinction. A
    /// run with no `momentum` is not the same cell as one with
    /// `momentum=0`.
    #[test]
    fn a_dimension_a_row_does_not_carry_is_itself_a_distinction() {
        let rows = labels(vec![
            cell(
                "r",
                &[("variant", "sweep"), ("momentum", "0")],
                vec![1.0, 2.0],
            ),
            cell("r", &[("variant", "sweep")], vec![3.0, 4.0]),
        ]);
        assert_eq!(rows, ["momentum=0", ""]);
    }

    /// **Nothing in this crate knows what a `variant` is.** It leads the
    /// label here because it is the column that varies most; rename it
    /// and the rule still puts it first, which is the only version of
    /// this that survives a second producer.
    #[test]
    fn the_dimension_that_varies_most_leads_the_label() {
        let rows = labels(vec![
            cell("r", &[("knob", "a"), ("steps", "10")], vec![1.0, 2.0]),
            cell("r", &[("knob", "b"), ("steps", "10")], vec![1.0, 2.0]),
            cell("r", &[("knob", "c"), ("steps", "20")], vec![1.0, 2.0]),
        ]);
        for row in &rows {
            assert!(row.starts_with("knob="), "got {row}");
        }
    }

    /// A group of one has no siblings, so nothing in it is redundant.
    /// The rule is vacuous there rather than restrictive — dropping
    /// every dimension would leave the row labelled with an em dash.
    #[test]
    fn a_group_of_one_keeps_the_dimensions_it_has() {
        assert_eq!(
            labels(vec![cell(
                "r",
                &[("experiment_slug", "001"), ("variant", "full")],
                vec![1.0, 2.0]
            )]),
            ["experiment_slug=001 variant=full"]
        );
    }

    // -----------------------------------------------------------------
    // Lines: what `j` moves through.
    // -----------------------------------------------------------------

    /// The pane is a tree and every other pane is a list. Flattening it
    /// here is what lets `detail_len` count it — counting feeds instead
    /// left the selection unable to move at all.
    #[test]
    fn every_line_the_pane_draws_is_one_the_model_can_count() {
        let p = project(
            vec![
                seeds("effective_rank", "full", vec![2.0, 3.0]),
                seeds("effective_rank", "no_ema", vec![1.0, 1.5]),
                seeds("embedding_std", "full", vec![0.4, 0.5]),
            ],
            None,
        );
        let feeds = metric_feeds(&p, at(0));
        let lines = metric_lines(&feeds);
        // One feed header, two metric headers, three series.
        assert_eq!(lines.len(), 6);
        assert!(matches!(lines[0], MetricLine::Feed(_)));
        assert!(matches!(lines[1], MetricLine::Group(_)));
        assert!(matches!(lines[2], MetricLine::Row { .. }));
        assert_eq!(feeds[0].series(), 3);
    }

    /// A row is drawn against its metric's extent, so the line carries
    /// that scale rather than the renderer looking it up again.
    #[test]
    fn a_lines_axis_is_its_own_metrics_and_no_other() {
        let p = project(
            vec![
                seeds("effective_rank", "full", vec![2.352, 2.791]),
                seeds("embedding_std", "full", vec![0.4933, 0.5311]),
            ],
            None,
        );
        let feeds = metric_feeds(&p, at(0));
        let axes: Vec<Option<(f64, f64)>> = metric_lines(&feeds)
            .iter()
            .filter_map(|line| match line {
                MetricLine::Row { axis, .. } => Some(*axis),
                _ => None,
            })
            .collect();
        assert_eq!(axes, [Some((2.352, 2.791)), Some((0.4933, 0.5311))]);
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
