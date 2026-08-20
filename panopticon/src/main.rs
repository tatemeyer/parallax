//! The Parallax cockpit.
//!
//! This is the one place a wall clock is sampled outside fixture mode,
//! the one place that touches a terminal, and the one place that decides
//! what this run may act on.
//!
//! **The two halves of that decision are not the same question.** A
//! *local* action shells out on this machine, so fixture mode builds no
//! local executors at all and a demo refuses out loud rather than
//! merging something from recorded state. A *remote* action is an HTTP
//! request, and in fixture mode it goes through a transport that holds a
//! map and owns no socket — so a recorded cockpit can be asked, can
//! answer from the recording, and still cannot reach a machine.

use panopticon::app::Panopticon;
use panopticon::control::{Control, Destination};
use panopticon::courier::{BoxedSubmitter, Courier};
use panopticon::fixtures;
use panopticon::refresh::{BoxedPeer, Clock, Refresher};
use parallax_baseline::actions::{RemoteExecutor, ACTION_PATH};
use parallax_baseline::adapters::factory::{executor_for, from_manifest, AdapterConfig};
use parallax_baseline::adapters::http::{FixtureTransport, HttpTransport, Method, UreqTransport};
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
number typed, not a keystroke. Fixture mode builds no local executors,
so an action on this machine is refused there and says why; an action
offered to a recorded machine answers from the recording, and reaches
no machine at all.

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
    let Loaded {
        projects,
        peers,
        submitters,
        clock,
        live,
    } = match load(source, &config) {
        Ok(loaded) => loaded,
        Err(problem) => return fail(&problem),
    };

    // A peer is named by its registry entry, never by what it answers
    // with, so the rail can show a machine it has not reached yet.
    let peer_names: Vec<String> = peers.iter().map(|p| p.name().to_string()).collect();

    let validated: Vec<Validated> = projects.iter().map(|(v, _)| v.clone()).collect();
    let control = Control::new(control_for(&validated, &config, live));
    // Built before the peers move into the refresh thread. In live mode
    // it carries to every machine this cockpit watches; in fixture mode,
    // to whichever recorded one declared a control surface.
    let courier = Courier::spawn(submitters);
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

