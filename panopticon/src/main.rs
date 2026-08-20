//! The Parallax cockpit.
//!
//! This is the one place a wall clock is sampled outside fixture mode,
//! the one place that touches a terminal, and the one place that decides
//! whether this run can act at all: fixture mode builds no executors, so
//! a demo refuses every action out loud rather than merging something
//! from recorded state.

use panopticon::app::Panopticon;
use panopticon::control::Control;
use panopticon::fixtures;
use panopticon::refresh::{Clock, Refresher};
use parallax_baseline::actions::ActionExecutor;
use parallax_baseline::adapters::factory::{executor_for, from_manifest, AdapterConfig};
use parallax_baseline::freshness::DEFAULT_POLL_INTERVAL;
use parallax_baseline::registry::Registry;
use parallax_baseline::state::ProjectAdapters;
use parallax_baseline::validate::Validated;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
panopticon — the Parallax cockpit (read-only)

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
    let (projects, clock, live) = match load(source, &config) {
        Ok(loaded) => loaded,
        Err(problem) => return fail(&problem),
    };

    let validated: Vec<Validated> = projects.iter().map(|(v, _)| v.clone()).collect();
    let control = Control::new(control_for(&validated, &config, live));
    let refresher = Refresher::spawn(projects, clock);
    let mut app =
        Panopticon::new(&validated, refresher, clock, DEFAULT_POLL_INTERVAL).with_control(control);

    if let Err(e) = ttui::app::run(&mut app) {
        eprintln!("panopticon: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// An executor per project, in the same order the rail shows them.
///
/// `live` is false in fixture mode, and then every slot is `None`. A
/// cockpit rendering recorded state that could merge a real pull request
/// is a demo with a loaded weapon in it.
fn control_for(
    projects: &[Validated],
    config: &AdapterConfig,
    live: bool,
) -> Vec<Option<Box<dyn ActionExecutor>>> {
    projects
        .iter()
        .map(|v| {
            if !live {
                return None;
            }
            executor_for(v, config).map(|e| Box::new(e) as Box<dyn ActionExecutor>)
        })
        .collect()
}

/// Turns the chosen source into projects and a clock.
/// What a source resolves to: the projects and their adapters, the
/// clock to read, and whether this run is allowed to act.
type Loaded = (Vec<(Validated, ProjectAdapters)>, Clock, bool);

/// Returns the projects, the clock, and whether this run may act.
fn load(source: Source, config: &AdapterConfig) -> Result<Loaded, String> {
    let registry = match source {
        Source::Fixtures(dir) => {
            let set = fixtures::load(&dir)?;
            // The one mode with no wall clock in it at all, and the one
            // that may not act: recorded state, real consequences.
            return Ok((set.projects, Clock::Frozen(set.now), false));
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

    Ok((projects, Clock::System, true))
}

/// An environment variable, when it is set to something.
fn non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("panopticon: {problem}\n\n{USAGE}");
    ExitCode::FAILURE
}
