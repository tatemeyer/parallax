//! The event loop.
//!
//! `on_tick` drains the refresh thread and returns. `update` applies a
//! keypress. **Neither ever calls an adapter** — `r`, `c`, and `C` send
//! a request and come straight back, because a key that performs I/O on
//! the UI thread is the rejected design wearing a different hat.

use crate::bell::Bell;
use crate::control::Control;
use crate::keys::{binder, Action};
use crate::refresh::{Clock, Refresher, Request, Update};
use crate::view::model::Declared;
use crate::view::render::{render, Frame, Tab};
use crossterm::event::{Event, KeyCode, KeyEventKind};
use parallax_baseline::actions::Action as BaseAction;
use parallax_baseline::state::{PlatformState, ProjectState};
use parallax_baseline::validate::Validated;
use std::collections::BTreeMap;
use std::time::Duration;
use ttui::app::App;
use ttui::buffer::LayerStack;
use ttui::input::InputBinder;
use ttui::layout::Rect;

/// How often the loop wakes. Refresh latency is bounded by one tick, and
/// for data whose own freshness budget is thirty seconds that is
/// imperceptible.
pub const TICK: Duration = Duration::from_millis(100);

/// The cockpit.
pub struct Panopticon {
    platform: PlatformState,
    declared: BTreeMap<String, Declared>,
    /// Which build checks have reported since this session started, per
    /// project. Anything a project declares that is not in here reads
    /// "not run this session".
    ran: BTreeMap<String, Vec<String>>,
    refresher: Refresher,
    binder: InputBinder<Action>,
    clock: Clock,
    poll_interval: Duration,
    since_refresh: Duration,
    selected: usize,
    tab: Tab,
    detail_selected: usize,
    help: bool,
    quit: bool,
    bell: Bell,
    control: Control,
}

impl Panopticon {
    /// A cockpit over the given projects, fed by `refresher`.
    pub fn new(
        projects: &[Validated],
        refresher: Refresher,
        clock: Clock,
        poll_interval: Duration,
    ) -> Self {
        let mut platform = PlatformState::default();
        let mut declared = BTreeMap::new();
        for validated in projects {
            let name = validated.manifest().project.name.clone();
            declared.insert(name.clone(), Declared::of(validated));
            // A row per registered project from the first frame, so the
            // rail is never briefly empty and then briefly wrong.
            platform.projects.push(ProjectState {
                name: name.clone(),
                methodology: validated.manifest().project.methodology.clone(),
                language: validated.manifest().project.language.clone(),
                ..Default::default()
            });
        }
        Self {
            platform,
            declared,
            ran: BTreeMap::new(),
            refresher,
            binder: binder(),
            clock,
            poll_interval,
            since_refresh: poll_interval, // refresh on the first tick
            selected: 0,
            tab: Tab::Work,
            detail_selected: 0,
            help: false,
            quit: false,
            bell: Bell::default(),
            // Inert until a caller hands over executors. A cockpit run
            // against fixtures keeps this one and refuses out loud.
            control: Control::inert(projects.len()),
        }
    }

    /// Gives the cockpit the ability to act, one executor per project in
    /// registry order. Without this it observes and refuses, which is
    /// exactly what fixture mode wants.
    pub fn with_control(mut self, control: Control) -> Self {
        self.control = control;
        self
    }

    /// What this session has attempted, for the log pane.
    fn log_lines(&self) -> Vec<(String, String, bool)> {
        self.control
            .log()
            .iter()
            .map(|e| (e.summary.clone(), e.result.clone(), e.ok))
            .collect()
    }

    /// The work row under the cursor, when the work pane is showing one.
    fn selected_work(&self) -> Option<crate::view::work::WorkRow> {
        if self.tab != Tab::Work {
            return None;
        }
        let project = self.platform.projects.get(self.selected)?;
        crate::view::work::work_rows(project)
            .into_iter()
            .nth(self.detail_selected)
    }

    /// The selected project's name, when there is one.
    fn selected_name(&self) -> Option<String> {
        self.platform
            .projects
            .get(self.selected)
            .map(|p| p.name.clone())
    }

