//! The `command` adapter: runs any shell command that writes images to
//! a declared path. The escape hatch that makes adoption free — TTUI
//! adopts Plumb by declaring its existing `visual-snapshot` invocation
//! here and changing nothing about that tool.

use super::{frame_count, substitute_out, CaptureError};
use crate::config::Scenario;
use crate::manifest::RunManifest;
use std::path::Path;
use std::process::{Command, Output};

/// Runs `line` through the platform shell verbatim.
///
/// On Windows this uses `raw_arg` rather than a normal argument: `cmd
/// /C` expects the rest of the command line exactly as written, and
/// `Command`'s default argument quoting (meant for passing one opaque
/// argument to a normal child process) mangles embedded quotes when a
/// whole shell command line is passed as a single argument.
fn spawn_shell(line: &str) -> std::io::Result<Output> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new("cmd").arg("/C").raw_arg(line).output()
    }
    #[cfg(not(windows))]
    {
        Command::new("sh").arg("-c").arg(line).output()
    }
}

/// Runs `scenario.args` (with `{out}` substituted) through the platform
/// shell and reports the single image it produced.
pub fn capture_command(
    scenario: &Scenario,
    run_dir: &Path,
    run_id: &str,
) -> Result<RunManifest, CaptureError> {
    let stem = run_dir.join(&scenario.name);
    let line = substitute_out(&scenario.args, &stem);

    let output = spawn_shell(&line).map_err(CaptureError::Spawn)?;

    if !output.status.success() {
        return Err(CaptureError::CommandFailed {
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(4000)
                .collect(),
        });
    }

    let mut produced = Vec::new();
    for ext in ["png", "gif"] {
        let candidate = stem.with_extension(ext);
        if candidate.exists() {
            produced.push(candidate);
        }
    }
    match produced.len() {
        0 => Err(CaptureError::NoOutput {
            expected_stem: stem,
        }),
        1 => {
            let image = produced.remove(0);
            let frames = frame_count(&image)?;
            Ok(RunManifest {
                run_id: run_id.to_string(),
                scenario: scenario.name.clone(),
                adapter: "command".into(),
                image: image.file_name().map(Into::into).unwrap_or(image.clone()),
                frame_count: frames,
                size: None,
                intent: scenario.intent.clone(),
                expects: scenario.expects.clone(),
                caveats: Vec::new(),
            })
        }
        _ => Err(CaptureError::AmbiguousOutput(produced)),
    }
}
