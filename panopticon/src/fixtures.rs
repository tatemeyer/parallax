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
use parallax_baseline::adapters::verification::ScriptedShellRunner;
use parallax_baseline::adapters::work::{check_runs_url, issues_url, pulls_url};
use parallax_baseline::peers::{PeerClient, STATE_PATH};
use parallax_baseline::registry::Registry;
use parallax_baseline::state::ProjectAdapters;
use parallax_baseline::validate::Validated;
use std::collections::BTreeMap;
use std::net::IpAddr;
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
    /// Every recorded peer that also recorded a control surface.
    ///
    /// Usually shorter than `peers`, and empty in most fixture sets:
    /// control is opt-in per machine in a deployment, and a fixture set
    /// spells that with the presence of a file. See [`CONTROL_SUFFIX`].
    ///
    /// **Recordings, not submitters.** This module reads files; it does
    /// not build the thing that acts. `main.rs` turns these into
    /// submitters, because the composition root is the one place allowed
    /// to decide what this run may act on — a rule `tests/read_only.rs`
    /// enforces by refusing to let any other file so much as name
    /// baseline's actions module.
    pub control: Vec<RecordedControl>,
}

/// The file naming the frozen instant, as Unix seconds.
pub const CLOCK_FILE: &str = "clock.txt";
/// Where a project's recorded GitHub responses live.
pub const GITHUB_DIR: &str = "github";
/// Where recorded peer envelopes live: one `<name>.json` per machine,
/// each holding exactly what that machine's probe would have served.
pub const PEERS_DIR: &str = "peers";

/// What a peer's recorded control surface is called: `<name>.control.json`.
///
/// **Its presence is what gives a fixture peer control**, mirroring
/// `--allow-control` on a real probe — where the machine that would
/// execute is the one that decides. A peer with no such file is watched
/// and not acted on, which is the default in a fixture set exactly as
/// it is in a deployment.
pub const CONTROL_SUFFIX: &str = ".control.json";

/// How a fixture cockpit names itself in an action it submits.
///
/// Pinned, with [`FIXTURE_RUN`], so that ids are `fixture-0-1`,
/// `fixture-0-2`, and so on. A recorded reply has to name the id it is
/// answering about, which means the ids cannot come from a clock.
pub const FIXTURE_CLIENT: &str = "fixture";

/// The run a fixture cockpit submits in. See [`FIXTURE_CLIENT`].
pub const FIXTURE_RUN: u64 = 0;

/// A peer's recorded control surface.
///
/// Read by hand rather than derived, because this crate takes
/// `serde_json` and deliberately not `serde` — and because two fields
/// typed out here can name the file in every complaint, which a derive
/// cannot.
///
/// **The bodies are recorded verbatim rather than typed as replies.** A
/// probe answering something the cockpit cannot parse is a case the
/// cockpit has distinct behaviour for — it reports the action's fate as
/// unknown rather than as refused, because an unreadable answer is not a
/// no — and a fixture that could not record one could not photograph
/// that behaviour. The file must be valid JSON; the bodies inside it
/// need not be replies this version understands.
pub struct RecordedControl {
    /// Which machine recorded it.
    pub peer: String,
    /// The base URL that machine is reached at — synthesized from its
    /// name, and therefore unable to resolve. See
    /// [`refuse_a_name_that_could_resolve`].
    pub url: String,
    /// The body `POST /action` answers with.
    pub submit: serde_json::Value,
    /// The body `GET /action/<id>` answers with, per id.
    ///
    /// An id with no entry here is a machine that accepted an action and
    /// then said nothing about it — not a gap in the fixture but the
    /// case the whole `Unknown` standing exists for.
    pub status: BTreeMap<String, serde_json::Value>,
}

impl RecordedControl {
    /// Reads one, naming `file` in anything it complains about.
    ///
    /// Unknown keys are refused. A person types this file, so a typo is
    /// the likely reading of `submitt:` — the same argument that makes
    /// strictness right for a manifest and wrong for a wire format
    /// between machines that upgrade at different times.
    fn parse(text: &str, file: &Path, peer: &str, url: &str) -> Result<Self, String> {
        let at = |what: &str| format!("fixture control {}: {what}", file.display());
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| at(&format!("{e}")))?;
        let object = value
            .as_object()
            .ok_or_else(|| at("expected an object with a `submit` key"))?;
        for key in object.keys() {
            if key != "submit" && key != "status" {
                return Err(at(&format!(
                    "`{key}` is not a key here; expected `submit` or `status`"
                )));
            }
        }
        let submit = object
            .get("submit")
            .ok_or_else(|| at("no `submit`: a control surface has to answer a submission"))?
            .clone();
        let mut status = BTreeMap::new();
        if let Some(recorded) = object.get("status") {
            let replies = recorded
                .as_object()
                .ok_or_else(|| at("`status` maps an action id to the reply for it"))?;
            for (id, reply) in replies {
                status.insert(id.clone(), reply.clone());
            }
        }
        Ok(Self {
            peer: peer.to_string(),
            url: url.to_string(),
            submit,
            status,
        })
    }
}

