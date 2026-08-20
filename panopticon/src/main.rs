//! The Parallax cockpit.
//!
//! This is the one place a wall clock is sampled outside fixture mode,
//! the one place that touches a terminal, and the one place that decides
//! whether this run can act at all: fixture mode builds no executors, so
//! a demo refuses every action out loud rather than merging something
//! from recorded state.

use panopticon::app::Panopticon;
use panopticon::control::{Control, Destination};
use panopticon::courier::{BoxedSubmitter, Courier};
use panopticon::fixtures;
use panopticon::refresh::{BoxedPeer, Clock, Refresher};
use parallax_baseline::actions::RemoteExecutor;
use parallax_baseline::adapters::factory::{executor_for, from_manifest, AdapterConfig};
use parallax_baseline::adapters::http::{HttpTransport, UreqTransport};
use parallax_baseline::freshness::DEFAULT_POLL_INTERVAL;
use parallax_baseline::peers::PeerClient;
use parallax_baseline::registry::Registry;
use parallax_baseline::state::ProjectAdapters;
use parallax_baseline::validate::Validated;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "\
panopticon — the Parallax cockpit

USAGE:
    panopticon [--projects-root <dir> | --registry <file> | --fixtures <dir>]

OPTIONS:
    --projects-root <dir>  Treat every child of <dir> holding a
                           parallax.yaml as a registered project.
    --registry <file>      Load the project roots a registry file lists.
    --fixtures <dir>       Render recorded state with a frozen clock.
                           Deterministic: the same frames every run.
    --github-token <tok>   Authenticate the GitHub work adapter. Falls
                           back to $GITHUB_TOKEN, then $GH_TOKEN.
    -h, --help             This.

KEYS:
    j / k     move within the detail pane      1-5  choose a pane
    Tab       next project                     r    refresh the readable sources
    c / C     run this project's / every project's build checks

    l         label the selected work item     R    request a re-review
    p         capture this project             P    push a branch  (confirm)
    m         merge the selected pull request  (type its number to confirm)
    5         what this session has done

    ?         help                             q    quit

An action that cannot be undone asks before it happens, and asks in a
way that cannot be answered by reflex: a merge wants the pull request
number typed, not a keystroke. Fixture mode builds no executors, so
every action there is refused and says why.

The refresh cycle never runs a build. Checks that do — `cargo test` and
friends — run only when you ask, because a cadence is the right shape
for observing state and the wrong shape for producing it.
";

