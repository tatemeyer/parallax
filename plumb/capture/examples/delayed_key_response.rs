//! Fixture binary for `pty_roundtrip` integration tests: waits a short but
//! nontrivial delay (`RESPONSE_DELAY`) after reading a key before drawing
//! its response, mirroring a real TUI app that does layout/redraw work in
//! response to input rather than echoing instantaneously. Proves the
//! post-`Key`-step capture path actually waits to observe the reaction
//! instead of declaring quiescence on zero observed change (see the final
//! review fix report's finding #4). Exits immediately on Esc, without the
//! response delay, so teardown isn't slowed down.
use crossterm::event::{self, Event};
use crossterm::terminal;
use std::io::Write;
use std::time::Duration;

const RESPONSE_DELAY: Duration = Duration::from_millis(180);

fn main() -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut out = std::io::stdout();
    loop {
        if let Event::Key(key) = event::read()? {
            if key.code == event::KeyCode::Esc {
                break;
            }
            std::thread::sleep(RESPONSE_DELAY);
            write!(out, "{:?}", key.code)?;
            out.flush()?;
        }
    }
    terminal::disable_raw_mode()?;
    Ok(())
}
