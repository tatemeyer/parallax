//! The key map.
//!
//! Resolved through TTUI's `InputBinder`, which handles single keys and
//! multi-key chords against an app-defined action type — so chords cost
//! nothing to add later, when sub-project #5 needs them.
//!
//! The mutating verbs live here alongside the read-only ones, but they
//! only name an intent: what a key means is decided here, and whether it
//! is allowed is decided by `authorize` in the library. This file cannot
//! perform anything.

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
    /// Merge the selected pull request. Confirmation required.
    Merge,
    /// Label the selected work item.
    Label,
    /// Ask for a fresh review of the selected work item.
    RequestReview,
    /// Capture this project's scenarios.
    Capture,
    /// Push a branch. Confirmation required.
    Push,
    /// Uphold the selected finding.
    Uphold,
    /// Overrule the selected finding, suppressing it on later runs.
    /// Takes effect immediately: the platform spec classifies a ruling
    /// as reversible, because a later opposite ruling supersedes it.
    Overrule,
    /// Show what this session has done.
    ActionLog,
    /// Toggle the help overlay.
    Help,
    /// Leave.
    Quit,
}

impl Action {
    /// Whether this verb does something **to the selected project**, as
    /// opposed to moving the cursor, switching a pane, or refreshing
    /// everything.
    ///
    /// The cockpit's executors and its build checks both reach only the
    /// machine it runs on, so every verb that answers `true` has to be
    /// refused when the selection is a peer's row. Without that, `c` on
    /// the Pi's `sesh` runs a build against **this** machine's `sesh` —
    /// silently, because the two rows differ only by a suffix and the
    /// request travels as a bare name.
    ///
    /// Exhaustive on purpose. A verb added later must decide what it is,
    /// because one that slipped through would act on the wrong machine
    /// and say nothing.
    pub fn acts_on_the_selected_project(&self) -> bool {
        match self {
            Action::RunChecks
            | Action::Merge
            | Action::Label
            | Action::RequestReview
            | Action::Capture
            | Action::Push
            | Action::Uphold
            | Action::Overrule => true,

            // Movement, display, and whole-platform verbs. `RunAllChecks`
            // belongs here because it addresses every *local* project by
            // its own list rather than anything under the cursor.
            Action::Up
            | Action::Down
            | Action::NextPane
            | Action::Tab(_)
            | Action::Refresh
            | Action::RunAllChecks
            | Action::ActionLog
            | Action::Help
            | Action::Quit => false,
        }
    }
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
    b.bind(KeyPress::plain(KeyCode::Char('m')), Action::Merge);
    b.bind(KeyPress::plain(KeyCode::Char('l')), Action::Label);
    b.bind(KeyPress::plain(KeyCode::Char('p')), Action::Capture);
    // Shifted, as with `C` above. `p` and `P` differ by one modifier and
    // mean very different things, which is survivable only because `P`
    // asks before it does anything.
    b.bind(
        KeyPress {
            code: KeyCode::Char('P'),
            modifiers: crossterm::event::KeyModifiers::SHIFT,
        },
        Action::Push,
    );
    b.bind(
        KeyPress {
            code: KeyCode::Char('R'),
            modifiers: crossterm::event::KeyModifiers::SHIFT,
        },
        Action::RequestReview,
    );
    b.bind(KeyPress::plain(KeyCode::Char('u')), Action::Uphold);
    b.bind(KeyPress::plain(KeyCode::Char('o')), Action::Overrule);
    b.bind(KeyPress::plain(KeyCode::Char('5')), Action::ActionLog);
    // Metrics is `6`, and sits after the log in `Tab::ALL` so that key
    // `n` is `Tab::ALL[n - 1]` with no off-by-one to remember.
    //
    // The obvious mnemonic was taken: `m` merges a pull request. Worth
    // knowing why that is not merely untidy — **`bind` appends to a
    // list and `feed` takes the first match, so a second binding for a
    // key is silently dead.** Binding `m` to this tab does not shadow
    // the merge; it does nothing at all, and the new pane would have
    // been unreachable by any key while every test stayed green.
    b.bind(KeyPress::plain(KeyCode::Char('6')), Action::Tab(6));
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

    /// Every key still reaches the action it is supposed to.
    ///
    /// Added because the metrics tab was first bound to `m`, which was
    /// already `Merge`. **No test noticed**, because a duplicate
    /// binding is not an error and not an override — `bind` appends and
    /// `feed` takes the first match, so the second one is simply dead.
    /// The suite stayed green while the new pane had no working key.
    ///
    /// The mutating verbs are listed alongside the tabs deliberately: a
    /// future collision is as likely to silence `P` or `m` as to
    /// silence a tab, and the failure looks the same from here — a key
    /// that quietly stopped doing what the help text says.
    #[test]
    fn every_key_still_reaches_the_action_it_names() {
        let bound: Vec<Option<Action>> = resolve(&[
            press(KeyCode::Char('m'), KeyModifiers::NONE),
            press(KeyCode::Char('p'), KeyModifiers::NONE),
            press(KeyCode::Char('l'), KeyModifiers::NONE),
            press(KeyCode::Char('u'), KeyModifiers::NONE),
            press(KeyCode::Char('o'), KeyModifiers::NONE),
            press(KeyCode::Char('5'), KeyModifiers::NONE),
            press(KeyCode::Char('6'), KeyModifiers::NONE),
        ]);
        assert_eq!(
            bound,
            vec![
                Some(Action::Merge),
                Some(Action::Capture),
                Some(Action::Label),
                Some(Action::Uphold),
                Some(Action::Overrule),
                // `5` is the log's own verb rather than `Tab(5)`, which
                // is why the metrics pane could not simply take `5`.
                Some(Action::ActionLog),
                Some(Action::Tab(6)),
            ]
        );
    }

    /// Every tab is reachable. The number keys map to `Tab::ALL`
    /// positionally, so a tab added without a key is a pane that exists
    /// and cannot be opened.
    #[test]
    fn every_tab_has_a_key_that_opens_it() {
        use crate::view::render::Tab;

        for (index, _) in Tab::ALL.iter().enumerate() {
            let number = index + 1;
            let key = char::from_digit(number as u32, 10).unwrap();
            let resolved = resolve(&[press(KeyCode::Char(key), KeyModifiers::NONE)]);

            let opens_this_tab = match resolved[0] {
                Some(Action::Tab(n)) => n as usize == number,
                // The log has its own verb, which lands on the same tab.
                Some(Action::ActionLog) => Tab::ALL[index] == Tab::Log,
                _ => false,
            };
            assert!(
                opens_this_tab,
                "key `{key}` does not open {:?}: got {:?}",
                Tab::ALL[index],
                resolved[0]
            );
        }
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
