//! Panopticon: a read-only TUI cockpit over `parallax-baseline`.
//! Loads every registered project's manifest, runs the real adapters,
//! and renders the platform's current state -- and nothing else. See
//! `overview` for the pure, headlessly-tested renderer; `app` is the
//! only code in this crate that touches a terminal.
#![warn(missing_docs)]

/// Wires manifest-declared adapters to Baseline's real implementations.
pub mod adapters;
/// The interactive `App` -- the only terminal-facing code in this crate.
pub mod app;
/// Discovers and validates `manifests/*.yaml`.
pub mod load;
/// The Overview screen: a pure renderer plus its cell logic.
pub mod overview;

use std::path::Path;

/// Loads `manifests/*.yaml` under `manifests_dir`, wires the real
/// adapters, and runs the Overview screen until the user quits.
///
/// Deliberately tolerant of a missing or empty manifests directory, an
/// unreachable network, and no GitHub token: offline is a supported
/// path, not a fallback. Every manifest that fails to parse or
/// validate is reported to stderr and skipped, not fatal to the rest;
/// every adapter that fails to reach its source degrades that one
/// source rather than blanking the screen.
pub fn run(manifests_dir: &Path, github_token: Option<&str>) -> std::io::Result<()> {
    let (validated, failures) = load::load_manifests(manifests_dir);
    for failure in &failures {
        eprintln!(
            "panopticon: skipping {}: {}",
            failure.path.display(),
            failure.reason
        );
    }

    let inputs = validated
        .into_iter()
        .map(|v| {
            let built = adapters::build_adapters(&v, github_token);
            (v, built)
        })
        .collect();

    let mut app = app::PanopticonApp::new(inputs);
    ttui::app::run(&mut app)
}