/// A submitter for one recorded machine.
///
/// **The real `RemoteExecutor`, not a double.** Id generation, the JSON,
/// the reading of a 4xx as a refusal and everything else as unknown —
/// all of it is the shipping code, and only the bytes are recorded. A
/// hand-written stand-in would be a second implementation of the thing
/// the scenarios exist to photograph, and a photograph of a second
/// implementation proves nothing about the first.
///
/// It cannot reach the machine it names. `FixtureTransport` holds a map
/// and owns no socket, and the URL was synthesized from a file name that
/// `fixtures` refused to let resolve.
fn recorded_submitter(recorded: &fixtures::RecordedControl) -> BoxedSubmitter {
    let mut transport = FixtureTransport::new();
    transport.insert_write(
        Method::Post,
        format!("{}{ACTION_PATH}", recorded.url),
        recorded.submit.to_string(),
    );
    for (id, reply) in &recorded.status {
        transport.insert(
            format!("{}{ACTION_PATH}/{id}", recorded.url),
            reply.to_string(),
            None,
        );
    }
    Box::new(RemoteExecutor::new(
        transport,
        &recorded.url,
        &recorded.peer,
        fixtures::FIXTURE_CLIENT,
        fixtures::FIXTURE_RUN,
    ))
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

/// What a source resolves to.
///
/// A struct rather than a tuple because `live` is the field that has to
/// be readable at a glance: it is what decides whether this cockpit can
/// run a command on the machine it is displayed on.
struct Loaded {
    /// The projects on this disk, with their adapters.
    projects: Vec<(Validated, ProjectAdapters)>,
    /// The machines this cockpit watches.
    peers: Vec<BoxedPeer>,
    /// Which machines it may also *ask*, and how.
    ///
    /// Built here rather than in `courier_for` because fixture mode
    /// records its submitters alongside the peers they belong to, and
    /// live mode builds them from URLs. The two sources differ; what
    /// the courier receives does not.
    submitters: Vec<BoxedSubmitter>,
    /// The clock to read.
    clock: Clock,
    /// Whether a *local* action may run a command on this machine.
    ///
    /// False in fixture mode, and that is not the same question as
    /// whether a remote one may be submitted: a recorded submission goes
    /// through a transport with no socket in it, so it can be offered
    /// without any machine being reachable.
    live: bool,
}

/// Returns the projects, the clock, and whether this run may act.
fn load(source: Source, config: &AdapterConfig) -> Result<Loaded, String> {
    let registry = match source {
        Source::Fixtures(dir) => {
            let set = fixtures::load(&dir)?;
            // The one mode with no wall clock in it at all, and the one
            // whose *local* actions may not run: recorded state, real
            // consequences. Its submitters are another matter — they
            // reach a `HashMap`, not a machine.
            let submitters = set.control.iter().map(recorded_submitter).collect();
            return Ok(Loaded {
                projects: set.projects,
                peers: set.peers,
                submitters,
                clock: Clock::Frozen(set.now),
                live: false,
            });
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

    let peers: Vec<BoxedPeer> = registry
        .peers()
        .iter()
        .map(|peer| {
            let transport: Box<dyn HttpTransport + Send> = Box::new(UreqTransport::new());
            PeerClient::new(transport, &peer.url).with_interval(config.poll_interval)
        })
        .collect();

    // Distinguishes this run of this cockpit from the last one, so a
    // restart cannot reuse ids a probe already has answers filed under.
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let client = this_machine();
    // **Every peer, not a configured subset.** Whether a machine will
    // accept an action is that machine's decision, made by the probe on
    // it and answered at the point of asking; a second list here would
    // be a second place for that to be wrong, and the one that cannot
    // see the flag.
    let submitters = peers
        .iter()
        .map(|p| {
            Box::new(RemoteExecutor::new(
                UreqTransport::new(),
                p.url(),
                p.name(),
                client.clone(),
                run,
            )) as BoxedSubmitter
        })
        .collect();

    Ok(Loaded {
        projects,
        peers,
        submitters,
        clock: Clock::System,
        live: true,
    })
}

/// An environment variable, when it is set to something.
fn non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("panopticon: {problem}\n\n{USAGE}");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_baseline::actions::{Action, Submitted};

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    /// **The guarantee this arc is most likely to have broken.** Giving
    /// fixture mode a submitter says nothing about local actions, and a
    /// fixture cockpit that could run a command on the machine it is
    /// being demonstrated on is the thing the whole mode exists to
    /// prevent.
    #[test]
    fn fixture_mode_still_sends_every_local_action_nowhere() {
        let set = fixtures::load(&fixture_dir()).expect("the fixture set loads");
        let validated: Vec<Validated> = set.projects.iter().map(|(v, _)| v.clone()).collect();
        assert!(!validated.is_empty(), "the fixture set lost its projects");

        let destinations = control_for(&validated, &AdapterConfig::default(), false);
        assert_eq!(destinations.len(), validated.len());
        for destination in &destinations {
            assert!(
                matches!(destination, Destination::Nowhere(_)),
                "a fixture-mode project got somewhere to send a local action"
            );
        }
    }

    /// A recorded submitter is a `RemoteExecutor` over a transport with
    /// no socket in it. It names the machine it would act on, which is
    /// what puts that machine's name in the confirmation prompt.
    #[test]
    fn a_recorded_submitter_names_the_machine_it_would_act_on() {
        let set = fixtures::load(&fixture_dir()).unwrap();
        let submitter = recorded_submitter(&set.control[0]);
        assert_eq!(submitter.peer(), "pi5");
    }

    /// The behaviour the `pi5` recording exists to photograph: a reply
    /// this version cannot parse leaves the action's fate **unknown**,
    /// never refused.
    ///
    /// The distinction is the reason the control arc exists. Refused
    /// means nothing happened; unknown means something may well have
    /// happened and nobody can say — and an operator told "refused" when
    /// the truth is "unknown" retries a merge that already went through.
    #[test]
    fn a_reply_this_version_cannot_read_leaves_the_action_unknown() {
        let set = fixtures::load(&fixture_dir()).unwrap();
        let mut submitter = recorded_submitter(&set.control[0]);

        let outcome = submitter.submit(
            &Action::MergePullRequest {
                project: "sesh".into(),
                number: 8,
            },
            None,
        );

        match outcome {
            Submitted::Unknown { id, reason } => {
                assert_eq!(
                    id.as_str(),
                    "fixture-0-1",
                    "ids are pinned so a recording can name them"
                );
                assert!(
                    reason.contains("pi5"),
                    "an unknown has to name the machine: {reason}"
                );
            }
            other => panic!(
                "an unreadable reply became {other:?}, which would tell an operator something false"
            ),
        }
    }
}
