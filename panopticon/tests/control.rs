//! The control surface at the keyboard, where the operator actually is.
//!
//! `control`'s own tests assert the decision. These assert the thing a
//! person does: press a key, see a question, answer it or do not — and
//! in particular that a question up on screen owns the keyboard, which
//! is the whole content of the cockpit having one modal.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use panopticon::app::Panopticon;
use panopticon::control::Control;
use panopticon::refresh::{Clock, Refresher};
use parallax_baseline::actions::{ActionError, ActionExecutor, ActionOutcome, Authorized, Effect};
use parallax_baseline::freshness::DEFAULT_POLL_INTERVAL;
use parallax_baseline::manifest::parse_manifest;
use parallax_baseline::state::ProjectAdapters;
use parallax_baseline::validate::{validate, Validated};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use ttui::app::App;

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

fn validated() -> Validated {
    validate(
        parse_manifest(
            "project:\n  name: ttui\n  root: /tmp/ttui\nwork:\n  adapter: github\n  \
             repo: tatemeyer/ttui\n  autonomy_map: {}\n",
        )
        .unwrap(),
    )
    .unwrap()
}

/// Records what reached it. Counting rather than panicking: the
/// question is whether an executor was reached at all, and a panic
/// would only prove it was reached loudly.
struct Recording {
    calls: Arc<AtomicUsize>,
    performed: Arc<Mutex<Vec<String>>>,
}

impl ActionExecutor for Recording {
    fn execute(&mut self, authorized: Authorized<'_>) -> Result<ActionOutcome, ActionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let summary = authorized.action().summary();
        self.performed.lock().unwrap().push(summary.clone());
        Ok(ActionOutcome {
            summary,
            effects: vec![Effect::CalledApi {
                method: "PUT".into(),
                url: "https://api.github.com/x".into(),
            }],
        })
    }
}

struct Harness {
    app: Panopticon,
    calls: Arc<AtomicUsize>,
    performed: Arc<Mutex<Vec<String>>>,
}

fn harness() -> Harness {
    let calls = Arc::new(AtomicUsize::new(0));
    let performed = Arc::new(Mutex::new(Vec::new()));
    let projects = vec![(validated(), ProjectAdapters::default())];
    let clock = Clock::Frozen(at(0));
    let refresher = Refresher::spawn(projects, clock);
    let control = Control::new(vec![Some(Box::new(Recording {
        calls: calls.clone(),
        performed: performed.clone(),
    }))]);
    let app = Panopticon::new(&[validated()], refresher, clock, DEFAULT_POLL_INTERVAL)
        .with_control(control);
    Harness {
        app,
        calls,
        performed,
    }
}

fn press(app: &mut Panopticon, code: KeyCode, modifiers: KeyModifiers) {
    app.update(&Event::Key(KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }));
}

fn ch(app: &mut Panopticon, c: char) {
    press(app, KeyCode::Char(c), KeyModifiers::NONE);
}

fn typed(app: &mut Panopticon, text: &str) {
    for c in text.chars() {
        ch(app, c);
    }
}

/// `P` raises a question and does nothing else. The contract asserted
/// at the surface rather than only in the library: a key that triggers
/// a confirmation-required action must reach no executor.
#[test]
fn a_confirmation_required_key_performs_nothing_by_itself() {
    let mut h = harness();
    press(&mut h.app, KeyCode::Char('P'), KeyModifiers::SHIFT);
    assert_eq!(
        h.calls.load(Ordering::SeqCst),
        0,
        "the executor was reached without an answer"
    );
}

/// The whole meaning of the cockpit having one modal. `q` quits — until
/// a question is up, when it is an answer to that question instead, and
/// the wrong one.
#[test]
fn a_question_owns_the_keyboard_while_it_is_up() {
    let mut h = harness();
    press(&mut h.app, KeyCode::Char('P'), KeyModifiers::SHIFT);
    ch(&mut h.app, 'q');
    assert!(
        !h.app.should_quit(),
        "`q` quit the cockpit while it was being asked a question"
    );
}

