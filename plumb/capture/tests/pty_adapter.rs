//! Exercises the `pty` adapter end to end through the public `capture()`
//! contract, against real spawned `[[example]]` fixtures — mirrors
//! `tests/command_adapter.rs`'s role for the `command` adapter. Uses
//! `adapter::pty::examples_dir()` (not `env!("CARGO_BIN_EXE_...")`) to
//! locate fixture binaries: `CARGO_BIN_EXE_<name>` is only set by Cargo
//! for `[[bin]]` targets, and `echo_key`/`echo_unmapped_glyph` are
//! `[[example]]` targets — the same reason `tests/pty_roundtrip.rs`
//! already uses this helper instead.

use parallax_plumb::adapter::pty::examples_dir;
use parallax_plumb::adapter::{capture, CaptureError};
use parallax_plumb::config::{AdapterKind, Scenario};
use parallax_plumb::glyph::GlyphMode;
use parallax_plumb::manifest::Caveat;
use parallax_plumb::script::Step;
use std::path::PathBuf;

/// Renders `steps` back to the flat JSON array `script::parse_script`
/// reads. `Step` derives only `Deserialize` (a script is read, never
/// written, in production), so this test-only inverse is hand-rolled
/// rather than reaching for `serde_json::to_string`.
fn steps_to_json(steps: &[Step]) -> String {
    let items: Vec<String> = steps
        .iter()
        .map(|s| match s {
            Step::Wait { wait_ms } => format!("{{\"wait_ms\":{wait_ms}}}"),
            Step::Key { key } => format!("{{\"key\":{}}}", serde_json::to_string(key).unwrap()),
            Step::Click { x, y } => format!("{{\"x\":{x},\"y\":{y}}}"),
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn fixture_binary(name: &str) -> PathBuf {
    let mut path = examples_dir();
    path.push(name);
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

/// Builds a `pty` scenario whose spawned command is `exe`, writing its
/// script to a temp path so the test owns both ends. Mirrors the task
/// brief's helper exactly, aside from sourcing `exe` from
/// `fixture_binary` instead of `env!("CARGO_BIN_EXE_...")` (see this
/// file's header) and generating a per-call-unique script path via
/// `tempfile::Builder` instead of a `process::id()`-keyed name.
///
/// Found on review: `process::id()` is constant for the whole test
/// *binary*, not per call, so under cargo's default parallel test
/// execution every `pty_scenario` call in this file raced on the same
/// fixed path — one test's write or delete landing mid another test's
/// read. Reproduced directly (3 of 5 consecutive `cargo test
/// --workspace` runs failed, with wrong `frame_count`, a missing
/// `animation`, and a JSON EOF parse error — all consistent with the
/// race). `tempfile::Builder::tempfile()` generates a securely random,
/// per-call-unique filename, so concurrent calls can never collide;
/// `.into_temp_path().keep()` persists it past this function's return
/// (a plain `NamedTempFile` would delete it on drop, before `capture()`
/// ever reads it).
fn pty_scenario(exe: &str, size: &str, steps: &[Step]) -> Scenario {
    use std::io::Write;
    let mut file = tempfile::Builder::new()
        .prefix("plumb-fixture-script-")
        .suffix(".json")
        .tempfile()
        .unwrap();
    file.write_all(steps_to_json(steps).as_bytes()).unwrap();
    let script = file.into_temp_path().keep().unwrap();
    Scenario {
        name: "fixture".into(),
        adapter: AdapterKind::Pty,
        args: exe.into(),
        size: Some(size.into()),
        script: Some(script),
        on_unmapped_glyph: GlyphMode::Error,
        touches: vec!["src/**".into()],
        ..Default::default()
    }
}

#[test]
fn a_pty_scenario_captures_and_reports_its_terminal_size() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture_binary("echo_key").to_string_lossy().into_owned();
    let s = pty_scenario(&exe, "80x24", &[]);

    let m = capture(&s, dir.path(), "r").unwrap();

    assert_eq!(m.adapter, "pty");
    assert_eq!(m.size.as_deref(), Some("80x24"));
    assert_eq!(
        m.frame_count, 1,
        "a zero-step script yields exactly one frame"
    );
    assert!(dir.path().join("fixture.png").exists());
}

#[test]
fn a_multi_step_pty_script_writes_a_gif() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture_binary("echo_key").to_string_lossy().into_owned();
    let s = pty_scenario(
        &exe,
        "80x24",
        &[Step::Key {
            key: "Right".into(),
        }],
    );

    let m = capture(&s, dir.path(), "r").unwrap();

    assert_eq!(m.frame_count, 2);
    assert!(dir.path().join("fixture.gif").exists());
}

/// The hop this task exists to close: a real spawned child that draws a
/// real unmapped codepoint, under `GlyphMode::Substitute`, must produce
/// a manifest whose `caveats` names that codepoint and the real count
/// `render_screen` recorded — not a hand-built `Caveat`, and not a
/// count with nothing behind it.
#[test]
fn a_real_unmapped_glyph_in_substitute_mode_becomes_a_disclosed_manifest_caveat() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture_binary("echo_unmapped_glyph")
        .to_string_lossy()
        .into_owned();
    let mut s = pty_scenario(&exe, "40x5", &[]);
    s.on_unmapped_glyph = GlyphMode::Substitute;

    let m = capture(&s, dir.path(), "r").unwrap();

    assert_eq!(
        m.caveats,
        vec![Caveat::UnmappedGlyphSubstituted {
            codepoint: "U+2726".into(),
            count: 1,
        }]
    );
}

/// Companion: the same real unmapped glyph, under the default
/// `GlyphMode::Error`, must still hard-error as a typed `CaptureError`
/// rather than silently substituting — capture failure is never a GO.
#[test]
fn a_real_unmapped_glyph_in_error_mode_is_a_typed_capture_failure() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture_binary("echo_unmapped_glyph")
        .to_string_lossy()
        .into_owned();
    let s = pty_scenario(&exe, "40x5", &[]);

    let err = capture(&s, dir.path(), "r").unwrap_err();

    assert!(
        matches!(err, CaptureError::Pty(_)),
        "expected a typed Pty capture failure, got {err:?}"
    );
}

