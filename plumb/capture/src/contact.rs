//! Tiles a multi-frame GIF into a single contact-sheet PNG. A lens
//! agent reaches a GIF as a decode-failure placeholder (see the Plumb
//! design's "What a lens can actually see"), so this module builds the
//! still image that actually reaches the prompt; the GIF itself is left
//! untouched for a human to watch.

use crate::adapter::{CaptureError, ContactSheetFailure, ImageFailure};
use image::{AnimationDecoder, GenericImage, Rgba, RgbaImage};
use std::path::Path;

/// Pixel width of the gutter drawn between and around every pane, so a
/// lens can tell adjacent frames apart rather than reading one smeared
/// image. `pub(crate)` so `report::geometry` can mirror this exact
/// layout when resolving a frame's crop rectangle, rather than
/// hand-maintaining a second copy of the constant.
pub(crate) const GUTTER_PX: u32 = 8;

/// The gutter's fill colour: a flat mid-grey chosen to sit outside the
/// range terminal captures actually use (near-black backgrounds through
/// saturated UI accents), so it reads unambiguously as spacing rather
/// than content.
const GUTTER_COLOR: Rgba<u8> = Rgba([128, 128, 128, 255]);

/// Decodes every frame of the GIF at `path`, in on-disk order — which
/// is capture order, since the adapter that wrote it never reorders
/// frames.
fn decode_frames(path: &Path) -> Result<Vec<RgbaImage>, CaptureError> {
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
    let frames = decoder.into_frames().collect_frames().map_err(|e| {
        CaptureError::UnreadableImage(ImageFailure {
            path: path.to_path_buf(),
            source: e.to_string(),
        })
    })?;
    Ok(frames.into_iter().map(|f| f.into_buffer()).collect())
}

/// Chooses a tile grid for `n` panes: as close to square as possible,
/// rounding the column count up so a non-square `n` (an 8-frame
/// capture, say) still gets a compact grid rather than one long strip.
/// Reading order is row-major — left to right, then top to bottom —
/// which is what "reading order" means for the frame placement this
/// tiles. `pub(crate)` so `report::geometry` can recompute the same
/// grid a sheet was tiled with, rather than re-deriving it.
pub(crate) fn grid_dims(n: usize) -> (u32, u32) {
    let n = (n.max(1)) as u32;
    let cols = (n as f64).sqrt().ceil() as u32;
    let cols = cols.max(1);
    let rows = n.div_ceil(cols);
    (cols, rows)
}

/// Tiles `frames` into one contact-sheet image in row-major reading
/// order, with a [`GUTTER_PX`]-wide gutter between and around every
/// pane. Panes are sized to the largest frame; a smaller frame is
/// placed at its pane's top-left corner rather than stretched, so
/// tiling never distorts content. Never drops a frame: the returned
/// sheet has exactly as many panes as `frames.len()`, even when the
/// grid has empty cells left over (an 8-frame sheet tiles 3x3, leaving
/// one pane as bare gutter).
///
/// # Panics
///
/// Panics if `frames` is empty — there is nothing to tile from a
/// capture with zero frames, and every caller in this crate only
/// invokes this once `frame_count` has already confirmed 2 or more.
pub fn tile_frames(frames: &[RgbaImage]) -> RgbaImage {
    assert!(
        !frames.is_empty(),
        "tile_frames requires at least one frame"
    );

    let pane_w = frames.iter().map(RgbaImage::width).max().unwrap();
    let pane_h = frames.iter().map(RgbaImage::height).max().unwrap();
    let (cols, rows) = grid_dims(frames.len());

    let sheet_w = cols * pane_w + (cols + 1) * GUTTER_PX;
    let sheet_h = rows * pane_h + (rows + 1) * GUTTER_PX;
    let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, GUTTER_COLOR);

    for (i, frame) in frames.iter().enumerate() {
        let i = i as u32;
        let col = i % cols;
        let row = i / cols;
        let x = GUTTER_PX + col * (pane_w + GUTTER_PX);
        let y = GUTTER_PX + row * (pane_h + GUTTER_PX);
        sheet
            .copy_from(frame, x, y)
            .expect("pane placement fits within the sheet by construction");
    }
    sheet
}

