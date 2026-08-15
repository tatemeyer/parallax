//! The capture contract: given args, write one or more images to a
//! declared path, or fail with a typed error. Nothing downstream knows
//! or cares which adapter produced a frame — adding a surface later
//! means one new module behind this signature and no change anywhere
//! else.

pub mod command;
pub mod pty;
pub mod window;

use crate::config::{AdapterKind, Scenario};
use crate::contact::write_contact_sheet;
use crate::encode::{self, EncodeError};
use crate::manifest::{Caveat, RunManifest};
use crate::script;
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
    /// A `pty` scenario's `size`/`script`/`args` fields could not be
    /// turned into something the adapter can run: no `size` declared,
    /// a `size` not shaped like `COLSxROWS`, or a `script` that failed
    /// to parse.
    InvalidPtyConfig(String),
    /// The `pty` adapter's spawn/drive/render process failed.
    Pty(pty::PtyError),
    /// A captured frame (or frames) could not be written to disk.
    Encode(EncodeError),
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
            CaptureError::InvalidPtyConfig(msg) => {
                write!(f, "invalid pty scenario config: {msg}")
            }
            CaptureError::Pty(e) => write!(f, "pty capture failed: {e}"),
            CaptureError::Encode(e) => write!(f, "could not write captured frame(s): {e}"),
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
        AdapterKind::Pty => capture_pty(scenario, run_dir, run_id),
        AdapterKind::Window => Err(CaptureError::NotImplemented {
            adapter: "window",
            reason: "deferred — no consumer exists yet (TTUI is a TUI, \
                     Model-Experiments is Python/CLI); the contract admits \
                     it, the implementation is out of v1 scope",
        }),
    }
}

/// Parses a `size` field (`COLSxROWS`, e.g. `80x24`) into `(cols, rows)`
/// — the order `pty::Session::spawn`/`run_script` expect their `rows,
/// cols` parameters reversed from. `None` (a `pty` scenario with no
/// declared size) and a malformed value are both reported as
/// [`CaptureError::InvalidPtyConfig`], since neither is something the
/// adapter can recover from.
fn parse_size(size: Option<&str>) -> Result<(u16, u16), CaptureError> {
    let size = size.ok_or_else(|| {
        CaptureError::InvalidPtyConfig("a `pty` scenario requires a `size` field".to_string())
    })?;
    let (cols, rows) = size.trim().split_once('x').ok_or_else(|| {
        CaptureError::InvalidPtyConfig(format!("size {size:?} must be COLSxROWS, e.g. \"80x24\""))
    })?;
    let malformed = || {
        CaptureError::InvalidPtyConfig(format!("size {size:?} must be COLSxROWS, e.g. \"80x24\""))
    };
    let cols: u16 = cols.parse().map_err(|_| malformed())?;
    let rows: u16 = rows.parse().map_err(|_| malformed())?;
    Ok((cols, rows))
}

/// Runs a `pty` scenario: splits `scenario.args` into argv, spawns it
/// under a pseudo-console sized from `scenario.size`, drives it through
/// `scenario.script`'s steps (or captures a single frame if none is
/// declared), and writes the result to disk exactly as `command`'s
/// contract requires — a `.png` for one frame, a `.gif` plus a tiled
/// contact sheet for 2+. Every unmapped-glyph substitution
/// `run_script` recorded is folded into a `Caveat::
/// UnmappedGlyphSubstituted` on the returned manifest, closing the path
/// from an unmapped codepoint to the sentence a lens agent reads.
fn capture_pty(
    scenario: &Scenario,
    run_dir: &Path,
    run_id: &str,
) -> Result<RunManifest, CaptureError> {
    let command: Vec<String> = scenario.args.split_whitespace().map(String::from).collect();
    let (cols, rows) = parse_size(scenario.size.as_deref())?;
    let steps = match &scenario.script {
        Some(path) => {
            script::parse_script(path).map_err(|e| CaptureError::InvalidPtyConfig(e.to_string()))?
        }
        None => Vec::new(),
    };

    let captured_frames = pty::run_script(&command, rows, cols, &steps, scenario.on_unmapped_glyph)
        .map_err(CaptureError::Pty)?;
    let frame_count = captured_frames.frames.len();

    let stem = run_dir.join(&scenario.name);
    let captured = if frame_count >= 2 {
        let path = stem.with_extension("gif");
        encode::write_gif(&captured_frames.frames, &path).map_err(CaptureError::Encode)?;
        path
    } else {
        let path = stem.with_extension("png");
        encode::write_png(&captured_frames.frames[0].0, &path).map_err(CaptureError::Encode)?;
        path
    };

    // Same contact-sheet promotion `command::capture_command` applies:
    // a single-frame capture's `image` is the raw capture; 2+ frames
    // gets a freshly-tiled contact sheet as `image`, keeping the GIF as
    // `animation` for a human to watch. Reuses `write_contact_sheet`
    // directly rather than re-deriving the tiling logic.
    let (image, animation) = if frame_count >= 2 {
        let sheet_path = captured.with_extension("png");
        write_contact_sheet(&captured, &sheet_path)?;
        (sheet_path, Some(captured))
    } else {
        (captured, None)
    };

    let caveats = captured_frames
        .substitutions
        .iter()
        .map(|&(ch, count)| Caveat::UnmappedGlyphSubstituted {
            codepoint: format!("U+{:04X}", ch as u32),
            count,
        })
        .collect();

    Ok(RunManifest {
        run_id: run_id.to_string(),
        scenario: scenario.name.clone(),
        adapter: "pty".into(),
        image: image.file_name().map(Into::into).unwrap_or(image.clone()),
        animation: animation.map(|a| a.file_name().map(Into::into).unwrap_or(a)),
        frame_count,
        size: scenario.size.clone(),
        intent: scenario.intent.clone(),
        expects: scenario.expects.clone(),
        caveats,
    })
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
