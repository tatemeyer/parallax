//! Fixture mode: a cockpit that renders the same frame every time.
//!
//! A first-class feature rather than a test scaffold. The cockpit is
//! verified through Plumb, and a live cockpit differs on every run —
//! ages tick, sessions age out, GitHub moves. Two runs of a fixture-mode
//! scenario must produce identical frames, which is what makes a NO-GO
//! mean "the layout is wrong" rather than "time passed".
//!
//! It is also how a human sees the cockpit before registering anything.
//!
//! Nothing here is a parallel implementation: adapters are built through
//! `parallax_baseline::adapters::factory::from_manifest_with`, the same
//! translation production uses, differing only in where the bytes come
//! from. That is why the factory takes transport and runner *factories*
//! rather than values.

use crate::refresh::BoxedPeer;
use parallax_baseline::adapters::factory::{from_manifest_with, AdapterConfig};
use parallax_baseline::adapters::http::{FixtureTransport, HttpTransport};
use parallax_baseline::adapters::verification::ScriptedRunner;
use parallax_baseline::adapters::work::{check_runs_url, issues_url, pulls_url};
use parallax_baseline::peers::{PeerClient, STATE_PATH};
use parallax_baseline::registry::Registry;
use parallax_baseline::state::ProjectAdapters;
use parallax_baseline::validate::Validated;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// A whole cockpit's worth of recorded state.
pub struct FixtureSet {
    /// The instant every observation is stamped with.
    pub now: SystemTime,
    /// Every project the fixture directory holds, with its adapters.
    pub projects: Vec<(Validated, ProjectAdapters)>,
    /// Every recorded peer, each backed by a `FixtureTransport` rather
    /// than a network. A cockpit showing three machines is exactly as
    /// reproducible as one showing three directories.
    pub peers: Vec<BoxedPeer>,
}

/// The file naming the frozen instant, as Unix seconds.
pub const CLOCK_FILE: &str = "clock.txt";
/// Where a project's recorded GitHub responses live.
pub const GITHUB_DIR: &str = "github";
/// Where recorded peer envelopes live: one `<name>.json` per machine,
/// each holding exactly what that machine's probe would have served.
pub const PEERS_DIR: &str = "peers";

/// Loads a fixture directory.
///
/// The directory holds one subdirectory per project — each a project
/// root, with its own `parallax.yaml`, exactly as a real checkout would
/// be — plus a `clock.txt`. Recorded GitHub responses live in each
/// project's `github/` directory.
pub fn load(dir: &Path) -> Result<FixtureSet, String> {
    let now = read_clock(dir)?;
    let registry = Registry::scan(dir);
    if let Some(failure) = registry.failures().first() {
        return Err(format!("fixture {failure}"));
    }
    let peers = load_peers(dir)?;
    if registry.is_empty() && peers.is_empty() {
        return Err(format!(
            "{} holds no project directories with a parallax.yaml and no {PEERS_DIR}/",
            dir.display()
        ));
    }

    let projects = registry
        .projects()
        .iter()
        .map(|project| {
            // Rebuilt per call rather than cloned: `FixtureTransport`
            // carries an `AdapterError`, which carries an `io::Error`,
            // which is not `Clone`. A manifest declares one work feed, so
            // this closure runs once.
            let root = project.root.clone();
            let manifest = project.manifest.manifest().clone();
            let adapters = from_manifest_with(
                &project.manifest,
                &AdapterConfig::default(),
                move || transport_for(&root, &manifest),
                ScriptedRunner::new,
            );
            (project.manifest.clone(), adapters)
        })
        .collect();

    Ok(FixtureSet {
        now,
        projects,
        peers,
    })
}

