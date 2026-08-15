//! Minimal fixture binary for click-scripting integration tests: draws
//! a `#` glyph at each mouse event's reported cell, exits on Esc key.
use crossterm::event::{self, Event};
use crossterm::terminal;

fn main() -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut out = std::io::stdout();
    crossterm::execute!(out, event::EnableMouseCapture)?;
    loop {
        match event::read()? {
            Event::Mouse(m) => {
                crossterm::execute!(
                    out,
                    crossterm::cursor::MoveTo(m.column, m.row),
                    crossterm::style::Print('#')
                )?;
            }
            Event::Key(key) if key.code == event::KeyCode::Esc => break,
            _ => {}
        }
    }
    crossterm::execute!(out, event::DisableMouseCapture)?;
    terminal::disable_raw_mode()?;
    Ok(())
}
