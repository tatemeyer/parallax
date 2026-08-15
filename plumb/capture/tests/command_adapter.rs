//! Exercises the `command` adapter end to end against a real
//! subprocess, the way a consumer's `cargo run -p visual-snapshot`
//! line will be run.

use image::{Rgba, RgbaImage};
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

/// End-to-end: a real capture command that produces a multi-frame GIF
/// must yield a manifest whose `image` is a tiled contact sheet (bare
/// filename) and whose `animation` names the untouched GIF (also bare)
/// — the fix Task 14a exists for, exercised through the full adapter
/// path rather than the tiling module alone. The assertion reads real
/// pixel data off the emitted contact sheet PNG, at the coordinates
/// each frame's position in the grid predicts, so this would fail if
/// tiling silently dropped a frame or scrambled capture order — the
/// exact defect class that shipped unreviewable GIFs the first time.
#[test]
fn a_command_that_writes_a_multiframe_gif_yields_a_contact_sheet_and_keeps_the_gif() {
    let colors = [
        Rgba([200u8, 0, 0, 255]),
        Rgba([0u8, 200, 0, 255]),
        Rgba([0u8, 0, 200, 255]),
        Rgba([200u8, 200, 0, 255]),
    ];
    let src_dir = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("fixture.gif");
    {
        let file = std::fs::File::create(&src).unwrap();
        let mut encoder = image::codecs::gif::GifEncoder::new(file);
        let frames = colors
            .iter()
            .map(|c| image::Frame::new(RgbaImage::from_pixel(8, 8, *c)));
        encoder.encode_frames(frames).unwrap();
    }
    let copy = if cfg!(windows) {
        format!("copy \"{}\" \"{{out}}.gif\"", src.display())
    } else {
        format!("cp '{}' '{{out}}.gif'", src.display())
    };

    let m = capture(&scenario(&copy), dir.path(), "20260814T101500Z").unwrap();

    assert_eq!(m.frame_count, 4);
    assert_eq!(m.image, std::path::PathBuf::from("fixture.png"));
    assert_eq!(m.animation, Some(std::path::PathBuf::from("fixture.gif")));
    // Both are bare filenames: the blinding property Task 4 established
    // must hold for the new field exactly as it does for `image`.
    assert_eq!(
        m.image,
        m.image.file_name().map(std::path::PathBuf::from).unwrap()
    );
    let animation = m.animation.clone().unwrap();
    assert_eq!(
        animation,
        animation.file_name().map(std::path::PathBuf::from).unwrap()
    );

    // The GIF itself is untouched — still 4 frames, still where the
    // manifest says.
    let gif_path = dir.path().join(&animation);
    assert!(gif_path.exists(), "the gif must remain beside the sheet");

    // The contact sheet actually carries every frame's content, at the
    // grid position frame order predicts (grid_dims(4) == 2 cols x 2
    // rows, GUTTER_PX == 8, panes 8x8).
    let sheet_path = dir.path().join(&m.image);
    let sheet = image::open(&sheet_path).unwrap().to_rgba8();
    const GUTTER_PX: u32 = 8;
    for (i, expected) in colors.iter().enumerate() {
        let i = i as u32;
        let col = i % 2;
        let row = i / 2;
        let x = GUTTER_PX + col * (8 + GUTTER_PX) + 4;
        let y = GUTTER_PX + row * (8 + GUTTER_PX) + 4;
        assert_eq!(
            *sheet.get_pixel(x, y),
            *expected,
            "frame {i} missing from its predicted pane in the emitted sheet"
        );
    }
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
