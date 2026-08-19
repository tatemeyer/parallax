//! The Parallax cockpit. Read-only: it observes every registered
//! project and mutates nothing, anywhere.
//!
//! This is the one place a wall clock is sampled outside fixture mode,
//! and the one place that touches a terminal.

use panopticon::app::Panopticon;
use panopticon::fixtures;
use panopticon::refresh::{Clock, Refresher};
use parallax_baseline::adapters::factory::{from_manifest, AdapterConfig};
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
    j / k     move within the detail pane      1-4  choose a pane
    Tab       next project                     r    refresh the readable sources
    c / C     run this project's / every project's build checks
    ?         help                             q    quit

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

    let (projects, clock) = match load(source, token) {
        Ok(loaded) => loaded,
        Err(problem) => return fail(&problem),
    };

    let validated: Vec<Validated> = projects.iter().map(|(v, _)| v.clone()).collect();
    let refresher = Refresher::spawn(projects, clock);
    let mut app = Panopticon::new(&validated, refresher, clock, DEFAULT_POLL_INTERVAL);

    if let Err(e) = ttui::app::run(&mut app) {
        eprintln!("panopticon: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Turns the chosen source into projects and a clock.
fn load(
    source: Source,
    token: Option<String>,
) -> Result<(Vec<(Validated, ProjectAdapters)>, Clock), String> {
    let config = AdapterConfig {
        poll_interval: DEFAULT_POLL_INTERVAL,
        github_token: token,
    };

    let registry = match source {
        Source::Fixtures(dir) => {
            let set = fixtures::load(&dir)?;
            // The one mode with no wall clock in it at all.
            return Ok((set.projects, Clock::Frozen(set.now)));
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
            let adapters = from_manifest(&p.manifest, &config);
            (p.manifest.clone(), adapters)
        })
        .collect();

    Ok((projects, Clock::System))
}

/// An environment variable, when it is set to something.
fn non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("panopticon: {problem}\n\n{USAGE}");
    ExitCode::FAILURE
}
