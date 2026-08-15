//! The capture contract: given args, write one or more images to a
//! declared path, or fail with a typed error. Nothing downstream knows
//! or cares which adapter produced a frame — adding a surface later
//! means one new module behind this signature and no change anywhere
//! else.

pub mod command;
pub mod pty;
pub mod window;

use crate::config::{AdapterKind, Scenario};
use crate::manifest::RunManifest;
use std::path::{Path, PathBuf};

/// An image that could not be opened or decoded, together with the
/// path that caused it — kept as a single field so
/// `CaptureError::UnreadableImage(_)` stays an opaque match, mirroring
/// `config::IoFailure`/`config::YamlFailure`.
#[derive(Debug)]
pub struct ImageFailure {
    /// The path that failed to open or decode.
    pub path: PathBuf,
    /// The underlying failure, rendered. `frame_count` can reach this
    /// from either a filesystem error (opening a GIF) or a decoder
    /// error (an unreadable PNG/GIF), so the field holds a rendered
    /// string rather than committing to one concrete source type.
    pub source: String,
}

/// A contact sheet that could not be assembled or written, together
/// with the path that caused it — kept as a single field so
/// `CaptureError::ContactSheetWrite(_)` stays an opaque match, mirroring
/// [`ImageFailure`].
#[derive(Debug)]
pub struct ContactSheetFailure {
    /// The GIF that failed to decode, or the PNG path that failed to
    /// write, whichever step failed.
    pub path: PathBuf,
    /// The underlying failure, rendered.
    pub source: String,
}

/// A capture that did not produce a usable image. Capture failure is
/// never a GO — a scenario whose capture fails is reported as HOLD
/// with this error, so every variant must be rich enough to name what
/// went wrong.
#[derive(Debug)]
pub enum CaptureError {
    /// This adapter has no v1 implementation.
    NotImplemented {
        /// Adapter name.
        adapter: &'static str,
        /// Why it is deferred, in words a reader can act on.
        reason: &'static str,
    },
    /// The adapter's process could not be spawned at all.
    Spawn(std::io::Error),
    /// The adapter's process ran and exited non-zero.
    CommandFailed {
        /// Exit status, rendered.
        status: String,
        /// Captured stderr, truncated to something readable.
        stderr: String,
    },
    /// The command succeeded but wrote no image at the declared stem.
    NoOutput {
        /// The stem images were expected at.
        expected_stem: PathBuf,
    },
    /// More than one image landed at the declared stem.
    AmbiguousOutput(Vec<PathBuf>),
    /// An image was produced but could not be opened or decoded.
    UnreadableImage(ImageFailure),
    /// A multi-frame capture's contact sheet could not be assembled or
    /// written.
    ContactSheetWrite(ContactSheetFailure),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::NotImplemented { adapter, reason } => {
                write!(f, "the `{adapter}` adapter is not implemented: {reason}")
            }
            CaptureError::Spawn(e) => write!(f, "could not start the capture command: {e}"),
            CaptureError::CommandFailed { status, stderr } => {
                write!(f, "capture command exited {status}\n{stderr}")
            }
            CaptureError::NoOutput { expected_stem } => write!(
                f,
                "capture command succeeded but wrote no image at {}.png/.gif",
                expected_stem.display()
            ),
            CaptureError::AmbiguousOutput(paths) => {
                write!(f, "capture wrote several images: {paths:?}")
            }
            CaptureError::UnreadableImage(e) => {
                write!(f, "could not decode {}: {}", e.path.display(), e.source)
            }
            CaptureError::ContactSheetWrite(e) => {
                write!(
                    f,
                    "could not write contact sheet {}: {}",
                    e.path.display(),
                    e.source
                )
            }
        }
    }
}
impl std::error::Error for CaptureError {}

/// Substitutes `{out}` with the run's output stem (a path with no
/// extension) everywhere it appears in `args`.
pub fn substitute_out(args: &str, out_stem: &Path) -> String {
    args.replace("{out}", &out_stem.display().to_string())
}

/// Counts frames in a captured image: 1 for a PNG, the decoded frame
/// count for a GIF.
pub fn frame_count(path: &Path) -> Result<usize, CaptureError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "gif" {
        use image::AnimationDecoder;
        let file = std::fs::File::open(path).map_err(|source| {
            CaptureError::UnreadableImage(ImageFailure {
                path: path.to_path_buf(),
                source: source.to_string(),
            })
        })?;
        let decoder =
            image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)).map_err(|e| {
                CaptureError::UnreadableImage(ImageFailure {
                    path: path.to_path_buf(),
                    source: e.to_string(),
                })
            })?;
        return Ok(decoder.into_frames().count());
    }
    image::open(path).map_err(|e| {
        CaptureError::UnreadableImage(ImageFailure {
            path: path.to_path_buf(),
            source: e.to_string(),
        })
    })?;
    Ok(1)
}

/// Runs `scenario`'s adapter, writing into `run_dir`, and returns the
/// manifest describing what was captured.
pub fn capture(
    scenario: &Scenario,
    run_dir: &Path,
    run_id: &str,
) -> Result<RunManifest, CaptureError> {
    match scenario.adapter {
        AdapterKind::Command => command::capture_command(scenario, run_dir, run_id),
        AdapterKind::Pty => Err(CaptureError::NotImplemented {
            adapter: "pty",
            reason: "landing in Arc 5; use the `command` adapter meanwhile",
        }),
        AdapterKind::Window => Err(CaptureError::NotImplemented {
            adapter: "window",
            reason: "deferred — no consumer exists yet (TTUI is a TUI, \
                     Model-Experiments is Python/CLI); the contract admits \
                     it, the implementation is out of v1 scope",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_placeholder_is_replaced_with_the_run_stem() {
        let args = "cargo run -p visual-snapshot -- --out {out}.gif";
        let got = substitute_out(args, std::path::Path::new("/runs/20260814/dial"));
        assert!(got.ends_with("dial.gif"), "got {got}");
        assert!(!got.contains("{out}"));
    }

    #[test]
    fn every_occurrence_of_the_placeholder_is_replaced() {
        let got = substitute_out("a {out}.png b {out}.log", std::path::Path::new("/r/s"));
        assert_eq!(got.matches("/r/s").count(), 2);
    }

    #[test]
    fn a_png_is_one_frame() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        image::RgbaImage::new(4, 4).save(&p).unwrap();
        assert_eq!(frame_count(&p).unwrap(), 1);
    }

    #[test]
    fn window_adapter_fails_with_a_typed_not_implemented_error() {
        // Deferred by design: no consumer exists. It must fail loudly
        // and specifically, never silently produce nothing.
        let e = window::capture_window("Some Title", std::path::Path::new("/tmp/x")).unwrap_err();
        match e {
            CaptureError::NotImplemented { adapter, reason } => {
                assert_eq!(adapter, "window");
                assert!(
                    reason.contains("no consumer"),
                    "reason must say why: {reason}"
                );
            }
            other => panic!("expected NotImplemented, got {other:?}"),
        }
    }
}
