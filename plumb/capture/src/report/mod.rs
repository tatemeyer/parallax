//! Renders Plumb's evidence into a human-facing report. So far this
//! Arc contributes contact-sheet frame geometry and PNG-to-data-URI
//! encoding (`geometry`), conservative region-to-frame resolution
//! (`region`), and the self-contained HTML skeleton a run renders into
//! (`render`); assembling real evidence into that skeleton is a later
//! task in this Arc and is deliberately not anticipated here.

pub mod geometry;
pub mod region;
pub mod render;

pub use geometry::{crop_png_data_uri, frame_rect, png_data_uri, FrameRect};
pub use region::resolve_frame;
pub use render::{render_report, RunReport, ScenarioReport};

use std::path::PathBuf;

/// An image that could not be opened, decoded, or re-encoded, together
/// with the path that caused it — kept as a single field so
/// `ReportError::Io(_)` stays an opaque match for callers that only
/// care *that* it failed, not why. Mirrors `evidence::IoFailure` /
/// `manifest::IoFailure`.
#[derive(Debug)]
pub struct IoFailure {
    /// The path the failing operation was acting on.
    pub path: PathBuf,
    /// The underlying image-codec failure. A single type covers both
    /// directions this module fails in — decoding an existing sheet
    /// and re-encoding a crop — since both go through the `image`
    /// crate's own error type.
    pub source: image::ImageError,
}

/// Failure building a piece of the rendered report.
#[derive(Debug)]
pub enum ReportError {
    /// A sheet or crop image could not be read or encoded, and the
    /// path that caused it.
    Io(IoFailure),
}

impl std::fmt::Display for ReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReportError::Io(e) => write!(f, "{}: {}", e.path.display(), e.source),
        }
    }
}
impl std::error::Error for ReportError {}
