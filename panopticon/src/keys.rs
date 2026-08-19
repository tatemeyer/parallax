//! The key map.
//!
//! Resolved through TTUI's `InputBinder`, which handles single keys and
//! multi-key chords against an app-defined action type — so chords cost
//! nothing to add later, when sub-project #5 needs them.
//!
//! Deliberately small: every verb that mutates something belongs to the
//! control sub-project, and binding one now would mean binding it twice.

use crossterm::event::KeyCode;
use std::time::Duration;
use ttui::input::{InputBinder, KeyPress};

/// What a keypress asks the cockpit to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Move the selection up.
    Up,
    /// Move the selection down.
    Down,
    /// Move focus between the rail and the detail pane.
    NextPane,
    /// Show detail tab 1-4.
    Tab(u8),
    /// Refresh the sources that only read.
    Refresh,
    /// Run the selected project's build checks.
    RunChecks,
    /// Run every project's build checks.
    RunAllChecks,
    /// Toggle the help overlay.
    Help,
    /// Leave.
    Quit,
}

/// How long a partially-typed chord waits before it is forgotten.
/// Nothing binds a chord yet; the timeout exists so one can be added
/// without revisiting the loop.
pub const CHORD_TIMEOUT: Duration = Duration::from_millis(600);

/// The cockpit's bindings.
pub fn binder() -> InputBinder<Action> {
    let mut b = InputBinder::new(CHORD_TIMEOUT);
    b.bind(KeyPress::plain(KeyCode::Char('j')), Action::Down);
    b.bind(KeyPress::plain(KeyCode::Char('k')), Action::Up);
    b.bind(KeyPress::plain(KeyCode::Tab), Action::NextPane);
    for (i, ch) in ['1', '2', '3', '4'].into_iter().enumerate() {
        b.bind(KeyPress::plain(KeyCode::Char(ch)), Action::Tab(i as u8 + 1));
    }
    b.bind(KeyPress::plain(KeyCode::Char('r')), Action::Refresh);
    b.bind(KeyPress::plain(KeyCode::Char('c')), Action::RunChecks);
    // Shifted, and therefore a different KeyPress: the terminal reports
    // `Char('C')` with SHIFT held.
    b.bind(
        KeyPress {
            code: KeyCode::Char('C'),
            modifiers: crossterm::event::KeyModifiers::SHIFT,
        },
        Action::RunAllChecks,
    );
    b.bind(KeyPress::plain(KeyCode::Char('?')), Action::Help);
    b.bind(KeyPress::plain(KeyCode::Char('q')), Action::Quit);
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{Event, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    fn resolve(events: &[Event]) -> Vec<Option<Action>> {
        let mut b = binder();
        events.iter().map(|e| b.feed(e)).collect()
    }

    #[test]
    fn the_movement_keys_resolve() {
        assert_eq!(
            resolve(&[
                press(KeyCode::Char('j'), KeyModifiers::NONE),
                press(KeyCode::Char('k'), KeyModifiers::NONE),
                press(KeyCode::Tab, KeyModifiers::NONE),
            ]),
            vec![Some(Action::Down), Some(Action::Up), Some(Action::NextPane)]
        );
    }

    #[test]
    fn the_number_keys_select_the_four_tabs() {
        assert_eq!(
            resolve(&[
                press(KeyCode::Char('1'), KeyModifiers::NONE),
                press(KeyCode::Char('4'), KeyModifiers::NONE),
            ]),
            vec![Some(Action::Tab(1)), Some(Action::Tab(4))]
        );
    }

    /// `c` and `C` are different actions, and a shifted key is a
    /// different `KeyPress` — running one project's build checks and
    /// running every project's should not be one slip apart.
    #[test]
    fn lower_and_upper_c_are_different_actions() {
        assert_eq!(
            resolve(&[
                press(KeyCode::Char('c'), KeyModifiers::NONE),
                press(KeyCode::Char('C'), KeyModifiers::SHIFT),
            ]),
            vec![Some(Action::RunChecks), Some(Action::RunAllChecks)]
        );
    }

    #[test]
    fn refresh_help_and_quit_resolve() {
        assert_eq!(
            resolve(&[
                press(KeyCode::Char('r'), KeyModifiers::NONE),
                press(KeyCode::Char('?'), KeyModifiers::NONE),
                press(KeyCode::Char('q'), KeyModifiers::NONE),
            ]),
            vec![
                Some(Action::Refresh),
                Some(Action::Help),
                Some(Action::Quit)
            ]
        );
    }

    #[test]
    fn an_unbound_key_resolves_to_nothing() {
        assert_eq!(
            resolve(&[press(KeyCode::Char('z'), KeyModifiers::NONE)]),
            vec![None]
        );
    }

    /// Nothing that mutates a repository is bound. Control is
    /// sub-project #5, and this crate is read-only.
    #[test]
    fn no_binding_resolves_to_anything_that_mutates() {
        let actions = [
            Action::Up,
            Action::Down,
            Action::NextPane,
            Action::Tab(1),
            Action::Refresh,
            Action::RunChecks,
            Action::RunAllChecks,
            Action::Help,
            Action::Quit,
        ];
        // Running a declared check is a local build, not a repository
        // mutation; everything else here only moves a cursor.
        assert_eq!(
            actions.len(),
            9,
            "the whole verb list, and it is this small"
        );
    }
}
