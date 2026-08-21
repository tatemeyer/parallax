//! The artifact family: files a run produced. Four built-in
//! implementations — `figure`, `metrics` (JSONL), `csv`, `capture`.
//!
//! The two metrics readers differ only in how they get from bytes to
//! records. Everything after that — long-format detection, dimensions,
//! the ordering claim, the grouping — is [`series_from`], shared, and
//! written once.

use super::{AdapterError, ProjectContext};
use crate::adapters::verification::{parse_verdict, VerificationOutcome};
use crate::freshness::Observed;
use crate::manifest::ArtifactKind;
use globset::GlobBuilder;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Whether a series' points are ordered, and by what.
///
/// **A curve is a claim.** Drawing points left to right asserts that
/// each one follows the last; a flat line asserts something was
/// measured repeatedly and did not change. Neither is true of a set of
/// measurements that merely arrived in some order.
///
/// Record order is not an order. A sweep feed is written by nested
/// loops, so the shape of a curve drawn over its record order is a fact
/// about the writing code — re-nest the loops and the picture changes
/// without a single measurement changing. This type exists so that
/// difference is carried rather than assumed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeriesOrder {
    /// Successive points are successive values of the named field,
    /// which was present on every record and never decreased. A curve
    /// is a claim the feed supports.
    By(String),
    /// Nothing orders these points. They are repeated measurements of
    /// one configuration — three seeds of the same cell — and the order
    /// they arrived in means nothing.
    Unordered,
}

/// One named scalar series read from a metrics feed.
///
/// **`order` is private and has no setter**, so a renderer handed one
/// of these cannot promote a group to a curve. The claim is made where
/// the feed is read — the only place that can see whether the feed
/// justified it — and travels with the data. This is the same
/// arrangement `Authorized` uses for the same reason: the dishonest
/// thing is unrepresentable rather than merely discouraged.
///
/// A struct literal cannot reach past the constructors:
///
/// ```compile_fail
/// use parallax_baseline::adapters::artifact::{Series, SeriesOrder};
/// // `order` and `dimensions` are private, so this does not compile.
/// let sneaky = Series {
///     name: "effective_rank".into(),
///     points: vec![2.779, 2.352, 2.791],
///     dimensions: Default::default(),
///     order: SeriesOrder::By("seed".into()),
/// };
/// ```
///
/// Neither can a renderer re-label a series it was given:
///
/// ```compile_fail
/// use parallax_baseline::adapters::artifact::{Series, SeriesOrder};
/// let mut series = Series::unordered("effective_rank", vec![2.779, 2.352]);
/// // No setter, and the field is private — a group stays a group.
/// series.order = SeriesOrder::By("seed".into());
/// ```
///
/// Reading the claim is of course fine:
///
/// ```
/// use parallax_baseline::adapters::artifact::{Series, SeriesOrder};
/// let series = Series::unordered("effective_rank", vec![2.779, 2.352]);
/// assert_eq!(*series.order(), SeriesOrder::Unordered);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    /// What was measured. For a wide feed this is the record's field
    /// name; for a long-format feed it is the value of its `metric`
    /// field — the measurement's own name, not the column holding it.
    pub name: String,
    /// Its values, in the order they were recorded — which is only
    /// meaningful when `order` says so.
    pub points: Vec<f64>,
    /// What distinguishes this series from others of the same `name`:
    /// the string fields of a long-format record, such as
    /// `variant=full`. Empty for a wide feed, which has no room to
    /// express one.
    dimensions: BTreeMap<String, String>,
    /// Whether `points` may be drawn as a curve.
    order: SeriesOrder,
}

impl Series {
    /// A series whose points nothing orders.
    pub fn unordered(name: impl Into<String>, points: Vec<f64>) -> Self {
        Self {
            name: name.into(),
            points,
            dimensions: BTreeMap::new(),
            order: SeriesOrder::Unordered,
        }
    }