/// And it gives the keyboard back.
#[test]
fn cancelling_returns_the_keyboard() {
    let mut h = harness();
    press(&mut h.app, KeyCode::Char('P'), KeyModifiers::SHIFT);
    press(&mut h.app, KeyCode::Esc, KeyModifiers::NONE);
    ch(&mut h.app, 'q');
    assert!(
        h.app.should_quit(),
        "the prompt kept the keyboard after Esc"
    );
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
}

/// Typing an answer completes the action with what was typed, rather
/// than performing a half-built one.
#[test]
fn a_pushed_branch_is_the_one_that_was_typed() {
    let mut h = harness();
    press(&mut h.app, KeyCode::Char('P'), KeyModifiers::SHIFT);
    typed(&mut h.app, "worktree-arc-3");
    press(&mut h.app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(h.calls.load(Ordering::SeqCst), 1);
    assert!(
        h.performed.lock().unwrap()[0].contains("worktree-arc-3"),
        "pushed something other than what was typed: {:?}",
        h.performed.lock().unwrap()
    );
}

/// Every attempt shows up where the operator can see it, including the
/// ones that did not happen. An action that quietly did not happen is
/// worse than one that visibly failed.
#[test]
fn a_cancelled_action_is_still_in_the_log() {
    let mut h = harness();
    press(&mut h.app, KeyCode::Char('P'), KeyModifiers::SHIFT);
    press(&mut h.app, KeyCode::Esc, KeyModifiers::NONE);

    let mut buf = ttui::buffer::LayerStack::new(120, 30);
    press(&mut h.app, KeyCode::Char('5'), KeyModifiers::NONE);
    h.app.view(
        ttui::layout::Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 30,
        },
        &mut buf,
    );
    let composited = buf.composite();
    let screen: String = (0..composited.height)
        .flat_map(|y| (0..composited.width).map(move |x| (x, y)))
        .map(|(x, y)| composited.get(x, y).symbol)
        .collect();
    assert!(
        screen.contains("cancelled"),
        "the log does not show the cancelled action"
    );
}

/// Fixture mode. A cockpit rendering recorded state that could merge a
/// real pull request is a demo with a loaded weapon in it.
#[test]
fn a_cockpit_with_no_executors_refuses_and_says_why() {
    let projects = vec![(validated(), ProjectAdapters::default())];
    let clock = Clock::Frozen(at(0));
    let refresher = Refresher::spawn(projects, clock);
    let mut app = Panopticon::new(&[validated()], refresher, clock, DEFAULT_POLL_INTERVAL);

    // `p` is reversible, so it goes straight through to the executor
    // that is not there.
    ch(&mut app, 'p');

    let mut buf = ttui::buffer::LayerStack::new(120, 30);
    press(&mut app, KeyCode::Char('5'), KeyModifiers::NONE);
    app.view(
        ttui::layout::Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 30,
        },
        &mut buf,
    );
    let composited = buf.composite();
    let screen: String = (0..composited.height)
        .flat_map(|y| (0..composited.width).map(move |x| (x, y)))
        .map(|(x, y)| composited.get(x, y).symbol)
        .collect();
    assert!(
        screen.contains("no executor"),
        "an inert cockpit did not say why nothing happened"
    );
}

/// Found by looking at a capture rather than at the code: Windows
/// reports a Release for every Press, so the key that opened the prompt
/// was arriving again and typing itself in. The confirmation for #142
/// greeted the operator already holding `m`.
#[test]
fn the_key_that_opens_a_prompt_does_not_type_itself_into_it() {
    let mut h = harness();
    press(&mut h.app, KeyCode::Char('P'), KeyModifiers::SHIFT);
    // The release of the very same key.
    h.app.update(&Event::Key(KeyEvent {
        code: KeyCode::Char('P'),
        modifiers: KeyModifiers::SHIFT,
        kind: KeyEventKind::Release,
        state: KeyEventState::NONE,
    }));
    typed(&mut h.app, "main");
    press(&mut h.app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(
        h.performed.lock().unwrap()[0],
        "ttui: push `main`",
        "the prompt swallowed a key release as typed input"
    );
}
