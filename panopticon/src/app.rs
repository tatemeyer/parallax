//! The interactive `App` -- the only code in this crate that touches a
//! terminal. Everything it does is delegate: input to selection state,
//! current state to `overview::render_overview`.

use crate::overview::render_overview;
use crossterm::event::{Event, KeyCode};
use parallax_baseline::state::{aggregate, PlatformState, ProjectAdapters};
use parallax_baseline::validate::Validated;
use std::time::SystemTime;
use ttui::app::App;
use ttui::buffer::LayerStack;
use ttui::layout::Rect;

/// The cockpit's Overview screen. Owns the adapters (so a manual
/// refresh can re-poll them) and the platform state they last produced.
/// Read-only: no key here ever writes anything back to a project.
pub struct PanopticonApp {
    inputs: Vec<(Validated, ProjectAdapters)>,
    platform: PlatformState,
    selected: usize,
    quit: bool,
}

impl PanopticonApp {
    /// Builds the app and runs the first aggregation immediately, so
    /// the very first frame already shows real (or honestly degraded)
    /// state rather than a blank screen.
    pub fn new(mut inputs: Vec<(Validated, ProjectAdapters)>) -> Self {
        let platform = aggregate(&mut inputs, SystemTime::now());
        PanopticonApp {
            inputs,
            platform,
            selected: 0,
            quit: false,
        }
    }

    fn refresh(&mut self) {
        self.platform = aggregate(&mut self.inputs, SystemTime::now());
        let last = self.platform.projects.len().saturating_sub(1);
        if self.selected > last {
            self.selected = last;
        }
    }

    fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn select_down(&mut self) {
        if self.selected + 1 < self.platform.projects.len() {
            self.selected += 1;
        }
    }
}

impl App for PanopticonApp {
    fn update(&mut self, event: &Event) {
        let Event::Key(key) = event else { return };
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Up => self.select_up(),
            KeyCode::Down => self.select_down(),
            KeyCode::Char('r') => self.refresh(),
            // Enter (detail) and Esc (back/dismiss) belong to screens
            // this dispatch does not build yet.
            _ => {}
        }
    }

    fn view(&self, area: Rect, buf: &mut LayerStack) {
        let rendered = render_overview(
            &self.platform,
            self.selected,
            SystemTime::now(),
            area.width,
            area.height,
        );
        rendered.blit(buf, area.x, area.y);
    }

    fn should_quit(&self) -> bool {
        self.quit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn platform_of(names: &[&str]) -> PlatformState {
        use parallax_baseline::state::ProjectState;
        PlatformState {
            projects: names
                .iter()
                .map(|n| ProjectState {
                    name: n.to_string(),
                    ..Default::default()
                })
                .collect(),
        }
    }

    fn app_with(names: &[&str]) -> PanopticonApp {
        // Built directly rather than through `new`, so these tests
        // don't need a real `Validated`/`ProjectAdapters` pair just to
        // exercise selection.
        PanopticonApp {
            inputs: Vec::new(),
            platform: platform_of(names),
            selected: 0,
            quit: false,
        }
    }

    #[test]
    fn q_requests_quit() {
        let mut app = app_with(&["a"]);
        app.update(&key(KeyCode::Char('q')));
        assert!(app.should_quit());
    }

    #[test]
    fn down_moves_selection_forward_and_stops_at_the_last_row() {
        let mut app = app_with(&["a", "b"]);
        app.update(&key(KeyCode::Down));
        assert_eq!(app.selected, 1);
        app.update(&key(KeyCode::Down));
        assert_eq!(app.selected, 1, "no third row to move to");
    }

    #[test]
    fn up_moves_selection_backward_and_stops_at_the_first_row() {
        let mut app = app_with(&["a", "b"]);
        app.selected = 1;
        app.update(&key(KeyCode::Up));
        assert_eq!(app.selected, 0);
        app.update(&key(KeyCode::Up));
        assert_eq!(app.selected, 0, "already at the top");
    }

    #[test]
    fn selection_on_an_empty_platform_never_panics() {
        let mut app = app_with(&[]);
        app.update(&key(KeyCode::Down));
        app.update(&key(KeyCode::Up));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn a_non_key_event_is_a_no_op() {
        let mut app = app_with(&["a"]);
        let before = app.selected;
        app.update(&Event::FocusGained);
        assert_eq!(app.selected, before);
        assert!(!app.should_quit());
    }
}
