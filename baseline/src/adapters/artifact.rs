//! The artifact family: files a run produced. Three built-in
//! implementations — `figure`, `metrics`, `capture`.

use super::{AdapterError, ProjectContext};
use crate::adapters::verification::{parse_verdict, VerificationOutcome};
use crate::freshness::Observed;
use crate::manifest::ArtifactKind;
use globset::GlobBuilder;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One named scalar series read from a metrics feed.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// The series' key in the source records.
    pub name: String,
    /// Its values, in the order they were recorded.
    pub points: Vec<f64>,
}

/// What an adapter learned about an artifact beyond its path.
#[derive(Debug, Clone, PartialEq)]
pub enum ArtifactDetail {
    /// A pre-rendered image. The core reads its size, never its pixels.
    Figure {
        /// File size in bytes.
        bytes: u64,
    },
    /// Scalar series parsed from a JSONL feed.
    Metrics {
        /// Every series found, sorted by name.
        series: Vec<Series>,
    },
    /// A Plumb run directory.
    Capture {
        /// The run's id, taken from its directory name.
        run_id: String,
        /// The run's verdict, or `NotRun` when it wrote none.
        outcome: VerificationOutcome,
    },
}

/// One artifact a run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Artifact {
    /// Absolute path to the artifact.
    pub path: PathBuf,
    /// Which feed produced it.
    pub kind: ArtifactKind,
    /// Its filesystem modification time.
    pub modified: SystemTime,
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

/// The filesystem modification time of a path, or the Unix epoch when
/// the filesystem does not report one.
fn modified_at(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
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
        for path in scan_glob(&ctx.root, &self.watch)? {
            if !path.is_dir() {
                continue;
            }
            let run_id = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let outcome = std::fs::read_to_string(path.join("verdict.md"))
                .ok()
                .and_then(|text| parse_verdict(&text))
                .unwrap_or(VerificationOutcome::NotRun);
            artifacts.push(Artifact {
                modified: modified_at(&path),
                path,
                kind: ArtifactKind::Capture,
                detail: ArtifactDetail::Capture { run_id, outcome },
            });
        }
        Ok(Observed::watched(artifacts, now))
    }
}

/// Parses a JSONL metrics feed into named scalar series.
///
/// One record per line, each a JSON object. Numeric fields become
/// series points in record order; non-numeric fields and unparseable
/// lines are skipped, never coerced and never fatal — a real producer
/// emits ragged records and string annotations, and losing the whole
/// file over one of them would be the wrong trade.
pub fn parse_metrics(text: &str) -> Vec<Series> {
    let mut series: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(serde_json::Value::Object(record)) = serde_json::from_str(line) else {
            continue;
        };
        for (key, value) in record {
            if let Some(number) = value.as_f64() {
                series.entry(key).or_default().push(number);
            }
        }
    }
    series
        .into_iter()
        .map(|(name, points)| Series { name, points })
        .collect()
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
            }
        );
        assert_eq!(
            artifacts[1].detail,
            ArtifactDetail::Capture {
                run_id: "20260814T112200Z".into(),
                outcome: crate::adapters::verification::VerificationOutcome::Fail,
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
