//! Fixture binary for `pty_roundtrip` integration tests: sleeps for
//! `STARTUP_DELAY` — deliberately longer than the old fixed
//! `SETTLE_DELAY` (100ms) that `Session::capture_frame` used to sleep
//! unconditionally — before writing anything, then blocks like
//! `echo_key` so a test can capture a frame and confirm the delayed
//! draw was still picked up correctly. Mirrors a real TUI app's
//! variable startup-to-first-draw latency (see the Task 12 flakiness
//! fix) without the nondeterminism of driving a real example.
use crossterm::event::{self, Event};
use crossterm::terminal;
use std::io::Write;
use std::time::Duration;

const STARTUP_DELAY: Duration = Duration::from_millis(500);

fn main() -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    std::thread::sleep(STARTUP_DELAY);
    let mut out = std::io::stdout();
    write!(out, "READY")?;
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
