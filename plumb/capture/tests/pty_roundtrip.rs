//! Exercises `adapter::pty` end to end against a real ConPTY/PTY-spawned
//! child process. Ported from
//! `tools/visual-snapshot/tests/pty_roundtrip.rs`, adapted for the crate
//! name and the arbitrary-command signature (`Session::spawn`/`run_script`
//! now take `&[String]` instead of `&Path` + `&[Step]`, and `run_script`
//! now takes a `GlyphMode` and returns `CaptureFrames` instead of a bare
//! `Vec`).
//!
//! Only the subset of the source file's tests that exercise the
//! `echo_key` fixture are ported: this task's scope (per the task brief's
//! Files list) is `examples/echo_key.rs` alone, not TTUI's
//! `delayed_draw`/`delayed_key_response`/`echo_mouse` fixtures, so the
//! three source tests that need those
//! (`capture_frame_waits_past_the_old_fixed_settle_delay_for_a_slow_first_draw`,
//! `a_key_steps_frame_waits_for_the_childs_actual_reaction_not_just_two_stable_polls`,
//! `a_click_step_actually_reaches_the_child_process`) are not ported —
//! see the task report for the full reasoning.
//!
//! No ported assertion's meaning was changed — only call sites were
//! updated for the new signatures.

use parallax_plumb::adapter::pty::{examples_dir, run_script, Session};
use parallax_plumb::glyph::GlyphMode;
use parallax_plumb::script::Step;
use std::path::PathBuf;

fn echo_key_binary() -> PathBuf {
    let mut path = examples_dir();
    path.push("echo_key");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

fn echo_key_command() -> Vec<String> {
    vec![echo_key_binary().to_string_lossy().into_owned()]
}

#[test]
fn spawning_and_capturing_one_frame_shows_the_process_alive() {
    let mut session = Session::spawn(&echo_key_command(), 5, 40).unwrap();
    // No input sent yet — the fixture is blocked on event::read(), so
    // the initial frame should just be a blank screen at the right size.
    let frame = session.capture_frame().unwrap();
    assert_eq!(frame.width(), 40 * 16);
    assert_eq!(frame.height(), 5 * 16);
}

#[test]
fn a_key_step_actually_reaches_the_child_process() {
    let steps = vec![
        Step::Key {
            key: "a".to_string(),
        },
        Step::Wait { wait_ms: 16 },
        Step::Key {
            key: "Esc".to_string(),
        },
    ];

    let out = run_script(&echo_key_command(), 5, 40, &steps, GlyphMode::Error).unwrap();

    // Initial frame + one per step.
    assert_eq!(out.frames.len(), 4);
    // The frame captured after sending "a" should show the fixture's
    // echoed `KeyCode::Char('a')` debug text somewhere on screen —
    // checked indirectly via a non-blank pixel outside the top-left
    // origin, since asserting exact glyph pixels here would duplicate
    // render.rs's own tests.
    let after_a = &out.frames[1].0;
    let any_non_background = after_a.pixels().any(|p| *p != image::Rgba([0, 0, 0, 255]));
    assert!(
        any_non_background,
        "expected the echoed key text to draw something"
    );
}

#[test]
fn frame_durations_match_each_steps_own_timing() {
    let steps = vec![
        Step::Wait { wait_ms: 250 },
        Step::Key {
            key: "Esc".to_string(),
        },
    ];

    let out = run_script(&echo_key_command(), 5, 40, &steps, GlyphMode::Error).unwrap();

    assert_eq!(out.frames[0].1, std::time::Duration::from_millis(0)); // initial frame
    assert_eq!(out.frames[1].1, std::time::Duration::from_millis(250)); // Wait step
    assert_eq!(out.frames[2].1, std::time::Duration::from_millis(150)); // Key step, fixed duration
}

/// Companion to the delayed-draw case the source file guards (not ported
/// here, since it needs the `delayed_draw` fixture — see this file's
/// header): proves the fix doesn't sacrifice the common case.
/// `echo_key` reacts to a keypress almost immediately, so `capture_frame`
/// should quiesce and return well before `MAX_SETTLE_WAIT` (2s) —
/// asserted generously (under 1s) to stay robust against ordinary
/// scheduling jitter while still clearly distinguishing "quiesced
/// quickly" from "hit the max bound".
#[test]
fn capture_frame_stays_fast_when_output_arrives_quickly() {
    let mut session = Session::spawn(&echo_key_command(), 5, 40).unwrap();
    let _ = session.capture_frame().unwrap(); // drain the initial blank frame
    session.send(b"a").unwrap();

    let start = std::time::Instant::now();
    let frame = session.capture_frame().unwrap();
    let elapsed = start.elapsed();

    let any_non_background = frame.pixels().any(|p| *p != image::Rgba([0, 0, 0, 255]));
    assert!(
        any_non_background,
        "expected the echoed key text to draw something"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "expected a fast-quiescing capture to return well under MAX_SETTLE_WAIT, took {elapsed:?}"
    );
}

/// Guards a Task 12 second-round review finding from the source: an
/// earlier version of the fix compared each poll only against the
/// screen's content from *before* `capture_frame` was even called,
/// rather than against the immediately preceding poll taken during the
/// same call. That meant a screen that was already fully drawn and
/// stable *before* the call started — an idle wait step, a key with no
/// visible effect, capturing a static screen twice — never saw a
/// "change" to react to, so it could never be recognized as quiescent
/// and paid the full `MAX_SETTLE_WAIT` (2s) every time, even though
/// nothing was actually still drawing. This test drives exactly that:
/// capture once to let the fixture's echoed "a" settle, then capture
/// again immediately with nothing further happening, and asserts the
/// second call returns fast rather than paying the full bound.
#[test]
fn capture_frame_is_fast_when_the_screen_is_already_stable() {
    let mut session = Session::spawn(&echo_key_command(), 5, 40).unwrap();
    let _ = session.capture_frame().unwrap(); // initial blank frame
    session.send(b"a").unwrap();
    let _ = session.capture_frame().unwrap(); // let the echoed "a" settle

    let start = std::time::Instant::now();
    let _ = session.capture_frame().unwrap(); // nothing new happened
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "expected an already-stable screen to be recognized as quiescent almost \
         immediately, not pay the full MAX_SETTLE_WAIT; took {elapsed:?}"
    );
}

/// New test, from the task brief: the whole point of the extraction — no
/// cargo, no `--example`, no assumption about where a binary lives.
#[test]
fn run_script_spawns_an_arbitrary_command_not_a_cargo_example() {
    let exe = echo_key_binary().to_string_lossy().into_owned();
    let out = run_script(
        &[exe],
        24,
        80,
        &[Step::Key {
            key: "Right".into(),
        }],
        GlyphMode::Error,
    )
    .unwrap();
    assert_eq!(out.frames.len(), 2, "an initial frame plus one per step");
}
