//! The `command` adapter: runs any shell command that writes images to
//! a declared path. The escape hatch that makes adoption free — TTUI
//! adopts Plumb by declaring its existing `visual-snapshot` invocation
//! here and changing nothing about that tool.

use super::{frame_count, substitute_out, CaptureError};
use crate::config::Scenario;
use crate::contact::write_contact_sheet;
use crate::manifest::{Caveat, RunManifest};
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
            let captured = produced.remove(0);
            let frames = frame_count(&captured)?;

            // A single-frame capture is unaffected: the manifest's
            // `image` names the raw capture directly, and `animation`
            // stays absent. A 2+ frame capture keeps the GIF (what a
            // human watches) but points `image` at a freshly-tiled
            // contact sheet instead — the still image a lens agent can
            // actually decode. See the design's "What a lens can
            // actually see — the contact sheet".
            let (image, animation) = if frames >= 2 {
                let sheet_path = captured.with_extension("png");
                write_contact_sheet(&captured, &sheet_path)?;
                (sheet_path, Some(captured))
            } else {
                (captured, None)
            };

            Ok(RunManifest {
                run_id: run_id.to_string(),
                scenario: scenario.name.clone(),
                adapter: "command".into(),
                image: image.file_name().map(Into::into).unwrap_or(image.clone()),
                animation: animation.map(|a| a.file_name().map(Into::into).unwrap_or(a)),
                frame_count: frames,
                size: None,
                intent: scenario.intent.clone(),
                expects: scenario.expects.clone(),
                // A multi-frame capture's first pane predates every
                // scripted step, so disclose it. See `Caveat::PreScriptFrame`.
                caveats: if frames >= 2 {
                    vec![Caveat::PreScriptFrame]
                } else {
                    Vec::new()
                },
            })
        }
        _ => Err(CaptureError::AmbiguousOutput(produced)),
    }
}