#[test]
fn a_pty_scenario_with_no_size_is_a_typed_invalid_config_error() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture_binary("echo_key").to_string_lossy().into_owned();
    let mut s = pty_scenario(&exe, "80x24", &[]);
    s.size = None;

    let err = capture(&s, dir.path(), "r").unwrap_err();

    assert!(matches!(err, CaptureError::InvalidPtyConfig(_)));
}

#[test]
fn a_malformed_pty_size_is_a_typed_invalid_config_error() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture_binary("echo_key").to_string_lossy().into_owned();
    let s = pty_scenario(&exe, "not-a-size", &[]);

    let err = capture(&s, dir.path(), "r").unwrap_err();

    match err {
        CaptureError::InvalidPtyConfig(msg) => assert!(msg.contains("not-a-size")),
        other => panic!("expected InvalidPtyConfig, got {other:?}"),
    }
}

/// A multi-frame `pty` capture must produce a contact sheet exactly as
/// the `command` adapter does: `image` is the tiled sheet (a bare
/// filename), `animation` names the untouched gif (also bare), and the
/// gif itself survives on disk beside the sheet.
#[test]
fn a_multiframe_pty_capture_yields_a_contact_sheet_and_keeps_the_gif() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture_binary("echo_key").to_string_lossy().into_owned();
    let s = pty_scenario(
        &exe,
        "40x5",
        &[Step::Key {
            key: "Right".into(),
        }],
    );

    let m = capture(&s, dir.path(), "r").unwrap();

    assert_eq!(m.image, PathBuf::from("fixture.png"));
    assert_eq!(m.animation, Some(PathBuf::from("fixture.gif")));
    assert!(dir.path().join("fixture.gif").exists());
    assert!(dir.path().join("fixture.png").exists());
}