    /// A series whose points are successive values of `by`.
    ///
    /// The caller is asserting the feed justified it. `parse_metrics`
    /// only calls this having checked; nothing reconstructs a series it
    /// was handed in order to change the answer.
    pub fn ordered(name: impl Into<String>, points: Vec<f64>, by: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            points,
            dimensions: BTreeMap::new(),
            order: SeriesOrder::By(by.into()),
        }
    }

    /// The same series, distinguished by `dimensions`.
    pub fn with_dimensions<K, V>(mut self, dimensions: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.dimensions = dimensions
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }

    /// Whether these points may be drawn as a curve.
    pub fn order(&self) -> &SeriesOrder {
        &self.order
    }

    /// What distinguishes this series from others of the same name.
    pub fn dimensions(&self) -> &BTreeMap<String, String> {
        &self.dimensions
    }
}

/// What an adapter learned about an artifact beyond its path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ArtifactDetail {
    /// A pre-rendered image. The core reads its size, never its pixels.
    Figure {
        /// File size in bytes.
        bytes: u64,
    },
    /// Scalar series parsed from a JSONL feed.
    Metrics {
        /// Every series found, sorted by name and then by dimensions —
        /// so the variants of one metric arrive adjacent, which is the
        /// order they have to be compared in.
        series: Vec<Series>,
    },
    /// A Plumb run directory.
    Capture {
        /// The run's id, taken from its directory name.
        run_id: String,
        /// The run's verdict, or `NotRun` when it wrote none.
        outcome: VerificationOutcome,
        /// The findings that survived merge, in the order the run
        /// ranked them. Empty for a run that found nothing, and for one
        /// that has not merged yet — which is the same on screen as
        /// "no findings" and is why the verdict is shown beside them.
        findings: Vec<RunFinding>,
    },
}

/// One finding from a Plumb run, as much of it as a cockpit row needs.
///
/// The fingerprint is the part that matters: it is what a ruling
/// addresses, and what Plumb suppresses a repeat of on the next run.
/// The evidence paragraph is deliberately not carried — it is written
/// for a reader with the image in front of them, and the cockpit is
/// not that reader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFinding {
    /// Plumb's stable identity for this finding.
    pub fingerprint: String,
    /// Which lens raised it.
    pub lens: String,
    /// How bad it says it is.
    pub severity: String,
    /// The one-line claim.
    pub claim: String,
}

