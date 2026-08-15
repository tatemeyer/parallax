//! Rasterizes a parsed `vt100::Screen` into an RGBA bitmap: one 16x16
//! pixel block per terminal cell, built from 2x-upscaled 8x8 glyphs.
//! Approximates bold/reverse/underline; deliberately does not attempt
//! italic (unrenderable in a fixed bitmap font) or dim/strikethrough
//! (not exposed by `vt100::Cell`).

use crate::glyph::GlyphMode;
use crate::{color, glyph};
use image::{Rgba, RgbaImage};
use std::collections::HashMap;

const CELL_PX: u32 = 16;
const GLYPH_PX: u32 = 8;
const SCALE: u32 = CELL_PX / GLYPH_PX;

/// A codepoint at a given (row, col) that `glyph::glyph_for` could not map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    /// The unmapped-glyph error, plus the cell position that hit it.
    Glyph(glyph::GlyphError, u16, u16),
}

/// `color::to_rgb` is context-free: for `vt100::Color::Default` it always
/// returns the default *foreground* shade (light gray), since it has no
/// way to know whether its caller means fg or bg. The ANSI convention —
/// default background is black — has to be applied by the caller for
/// background colors specifically; this mirrors the hardcoded black
/// already used below for cells with no data at all.
fn bg_to_rgb(c: vt100::Color) -> image::Rgb<u8> {
    match c {
        vt100::Color::Default => image::Rgb([0, 0, 0]),
        other => color::to_rgb(other),
    }
}

/// A rasterized screen, plus every unmapped-codepoint substitution made
/// while rendering it (`(codepoint) -> (cells substituted)`). Always
/// empty in `GlyphMode::Error`, since that mode fails outright via
/// `RenderError::Glyph` on the first unmapped codepoint rather than
/// substituting one. Callers fold this into `Caveat::
/// UnmappedGlyphSubstituted` entries for the run manifest.
#[derive(Debug)]
pub struct RenderedScreen {
    /// The rasterized image.
    pub image: RgbaImage,
    /// `(codepoint, cells substituted)`, one entry per distinct
    /// unmapped codepoint seen.
    pub substitutions: HashMap<char, usize>,
}