/// Decodes `gif_path` and writes a tiled contact sheet PNG to
/// `sheet_path`. The caller is responsible for only invoking this on a
/// capture already known to have 2+ frames.
pub fn write_contact_sheet(gif_path: &Path, sheet_path: &Path) -> Result<(), CaptureError> {
    let frames = decode_frames(gif_path)?;
    let sheet = tile_frames(&frames);
    sheet.save(sheet_path).map_err(|e| {
        CaptureError::ContactSheetWrite(ContactSheetFailure {
            path: sheet_path.to_path_buf(),
            source: e.to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Frame;

    /// Writes a GIF at `path` from `frames`, in the given order —
    /// mirrors what a real capture adapter produces, for tests that
    /// need a real file on disk to decode.
    fn write_gif(path: &Path, frames: Vec<RgbaImage>) {
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = image::codecs::gif::GifEncoder::new(file);
        encoder
            .encode_frames(frames.into_iter().map(Frame::new))
            .unwrap();
    }

    /// A solid-colour frame, so a test can identify which frame's pixel
    /// landed at a given sheet coordinate.
    fn solid(w: u32, h: u32, color: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(w, h, color)
    }

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const GREEN: Rgba<u8> = Rgba([0, 255, 0, 255]);
    const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);
    const YELLOW: Rgba<u8> = Rgba([255, 255, 0, 255]);

    #[test]
    fn grid_dims_prefers_a_compact_square_ish_grid() {
        assert_eq!(grid_dims(1), (1, 1));
        assert_eq!(grid_dims(2), (2, 1));
        assert_eq!(grid_dims(3), (2, 2));
        assert_eq!(grid_dims(4), (2, 2));
        assert_eq!(grid_dims(8), (3, 3));
    }

    /// The core assertion this task exists for: tiling four
    /// distinguishable frames must place each one's actual pixel
    /// content at the sheet coordinate its position in the grid
    /// predicts — not merely produce a same-sized image.
    #[test]
    fn tile_frames_places_each_frames_content_at_its_grid_position() {
        let frames = vec![
            solid(10, 10, RED),
            solid(10, 10, GREEN),
            solid(10, 10, BLUE),
            solid(10, 10, YELLOW),
        ];

        let sheet = tile_frames(&frames);

        // grid_dims(4) == (2, 2): a 2x2 grid of 10x10 panes plus a
        // GUTTER_PX gutter on every side and between columns/rows.
        assert_eq!(sheet.width(), 2 * 10 + 3 * GUTTER_PX);
        assert_eq!(sheet.height(), 2 * 10 + 3 * GUTTER_PX);

        let pane_center = |col: u32, row: u32| {
            let x = GUTTER_PX + col * (10 + GUTTER_PX) + 5;
            let y = GUTTER_PX + row * (10 + GUTTER_PX) + 5;
            *sheet.get_pixel(x, y)
        };

        assert_eq!(pane_center(0, 0), RED, "frame 0 -> top-left pane");
        assert_eq!(pane_center(1, 0), GREEN, "frame 1 -> top-right pane");
        assert_eq!(pane_center(0, 1), BLUE, "frame 2 -> bottom-left pane");
        assert_eq!(pane_center(1, 1), YELLOW, "frame 3 -> bottom-right pane");

        // A gutter pixel between the top two panes must be the gutter
        // colour, not a smeared blend of neighbouring frame content —
        // this is the "panes are distinguishable" requirement.
        let gutter_x = GUTTER_PX + 10 + GUTTER_PX / 2;
        assert_eq!(*sheet.get_pixel(gutter_x, GUTTER_PX + 5), GUTTER_COLOR);
    }

    /// No silent frame loss: an 8-frame GIF must produce an 8-pane
    /// sheet, with each pane's content still identifiable and still in
    /// capture order — decoded from a real GIF file on disk, not from
    /// in-memory `RgbaImage`s built directly, so this exercises the
    /// exact decode path `write_contact_sheet` uses in production.
    #[test]
    fn contact_sheet_preserves_all_eight_frames_in_capture_order() {
        let colors = [
            Rgba([10, 10, 10, 255]),
            Rgba([20, 20, 20, 255]),
            Rgba([30, 30, 30, 255]),
            Rgba([40, 40, 40, 255]),
            Rgba([50, 50, 50, 255]),
            Rgba([60, 60, 60, 255]),
            Rgba([70, 70, 70, 255]),
            Rgba([80, 80, 80, 255]),
        ];
        let dir = tempfile::tempdir().unwrap();
        let gif_path = dir.path().join("eight.gif");
        write_gif(&gif_path, colors.iter().map(|c| solid(6, 6, *c)).collect());

        let sheet_path = dir.path().join("eight.png");
        write_contact_sheet(&gif_path, &sheet_path).unwrap();

        let sheet = image::open(&sheet_path).unwrap().to_rgba8();

        // grid_dims(8) == (3, 3): 3 columns wide.
        assert_eq!(sheet.width(), 3 * 6 + 4 * GUTTER_PX);
        assert_eq!(sheet.height(), 3 * 6 + 4 * GUTTER_PX);

        for (i, expected) in colors.iter().enumerate() {
            let i = i as u32;
            let col = i % 3;
            let row = i / 3;
            let x = GUTTER_PX + col * (6 + GUTTER_PX) + 3;
            let y = GUTTER_PX + row * (6 + GUTTER_PX) + 3;
            assert_eq!(
                *sheet.get_pixel(x, y),
                *expected,
                "frame {i} content missing from its predicted pane"
            );
        }
    }

    #[test]
    fn a_frame_smaller_than_the_pane_lands_at_its_top_left_corner_unstretched() {
        let frames = vec![solid(10, 10, RED), solid(4, 4, GREEN)];
        let sheet = tile_frames(&frames);

        // pane size is the max across frames: 10x10.
        let pane_x0 = GUTTER_PX + (10 + GUTTER_PX);
        let pane_y0 = GUTTER_PX;
        assert_eq!(*sheet.get_pixel(pane_x0, pane_y0), GREEN);
        // Outside the small frame's 4x4 footprint, its pane is bare
        // gutter colour, not stretched green.
        assert_eq!(*sheet.get_pixel(pane_x0 + 5, pane_y0 + 5), GUTTER_COLOR);
    }
}
