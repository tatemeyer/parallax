//! The interactive `App` -- the only code in this crate that touches a
//! terminal. Everything it does is delegate: input to selection/screen
//! state, current state to `overview::render_overview` or
//! `detail::render_detail`, plus the Cloister Bell overlay on top of
//! whichever screen is active.

use crate::bell::{self, DegradationSet};
use crate::detail::render_detail;
use crate::overview::render_overview;
use crossterm::event::{Event, KeyCode};
use parallax_baseline::state::{aggregate, PlatformState, ProjectAdapters};
use parallax_baseline::validate::Validated;
use std::time::SystemTime;
use ttui::app::App;
use ttui::buffer::LayerStack;
use ttui::layout::Rect;

/// Which screen is currently on top. The Bell is not a screen of its
/// own -- it overlays whichever of these is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Overview,
    Detail,
}

/// The cockpit's `App`. Owns the adapters (so a manual refresh can
/// re-poll them), the platform state they last produced, which screen
/// is open, and the Cloister Bell's dismissal state. Read-only: no key
/// here ever writes anything back to a project.
pub struct PanopticonApp {
    inputs: Vec<(Validated, ProjectAdapters)>,
    platform: PlatformState,
    selected: usize,
    screen: Screen,
    /// Whether the user dismissed the Bell for the current degradation
    /// set. Reset to `false` (i.e. the Bell reappears) only when
    /// `last_degradations` changes -- not on every refresh.
    bell_dismissed: bool,
    last_degradations: DegradationSet,
    quit: bool,
}

impl PanopticonApp {
    /// Builds the app and runs the first aggregation immediately, so
    /// the very first frame already shows real (or honestly degraded)
    /// state rather than a blank screen.
    pub fn new(mut inputs: Vec<(Validated, ProjectAdapters)>) -> Self {
        let platform = aggregate(&mut inputs, SystemTime::now());
        let mut app = PanopticonApp {
            inputs,
            platform,
            selected: 0,
            screen: Screen::Overview,
            bell_dismissed: false,
            last_degradations: DegradationSet::new(),
            quit: false,
        };
        app.note_degradations();
        app
    }

    fn refresh(&mut self) {
        self.platform = aggregate(&mut self.inputs, SystemTime::now());
        let last = self.platform.projects.len().saturating_sub(1);
        if self.selected > last {
            self.selected = last;
        }
        self.note_degradations();
    }

    /// Compares this aggregation's degradation set against the last one
    /// the Bell was shown (or dismissed) for. A changed set un-dismisses
    /// the Bell -- this is the entire "reappears only when the
    /// degradation set changes" rule; an unchanged set leaves whatever
    /// the user last did alone, which is what keeps a plain refresh from
    /// resurrecting a Bell the user already dismissed.
    fn note_degradations(&mut self) {
        let current = bell::degradation_set(&self.platform);
        if current != self.last_degradations {
            self.bell_dismissed = false;
            self.last_degradations = current;
        }
    }

    /// Whether the Bell should currently be drawn.
    fn bell_visible(&self) -> bool {
        !self.bell_dismissed && !self.last_degradations.is_empty()
    }

    fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn select_down(&mut self) {
        if self.selected + 1 < self.platform.projects.len() {
            self.selected += 1;
        }
    }

    fn open_detail(&mut self) {
        if !self.platform.projects.is_empty() {
            self.screen = Screen::Detail;
        }
    }

    /// `Esc`'s two jobs, in priority order: close the Detail screen if
    /// it's open, otherwise dismiss the Bell. Never both from one
    /// keypress -- the design's "back, or dismiss the bell" is an
    /// either/or, not a combination.
    fn escape(&mut self) {
        match self.screen {
            Screen::Detail => self.screen = Screen::Overview,
            Screen::Overview => self.bell_dismissed = true,
        }
    }
}

