//! Exercises the `command` adapter end to end against a real
//! subprocess, the way a consumer's `cargo run -p visual-snapshot`
//! line will be run.

use parallax_plumb::adapter::{capture, CaptureError};
use parallax_plumb::config::{AdapterKind, Scenario};

fn scenario(args: &str) -> Scenario {
    Scenario {
        name: "fixture".into(),
        adapter: AdapterKind::Command,
        args: args.into(),
        intent: Some("a 4x4 image exists".into()),
        touches: vec!["src/**".into()],
        ..Default::default()
    }
}

#[test]
fn a_command_that_writes_a_png_yields_a_one_frame_manifest() {
    // The source fixture lives in its own tempdir, separate from the
    // run directory: the scenario is named "fixture", which makes the
    // capture destination stem `<run_dir>/fixture` — the same name a
    // same-directory source fixture would collide with, making
    // `copy`/`cp` fail with "cannot copy a file onto itself".
    let src_dir = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("fixture.png");
    image::RgbaImage::new(4, 4).save(&src).unwrap();
    let copy = if cfg!(windows) {
        format!("copy \"{}\" \"{{out}}.png\"", src.display())
    } else {
        format!("cp '{}' '{{out}}.png'", src.display())
    };

    let m = capture(&scenario(&copy), dir.path(), "20260814T101500Z").unwrap();

    assert_eq!(m.adapter, "command");
    assert_eq!(m.frame_count, 1);
    assert_eq!(m.image, std::path::PathBuf::from("fixture.png"));
    assert_eq!(m.intent.as_deref(), Some("a 4x4 image exists"));
}

#[test]
fn a_command_that_writes_nothing_is_a_typed_no_output_error() {
    let dir = tempfile::tempdir().unwrap();
    // `{out}` is deliberately omitted: this test exercises "the command
    // succeeded but produced no image," not substitution (that's
    // covered by the unit tests and the successful-copy test above).
    // A bare no-op is used rather than referencing the stem in an
    // argument `cd` would try to parse, which behaves inconsistently
    // across cmd.exe and sh when handed extra positional arguments.
    let s = scenario("cd .");
    assert!(matches!(
        capture(&s, dir.path(), "r").unwrap_err(),
        CaptureError::NoOutput { .. }
    ));
}

#[test]
fn a_failing_command_reports_its_status_and_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let s = scenario("this-command-does-not-exist --out {out}.png");
    match capture(&s, dir.path(), "r").unwrap_err() {
        CaptureError::CommandFailed { status, .. } => assert!(!status.is_empty()),
        CaptureError::Spawn(_) => {}
        other => panic!("expected a command failure, got {other:?}"),
    }
}
