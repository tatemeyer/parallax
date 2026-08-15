//! Looks up 8x8 bitmaps for characters TTUI actually draws, across the
//! specific `font8x8` tables that cover them. Deliberately does not fall
//! back to silently blank glyphs: an unmapped codepoint is a hard error.

use font8x8::{UnicodeFonts, BASIC_FONTS, BLOCK_FONTS, BOX_FONTS, LATIN_FONTS, MISC_FONTS};

/// A codepoint that none of the checked `font8x8` tables cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphError {
    /// The unmapped character.
    Unmapped(char),
}

/// Looks up `ch`'s 8x8 bitmap across every font8x8 table TTUI's actual
/// glyph set draws from, plus algorithmically-generated Braille Patterns
/// glyphs (`braille_glyph_for`) — `font8x8` doesn't cover that block at
/// all. Returns a hard error naming the codepoint if nothing covers it —
/// see `GlyphError::Unmapped`.
pub fn glyph_for(ch: char) -> Result<[u8; 8], GlyphError> {
    if let Some(bitmap) = braille_glyph_for(ch) {
        return Ok(bitmap);
    }
    BASIC_FONTS
        .get(ch)
        .or_else(|| LATIN_FONTS.get(ch))
        .or_else(|| BLOCK_FONTS.get(ch))
        .or_else(|| BOX_FONTS.get(ch))
        .or_else(|| MISC_FONTS.get(ch))
        .ok_or(GlyphError::Unmapped(ch))
}

/// Renders a Braille Patterns codepoint (U+2800-U+28FF, the block TTUI's
/// `Canvas` in `Braille` mode emits — see `src/canvas.rs`'s `blit_braille`)
/// as an 8x8 bitmap. `font8x8` has no table for this block at all, but the
/// block's encoding makes it trivial to render algorithmically: each of
/// the low 8 bits of `ch - U+2800` directly names one dot in the
/// character's fixed 2-column x 4-row dot grid (bit-to-dot-position
/// layout below mirrors `blit_braille`'s own `DOT_BITS` exactly, so a
/// snapshot's braille rendering matches what `Canvas` itself considers
/// "on"). Each dot is scaled to a 4x2-pixel block within the 8x8 cell.
/// Returns `None` for any codepoint outside the block, so callers can
/// fall through to the font8x8 tables unconditionally.
fn braille_glyph_for(ch: char) -> Option<[u8; 8]> {
    let cp = ch as u32;
    if !(0x2800..=0x28FF).contains(&cp) {
        return None;
    }
    let mask = (cp - 0x2800) as u8;
    // bit index -> (dot_row 0..4, dot_col 0..2); matches
    // `src/canvas.rs`'s `blit_braille::DOT_BITS` layout exactly.
    const DOT_POSITIONS: [(u8, u8); 8] = [
        (0, 0),
        (1, 0),
        (2, 0),
        (0, 1),
        (1, 1),
        (2, 1),
        (3, 0),
        (3, 1),
    ];
    let mut bitmap = [0u8; 8];
    for (bit, &(dot_row, dot_col)) in DOT_POSITIONS.iter().enumerate() {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let px_row_start = dot_row * 2;
        let px_col_start = dot_col * 4;
        for r in 0..2 {
            for c in 0..4 {
                bitmap[(px_row_start + r) as usize] |= 1 << (px_col_start + c);
            }
        }
    }
    Some(bitmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_letter_resolves_to_a_nonblank_bitmap() {
        let bitmap = glyph_for('A').unwrap();
        assert!(
            bitmap.iter().any(|row| *row != 0),
            "expected 'A' to draw at least one pixel"
        );
    }

    #[test]
    fn space_and_a_resolve_to_different_bitmaps() {
        assert_ne!(glyph_for(' ').unwrap(), glyph_for('A').unwrap());
    }

    #[test]
    fn block_element_glyphs_resolve() {
        // The block-elements TTUI widgets actually emit (src/glitch.rs, src/canvas.rs).
        for ch in ['░', '▒', '▓', '█', '▀', '▄', '▌'] {
            glyph_for(ch).unwrap_or_else(|e| panic!("expected {ch:?} to be mapped, got {e:?}"));
        }
    }

    #[test]
    fn ascii_border_glyphs_resolve() {
        for ch in ['-', '|', '+'] {
            glyph_for(ch).unwrap();
        }
    }

    #[test]
    fn dingbat_star_is_unmapped() {
        // Confirmed during spec review: font8x8's MISC_FONTS does not
        // reach the Dingbats block. EnergyCore's charged-state glyph
        // is expected to hit this path in real use.
        let err = glyph_for('\u{2726}').unwrap_err();
        assert_eq!(err, GlyphError::Unmapped('\u{2726}'));
    }

    #[test]
    fn blank_braille_pattern_is_an_all_zero_bitmap() {
        // U+2800 itself: every dot in the 2x4 grid is off.
        assert_eq!(glyph_for('\u{2800}').unwrap(), [0u8; 8]);
    }

    #[test]
    fn fully_set_braille_pattern_fills_every_pixel() {
        // U+28FF: mask 0xFF, every dot on -> every pixel of the 8x8 cell set.
        assert_eq!(glyph_for('\u{28FF}').unwrap(), [0xFFu8; 8]);
    }

    #[test]
    fn single_top_left_dot_lights_only_its_quadrant() {
        // U+2801: mask 0x01 (bit0), the top-left dot only — rows 0-1,
        // columns 0-3 (the left half of the top two pixel rows).
        let bitmap = glyph_for('\u{2801}').unwrap();
        assert_eq!(bitmap, [0x0F, 0x0F, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn single_bottom_right_dot_lights_only_its_quadrant() {
        // U+28FF's bit7 alone would be 0x80 -> U+2880: the bottom-right
        // dot only — rows 6-7, columns 4-7.
        let bitmap = glyph_for('\u{2880}').unwrap();
        assert_eq!(bitmap, [0, 0, 0, 0, 0, 0, 0xF0, 0xF0]);
    }

    #[test]
    fn braille_bit_layout_matches_canvas_rs_dot_bits() {
        // src/canvas.rs's `blit_braille` DOT_BITS: bit0/bit3 = row0
        // col0/col1, bit1/bit4 = row1, bit2/bit5 = row2, bit6/bit7 =
        // row3. Spot-check bit3 (top-right dot, U+2808) lands in the
        // top rows' right half, matching that layout rather than a
        // naive top-to-bottom bit order.
        let bitmap = glyph_for('\u{2808}').unwrap();
        assert_eq!(bitmap, [0xF0, 0xF0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn non_braille_codepoints_still_fall_through_to_font8x8() {
        // Guards against `braille_glyph_for`'s range check accidentally
        // swallowing codepoints outside U+2800-U+28FF.
        assert!(glyph_for('A').is_ok());
        assert_eq!(
            glyph_for('\u{2726}').unwrap_err(),
            GlyphError::Unmapped('\u{2726}')
        );
    }
}