/// Reads `merge/survivors.json` from a run directory.
///
/// A missing or unreadable file is no findings rather than an error: a
/// run still capturing has not written one yet, and a run directory
/// that cannot be parsed should not take the whole artifacts pane down
/// with it.
fn read_findings(run: &Path) -> Vec<RunFinding> {
    let Ok(text) = std::fs::read_to_string(run.join("merge").join("survivors.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    value
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let finding = entry.get("finding")?;
                    Some(RunFinding {
                        fingerprint: entry.get("fingerprint")?.as_str()?.to_string(),
                        lens: string_at(finding, "lens"),
                        severity: string_at(finding, "severity"),
                        claim: string_at(finding, "claim"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One string field, or `?` — a finding with a fingerprint and a
/// missing lens is still a finding worth ruling on.
fn string_at(value: &serde_json::Value, field: &str) -> String {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string()
}

/// One artifact a run produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    /// Absolute path to the artifact.
    pub path: PathBuf,
    /// Which feed produced it.
    pub kind: ArtifactKind,
    /// When the producer last wrote it, on **this machine's** clock —
    /// re-based on receipt for an artifact that came from a peer, the
    /// same way an observation's own timestamp is.
    ///
    /// `None` when nobody could say: a filesystem that does not report
    /// modification times, or a peer whose own numbers did not admit an
    /// age. An absent value is the honest rendering of "unknown", and
    /// the alternative — a fallback timestamp — is indistinguishable
    /// from a measurement once it reaches a screen.
    pub modified: Option<SystemTime>,
    /// What the adapter read from it.
    pub detail: ArtifactDetail,
}

/// A source of artifacts.
pub trait ArtifactAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Scans the feed's `watch` glob as of `now`.
    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Artifact>>, AdapterError>;
}

/// Walks `root` and returns every path matching `pattern`, which is
/// interpreted relative to `root`.
///
/// This walks on demand rather than holding an OS watch handle: a
/// headless library must not own background threads, and a caller that
/// decides when to poll should also decide when to scan.
pub fn scan_glob(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, AdapterError> {
    // `literal_separator` is what keeps `*` inside one path component;
    // without it `runs/*/verdict.md` would also match `runs/a/b/verdict.md`,
    // and a manifest's globs would silently match more than they say.
    let matcher = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|e| AdapterError::Parse(format!("`{pattern}` is not a valid glob: {e}")))?
        .compile_matcher();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        if matcher.is_match(relative) {
            found.push(entry.path().to_path_buf());
        }
    }
    found.sort();
    Ok(found)
}

/// The outermost directories among `paths`, dropping any that sits
/// inside another match.
///
/// A `**` watch matches at every depth, and a real Plumb run directory
/// holds `lenses/<lens>.<scenario>/` and `merge/` subdirectories per the
/// evidence contract. Without this, one completed run reads as several:
/// the run itself plus one phantom run per nested directory, each with
/// no verdict of its own.
///
/// Relies on `scan_glob` returning sorted paths, so a parent always
/// precedes its children.
pub(crate) fn outermost_dirs(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !path.is_dir() {
            continue;
        }
        // `Path::starts_with` compares whole components, so a sibling
        // named `20260814T112200Z-retry` is not mistaken for a child of
        // `20260814T112200Z`.
        if roots.iter().any(|root| path.starts_with(root)) {
            continue;
        }
        roots.push(path);
    }
    roots
}

/// The filesystem modification time of a path, or `None` when the
/// filesystem does not report one.
///
/// `None` rather than the Unix epoch. A fallback timestamp is a lie a
/// renderer cannot detect — "produced 56 years ago" is a sentence about
/// a missing value dressed up as a measurement — and the whole point of
/// the second age is that a reader can trust it.
fn modified_at(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Reports pre-rendered images: path, size, modification time. It never
/// reads their pixels — rendering is a frontend's problem.
pub struct FigureArtifactAdapter {
    watch: String,
}

impl FigureArtifactAdapter {
    /// An adapter scanning `watch`, relative to the project root.
    pub fn new(watch: impl Into<String>) -> Self {
        Self {
            watch: watch.into(),
        }
    }
}

impl ArtifactAdapter for FigureArtifactAdapter {
    fn source_name(&self) -> String {
        "artifact:figure".into()
    }

    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Artifact>>, AdapterError> {
        let mut artifacts = Vec::new();
        for path in scan_glob(&ctx.root, &self.watch)? {
            if !path.is_file() {
                continue;
            }
            let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            artifacts.push(Artifact {
                modified: modified_at(&path),
                path,
                kind: ArtifactKind::Figure,
                detail: ArtifactDetail::Figure { bytes },
            });
        }
        Ok(Observed::watched(artifacts, now))
    }
}

/// Reports Plumb run directories: the run id, and the verdict it
/// rendered. A run still in progress reports `NotRun` and stays
/// visible — a capture that vanished from the list reads as a run that
/// never happened.
pub struct CaptureArtifactAdapter {
    watch: String,
}

impl CaptureArtifactAdapter {
    /// An adapter scanning `watch`, relative to the project root.
    pub fn new(watch: impl Into<String>) -> Self {
        Self {
            watch: watch.into(),
        }
    }
}

impl ArtifactAdapter for CaptureArtifactAdapter {
    fn source_name(&self) -> String {
        "artifact:capture".into()
    }

    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Artifact>>, AdapterError> {
        let mut artifacts = Vec::new();
        for path in outermost_dirs(scan_glob(&ctx.root, &self.watch)?) {
            let run_id = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let outcome = std::fs::read_to_string(path.join("verdict.md"))
                .ok()
                .and_then(|text| parse_verdict(&text))
                .unwrap_or(VerificationOutcome::NotRun);
            let findings = read_findings(&path);
            artifacts.push(Artifact {
                modified: modified_at(&path),
                path,
                kind: ArtifactKind::Capture,
                detail: ArtifactDetail::Capture {
                    run_id,
                    outcome,
                    findings,
                },
            });
        }
        Ok(Observed::watched(artifacts, now))
    }
}

/// A long-format record names its metric here.
const METRIC_FIELD: &str = "metric";
/// A long-format record carries its measurement here.
const VALUE_FIELD: &str = "value";
/// The fields a wide feed may use to order its records, in the order
/// they are tried.
const ORDERING_FIELDS: [&str; 3] = ["step", "epoch", "iteration"];

/// A record's contribution to one series: its key, and its value.
type Keyed = ((String, BTreeMap<String, String>), f64);

/// Reads a long-format record — one that names its own metric.
///
/// The shape is `{"metric": "effective_rank", "value": 2.779, ...}`:
/// one record per *measurement* rather than per timestep. Its remaining
/// **string** fields are dimensions — `variant`, `experiment` — and
/// become part of the key, because they are what distinguishes two
/// series of the same metric.
///
/// Its remaining **numeric** fields are not charted. On a wide record a
/// number is a measurement; here the measurement is already named, so a
/// second number is an identifier — an issue number, a seed index — and
/// the one thing it must not become is a series. Note the asymmetry is
/// the point: `seed` is deliberately not part of the key either, since
/// the three seeds of one cell are the repeated measurements whose
/// spread carries the result.
fn observation(record: &serde_json::Map<String, serde_json::Value>) -> Option<Keyed> {
    let name = record.get(METRIC_FIELD)?.as_str()?.to_string();
    let value = record.get(VALUE_FIELD)?.as_f64()?;
    let dimensions = record
        .iter()
        .filter(|(key, _)| key.as_str() != METRIC_FIELD)
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
        .collect();
    Some(((name, dimensions), value))
}

/// Whether a wide feed's records are ordered, and by what.
///
/// Demanding the field be present on *every* record and never decrease
/// is deliberately strict. The claim being made is that successive
/// points are successive steps, and a feed missing the field on one
/// record in fifty does not support it.
fn ordering(records: &[serde_json::Map<String, serde_json::Value>]) -> SeriesOrder {
    // One record cannot establish an order, and an empty file cannot
    // either — both would otherwise pass the checks below vacuously.
    if records.len() < 2 {
        return SeriesOrder::Unordered;
    }
    for field in ORDERING_FIELDS {
        let values: Vec<f64> = records
            .iter()
            .filter_map(|record| record.get(field)?.as_f64())
            .collect();
        if values.len() == records.len() && values.windows(2).all(|pair| pair[0] <= pair[1]) {
            return SeriesOrder::By(field.to_string());
        }
    }
    SeriesOrder::Unordered
}

/// Parses a JSONL metrics feed into named scalar series.
///
/// One record per line, each a JSON object. Unparseable lines are
/// skipped, never fatal — a real producer emits ragged records, and
/// losing the whole file over one of them would be the wrong trade.
///
/// **Two shapes, told apart by the records themselves.** A *wide*
/// record is one timestep and every numeric field is a metric. A
/// *long-format* record is one measurement that names itself, via a
/// string `metric` and a numeric `value`. Both are real; the JEPA
/// sweep in `tests/fixtures/metrics/` is the second, and reading it as
/// the first charts its issue numbers and heaps three unrelated metrics
/// onto one axis.
///
/// **A curve is only claimed when the feed justifies it.** A long-format
/// feed never justifies it: its records are measurements of separate
/// configurations, and their order is the writing loop's nesting. A wide
/// feed justifies it only via a monotonic ordering field. Anything else
/// is `Unordered`, which under-claims — the safe direction, because the
/// cost of drawing a curve that is not there is a reader believing a
/// trend that does not exist.
pub fn parse_metrics(text: &str) -> Vec<Series> {
    let records: Vec<serde_json::Map<String, serde_json::Value>> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| match serde_json::from_str(line) {
            Ok(serde_json::Value::Object(record)) => Some(record),
            _ => None,
        })
        .collect();
    series_from(records)
}

/// Groups records into named series, whatever read them.
///
/// This is the whole of what a metrics feed *means*, and it is
/// deliberately one function: a second format must not be a second
/// opinion about what a dimension is or when points may be drawn as a
/// curve. A CSV reader is a different way to get here, not a different
/// answer once here.
fn series_from(records: Vec<serde_json::Map<String, serde_json::Value>>) -> Vec<Series> {
    let mut groups: BTreeMap<(String, BTreeMap<String, String>), Vec<f64>> = BTreeMap::new();
    let mut any_long_format = false;

    for record in &records {
        if let Some((key, value)) = observation(record) {
            any_long_format = true;
            groups.entry(key).or_default().push(value);
            continue;
        }
        for (key, value) in record {
            if let Some(number) = value.as_f64() {
                groups
                    .entry((key.clone(), BTreeMap::new()))
                    .or_default()
                    .push(number);
            }
        }
    }

    let order = if any_long_format {
        SeriesOrder::Unordered
    } else {
        ordering(&records)
    };

    groups
        .into_iter()
        .map(|((name, dimensions), points)| Series {
            name,
            points,
            dimensions,
            order: order.clone(),
        })
        .collect()
}

/// Reads a long-format CSV metrics feed into named scalar series.
///
/// The shape is the one `results.csv` files in research repositories
/// already have: a header row, one row per *measurement*, a `metric`
/// column naming what was measured and a `value` column holding it.
/// Every other column is a dimension — except those the manifest named
/// as [`identifiers`], which are dropped.
///
/// **Why the manifest has to name them.** The JSONL reader tells a
/// dimension from an identifier by type: a string partitions, a number
/// does not. CSV has no types. `seed` and `steps` are both just text,
/// and dropping the wrong one either shatters a cell into one-point
/// series or merges a whole sweep into one. The file does not contain
/// the answer, so the producer states it rather than this code guessing.
///
/// **An empty cell is an absent dimension, not an empty one.** Coverage
/// in a real sweep is ragged — a column belongs to the experiment that
/// varied it — and a CSV spells "this row has no `momentum`" with a
/// blank. `momentum=` is not a value anybody measured.
///
/// A row whose `value` does not parse as a number is skipped, never
/// fatal, for the reason the JSONL reader skips an unparseable line: a
/// real producer emits ragged records and losing the file over one of
/// them is the wrong trade.
///
/// A header with no `metric` or no `value` column is a different thing
/// and **is** an error: it means the feed was declared as a shape it
/// does not have, and returning an empty feed there is exactly the
/// silence this adapter exists to end.
///
/// [`identifiers`]: crate::manifest::ArtifactEntry::identifiers
pub fn parse_metrics_csv(text: &str, identifiers: &[String]) -> Result<Vec<Series>, AdapterError> {
    // A hand-rolled reader would be about forty lines and would be
    // wrong on the first quoted cell holding a comma — the JEPA feed's
    // `params` column is embedded JSON, quotes and all.
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| AdapterError::Parse(format!("reading the header row: {e}")))?
        .iter()
        .map(|column| column.trim().to_string())
        .collect();

    for required in [METRIC_FIELD, VALUE_FIELD] {
        if !headers.iter().any(|column| column == required) {
            return Err(AdapterError::Parse(format!(
                "a long-format csv feed needs a `{required}` column; this one has [{}]",
                headers.join(", ")
            )));
        }
    }

    let mut records = Vec::new();
    for row in reader.records() {
        // One malformed row is not the file's fate.
        let Ok(row) = row else { continue };
        let mut record = serde_json::Map::new();
        for (column, cell) in headers.iter().zip(row.iter()) {
            if cell.is_empty() || identifiers.iter().any(|dropped| dropped == column) {
                continue;
            }
            if column == VALUE_FIELD {
                match cell
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                {
                    Some(number) => record.insert(column.clone(), number.into()),
                    // Not a measurement. `observation` would reject the
                    // record anyway; dropping it here says so once.
                    None => break,
                };
            } else {
                record.insert(column.clone(), cell.into());
            }
        }
        if record.get(VALUE_FIELD).is_some_and(|v| v.is_number())
            && record.get(METRIC_FIELD).is_some_and(|v| v.is_string())
        {
            records.push(record);
        }
    }
    Ok(series_from(records))
}

/// Reports long-format CSV scalar series. Selected by a manifest writing
/// `adapter: csv`.
///
/// Separate from [`MetricsArtifactAdapter`] rather than a mode on it,
/// because the two answer differently when they cannot read a file and
/// a degraded source has to say which reader was disappointed.
pub struct CsvMetricsArtifactAdapter {
    watch: String,
    identifiers: Vec<String>,
}

impl CsvMetricsArtifactAdapter {
    /// An adapter scanning `watch`, relative to the project root,
    /// dropping the columns `identifiers` names.
    pub fn new(watch: impl Into<String>, identifiers: Vec<String>) -> Self {
        Self {
            watch: watch.into(),
            identifiers,
        }
    }
}

impl ArtifactAdapter for CsvMetricsArtifactAdapter {
    fn source_name(&self) -> String {
        "artifact:metrics:csv".into()
    }

    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Artifact>>, AdapterError> {
        let mut artifacts = Vec::new();
        for path in scan_glob(&ctx.root, &self.watch)? {
            if !path.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&path)?;
            // The file is named, because "a long-format csv feed needs a
            // `metric` column" is not actionable when a glob matched
            // four files and one of them is a log.
            let series = parse_metrics_csv(&text, &self.identifiers)
                .map_err(|e| AdapterError::Parse(format!("{}: {e}", path.display())))?;
            artifacts.push(Artifact {
                modified: modified_at(&path),
                path,
                kind: ArtifactKind::Metrics,
                detail: ArtifactDetail::Metrics { series },
            });
        }
        Ok(Observed::watched(artifacts, now))
    }
}

