//! Writes rendered terminal frames to disk: a single frame as a PNG, or a
//! sequence of timed frames as an animated GIF.

use image::codecs::gif::GifEncoder;
use image::{Delay, Frame, RgbaImage};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// An I/O failure writing an encoded image to disk, together with the
/// path that caused it — kept as a single field so `EncodeError::Io(_)`
/// stays an opaque, one-wildcard match for callers that only care
/// *that* it failed, not why. Mirrors `config::IoFailure`.
///
/// Deviation from `tools/visual-snapshot`'s `encode.rs`: the source
/// wraps a bare `std::io::Error` via a blanket `From<std::io::Error>`
/// impl. Plumb's settled error convention removed blanket
/// `From<io::Error>` impls project-wide in favor of a path-carrying
/// wrapper constructed explicitly, so this struct and the explicit
/// `map_err` in `write_gif` below are a forced deviation, not a
/// stylistic choice. `EncodeError::Image` is unchanged from the source
/// (it already wraps `image::ImageError`, not a bare `io::Error`, so
/// the removed-blanket-impl rule doesn't reach it).
#[derive(Debug)]
pub struct IoFailure {
    /// The path the encoder was asked to write.
    pub path: PathBuf,
    /// The underlying I/O error.
    pub source: std::io::Error,
}

/// Failure writing an encoded image to disk.
#[derive(Debug)]
pub enum EncodeError {
    /// Underlying filesystem I/O failure, and the path that caused it.
    Io(IoFailure),
    /// Failure from the `image` crate's encoder.
    Image(image::ImageError),
}

impl From<image::ImageError> for EncodeError {
    fn from(e: image::ImageError) -> Self {
        EncodeError::Image(e)
    }
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::Io(e) => write!(f, "writing {}: {}", e.path.display(), e.source),
            EncodeError::Image(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for EncodeError {}

/// Writes a single frame as a PNG.
pub fn write_png(img: &RgbaImage, path: &Path) -> Result<(), EncodeError> {
    img.save(path)?;
    Ok(())
}

/// Encodes a single frame as in-memory PNG bytes, for callers (like
/// `--review`) that need the encoded bytes without writing to disk.
pub fn png_bytes(img: &RgbaImage) -> Result<Vec<u8>, EncodeError> {
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cursor, image::ImageFormat::Png)?;
    Ok(buf)
}

/// Floor applied to a GIF frame's display delay: some viewers treat a 0ms
/// delay oddly (skipping the frame entirely, or racing through it faster
/// than intended), and the initial frame's recorded duration is always
/// `Duration::ZERO` (it's captured before any step runs, so there's no
/// step duration to attach). This only affects the encoded GIF's timing
/// metadata, not the `Duration` values a capture run returns.
const MIN_GIF_FRAME_DELAY: Duration = Duration::from_millis(20);

/// Writes multiple frames as an animated GIF, each held for its paired duration.
pub fn write_gif(frames: &[(RgbaImage, Duration)], path: &Path) -> Result<(), EncodeError> {
    let file = File::create(path).map_err(|source| {
        EncodeError::Io(IoFailure {
            path: path.to_path_buf(),
            source,
        })
    })?;
    let mut encoder = GifEncoder::new(file);
    for (img, duration) in frames {
        let delay = Delay::from_saturating_duration((*duration).max(MIN_GIF_FRAME_DELAY));
        let frame = Frame::from_parts(img.clone(), 0, 0, delay);
        encoder.encode_frame(frame)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::time::Duration;

    fn solid(w: u32, h: u32, px: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(w, h, px)
    }

    #[test]
    fn write_png_round_trips_dimensions_and_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frame.png");
        let img = solid(4, 2, Rgba([10, 20, 30, 255]));

        write_png(&img, &path).unwrap();

        let reopened = image::open(&path).unwrap().to_rgba8();
        assert_eq!(reopened.dimensions(), (4, 2));
        assert_eq!(*reopened.get_pixel(0, 0), Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn write_gif_round_trips_frame_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seq.gif");
        let frames = vec![
            (
                solid(2, 2, Rgba([255, 0, 0, 255])),
                Duration::from_millis(16),
            ),
            (
                solid(2, 2, Rgba([0, 255, 0, 255])),
                Duration::from_millis(150),
            ),
        ];

        write_gif(&frames, &path).unwrap();

        let file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
        let decoder = image::codecs::gif::GifDecoder::new(file).unwrap();
        let decoded_frames: Vec<_> = image::AnimationDecoder::into_frames(decoder)
            .collect_frames()
            .unwrap();
        assert_eq!(decoded_frames.len(), 2);
    }

    #[test]
    fn png_bytes_round_trips_dimensions_and_pixels() {
        let img = solid(4, 2, Rgba([10, 20, 30, 255]));

        let bytes = png_bytes(&img).unwrap();

        let reopened = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(reopened.dimensions(), (4, 2));
        assert_eq!(*reopened.get_pixel(0, 0), Rgba([10, 20, 30, 255]));
    }
}
