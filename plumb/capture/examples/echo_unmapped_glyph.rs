//! Fixture binary for `adapter::pty`'s substitute-mode tests: draws a
//! real unmapped codepoint (U+2726, the Dingbats star `glyph::glyph_for`
//! cannot map — see `glyph.rs`'s `dingbat_star_is_unmapped` test) as
//! soon as it starts, then blocks like `echo_key` so a single
//! `capture_frame` after spawn sees it. Exists so `adapter::pty`'s
//! `GlyphMode::Substitute` tests exercise a real spawned process and a
//! real rasterized frame, not just a hand-built `vt100::Parser` screen
//! (that path is already covered directly by `render.rs`'s own tests).
use crossterm::event::{self, Event};
use crossterm::terminal;
use std::io::Write;

fn main() -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut out = std::io::stdout();
    write!(out, "\u{2726}")?;
    out.flush()?;
    loop {
        if let Event::Key(key) = event::read()? {
            if key.code == event::KeyCode::Esc {
                break;
            }
        }
    }
    terminal::disable_raw_mode()?;
    Ok(())
}
