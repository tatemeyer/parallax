//! End-to-end tests that spawn the actual compiled `plumb` binary, the
//! only way to observe what a caller (a pre-PR check, the orchestrating
//! skill) actually sees: real process exit codes and real stdout/stderr
//! text, as opposed to the in-process `dispatch()` unit tests in
//! `src/main.rs`. Uses `CARGO_BIN_EXE_plumb`, which `cargo test`
//! supplies automatically — no new dependency needed.

use std::path::Path;
use std::process::{Command, Output};

fn plumb() -> Command {
    Command::new(env!("CARGO_BIN_EXE_plumb"))
}

fn run(cmd: &mut Command) -> Output {
    cmd.output().expect("failed to spawn plumb")
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

const SAMPLE_CONFIG: &str = "scenarios:\n  - name: dial\n    adapter: command\n    args: 'x {out}.png'\n    touches: ['src/widgets/dial.rs']\n";

#[test]
fn init_exits_0_and_scaffolds_a_fresh_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(".plumb");

    let out = run(plumb().arg("init").arg("--dir").arg(&dir));

    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.status.code(), Some(0));
    assert!(dir.join("config.yaml").is_file());
    assert!(dir.join("taste.md").is_file());
    assert!(dir.join("scripts").is_dir());
    assert!(dir.join("runs").is_dir());
}

#[test]
fn select_with_no_match_exits_3_and_prints_the_selection_as_json() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config.yaml");
    write(&config, SAMPLE_CONFIG);
    let changed = tmp.path().join("changed.txt");
    write(&changed, "README.md\n");

    let out = run(plumb()
        .arg("select")
        .arg("--config")
        .arg(&config)
        .arg("--changed")
        .arg(&changed));

    assert_eq!(out.status.code(), Some(3), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n{stdout}"));
    assert_eq!(parsed["selected"].as_array().unwrap().len(), 0);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("nothing to review"),
        "stderr should explain why: {stderr}"
    );
}

#[test]
fn select_by_scenario_name_exits_0() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config.yaml");
    write(&config, SAMPLE_CONFIG);

    let out = run(plumb()
        .arg("select")
        .arg("--config")
        .arg(&config)
        .arg("--scenario")
        .arg("dial"));

    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[test]
fn capture_of_a_scenario_whose_command_fails_exits_2_and_says_hold() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config.yaml");
    write(
        &config,
        "scenarios:\n  - name: fixture\n    adapter: command\n    args: 'this-command-does-not-exist --out {out}.png'\n    touches: ['src/**']\n",
    );
    let run_dir = tmp.path().join("run1");

    let out = run(plumb()
        .arg("capture")
        .arg("--config")
        .arg(&config)
        .arg("--run-dir")
        .arg(&run_dir)
        .arg("--scenario")
        .arg("fixture"));

    assert_eq!(
        out.status.code(),
        Some(2),
        "a failed capture must never exit 0 (GO): {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("HOLD"), "stderr: {stderr}");
}

#[test]
fn capture_of_a_working_command_scenario_exits_0_and_writes_a_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("fixture.png");
    image::RgbaImage::new(4, 4).save(&src).unwrap();
    // Both branches quote with `"` — see `cli::capture`'s copy_cmd: this
    // is embedded in a single-quoted YAML scalar, which a `'`-quoted
    // command would terminate early.
    let copy_cmd = if cfg!(windows) {
        format!("copy \"{}\" \"{{out}}.png\"", src.display())
    } else {
        format!("cp \"{}\" \"{{out}}.png\"", src.display())
    };
    let config = tmp.path().join("config.yaml");
    write(
        &config,
        &format!(
            "scenarios:\n  - name: fixture\n    adapter: command\n    args: '{copy_cmd}'\n    touches: ['src/**']\n"
        ),
    );
    let run_dir = tmp.path().join("run1");

    let out = run(plumb()
        .arg("capture")
        .arg("--config")
        .arg(&config)
        .arg("--run-dir")
        .arg(&run_dir)
        .arg("--scenario")
        .arg("fixture"));

    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let manifest_path = Path::new(stdout.trim());
    assert!(manifest_path.is_file(), "printed path: {stdout:?}");

    // The blinding boundary, checked end to end: nothing the CLI wrote
    // into the run directory (the manifest) or printed to its own
    // stdout carries the adapter's command line.
    let manifest_text = std::fs::read_to_string(manifest_path).unwrap();
    // Assert on the command and the source path themselves rather than a
    // quoting-specific prefix, so this keeps testing the blinding
    // boundary if the fixture's quoting changes again.
    assert!(
        !manifest_text.contains(&copy_cmd),
        "manifest leaked the adapter command"
    );
    assert!(
        !manifest_text.contains(&src.display().to_string()),
        "manifest leaked the source path"
    );
    assert!(!stdout.contains(&src.display().to_string()));
}
