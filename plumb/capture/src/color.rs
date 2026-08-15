//! Maps `vt100`'s parsed terminal colors (16-color, 256-color, and true
//! color) to RGB pixels, plus small approximations for SGR attributes
//! (`bold`, `reverse`) that `render.rs` applies on top of a cell's base
//! fg/bg color.

use image::Rgb;
use vt100::Color;

const BASIC_16: [Rgb<u8>; 16] = [
    Rgb([0, 0, 0]),       // 0 black
    Rgb([205, 0, 0]),     // 1 red
    Rgb([0, 205, 0]),     // 2 green
    Rgb([205, 205, 0]),   // 3 yellow
    Rgb([0, 0, 238]),     // 4 blue
    Rgb([205, 0, 205]),   // 5 magenta
    Rgb([0, 205, 205]),   // 6 cyan
    Rgb([229, 229, 229]), // 7 white
    Rgb([127, 127, 127]), // 8 bright black
    Rgb([255, 0, 0]),     // 9 bright red
    Rgb([0, 255, 0]),     // 10 bright green
    Rgb([255, 255, 0]),   // 11 bright yellow
    Rgb([92, 92, 255]),   // 12 bright blue
    Rgb([255, 0, 255]),   // 13 bright magenta
    Rgb([0, 255, 255]),   // 14 bright cyan
    Rgb([255, 255, 255]), // 15 bright white
];

fn ansi256_to_rgb(idx: u8) -> Rgb<u8> {
    match idx {
        0..=15 => BASIC_16[idx as usize],
        16..=231 => {
            let i = idx - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Rgb([level(r), level(g), level(b)])
        }
        232..=255 => {
            let v = 8 + (idx - 232) * 10;
            Rgb([v, v, v])
        }
    }
}

/// Maps a parsed `vt100::Color` to an RGB pixel value.
pub fn to_rgb(c: Color) -> Rgb<u8> {
    match c {
        Color::Default => Rgb([229, 229, 229]),
        Color::Idx(i) => ansi256_to_rgb(i),
        Color::Rgb(r, g, b) => Rgb([r, g, b]),
    }
}

/// Approximates SGR bold by lightening each channel toward white.
pub fn brighten(c: Rgb<u8>) -> Rgb<u8> {
    Rgb([
        c.0[0].saturating_add(40),
        c.0[1].saturating_add(40),
        c.0[2].saturating_add(40),
    ])
}

/// Approximates SGR reverse video by exchanging fg/bg.
pub fn swap(fg: Rgb<u8>, bg: Rgb<u8>) -> (Rgb<u8>, Rgb<u8>) {
    (bg, fg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;
    use vt100::Color;

    #[test]
    fn default_color_maps_to_the_conventional_terminal_default() {
        // Matches the ANSI convention: default fg is light gray, default bg is black.
        assert_eq!(to_rgb(Color::Default), Rgb([229, 229, 229]));
    }

    #[test]
    fn rgb_color_passes_through_unchanged() {
        assert_eq!(to_rgb(Color::Rgb(10, 20, 30)), Rgb([10, 20, 30]));
    }

    #[test]
    fn basic_16_indices_map_to_standard_ansi_colors() {
        assert_eq!(to_rgb(Color::Idx(1)), Rgb([205, 0, 0])); // red
        assert_eq!(to_rgb(Color::Idx(2)), Rgb([0, 205, 0])); // green
        assert_eq!(to_rgb(Color::Idx(9)), Rgb([255, 0, 0])); // bright red
    }

    #[test]
    fn color_cube_indices_map_via_the_standard_xterm_formula() {
        // idx 16 is the cube's origin (0,0,0) -> black.
        assert_eq!(to_rgb(Color::Idx(16)), Rgb([0, 0, 0]));
        // idx 231 is the cube's far corner (5,5,5) -> white-ish (255,255,255).
        assert_eq!(to_rgb(Color::Idx(231)), Rgb([255, 255, 255]));
    }

    #[test]
    fn grayscale_ramp_indices_map_to_increasing_gray() {
        assert_eq!(to_rgb(Color::Idx(232)), Rgb([8, 8, 8]));
        assert_eq!(to_rgb(Color::Idx(255)), Rgb([238, 238, 238]));
    }

    #[test]
    fn brighten_lightens_each_channel_toward_white_and_saturates() {
        assert_eq!(brighten(Rgb([100, 100, 100])), Rgb([140, 140, 140]));
        assert_eq!(brighten(Rgb([250, 0, 0])), Rgb([255, 40, 40]));
    }

    #[test]
    fn swap_exchanges_fg_and_bg() {
        let fg = Rgb([1, 2, 3]);
        let bg = Rgb([4, 5, 6]);
        assert_eq!(swap(fg, bg), (bg, fg));
    }
}
