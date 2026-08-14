//! The `window` adapter — capturing a native OS window by title.
//!
//! **Deliberately unimplemented.** The Plumb design draws the adapter
//! boundary so this can slot in behind the same contract later, but no
//! consumer for it exists: TTUI is a terminal UI, Model-Experiments is
//! Python/CLI, and neither is a desktop app. Implementing it now would
//! be speculative surface with no caller to shape it. This module
//! exists to make the deferral explicit and typed rather than a gap.

use super::CaptureError;
use std::path::Path;

/// Always fails with a typed, actionable `NotImplemented`.
pub fn capture_window(_title: &str, _out_stem: &Path) -> Result<(), CaptureError> {
    Err(CaptureError::NotImplemented {
        adapter: "window",
        reason: "deferred — no consumer exists yet (TTUI is a TUI, \
                 Model-Experiments is Python/CLI); the contract admits \
                 it, the implementation is out of v1 scope",
    })
}