/// What the operator asked for on the command line.
enum Source {
    ProjectsRoot(PathBuf),
    RegistryFile(PathBuf),
    Fixtures(PathBuf),
    /// Nothing given. The cockpit still starts, and says what it looked
    /// for — "no projects" is the common answer today, and a cockpit
    /// that exits on it teaches nothing about why.
    Nothing,
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut source = Source::Nothing;
    let mut token = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--projects-root" => match args.next() {
                Some(dir) => source = Source::ProjectsRoot(dir.into()),
                None => return fail("--projects-root needs a directory"),
            },
            "--registry" => match args.next() {
                Some(file) => source = Source::RegistryFile(file.into()),
                None => return fail("--registry needs a file"),
            },
            "--fixtures" => match args.next() {
                Some(dir) => source = Source::Fixtures(dir.into()),
                None => return fail("--fixtures needs a directory"),
            },
            "--github-token" => match args.next() {
                Some(t) => token = Some(t),
                None => return fail("--github-token needs a token"),
            },
            other => return fail(&format!("unknown argument `{other}`")),
        }
    }

    // Credential discovery is frontend work: `parallax-baseline` takes a
    // token and never reads the environment, so that it runs identically
    // in a test. Somebody has to look, though — without it a private
    // repository degrades to a 404 and the cockpit reports a project it
    // simply is not allowed to see.
    let token = token
        .or_else(|| non_empty("GITHUB_TOKEN"))
        .or_else(|| non_empty("GH_TOKEN"));

    let config = AdapterConfig {
        poll_interval: DEFAULT_POLL_INTERVAL,
        github_token: token,
    };
    let (projects, peers, clock, live) = match load(source, &config) {
        Ok(loaded) => loaded,
        Err(problem) => return fail(&problem),
    };

    // A peer is named by its registry entry, never by what it answers
    // with, so the rail can show a machine it has not reached yet.
    let peer_names: Vec<String> = peers.iter().map(|p| p.name().to_string()).collect();
    let peer_urls: Vec<(String, String)> = peers
        .iter()
        .map(|p| (p.name().to_string(), p.url().to_string()))
        .collect();

    let validated: Vec<Validated> = projects.iter().map(|(v, _)| v.clone()).collect();
    let control = Control::new(control_for(&validated, &config, live));
    // Built before the peers move into the refresh thread, and from the
    // same list, so a machine this cockpit watches is a machine it can
    // at least *offer* an action to. Whether that machine accepts is the
    // machine's own decision, answered at the point of asking.
    let courier = courier_for(&peer_urls, live);
    let refresher = Refresher::spawn_with_peers(projects, peers, clock);
    let mut app = Panopticon::new(&validated, refresher, clock, DEFAULT_POLL_INTERVAL)
        .with_control(control)
        .with_courier(courier)
        .with_peers(peer_names);

    if let Err(e) = ttui::app::run(&mut app) {
        eprintln!("panopticon: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// A destination per **local** project, in the same order the rail shows
/// them. A peer's rows are not here: they are routed by machine name,
/// because they come and go as that machine answers.
///
/// `live` is false in fixture mode, and then every destination goes
/// nowhere and says so. A cockpit rendering recorded state that could
/// merge a real pull request is a demo with a loaded weapon in it.
fn control_for(projects: &[Validated], config: &AdapterConfig, live: bool) -> Vec<Destination> {
    projects
        .iter()
        .map(|v| {
            if !live {
                return Destination::Nowhere("the cockpit is running against fixtures".to_string());
            }
            match executor_for(v, config) {
                Some(e) => Destination::Local(Box::new(e)),
                None => Destination::Nowhere(
                    "this project declares no work feed to address".to_string(),
                ),
            }
        })
        .collect()
}

/// A courier over every machine this cockpit watches.
///
/// **Every peer, not a configured subset.** Whether a machine will
/// accept an action is that machine's decision, made by the probe on it
/// and answered at the point of asking; a second list here would be a
/// second place for that to be wrong, and the one that cannot see the
/// flag. `live` is false in fixture mode, and then it carries nothing —
/// a recorded cockpit that could act on a real Pi is a demo with a
/// loaded weapon in it.
fn courier_for(peers: &[(String, String)], live: bool) -> Courier {
    if !live {
        return Courier::idle();
    }
    // Distinguishes this run of this cockpit from the last one, so a
    // restart cannot reuse ids a probe already has answers filed under.
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let client = this_machine();
    let submitters = peers
        .iter()
        .map(|(name, url)| {
            Box::new(RemoteExecutor::new(
                UreqTransport::new(),
                url,
                name,
                client.clone(),
                run,
            )) as BoxedSubmitter
        })
        .collect();
    Courier::spawn(submitters)
}

/// How this machine names itself in an action it sends.
///
/// Recorded by the far probe as a **claim** — nothing authenticates it,
/// and it exists so an audit line says `desktop` rather than the
/// `127.0.0.1` every request arrives from behind `tailscale serve`.
fn this_machine() -> String {
    non_empty("COMPUTERNAME")
        .or_else(|| non_empty("HOSTNAME"))
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Turns the chosen source into projects and a clock.
/// What a source resolves to: the projects and their adapters, the
/// clock to read, and whether this run is allowed to act.
type Loaded = (
    Vec<(Validated, ProjectAdapters)>,
    Vec<BoxedPeer>,
    Clock,
    bool,
);

/// Returns the projects, the clock, and whether this run may act.
fn load(source: Source, config: &AdapterConfig) -> Result<Loaded, String> {
    let registry = match source {
        Source::Fixtures(dir) => {
            let set = fixtures::load(&dir)?;
            // The one mode with no wall clock in it at all, and the one
            // that may not act: recorded state, real consequences.
            return Ok((set.projects, set.peers, Clock::Frozen(set.now), false));
        }
        Source::ProjectsRoot(dir) => Registry::scan(&dir),
        Source::RegistryFile(file) => Registry::from_file(&file).map_err(|e| e.to_string())?,
        Source::Nothing => Registry::default(),
    };

    for failure in registry.failures() {
        // A project that could not be loaded degrades itself and nothing
        // else, and the operator hears about it rather than wondering
        // where a row went.
        eprintln!("panopticon: skipping {failure}");
    }

    let projects = registry
        .projects()
        .iter()
        .map(|p| {
            let adapters = from_manifest(&p.manifest, config);
            (p.manifest.clone(), adapters)
        })
        .collect();

    let peers = registry
        .peers()
        .iter()
        .map(|peer| {
            let transport: Box<dyn HttpTransport + Send> = Box::new(UreqTransport::new());
            PeerClient::new(transport, &peer.url).with_interval(config.poll_interval)
        })
        .collect();

    Ok((projects, peers, Clock::System, true))
}

/// An environment variable, when it is set to something.
fn non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("panopticon: {problem}\n\n{USAGE}");
    ExitCode::FAILURE
}