/// Refuses a fixture peer name that could name a machine on a network.
///
/// A peer's URL is synthesized from its file name, so the name is the
/// only thing standing between a fixture run and a real address. A bare
/// label cannot resolve; a dotted name or an address can.
///
/// **`FixtureTransport` makes this redundant today** — it holds a map
/// and has no socket, so even a resolvable name reaches nothing. It is
/// checked anyway, because the guarantee should not rest on the current
/// implementation of a transport that could reasonably grow a
/// passthrough for recording, and because a fixture set is data, which
/// is the part of a system that gets copied without its reasons.
fn refuse_a_name_that_could_resolve(name: &str) -> Result<(), String> {
    if name.parse::<IpAddr>().is_ok() {
        return Err(format!(
            "`{name}` is an address, and a fixture peer is named by a bare label so that \
             nothing here can reach a machine"
        ));
    }
    if name.contains('.') {
        return Err(format!(
            "`{name}` looks like a host name, and a fixture peer is named by a bare label \
             so that nothing here can reach a machine"
        ));
    }
    Ok(())
}

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
    let (peers, control) = load_peers(dir)?;
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
                ScriptedShellRunner::new,
            );
            (project.manifest.clone(), adapters)
        })
        .collect();

    Ok(FixtureSet {
        now,
        projects,
        peers,
        control,
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
fn load_peers(dir: &Path) -> Result<(Vec<BoxedPeer>, Vec<RecordedControl>), String> {
    let Ok(entries) = std::fs::read_dir(dir.join(PEERS_DIR)) else {
        return Ok((Vec::new(), Vec::new()));
    };
    // Sorted: `read_dir` order is filesystem-defined, and two runs must
    // put the same machine in the same row.
    //
    // Control files are filtered out rather than treated as machines:
    // `pi5.control.json` has the extension and would otherwise load as a
    // peer named `pi5.control` — which the name check would then refuse,
    // reporting a typo nobody made.
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter(|p| !p.to_string_lossy().ends_with(CONTROL_SUFFIX))
        .collect();
    files.sort();

    let mut peers = Vec::new();
    let mut control = Vec::new();
    for file in files {
        let name = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .ok_or_else(|| format!("fixture peer {} has no name", file.display()))?;
        refuse_a_name_that_could_resolve(&name)
            .map_err(|why| format!("fixture peer {}: {why}", file.display()))?;
        let body = std::fs::read_to_string(&file)
            .map_err(|e| format!("fixture peer {}: {e}", file.display()))?;
        let url = format!("https://{name}");
        let mut transport = FixtureTransport::new();
        transport.insert(format!("{url}{STATE_PATH}"), body, None);

        if let Some(recorded) = recorded_control(dir, &name, &url)? {
            control.push(recorded);
        }

        let transport: Box<dyn HttpTransport + Send> = Box::new(transport);
        peers.push(PeerClient::new(transport, url));
    }
    Ok((peers, control))
}

/// The contents of `peers/<name>.control.json`, if there is one.
///
/// `None` means this machine has no control surface — the fixture-set
/// spelling of a probe started without `--allow-control`.
///
/// A file that is present but unreadable is an error rather than a
/// silent `None`. A fixture set that half-loads is worse than one that
/// refuses: the cockpit would render the machine as un-actable and the
/// operator would believe it.
fn recorded_control(dir: &Path, name: &str, url: &str) -> Result<Option<RecordedControl>, String> {
    let file = dir.join(PEERS_DIR).join(format!("{name}{CONTROL_SUFFIX}"));
    if !file.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("fixture control {}: {e}", file.display()))?;
    RecordedControl::parse(&text, &file, name, url).map(Some)
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
        assert_eq!(set.projects.len(), 3, "model-experiments, sesh and ttui");
        assert_eq!(
            set.now,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
        );
    }

    /// The set holds one project whose *output* is the point rather
    /// than its development, and it is recorded rather than written:
    /// 318 real records from `Model-Experiments@main`. Two defects in
    /// the metrics pane survived every feed that had been invented for
    /// it and did not survive this one — see the fixture's README.
    #[test]
    fn one_shipped_project_carries_a_real_metrics_feed() {
        let set = load(&fixtures()).unwrap();
        let (me, adapters) = set
            .projects
            .iter()
            .find(|(v, _)| v.manifest().project.name == "model-experiments")
            .expect("model-experiments is in the fixture set");
        assert!(
            me.manifest()
                .artifacts
                .iter()
                .any(|a| a.kind == parallax_baseline::manifest::ArtifactKind::Metrics),
            "the recorded manifest still declares a metrics feed"
        );
        assert_eq!(
            adapters.artifacts.len(),
            2,
            "the figure feed and the metrics feed"
        );
        assert!(
            adapters.sessions.is_none(),
            "that repository has no .claude/worktrees/"
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

    /// A bare label is the only shape a fixture peer may have, because
    /// the URL is built from it and a fixture that could reach a network
    /// is not a fixture.
    #[test]
    fn a_peer_name_that_could_name_a_real_machine_is_refused() {
        assert!(refuse_a_name_that_could_resolve("pi5").is_ok());
        assert!(refuse_a_name_that_could_resolve("tates-laptop").is_ok());
        for hostile in ["pi5.tail-scale.ts.net", "10.0.0.4", "::1", "127.0.0.1"] {
            let err = refuse_a_name_that_could_resolve(hostile)
                .expect_err("accepted a name that could reach a machine");
            assert!(err.contains(hostile), "got {err}");
        }
    }

    /// The operator is looking at a directory, so the complaint has to
    /// name the file rather than only the rule it broke.
    #[test]
    fn a_refused_peer_name_is_reported_against_its_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CLOCK_FILE), "1700000000").unwrap();
        std::fs::create_dir_all(dir.path().join(PEERS_DIR)).unwrap();
        std::fs::write(dir.path().join(PEERS_DIR).join("10.0.0.4.json"), "{}").unwrap();

        let err = error_from(dir.path());
        assert!(err.contains("10.0.0.4.json"), "got {err}");
    }

    /// Control is opt-in per machine, and a fixture set spells that with
    /// the presence of a file — so a control file must not also load as
    /// a machine of its own.
    #[test]
    fn a_control_file_is_not_mistaken_for_a_machine() {
        let set = load(&fixtures()).unwrap();
        let names: Vec<&str> = set.peers.iter().map(|p| p.name()).collect();
        assert_eq!(names, ["pi5", "tates-laptop"]);
    }

    /// The shipped fixture set covers both answers a real deployment
    /// gives: one machine that may be asked, one that may only be
    /// watched.
    #[test]
    fn exactly_one_shipped_machine_recorded_a_control_surface() {
        let set = load(&fixtures()).unwrap();
        let controlled: Vec<&str> = set.control.iter().map(|c| c.peer.as_str()).collect();
        assert_eq!(controlled, ["pi5"]);
        assert_eq!(set.control[0].url, "https://pi5");
    }

    #[test]
    fn a_peer_with_no_control_file_records_no_control_surface() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CLOCK_FILE), "1700000000").unwrap();
        std::fs::create_dir_all(dir.path().join(PEERS_DIR)).unwrap();
        std::fs::write(
            dir.path().join(PEERS_DIR).join("laptop.json"),
            r#"{"apiVersion":"parallax/v1","peer":"laptop","now":{"secs_since_epoch":1700000000,"nanos_since_epoch":0},"projects":[]}"#,
        )
        .unwrap();

        let set = load(dir.path()).expect("a peer-only fixture set loads");
        assert_eq!(set.peers.len(), 1);
        assert!(
            set.control.is_empty(),
            "a machine gained control from nothing"
        );
    }

    /// A fixture set that half-loads is worse than one that refuses: the
    /// cockpit would render the machine as un-actable and the operator
    /// would believe it.
    #[test]
    fn a_control_file_that_cannot_be_read_is_an_error_rather_than_no_control() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CLOCK_FILE), "1700000000").unwrap();
        std::fs::create_dir_all(dir.path().join(PEERS_DIR)).unwrap();
        std::fs::write(
            dir.path().join(PEERS_DIR).join("laptop.json"),
            r#"{"apiVersion":"parallax/v1","peer":"laptop","now":{"secs_since_epoch":1700000000,"nanos_since_epoch":0},"projects":[]}"#,
        )
        .unwrap();
        let control = dir.path().join(PEERS_DIR).join("laptop.control.json");

        std::fs::write(&control, "{ not json").unwrap();
        assert!(error_from(dir.path()).contains("laptop.control.json"));

        // A typo is the likely reading of an unknown key, and a fixture
        // that ignored one would silently record nothing.
        std::fs::write(&control, r#"{"submitt":{"result":"refused"}}"#).unwrap();
        let err = error_from(dir.path());
        assert!(err.contains("submitt"), "got {err}");

        std::fs::write(&control, r#"{"status":{}}"#).unwrap();
        let err = error_from(dir.path());
        assert!(err.contains("submit"), "got {err}");
    }

    /// The bodies are recorded verbatim, including one this version
    /// cannot parse — which is a case the cockpit has behaviour for, not
    /// a broken fixture. See `fixtures/peers/README.md`.
    #[test]
    fn a_recorded_reply_is_kept_even_when_this_version_cannot_read_it() {
        let set = load(&fixtures()).unwrap();
        let recorded = &set.control[0];
        assert_eq!(recorded.submit["result"], "queued");
        assert!(
            recorded.status.is_empty(),
            "a machine that says nothing about an action is the point of this recording"
        );
    }
}
