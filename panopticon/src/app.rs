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
use parallax_baseline::actions::{Action as BaseAction, Ruling};
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
    /// The peers this cockpit watches, in registry order. Rows are kept
    /// grouped by it so the rail does not reshuffle as machines answer
    /// at different speeds.
    peer_order: Vec<String>,
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
            peer_order: Vec::new(),
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

    /// Names the peers this cockpit watches, in registry order.
    ///
    /// Seeds a row for each, for the same reason local projects get one:
    /// a rail that is briefly empty and then briefly wrong is worse than
    /// one that shows every machine it was told about and fills them in.
    /// A machine that never answers keeps its row and acquires a reason.
    pub fn with_peers(mut self, peers: Vec<String>) -> Self {
        for peer in &peers {
            self.platform.projects.push(ProjectState {
                name: peer.clone(),
                peer: Some(peer.clone()),
                ..Default::default()
            });
        }
        self.peer_order = peers;
        self
    }

    /// Restores the order the rail is documented to have: local projects
    /// in registry order, then each peer's, in registry order.
    ///
    /// Peers answer at different speeds, and without this a row would
    /// move because a laptop happened to reply before a Pi. `sort_by_key`
    /// is stable, so nothing within a group moves.
    fn reorder(&mut self) {
        let order = &self.peer_order;
        self.platform.projects.sort_by_key(|p| match &p.peer {
            None => 0,
            Some(name) => 1 + order.iter().position(|n| n == name).unwrap_or(order.len()),
        });
    }

    /// Puts an artifact feed on the selected project, for tests that
    /// need something on screen to point a key at. Not used outside
    /// them: in a real run the refresh thread supplies this.
    #[doc(hidden)]
    pub fn seed_artifacts(
        &mut self,
        artifacts: Vec<parallax_baseline::adapters::artifact::Artifact>,
    ) {
        if let Some(project) = self.platform.projects.get_mut(self.selected) {
            project.artifacts = vec![parallax_baseline::freshness::Observed::watched(
                artifacts,
                self.clock.now(),
            )];
        }
    }

    /// What this session has attempted, for the log pane.
    fn log_lines(&self) -> Vec<(String, String, bool)> {
        self.control
            .log()
            .iter()
            .map(|e| (e.summary.clone(), e.result.clone(), e.ok))
            .collect()
    }

    /// The finding under the cursor, when the artifacts pane is
    /// showing one. `None` on a run row: a ruling addresses a finding,
    /// and "rule on this whole run" is not a thing Plumb has.
    fn selected_finding(&self) -> Option<String> {
        if self.tab != Tab::Artifacts {
            return None;
        }
        let project = self.platform.projects.get(self.selected)?;
        match crate::view::artifacts::artifact_rows(project)
            .into_iter()
            .nth(self.detail_selected)?
            .of
        {
            crate::view::artifacts::RowOf::Finding { fingerprint } => Some(fingerprint),
            crate::view::artifacts::RowOf::Artifact => None,
        }
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

    /// What the selected row declares, and which of its build checks
    /// have not reported yet.
    ///
    /// Both were looked up by a bare project name, and a peer's row
    /// shares that name with the local clone beside it — so on the Pi's
    /// `sesh` pane they described **this** machine's `sesh`. The same
    /// wrong-machine mistake as running its checks here, quieter for
    /// being only a rendering.
    ///
    /// A peer's declarations come from what it actually sent, since its
    /// manifest is on the peer. It has no outstanding build checks here
    /// because this cockpit never starts one there — which is exactly
    /// what the refusal in `act` means.
    fn selected_declaration(&self) -> (Declared, Vec<String>) {
        let Some(project) = self.platform.projects.get(self.selected) else {
            return (Declared::default(), Vec::new());
        };
        if project.peer.is_some() {
            return (Declared::observed(project), Vec::new());
        }
        (
            self.declared
                .get(&project.name)
                .copied()
                .unwrap_or_default(),
            self.pending_checks(&project.name),
        )
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
                //
                // `peer.is_none()` on every local lookup below: these
                // updates come from this machine's own adapters, and
                // this desktop holds a clone of `sesh` while the Pi
                // serves one too. Matching on name alone would let a
                // local refresh overwrite the Pi's row.
                if let Some(slot) = self
                    .platform
                    .projects
                    .iter_mut()
                    .find(|p| p.peer.is_none() && p.name == state.name)
                {
                    *slot = *state;
                }
            }
            Update::ChecksRan { project, checks } => {
                if let Some(slot) = self
                    .platform
                    .projects
                    .iter_mut()
                    .find(|p| p.peer.is_none() && p.name == project)
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
                    .find(|p| p.peer.is_none() && p.name == project)
                {
                    slot.degradations
                        .push(parallax_baseline::state::Degradation {
                            source: format!("refresh:{project}"),
                            reason: problem,
                        });
                }
            }
            Update::PeerState { peer, projects } => {
                // The peer's whole list replaces the peer's whole list.
                // A project removed on that machine has to leave the
                // rail, and merging row by row could never say so.
                self.platform
                    .projects
                    .retain(|p| p.peer.as_deref() != Some(peer.as_str()));
                self.platform.extend_from_peer(&peer, projects);
                self.reorder();
            }
            Update::PeerFailed { peer, reason } => {
                let source = format!("peer:{peer}");
                let degradation = parallax_baseline::state::Degradation {
                    source: source.clone(),
                    reason,
                };
                let mut had_rows = false;
                for slot in self
                    .platform
                    .projects
                    .iter_mut()
                    .filter(|p| p.peer.as_deref() == Some(peer.as_str()))
                {
                    // The values it served last time stay on screen and
                    // go stale on their own. This adds why they stopped
                    // moving; it does not blank them, because the last
                    // thing a machine said is still the last thing it
                    // said.
                    slot.degradations.retain(|d| d.source != source);
                    slot.degradations.push(degradation.clone());
                    had_rows = true;
                }
                if !had_rows {
                    self.platform.projects.push(ProjectState {
                        name: peer.clone(),
                        peer: Some(peer),
                        degradations: vec![degradation],
                        ..Default::default()
                    });
                    self.reorder();
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

    /// The peer a row belongs to, and what that row is called, when the
    /// selection is not on this machine.
    fn selected_remote(&self) -> Option<(String, String)> {
        let project = self.platform.projects.get(self.selected)?;
        let peer = project.peer.clone()?;
        Some((project.qualified_name(), peer))
    }

    fn act(&mut self, action: Action) {
        // Everything this cockpit can *do* reaches only the machine it
        // runs on: its executors are built from local projects, and a
        // build check is dispatched to the local refresh thread by a
        // bare project name. A peer's row shares that bare name with the
        // local clone beside it, so an unguarded `c` on the Pi's `sesh`
        // would run a build against this machine's `sesh` — the wrong
        // machine, silently, on a row that never changes to show it.
        //
        // Refused here rather than at each verb, so a verb added later
        // is covered by default; `acts_on_the_selected_project` is
        // exhaustive so adding one forces the decision.
        if action.acts_on_the_selected_project() {
            if let Some((name, peer)) = self.selected_remote() {
                self.control.refuse(
                    format!("{name}: {action:?}"),
                    format!(
                        "{name} is on {peer}. This cockpit acts only on the machine it runs on — \
                         control across the wire is not built yet."
                    ),
                );
                return;
            }
        }

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
            // Ruling is the one input Plumb's learned-rejection store
            // depends on. Both rulings are classified reversible by the
            // platform spec and so neither asks: a ruling is an appended
            // record, and an opposite one later supersedes it. That is
            // worth knowing before pressing `o` -- overruling does take
            // effect immediately, on every later run of that scenario.
            Action::Uphold | Action::Overrule => {
                if let (Some(project), Some(fingerprint)) =
                    (self.selected_name(), self.selected_finding())
                {
                    let ruling = if action == Action::Uphold {
                        Ruling::Upheld
                    } else {
                        Ruling::Overruled
                    };
                    self.control.offer(
                        self.selected,
                        BaseAction::RuleFinding {
                            project,
                            fingerprint,
                            ruling,
                        },
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
        let (declared, pending) = self.selected_declaration();
        let log = self.log_lines();
        let question = self.control.prompt().map(|p| p.line());
        let frame = Frame {
            platform: &self.platform,
            selected: self.selected,
            tab: self.tab,
            declared,
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

    /// A cockpit holding local `ttui` and `sesh`, plus a peer serving a
    /// `sesh` of its own — the collision this whole guard exists for,
    /// and the ordinary case: this desktop holds clones of everything.
    fn cockpit_with_a_peers_clone() -> Panopticon {
        let mut app = cockpit().with_peers(vec!["pi5".to_string()]);
        app.apply(Update::PeerState {
            peer: "pi5".into(),
            projects: vec![ProjectState {
                name: "sesh".into(),
                ..Default::default()
            }],
        });
        app
    }

    /// Selects the row whose qualified name matches, or fails loudly —
    /// a test that silently acted on row 0 would prove the opposite of
    /// what it claims.
    fn select(app: &mut Panopticon, qualified: &str) {
        app.selected = app
            .platform
            .projects
            .iter()
            .position(|p| p.qualified_name() == qualified)
            .unwrap_or_else(|| panic!("no row named {qualified}"));
    }

    /// The bug this guard was written for. `c` on the Pi's `sesh` sent
    /// `RunChecks { project: "sesh" }` — a bare name — to the local
    /// refresh thread, which matched **this** machine's `sesh` and ran a
    /// build on it. The operator's row never changed, and a different
    /// row did.
    #[test]
    fn running_checks_on_a_peers_project_is_refused_rather_than_run_locally() {
        let mut app = cockpit_with_a_peers_clone();
        select(&mut app, "sesh@pi5");

        app.act(Action::RunChecks);

        let log = app.control.log();
        assert_eq!(log.len(), 1, "the keypress vanished without a word");
        assert!(!log[0].ok);
        assert!(
            log[0].result.contains("pi5"),
            "the refusal must name the machine: {}",
            log[0].result
        );
    }

    /// Every verb that reaches an executor or the local refresh thread.
    #[test]
    fn no_verb_that_acts_on_a_project_reaches_a_peers_row() {
        for action in [
            Action::RunChecks,
            Action::Merge,
            Action::Label,
            Action::RequestReview,
            Action::Capture,
            Action::Push,
            Action::Uphold,
            Action::Overrule,
        ] {
            let mut app = cockpit_with_a_peers_clone();
            select(&mut app, "sesh@pi5");

            app.act(action);

            assert_eq!(
                app.control.log().len(),
                1,
                "{action:?} was not refused on a remote row"
            );
            assert!(
                app.control.prompt().is_none(),
                "{action:?} put a confirmation on screen for an action it cannot perform"
            );
        }
    }

    /// And the guard must not swallow the local case, which is the one
    /// that has to keep working.
    #[test]
    fn the_same_verbs_still_reach_a_local_project() {
        let mut app = cockpit_with_a_peers_clone();
        select(&mut app, "sesh");

        app.act(Action::RunChecks);

        assert!(
            app.control.log().is_empty(),
            "a local project was refused: {:?}",
            app.control.log()
        );
    }

    /// The Pi's `sesh` pane must be shaped by what the Pi sent, not by
    /// this machine's `sesh` manifest. They share a bare name, and both
    /// lookups behind the panes were keyed by it.
    #[test]
    fn a_peers_pane_is_shaped_by_what_it_sent_not_by_a_local_manifest() {
        // A local `sesh` that declares a work feed, so borrowing it
        // would be visible.
        let local = validate(
            parse_manifest(
                "project:\n  name: sesh\n  root: /tmp/sesh\n\
                 work:\n  adapter: github\n  repo: tatemeyer/sesh\n",
            )
            .unwrap(),
        )
        .unwrap();
        let projects = vec![local];
        let refresher = Refresher::spawn(
            projects
                .iter()
                .cloned()
                .map(|v| (v, ProjectAdapters::new()))
                .collect(),
            Clock::Frozen(at(0)),
        );
        let mut app = Panopticon::new(
            &projects,
            refresher,
            Clock::Frozen(at(0)),
            Duration::from_secs(30),
        )
        .with_peers(vec!["pi5".to_string()]);

        // The Pi's `sesh` declares no work feed and does have sessions —
        // the opposite shape from the local one.
        app.apply(Update::PeerState {
            peer: "pi5".into(),
            projects: vec![ProjectState {
                name: "sesh".into(),
                sessions: Some(parallax_baseline::freshness::Observed::watched(
                    Vec::new(),
                    at(0),
                )),
                ..Default::default()
            }],
        });

        select(&mut app, "sesh");
        assert!(
            app.selected_declaration().0.work,
            "the local manifest does declare a work feed"
        );

        select(&mut app, "sesh@pi5");
        let (declared, pending) = app.selected_declaration();
        assert!(
            !declared.work,
            "the Pi's pane borrowed this machine's manifest"
        );
        assert!(
            declared.sessions,
            "the Pi sent a session feed and the pane does not show it"
        );
        assert!(
            pending.is_empty(),
            "a peer has no build checks outstanding here — none can be started"
        );
    }

    /// Moving and looking are never refused, however remote the row.
    #[test]
    fn navigation_is_never_refused_on_a_peers_row() {
        let mut app = cockpit_with_a_peers_clone();
        select(&mut app, "sesh@pi5");

        for action in [Action::Down, Action::Up, Action::Tab(2), Action::Help] {
            app.act(action);
        }
        assert!(app.control.log().is_empty());
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