    /// Build checks this project declares that have not reported yet.
    fn pending_checks(&self, project: &str) -> Vec<String> {
        let ran = self.ran.get(project).map(Vec::as_slice).unwrap_or(&[]);
        self.refresher
            .executor_kinds(project)
            .iter()
            .filter(|kind| !ran.contains(kind))
            .cloned()
            .collect()
    }

    /// Applies one update from the refresh thread.
    fn apply(&mut self, update: Update) {
        match update {
            Update::Project(state) => {
                // An update naming a project the registry never listed is
                // dropped: the registry is the source of which projects
                // exist, and a row no manifest backs would be a lie.
                if let Some(slot) = self
                    .platform
                    .projects
                    .iter_mut()
                    .find(|p| p.name == state.name)
                {
                    *slot = *state;
                }
            }
            Update::ChecksRan { project, checks } => {
                if let Some(slot) = self
                    .platform
                    .projects
                    .iter_mut()
                    .find(|p| p.name == project)
                {
                    for check in checks {
                        let kind = check.value.kind.clone();
                        self.ran
                            .entry(project.clone())
                            .or_default()
                            .push(kind.clone());
                        match slot.verification.iter_mut().find(|v| v.value.kind == kind) {
                            Some(existing) => *existing = check,
                            None => slot.verification.push(check),
                        }
                    }
                }
            }
            Update::Failed { project, problem } => {
                if let Some(slot) = self
                    .platform
                    .projects
                    .iter_mut()
                    .find(|p| p.name == project)
                {
                    slot.degradations
                        .push(parallax_baseline::state::Degradation {
                            source: format!("refresh:{project}"),
                            reason: problem,
                        });
                }
            }
        }
    }

    /// How many rows the detail pane currently holds, for clamping.
    fn detail_len(&self) -> usize {
        self.platform
            .projects
            .get(self.selected)
            .map(|p| match self.tab {
                Tab::Work => crate::view::work::work_rows(p).len(),
                Tab::Verification => p.verification.len().max(1),
                Tab::Artifacts => crate::view::artifacts::artifact_rows(p).len(),
                Tab::Sessions => crate::view::sessions::session_rows(p, self.clock.now()).len(),
                Tab::Log => self.control.log().len(),
            })
            .unwrap_or(0)
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::Down => {
                let last = self.detail_len().saturating_sub(1);
                self.detail_selected = (self.detail_selected + 1).min(last);
            }
            Action::Up => self.detail_selected = self.detail_selected.saturating_sub(1),
            Action::NextPane => {
                let last = self.platform.projects.len().saturating_sub(1);
                self.selected = if self.selected >= last {
                    0
                } else {
                    self.selected + 1
                };
                self.detail_selected = 0;
            }
            Action::Tab(n) => {
                self.tab = Tab::ALL[(n as usize - 1).min(Tab::ALL.len() - 1)];
                self.detail_selected = 0;
            }
            Action::ActionLog => {
                self.tab = Tab::Log;
                self.detail_selected = 0;
            }
            // Requests, not work. Nothing below this line touches an
            // adapter, which is what keeps the loop responsive.
            Action::Refresh => {
                self.refresher.request(Request::RefreshReads);
                self.since_refresh = Duration::ZERO;
            }
            Action::RunChecks => {
                if let Some(project) = self.selected_name() {
                    self.refresher.request(Request::RunChecks { project });
                }
            }
            Action::RunAllChecks => self.refresher.request(Request::RunAllChecks),
            // The control verbs. Each builds an action from what is
            // under the cursor and offers it; whether it may happen is
            // `authorize`'s call and whether it does is the operator's.
            // Nothing here performs anything.
            Action::Merge => {
                if let (Some(project), Some(row)) = (self.selected_name(), self.selected_work()) {
                    // Issues do not merge. Offering it anyway would put
                    // a question on screen whose only answer is no.
                    if row.kind == '>' {
                        self.control.offer(
                            self.selected,
                            BaseAction::MergePullRequest {
                                project,
                                number: row.number,
                            },
                        );
                    }
                }
            }
            Action::Label => {
                if let (Some(project), Some(row)) = (self.selected_name(), self.selected_work()) {
                    let question = format!("label #{}", row.number);
                    self.control.ask(
                        BaseAction::SetAutonomyLabel {
                            project,
                            item: row.number,
                            label: String::new(),
                        },
                        question,
                    );
                }
            }
            Action::RequestReview => {
                if let (Some(project), Some(row)) = (self.selected_name(), self.selected_work()) {
                    self.control.offer(
                        self.selected,
                        BaseAction::RequestReReview {
                            project,
                            item: row.number,
                        },
                    );
                }
            }
            Action::Capture => {
                if let Some(project) = self.selected_name() {
                    self.control.offer(
                        self.selected,
                        BaseAction::TriggerCapture {
                            project,
                            scenario: None,
                        },
                    );
                }
            }
            Action::Push => {
                if let Some(project) = self.selected_name() {
                    self.control.ask(
                        BaseAction::Push {
                            project,
                            branch: String::new(),
                        },
                        "push which branch".to_string(),
                    );
                }
            }
            Action::Help => self.help = !self.help,
            Action::Quit => self.quit = true,
        }
    }
}

