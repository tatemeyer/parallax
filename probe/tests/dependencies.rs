//! The probe is headless, and checkably so.
//!
//! `parallax-baseline` keeps Plumb out of itself with a test rather than
//! a promise, for the reason its own spec gives: a dependency nobody
//! meant to add arrives in a pull request that was about something else.
//! The same argument applies here, pointing the other way — the probe
//! must never reach *up* into a frontend.

use std::path::Path;

fn manifest() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    std::fs::read_to_string(path).expect("the probe has a Cargo.toml")
}

/// A cockpit is one client of the probe, not a component of it. If this
/// fails, the probe can no longer be run on a machine that has no TUI —
/// which is every machine it was written for.
#[test]
fn the_probe_never_depends_on_a_cockpit() {
    let toml = manifest();
    assert!(
        !toml.contains("panopticon"),
        "the probe grew a dependency on a frontend"
    );
}

/// The whole point of the probe is that the machine which owns a project
/// aggregates it locally. A dependency that draws frames would mean it
/// had stopped being headless.
#[test]
fn the_probe_pulls_in_no_terminal_machinery() {
    let toml = manifest();
    for forbidden in ["ttui", "crossterm", "termion", "ratatui"] {
        assert!(
            !toml.contains(forbidden),
            "the probe grew a terminal dependency: {forbidden}"
        );
    }
}
