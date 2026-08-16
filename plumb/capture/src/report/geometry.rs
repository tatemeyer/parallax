//! Frame-rectangle geometry for a contact sheet, and PNG-to-data-URI
//! encoding so a crop can be embedded directly in the rendered report
//! rather than shipped as a sibling file. `frame_rect` mirrors
//! `contact::tile_frames`'s layout exactly — a gutter on every edge,
//! not only between frames — since a formula that drifts from that
//! layout would silently crop the wrong pixels beside a finding's
//! claim.

use crate::contact::{grid_dims, GUTTER_PX};
use crate::report::{IoFailure, ReportError};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;
use std::path::Path;

/// One frame's pixel rectangle within a contact sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRect {
    /// Left edge, in sheet pixels.
    pub x: u32,
    /// Top edge, in sheet pixels.
    pub y: u32,
    /// Width, in pixels.
    pub w: u32,
    /// Height, in pixels.
    pub h: u32,
}

/// Resolves the pixel rectangle frame `index` occupies within a
/// `frame_count`-frame contact sheet sized `sheet_w x sheet_h`, using
/// the same `cols x rows` grid and every-edge gutter that
/// `contact::tile_frames` laid the sheet out with. Returns `None` when
/// `index` is out of range for `frame_count`.
pub fn frame_rect(
    index: usize,
    frame_count: usize,
    sheet_w: u32,
    sheet_h: u32,
) -> Option<FrameRect> {
    if index >= frame_count {
        return None;
    }
    let (cols, rows) = grid_dims(frame_count);
    let frame_w = (sheet_w - (cols + 1) * GUTTER_PX) / cols;
    let frame_h = (sheet_h - (rows + 1) * GUTTER_PX) / rows;
    let index = index as u32;
    let col = index % cols;
    let row = index / cols;
    let x = GUTTER_PX + col * (frame_w + GUTTER_PX);
    let y = GUTTER_PX + row * (frame_h + GUTTER_PX);
    Some(FrameRect {
        x,
        y,
        w: frame_w,
        h: frame_h,
    })
}

/// Loads `sheet`, crops it to `rect`, and re-encodes the crop as a PNG
/// `data:` URI — what the rendered report embeds beside a finding's
/// claim so the crop travels with the HTML.
pub fn crop_png_data_uri(sheet: &Path, rect: FrameRect) -> Result<String, ReportError> {
    let image = open_image(sheet)?;
    let cropped = image::imageops::crop_imm(&image, rect.x, rect.y, rect.w, rect.h).to_image();
    encode_data_uri(&DynamicImage::ImageRgba8(cropped), sheet)
}

/// Loads `path` and encodes it as a PNG `data:` URI, uncropped — used
/// for embedding a full sheet or single-frame capture alongside
/// individual crops.
pub fn png_data_uri(path: &Path) -> Result<String, ReportError> {
    let image = open_image(path)?;
    encode_data_uri(&image, path)
}

/// Opens `path` as an image, wrapping a decode failure with the path
/// that caused it.
fn open_image(path: &Path) -> Result<DynamicImage, ReportError> {
    image::open(path).map_err(|source| {
        ReportError::Io(IoFailure {
            path: path.to_path_buf(),
            source,
        })
    })
}

/// Encodes `image` as PNG bytes in memory and base64-encodes it into a
/// `data:image/png;base64,...` URI. `path` is carried through only to
/// name the file in an encode failure.
fn encode_data_uri(image: &DynamicImage, path: &Path) -> Result<String, ReportError> {
    let mut bytes: Vec<u8> = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .map_err(|source| {
            ReportError::Io(IoFailure {
                path: path.to_path_buf(),
                source,
            })
        })?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    /// The arithmetic this task exists for, pinned against a real
    /// on-disk contact sheet: 8 frames of 1920x640 tile 3x3 at
    /// `3*1920 + 4*8 = 5792` wide and `3*640 + 4*8 = 1952` tall. Using
    /// `cols - 1` gutters instead of `cols + 1` would yield 5776 and
    /// shift every crop.
    #[test]
    fn frame_rects_match_a_real_contact_sheet() {
        let (w, h) = (5792u32, 1952u32);
        let r0 = frame_rect(0, 8, w, h).expect("frame 0");
        assert_eq!((r0.x, r0.y, r0.w, r0.h), (8, 8, 1920, 640));

        let r2 = frame_rect(2, 8, w, h).expect("frame 2");
        assert_eq!(
            r2.x,
            8 + 2 * (1920 + 8),
            "third column starts past two gutters"
        );
        assert_eq!(r2.y, 8);

        let r3 = frame_rect(3, 8, w, h).expect("frame 3");
        assert_eq!(r3.x, 8, "fourth frame wraps to the second row");
        assert_eq!(r3.y, 8 + (640 + 8));
    }

    #[test]
    fn an_out_of_range_frame_has_no_rect() {
        assert!(frame_rect(8, 8, 5792, 1952).is_none());
    }

    /// A solid-colour image, so a test can identify which pixels landed
    /// in a crop by colour alone.
    fn solid(w: u32, h: u32, color: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(w, h, color)
    }

    /// Decodes a `data:image/png;base64,...` URI back into pixels, so a
    /// test can assert on what actually got encoded rather than trusting
    /// the prefix alone.
    fn decode_data_uri(uri: &str) -> RgbaImage {
        let b64 = uri
            .strip_prefix("data:image/png;base64,")
            .expect("data URI carries the expected PNG prefix");
        let bytes = STANDARD.decode(b64).expect("valid base64");
        image::load_from_memory(&bytes)
            .expect("valid PNG bytes")
            .to_rgba8()
    }

    #[test]
    fn crop_png_data_uri_extracts_only_the_named_rectangle() {
        const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
        const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);

        // A 2x1 sheet, gutter-free for simplicity: a red 4x4 pane at
        // (0,0) and a blue 4x4 pane at (4,0).
        let mut sheet = RgbaImage::from_pixel(8, 4, RED);
        for x in 4..8 {
            for y in 0..4 {
                sheet.put_pixel(x, y, BLUE);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let sheet_path = dir.path().join("sheet.png");
        sheet.save(&sheet_path).unwrap();

        let rect = FrameRect {
            x: 4,
            y: 0,
            w: 4,
            h: 4,
        };
        let uri = crop_png_data_uri(&sheet_path, rect).unwrap();
        let cropped = decode_data_uri(&uri);

        assert_eq!((cropped.width(), cropped.height()), (4, 4));
        for pixel in cropped.pixels() {
            assert_eq!(*pixel, BLUE, "crop must contain only the blue pane");
        }
    }

    #[test]
    fn png_data_uri_round_trips_the_whole_image_uncropped() {
        const GREEN: Rgba<u8> = Rgba([0, 255, 0, 255]);
        let image = solid(6, 6, GREEN);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("full.png");
        image.save(&path).unwrap();

        let uri = png_data_uri(&path).unwrap();
        let decoded = decode_data_uri(&uri);

        assert_eq!((decoded.width(), decoded.height()), (6, 6));
        assert!(decoded.pixels().all(|p| *p == GREEN));
    }

    #[test]
    fn a_missing_sheet_reports_an_opaque_io_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.png");

        let err = png_data_uri(&missing).unwrap_err();
        match err {
            ReportError::Io(e) => assert_eq!(e.path, missing),
        }
    }
}
