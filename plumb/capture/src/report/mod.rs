//! Renders Plumb's evidence into a human-facing report. `geometry`
//! resolves contact-sheet frame rectangles and encodes images as
//! `data:` URIs; `region` conservatively matches a finding's free-text
//! region to a frame; `render` (plus the sibling `lens_render`) turns
//! an assembled [`RunReport`] into a self-contained HTML document;
//! `assemble` walks a run directory — manifests, prompts, replies,
//! kept/dropped/clamped findings, and whatever a ruling later
//! suppressed — and turns what's actually on disk into that
//! `RunReport`. Read-only throughout: nothing under this module ever
//! writes into the run directory it reads.

pub mod assemble;
pub mod geometry;
mod lens_render;
pub mod region;
pub mod render;

pub use assemble::build_run_report;
pub use geometry::{crop_png_data_uri, frame_rect, png_data_uri, FrameRect};
pub use lens_render::{LensReport, RenderedFinding};
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