impl App for Panopticon {
    fn update(&mut self, event: &Event) {
        // A question owns the keyboard while it is up. This is what the
        // cockpit's one modal actually means: `q` does not quit and `j`
        // does not move, because both would be answers to a question
        // nobody read.
        if self.control.prompt().is_some() {
            if let Event::Key(key) = event {
                // Windows reports a Release for every Press, and the
                // binder filters them where the read-only keys are
                // resolved. This path reads raw events, so it has to do
                // the same — without it, the very key that opens a
                // prompt types itself into it, and a merge confirmation
                // greets the operator pre-filled with `m`.
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Esc => self.control.cancel(),
                    KeyCode::Enter => {
                        self.control.key(self.selected, None);
                    }
                    KeyCode::Backspace => {
                        self.control.key(self.selected, Some('\u{8}'));
                    }
                    KeyCode::Char(c) => {
                        self.control.key(self.selected, Some(c));
                    }
                    _ => {}
                }
            }
            return;
        }
        if let Some(action) = self.binder.feed(event) {
            self.act(action);
        }
    }

    fn view(&self, area: Rect, buf: &mut LayerStack) {
        let name = self.selected_name().unwrap_or_default();
        let pending = self.pending_checks(&name);
        let log = self.log_lines();
        let question = self.control.prompt().map(|p| p.line());
        let frame = Frame {
            platform: &self.platform,
            selected: self.selected,
            tab: self.tab,
            declared: self.declared.get(&name).copied().unwrap_or_default(),
            pending_checks: &pending,
            now: self.clock.now(),
            detail_selected: self.detail_selected,
            log: &log,
            question: question.as_deref(),
            alarm: self.bell.ringing(self.clock.now()),
        };
        render(&frame, area, buf.push_layer());
    }

    fn should_quit(&self) -> bool {
        self.quit
    }

    fn tick_rate(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn on_tick(&mut self, elapsed: Duration) {
        self.binder.expire(elapsed);

        // The cadence lives here rather than in the thread, so a test can
        // drive it without waiting for a real interval to pass.
        self.since_refresh += elapsed;
        if self.since_refresh >= self.poll_interval {
            self.refresher.request(Request::RefreshReads);
            self.since_refresh = Duration::ZERO;
        }

        for update in self.refresher.drain() {
            self.apply(update);
        }

        // After the updates, not before: the bell reports what the frame
        // is about to show.
        self.bell.observe(&self.platform, self.clock.now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use parallax_baseline::manifest::parse_manifest;
    use parallax_baseline::state::ProjectAdapters;
    use parallax_baseline::validate::validate;
    use std::time::SystemTime;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    fn validated(name: &str) -> Validated {
        validate(
            parse_manifest(&format!("project:\n  name: {name}\n  root: /tmp/{name}\n")).unwrap(),
        )
        .unwrap()
    }

    /// A cockpit over two projects with no adapters at all: the refresh
    /// thread has nothing to poll, which is exactly what the UI tests
    /// want.
    fn cockpit() -> Panopticon {
        let projects = vec![validated("ttui"), validated("sesh")];
        let refresher = Refresher::spawn(
            projects
                .iter()
                .cloned()
                .map(|v| (v, ProjectAdapters::new()))
                .collect(),
            Clock::Frozen(at(0)),
        );
        Panopticon::new(
            &projects,
            refresher,
            Clock::Frozen(at(0)),
            Duration::from_secs(30),
        )
    }

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    #[test]
    fn every_registered_project_has_a_row_from_the_first_frame() {
        let app = cockpit();
        assert_eq!(app.platform.projects.len(), 2);
        assert_eq!(app.platform.projects[0].name, "ttui");
    }

    #[test]
    fn q_quits() {
        let mut app = cockpit();
        assert!(!app.should_quit());
        app.update(&press(KeyCode::Char('q')));
        assert!(app.should_quit());
    }

    #[test]
    fn tab_moves_between_projects_and_wraps() {
        let mut app = cockpit();
        app.update(&press(KeyCode::Tab));
        assert_eq!(app.selected, 1);
        app.update(&press(KeyCode::Tab));
        assert_eq!(app.selected, 0, "wraps rather than sticking at the end");
    }

    #[test]
    fn the_number_keys_select_a_detail_tab() {
        let mut app = cockpit();
        app.update(&press(KeyCode::Char('3')));
        assert_eq!(app.tab, Tab::Artifacts);
    }

    /// Nothing to scroll, so the selection must not run off the end.
    #[test]
    fn moving_down_an_empty_pane_stays_put() {
        let mut app = cockpit();
        app.update(&press(KeyCode::Char('j')));
        assert_eq!(app.detail_selected, 0);
    }

    #[test]
    fn the_tick_rate_is_the_documented_one() {
        assert_eq!(cockpit().tick_rate(), Some(TICK));
    }

    /// An update naming a project the registry never listed is dropped —
    /// the registry is the source of which projects exist.
    #[test]
    fn an_update_for_an_unknown_project_is_ignored_rather_than_appended() {
        let mut app = cockpit();
        app.apply(Update::Project(Box::new(ProjectState {
            name: "not-registered".into(),
            ..Default::default()
        })));
        assert_eq!(app.platform.projects.len(), 2);
        assert!(app.platform.project("not-registered").is_none());
    }

    #[test]
    fn a_project_update_replaces_that_projects_row_and_no_other() {
        let mut app = cockpit();
        app.apply(Update::Project(Box::new(ProjectState {
            name: "sesh".into(),
            language: Some("rust".into()),
            ..Default::default()
        })));
        assert_eq!(
            app.platform.project("sesh").unwrap().language.as_deref(),
            Some("rust")
        );
        assert_eq!(
            app.platform.projects[0].name, "ttui",
            "and ttui is untouched"
        );
    }

    #[test]
    fn a_failed_refresh_degrades_that_project_rather_than_vanishing() {
        let mut app = cockpit();
        app.apply(Update::Failed {
            project: "ttui".into(),
            problem: "the manifest moved".into(),
        });
        let ttui = app.platform.project("ttui").unwrap();
        assert_eq!(ttui.degradations.len(), 1);
        assert!(ttui.degradations[0].reason.contains("manifest moved"));
    }

    /// Several updates queued between ticks are all applied in one tick.
    #[test]
    fn on_tick_drains_everything_waiting() {
        let mut app = cockpit();
        for name in ["ttui", "sesh"] {
            app.apply(Update::Project(Box::new(ProjectState {
                name: name.into(),
                language: Some("applied".into()),
                ..Default::default()
            })));
        }
        assert!(app
            .platform
            .projects
            .iter()
            .all(|p| p.language.as_deref() == Some("applied")));
    }
}