impl App for PanopticonApp {
    fn update(&mut self, event: &Event) {
        let Event::Key(key) = event else { return };
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Enter => self.open_detail(),
            KeyCode::Esc => self.escape(),
            KeyCode::Up if self.screen == Screen::Overview => self.select_up(),
            KeyCode::Down if self.screen == Screen::Overview => self.select_down(),
            _ => {}
        }
    }

    fn view(&self, area: Rect, buf: &mut LayerStack) {
        let now = SystemTime::now();
        let mut rendered = match self.screen {
            Screen::Overview => {
                render_overview(&self.platform, self.selected, now, area.width, area.height)
            }
            Screen::Detail => {
                // `selected` only ever indexes an existing project:
                // `open_detail` refuses to switch screens when the
                // platform has none, and the project count never
                // shrinks between refreshes (same manifests, re-polled).
                render_detail(
                    &self.platform.projects[self.selected],
                    now,
                    area.width,
                    area.height,
                )
            }
        };
        if self.bell_visible() {
            let banner = bell::render_bell(&self.platform, area.width);
            banner.blit(&mut rendered, 0, 0);
        }
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
    use parallax_baseline::state::{Degradation, ProjectState};
    use ttui::buffer::LayerStack;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn platform_of(names: &[&str]) -> PlatformState {
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
        // exercise selection/screen state.
        let mut app = PanopticonApp {
            inputs: Vec::new(),
            platform: platform_of(names),
            selected: 0,
            screen: Screen::Overview,
            bell_dismissed: false,
            last_degradations: DegradationSet::new(),
            quit: false,
        };
        app.note_degradations();
        app
    }

    fn composite_row(app: &PanopticonApp, width: u16, height: u16, y: u16) -> String {
        let mut stack = LayerStack::new(width, height);
        app.view(
            Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            &mut stack,
        );
        let buf = stack.composite();
        (0..width).map(|x| buf.get(x, y).symbol).collect()
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

    // --- Detail screen: opened with Enter, closed with Esc ---

    #[test]
    fn enter_opens_the_detail_screen_for_the_selected_project() {
        let mut app = app_with(&["a", "b"]);
        app.update(&key(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Detail);
    }

    #[test]
    fn enter_on_an_empty_platform_does_not_open_detail() {
        let mut app = app_with(&[]);
        app.update(&key(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Overview);
    }

    #[test]
    fn esc_closes_the_detail_screen_and_returns_to_overview() {
        let mut app = app_with(&["a"]);
        app.update(&key(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Detail);
        app.update(&key(KeyCode::Esc));
        assert_eq!(app.screen, Screen::Overview);
    }

    #[test]
    fn up_and_down_do_nothing_while_the_detail_screen_is_open() {
        let mut app = app_with(&["a", "b"]);
        app.update(&key(KeyCode::Enter));
        app.update(&key(KeyCode::Down));
        assert_eq!(app.selected, 0, "selection belongs to the Overview screen");
    }

    #[test]
    fn the_detail_screen_renders_the_selected_projects_name() {
        let mut app = app_with(&["ttui", "other"]);
        app.selected = 1;
        app.update(&key(KeyCode::Enter));
        let row = composite_row(&app, 40, 10, 0);
        assert!(row.starts_with("other"), "{row:?}");
    }

    // --- Cloister Bell: appears with degradations, dismissible, and ---
    // --- reappears only when the degradation set actually changes.  ---

    fn with_degradation(mut platform: PlatformState, project: &str, source: &str) -> PlatformState {
        for p in &mut platform.projects {
            if p.name == project {
                p.degradations.push(Degradation {
                    source: source.to_string(),
                    reason: "unreachable".into(),
                });
            }
        }
        platform
    }

    #[test]
    fn the_bell_is_visible_on_startup_when_a_project_is_already_degraded() {
        let mut app = app_with(&["a"]);
        app.platform = with_degradation(platform_of(&["a"]), "a", "work:github");
        app.note_degradations();
        assert!(app.bell_visible());
    }

    #[test]
    fn a_clean_platform_never_shows_the_bell() {
        let app = app_with(&["a", "b"]);
        assert!(!app.bell_visible());
    }

    #[test]
    fn esc_on_the_overview_screen_dismisses_the_bell() {
        let mut app = app_with(&["a"]);
        app.platform = with_degradation(platform_of(&["a"]), "a", "work:github");
        app.note_degradations();
        assert!(app.bell_visible());

        app.update(&key(KeyCode::Esc));
        assert!(!app.bell_visible());
    }

    #[test]
    fn a_refresh_with_the_same_degradation_set_does_not_resurrect_a_dismissed_bell() {
        let mut app = app_with(&["a"]);
        app.platform = with_degradation(platform_of(&["a"]), "a", "work:github");
        app.note_degradations();
        app.bell_dismissed = true;

        // Same degradation set again -- a no-op refresh, not a change.
        app.platform = with_degradation(platform_of(&["a"]), "a", "work:github");
        app.note_degradations();

        assert!(
            !app.bell_visible(),
            "the set did not change, so the dismissal must stand"
        );
    }

    #[test]
    fn a_changed_degradation_set_un_dismisses_the_bell() {
        let mut app = app_with(&["a"]);
        app.platform = with_degradation(platform_of(&["a"]), "a", "work:github");
        app.note_degradations();
        app.bell_dismissed = true;

        // A *different* source now degrades too -- the set changed.
        app.platform = with_degradation(
            with_degradation(platform_of(&["a"]), "a", "work:github"),
            "a",
            "session:filesystem",
        );
        app.note_degradations();

        assert!(
            app.bell_visible(),
            "a changed degradation set must un-dismiss the bell"
        );
    }

    #[test]
    fn a_cleared_degradation_hides_the_bell_and_a_later_new_one_shows_it_again() {
        let mut app = app_with(&["a"]);
        app.platform = with_degradation(platform_of(&["a"]), "a", "work:github");
        app.note_degradations();
        assert!(app.bell_visible());

        // The source recovers: the set changes back to empty.
        app.platform = platform_of(&["a"]);
        app.note_degradations();
        assert!(!app.bell_visible());

        // A new problem appears later.
        app.platform = with_degradation(platform_of(&["a"]), "a", "verification:command:lint");
        app.note_degradations();
        assert!(app.bell_visible());
    }

    #[test]
    fn the_bell_renders_over_the_overview_screen_when_visible() {
        let mut app = app_with(&["a"]);
        app.platform = with_degradation(platform_of(&["a"]), "a", "work:github");
        app.note_degradations();

        let row = composite_row(&app, 80, 10, 0);
        assert!(row.trim_start().starts_with('!'), "{row:?}");
    }

    #[test]
    fn the_bell_renders_over_the_detail_screen_too() {
        let mut app = app_with(&["a"]);
        app.platform = with_degradation(platform_of(&["a"]), "a", "work:github");
        app.note_degradations();
        app.update(&key(KeyCode::Enter));

        let row = composite_row(&app, 80, 10, 0);
        assert!(row.trim_start().starts_with('!'), "{row:?}");
    }

    #[test]
    fn esc_closes_detail_first_leaving_a_visible_bell_untouched() {
        // One Esc while Detail is open must close Detail, not also
        // dismiss the Bell -- "back, or dismiss," never both at once.
        let mut app = app_with(&["a"]);
        app.platform = with_degradation(platform_of(&["a"]), "a", "work:github");
        app.note_degradations();
        app.update(&key(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Detail);

        app.update(&key(KeyCode::Esc));
        assert_eq!(app.screen, Screen::Overview);
        assert!(app.bell_visible(), "the bell must still be showing");

        app.update(&key(KeyCode::Esc));
        assert!(!app.bell_visible());
    }
}