/// Reports JSONL scalar series. Also selected by a manifest writing
/// `adapter: jsonl`.
pub struct MetricsArtifactAdapter {
    watch: String,
}

impl MetricsArtifactAdapter {
    /// An adapter scanning `watch`, relative to the project root.
    pub fn new(watch: impl Into<String>) -> Self {
        Self {
            watch: watch.into(),
        }
    }
}

impl ArtifactAdapter for MetricsArtifactAdapter {
    fn source_name(&self) -> String {
        "artifact:metrics".into()
    }

    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Artifact>>, AdapterError> {
        let mut artifacts = Vec::new();
        for path in scan_glob(&ctx.root, &self.watch)? {
            if !path.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&path)?;
            artifacts.push(Artifact {
                modified: modified_at(&path),
                path,
                kind: ArtifactKind::Metrics,
                detail: ArtifactDetail::Metrics {
                    series: parse_metrics(&text),
                },
            });
        }
        Ok(Observed::watched(artifacts, now))
    }
}

#[cfg(test)]
mod scan_tests {
    use super::*;
    use crate::adapters::ProjectContext;
    use std::time::{Duration, SystemTime};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    /// Builds a project tree and returns its tempdir.
    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (relative, contents) in files {
            let path = dir.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        }
        dir
    }

    #[test]
    fn a_double_star_glob_matches_at_any_depth() {
        let dir = tree(&[
            ("projects/a/results/run1/loss.png", "x"),
            ("projects/b/results/deep/nested/acc.png", "yy"),
            ("projects/a/results/notes.txt", "z"),
        ]);
        let mut found = scan_glob(dir.path(), "projects/*/results/**/*.png").unwrap();
        found.sort();
        assert_eq!(found.len(), 2, "the .txt does not match");
    }

    #[test]
    fn a_single_star_glob_does_not_cross_a_directory_boundary() {
        let dir = tree(&[("runs/a/verdict.md", "x"), ("runs/a/b/verdict.md", "y")]);
        assert_eq!(scan_glob(dir.path(), "runs/*/verdict.md").unwrap().len(), 1);
    }

    #[test]
    fn a_glob_matching_nothing_is_an_empty_result_not_an_error() {
        let dir = tree(&[("a.txt", "x")]);
        assert!(scan_glob(dir.path(), "**/*.png").unwrap().is_empty());
    }

    #[test]
    fn a_missing_root_is_an_empty_result_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scan_glob(&dir.path().join("nope"), "**/*.png")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn an_invalid_glob_is_a_parse_error_naming_the_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let err = scan_glob(dir.path(), "[").unwrap_err().to_string();
        assert!(err.contains('['), "got {err}");
    }

    #[test]
    fn figure_artifacts_report_their_size_and_never_their_pixels() {
        let dir = tree(&[(
            "out/field.png",
            "PNG

0123456789",
        )]);
        let mut a = FigureArtifactAdapter::new("out/**/*.png");
        let artifacts = a
            .scan(&ProjectContext::new("me", dir.path()), at(0))
            .unwrap()
            .value;
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].kind, crate::manifest::ArtifactKind::Figure);
        assert_eq!(artifacts[0].detail, ArtifactDetail::Figure { bytes: 18 });
    }

    #[test]
    fn capture_artifacts_carry_their_run_id_and_verdict() {
        let dir = tree(&[
            (
                ".plumb/runs/20260814T101500Z/verdict.md",
                "# run 20260814T101500Z — GO
",
            ),
            (
                ".plumb/runs/20260814T112200Z/verdict.md",
                "# run 20260814T112200Z — NO-GO
",
            ),
        ]);
        let mut a = CaptureArtifactAdapter::new(".plumb/runs/**");
        let mut artifacts = a
            .scan(&ProjectContext::new("ttui", dir.path()), at(0))
            .unwrap()
            .value;
        artifacts.sort_by(|x, y| x.path.cmp(&y.path));
        assert_eq!(artifacts.len(), 2);
        assert_eq!(
            artifacts[0].detail,
            ArtifactDetail::Capture {
                run_id: "20260814T101500Z".into(),
                outcome: crate::adapters::verification::VerificationOutcome::Pass,
                findings: Vec::new(),
            }
        );
        assert_eq!(
            artifacts[1].detail,
            ArtifactDetail::Capture {
                run_id: "20260814T112200Z".into(),
                outcome: crate::adapters::verification::VerificationOutcome::Fail,
                findings: Vec::new(),
            }
        );
    }

    /// A real Plumb run directory holds `lenses/<lens>.<scenario>/` and
    /// `merge/` subdirectories (the evidence contract), and the manifest's
    /// declared `.plumb/runs/**` matches a directory at any depth. Each of
    /// those subdirectories would otherwise be reported as its own run —
    /// one completed capture reading as five, four of them phantom.
    #[test]
    fn a_runs_subdirectory_is_part_of_its_run_rather_than_a_run_of_its_own() {
        let dir = tree(&[
            (
                ".plumb/runs/20260814T112200Z/verdict.md",
                "# run 20260814T112200Z — NO-GO
",
            ),
            (
                ".plumb/runs/20260814T112200Z/lenses/breakage.omnitrix/prompt.txt",
                "...",
            ),
            (".plumb/runs/20260814T112200Z/merge/survivors.json", "[]"),
        ]);
        let mut a = CaptureArtifactAdapter::new(".plumb/runs/**");
        let artifacts = a
            .scan(&ProjectContext::new("ttui", dir.path()), at(0))
            .unwrap()
            .value;
        assert_eq!(artifacts.len(), 1, "one run, not one per subdirectory");
        assert_eq!(
            artifacts[0].detail,
            ArtifactDetail::Capture {
                run_id: "20260814T112200Z".into(),
                outcome: crate::adapters::verification::VerificationOutcome::Fail,
                findings: Vec::new(),
            }
        );
    }

    #[test]
    fn a_capture_run_with_no_verdict_yet_reports_not_run_rather_than_being_dropped() {
        let dir = tree(&[(".plumb/runs/20260814T130000Z/omnitrix.png", "x")]);
        let mut a = CaptureArtifactAdapter::new(".plumb/runs/**");
        let artifacts = a
            .scan(&ProjectContext::new("ttui", dir.path()), at(0))
            .unwrap()
            .value;
        assert_eq!(artifacts.len(), 1, "an in-progress run is still visible");
        assert!(matches!(
            artifacts[0].detail,
            ArtifactDetail::Capture {
                outcome: crate::adapters::verification::VerificationOutcome::NotRun,
                ..
            }
        ));
    }

    #[test]
    fn artifacts_read_from_disk_are_live() {
        let dir = tree(&[("out/a.png", "x")]);
        let mut a = FigureArtifactAdapter::new("out/**/*.png");
        let observed = a
            .scan(&ProjectContext::new("me", dir.path()), at(0))
            .unwrap();
        assert_eq!(
            observed.freshness(at(9999)),
            crate::freshness::Freshness::Live
        );
    }
}
