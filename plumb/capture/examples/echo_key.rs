//! Minimal fixture binary for `pty_roundtrip` integration tests: echoes
//! the debug representation of each key it receives, exits on Esc.
use crossterm::event::{self, Event};
use crossterm::terminal;
use std::io::Write;

fn main() -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut out = std::io::stdout();
    loop {
        if let Event::Key(key) = event::read()? {
            write!(out, "{:?}", key.code)?;
            out.flush()?;
            if key.code == event::KeyCode::Esc {
                break;
            }
        }
    }
    terminal::disable_raw_mode()?;
    Ok(())
}
