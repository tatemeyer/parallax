//! The artifact family: files a run produced. Three built-in
//! implementations — `figure`, `metrics`, `capture`.

use super::{AdapterError, ProjectContext};
use crate::adapters::verification::VerificationOutcome;
use crate::freshness::Observed;
use crate::manifest::ArtifactKind;
use std::path::PathBuf;
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