/// Builds a peer per `peers/<name>.json`, each served by a transport
/// holding that one recorded envelope.
///
/// The file name is the machine's name, and the URL is synthesized from
/// it — no fixture set should contain a real address, because a fixture
/// that could reach a network is not a fixture. An absent `peers/`
/// directory is a fixture set with no peers, not an error: every
/// recording made before remote hosts existed still loads.
fn load_peers(dir: &Path) -> Result<Vec<BoxedPeer>, String> {
    let Ok(entries) = std::fs::read_dir(dir.join(PEERS_DIR)) else {
        return Ok(Vec::new());
    };
    // Sorted: `read_dir` order is filesystem-defined, and two runs must
    // put the same machine in the same row.
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();

    let mut peers = Vec::new();
    for file in files {
        let name = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .ok_or_else(|| format!("fixture peer {} has no name", file.display()))?;
        let body = std::fs::read_to_string(&file)
            .map_err(|e| format!("fixture peer {}: {e}", file.display()))?;
        let url = format!("https://{name}");
        let mut transport = FixtureTransport::new();
        transport.insert(format!("{url}{STATE_PATH}"), body, None);
        let transport: Box<dyn HttpTransport + Send> = Box::new(transport);
        peers.push(PeerClient::new(transport, url));
    }
    Ok(peers)
}

/// Reads the frozen instant. A fixture set without one is rejected
/// rather than quietly falling back to the system clock — that fallback
/// is exactly the bug this mode exists to prevent.
fn read_clock(dir: &Path) -> Result<SystemTime, String> {
    let path = dir.join(CLOCK_FILE);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let secs: u64 = text
        .trim()
        .parse()
        .map_err(|_| format!("{} must hold Unix seconds", path.display()))?;
    Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

/// A transport preloaded with whatever `<root>/github/` holds.
///
/// A project with no recorded responses gets an empty transport, whose
/// every URL 404s — which surfaces as a degraded work source saying so,
/// rather than as an empty pane pretending there is no work.
fn transport_for(
    root: &Path,
    manifest: &parallax_baseline::manifest::Manifest,
) -> FixtureTransport {
    let mut transport = FixtureTransport::new();
    let Some(work) = &manifest.work else {
        return transport;
    };
    let dir = root.join(GITHUB_DIR);
    let repo = &work.repo;

    let _ = transport.insert_from_file(issues_url(repo), &dir.join("issues.json"), None);
    let _ = transport.insert_from_file(pulls_url(repo), &dir.join("pulls.json"), None);

    // Check runs are keyed by head SHA, so every SHA in the recorded
    // pulls gets the same recorded response. Real fidelity would need a
    // file per SHA; this is a fixture, and the pane shows counts.
    if let Ok(text) = std::fs::read_to_string(dir.join("pulls.json")) {
        if let Ok(serde_json::Value::Array(pulls)) = serde_json::from_str(&text) {
            for pull in pulls {
                if let Some(sha) = pull["head"]["sha"].as_str() {
                    let _ = transport.insert_from_file(
                        check_runs_url(repo, sha),
                        &dir.join("check-runs.json"),
                        None,
                    );
                }
            }
        }
    }
    transport
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    #[test]
    fn the_shipped_fixture_set_loads() {
        let set = load(&fixtures()).expect("the fixture set loads");
        assert_eq!(set.projects.len(), 2, "ttui and sesh");
        assert_eq!(
            set.now,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
        );
    }

    #[test]
    fn every_project_gets_the_adapters_its_manifest_declares() {
        let set = load(&fixtures()).unwrap();
        let (ttui, adapters) = set
            .projects
            .iter()
            .find(|(v, _)| v.manifest().project.name == "ttui")
            .expect("ttui is in the fixture set");
        assert!(ttui.manifest().work.is_some());
        assert!(adapters.work.is_some());
        assert_eq!(adapters.verification.len(), 3);
        assert!(adapters.sessions.is_some());
    }

    /// `FixtureSet` holds adapters, which are not `Debug`, so an error
    /// is taken rather than unwrapped.
    fn error_from(dir: &Path) -> String {
        match load(dir) {
            Err(e) => e,
            Ok(_) => panic!("expected this fixture directory to be rejected"),
        }
    }

    /// The point of the mode: no wall clock anywhere in it.
    #[test]
    fn a_fixture_set_without_a_clock_is_rejected_rather_than_falling_back() {
        let dir = tempfile::tempdir().unwrap();
        let err = error_from(dir.path());
        assert!(err.contains("clock.txt"), "got {err}");
    }

    #[test]
    fn a_directory_with_a_clock_but_no_projects_says_so() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CLOCK_FILE), "1700000000").unwrap();
        let err = error_from(dir.path());
        assert!(err.contains("no project directories"), "got {err}");
    }
}