/// Rasterizes a parsed terminal screen to a 2x-upscaled RGBA image, one
/// 16x16 block per cell. `mode` governs what happens on an unmapped
/// codepoint: `GlyphMode::Error` hard-errors naming it and the cell
/// position (`RenderError::Glyph`), the same behavior this function has
/// always had; `GlyphMode::Substitute` draws `glyph::PLACEHOLDER_BOX` in
/// its place and records the substitution in the returned
/// [`RenderedScreen::substitutions`] instead of failing.
///
/// Does not call `glyph::glyph_for_mode` directly: that function's
/// `Result<[u8; 8], GlyphError>` return has no room to also report
/// *that* a substitution happened, which this function needs in order
/// to count it. Both functions share the same `glyph_for`-then-branch-
/// on-`mode` logic; `glyph_for_mode` exists as its own tested unit
/// because the brief specifies it as its own interface, not because
/// this function calls through it.
pub fn render_screen(
    screen: &vt100::Screen,
    mode: GlyphMode,
) -> Result<RenderedScreen, RenderError> {
    let (rows, cols) = screen.size();
    let mut img = RgbaImage::new(cols as u32 * CELL_PX, rows as u32 * CELL_PX);
    let mut substitutions: HashMap<char, usize> = HashMap::new();

    for row in 0..rows {
        for col in 0..cols {
            let cell = screen.cell(row, col);
            let (ch, fg, bg, bold, underline, inverse) = match cell {
                Some(c) => (
                    // Known caveat: this takes only the cell's first char,
                    // so a combining mark stacked onto a base character
                    // (rare in TTUI's own glyph set, but not impossible in
                    // arbitrary terminal output) is silently dropped rather
                    // than composed, and a double-width glyph's trailing
                    // continuation cell (which `vt100` represents as an
                    // empty-string cell) renders as blank rather than
                    // widened. Not a known issue in practice for TTUI's
                    // current widgets, which draw single-width glyphs.
                    c.contents().chars().next().unwrap_or(' '),
                    color::to_rgb(c.fgcolor()),
                    bg_to_rgb(c.bgcolor()),
                    c.bold(),
                    c.underline(),
                    c.inverse(),
                ),
                None => (
                    ' ',
                    color::to_rgb(vt100::Color::Default),
                    image::Rgb([0u8, 0, 0]),
                    false,
                    false,
                    false,
                ),
            };

            let mut fg = fg;
            let mut bg = bg;
            if bold {
                fg = color::brighten(fg);
            }
            if inverse {
                let (nf, nb) = color::swap(fg, bg);
                fg = nf;
                bg = nb;
            }

            let bitmap = match glyph::glyph_for(ch) {
                Ok(bitmap) => bitmap,
                Err(e) => match mode {
                    GlyphMode::Error => return Err(RenderError::Glyph(e, row, col)),
                    GlyphMode::Substitute => {
                        *substitutions.entry(ch).or_insert(0) += 1;
                        glyph::PLACEHOLDER_BOX
                    }
                },
            };

            let ox = col as u32 * CELL_PX;
            let oy = row as u32 * CELL_PX;
            for gy in 0..GLYPH_PX {
                let row_bits = bitmap[gy as usize];
                for gx in 0..GLYPH_PX {
                    let set = (row_bits >> gx) & 1 == 1;
                    let px = if set { fg } else { bg };
                    for sy in 0..SCALE {
                        for sx in 0..SCALE {
                            img.put_pixel(
                                ox + gx * SCALE + sx,
                                oy + gy * SCALE + sy,
                                Rgba([px.0[0], px.0[1], px.0[2], 255]),
                            );
                        }
                    }
                }
            }

            if underline {
                let fg_px = Rgba([fg.0[0], fg.0[1], fg.0[2], 255]);
                for x in 0..CELL_PX {
                    img.put_pixel(ox + x, oy + CELL_PX - 1, fg_px);
                }
            }
        }
    }

    Ok(RenderedScreen {
        image: img,
        substitutions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_PX: u32 = 16; // 8x8 glyph, 2x upscaled

    fn parse(bytes: &[u8]) -> vt100::Parser {
        let mut p = vt100::Parser::new(1, 4, 0);
        p.process(bytes);
        p
    }

    #[test]
    #[allow(clippy::identity_op)] // `1 * CELL_PX` spells out "1 row" for symmetry with `4 * CELL_PX` above it
    fn image_dimensions_match_screen_size_times_cell_px() {
        let parser = parse(b"abcd");
        let img = render_screen(parser.screen(), GlyphMode::Error)
            .unwrap()
            .image;
        assert_eq!(img.width(), 4 * CELL_PX);
        assert_eq!(img.height(), 1 * CELL_PX);
    }

    #[test]
    fn plain_text_uses_default_fg_over_default_bg() {
        let parser = parse(b"a");
        let img = render_screen(parser.screen(), GlyphMode::Error)
            .unwrap()
            .image;
        // A glyph's background rectangle should show through wherever
        // the 8x8 'a' bitmap has no set pixel — check a corner pixel
        // known to be background for every font8x8 letterform.
        let bg_pixel = img.get_pixel(0, 0);
        assert_eq!(*bg_pixel, image::Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn bold_text_brightens_the_foreground() {
        let parser = parse(b"\x1b[1ma\x1b[0m");
        let img = render_screen(parser.screen(), GlyphMode::Error)
            .unwrap()
            .image;
        // Find a foreground-colored pixel (non-background) and confirm
        // it's brighter than the plain-text case's foreground would be.
        let has_bright_pixel = img.pixels().any(|p| p.0[0] > 229);
        assert!(has_bright_pixel, "expected a brightened foreground pixel");
    }

    #[test]
    fn reverse_video_swaps_fg_and_bg_across_the_whole_cell() {
        let parser = parse(b"\x1b[7ma\x1b[0m");
        let img = render_screen(parser.screen(), GlyphMode::Error)
            .unwrap()
            .image;
        // With fg/bg swapped, the corner background pixel becomes the
        // (light) default foreground color instead of black.
        let corner = img.get_pixel(0, 0);
        assert_eq!(*corner, image::Rgba([229, 229, 229, 255]));
    }

    #[test]
    fn underline_draws_a_line_on_the_bottom_row_of_the_cell() {
        let parser = parse(b"\x1b[4m \x1b[0m"); // underlined space
        let img = render_screen(parser.screen(), GlyphMode::Error)
            .unwrap()
            .image;
        let bottom_row_pixel = img.get_pixel(0, CELL_PX - 1);
        assert_eq!(*bottom_row_pixel, image::Rgba([229, 229, 229, 255]));
    }

    #[test]
    fn unmapped_glyph_is_a_hard_error_naming_the_codepoint_and_position() {
        let mut p = vt100::Parser::new(1, 1, 0);
        p.process("\u{2726}".as_bytes());
        let err = render_screen(p.screen(), GlyphMode::Error).unwrap_err();
        assert_eq!(
            err,
            RenderError::Glyph(glyph::GlyphError::Unmapped('\u{2726}'), 0, 0)
        );
    }

    #[test]
    fn substitute_mode_draws_a_visible_placeholder_at_the_unmapped_cell() {
        let mut p = vt100::Parser::new(1, 1, 0);
        p.process("\u{2726}".as_bytes());
        let rendered = render_screen(p.screen(), GlyphMode::Substitute).unwrap();
        // PLACEHOLDER_BOX's top row (0xFF) sets every pixel across the
        // cell's top edge — real pixel data, not just a returned flag.
        let top_left = rendered.image.get_pixel(0, 0);
        assert_ne!(
            *top_left,
            image::Rgba([0, 0, 0, 255]),
            "expected the placeholder box to actually draw, not leave the cell blank"
        );
        assert_eq!(rendered.substitutions.get(&'\u{2726}'), Some(&1));
    }

    #[test]
    fn substitutions_are_counted_per_codepoint_for_the_manifest() {
        // Two cells of U+2726 and one of U+1F4A5 must disclose as such.
        let mut parser = vt100::Parser::new(4, 10, 0);
        parser.process("\u{2726}\u{2726}\u{1F4A5}".as_bytes());
        let counts = render_screen(parser.screen(), GlyphMode::Substitute)
            .unwrap()
            .substitutions;
        assert_eq!(counts.get(&'\u{2726}'), Some(&2));
        assert_eq!(counts.get(&'\u{1F4A5}'), Some(&1));
    }

    #[test]
    fn error_mode_still_hard_errors_even_with_a_mapped_codepoint_present() {
        // Guards against a mode-branch bug that substitutes everywhere:
        // a mapped glyph alongside an unmapped one, under Error mode,
        // must still hard-error rather than silently drawing the mapped
        // one and skipping the unmapped one.
        let mut p = vt100::Parser::new(1, 2, 0);
        p.process("A\u{2726}".as_bytes());
        let err = render_screen(p.screen(), GlyphMode::Error).unwrap_err();
        assert_eq!(
            err,
            RenderError::Glyph(glyph::GlyphError::Unmapped('\u{2726}'), 0, 1)
        );
    }
}
