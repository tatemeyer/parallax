//! `parallax-probe` — serves one machine's platform state.
//!
//! The command line: arguments, credential discovery, and the bind.
//! Everything it does lives in the library beside it — see `lib.rs` for
//! what the probe is and `server::bind_address` for why it only ever
//! listens on loopback.
//!
//! This is the one file here that reads the environment. The library
//! takes what it is given, so it behaves identically in a test.

#![warn(missing_docs)]

use parallax_baseline::adapters::factory::AdapterConfig;
use parallax_baseline::freshness::DEFAULT_POLL_INTERVAL;
use parallax_baseline::registry::Registry;
use parallax_probe::server::{Probe, DEFAULT_PORT};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
parallax-probe — serves this machine's Parallax state on loopback.

USAGE:
    parallax-probe [--projects-root <dir> | --registry <file>] [OPTIONS]

    --projects-root <dir>  Treat every child of <dir> holding a
                           parallax.yaml as registered.
    --registry <file>      Use the roots a registry file lists.
    --port <n>             Loopback port (default 8737).
    --peer <name>          How this machine names itself (default: hostname).
    --github-token <tok>   Authenticate the GitHub work adapter. Falls
                           back to $GITHUB_TOKEN, then $GH_TOKEN.
    -h, --help             This.

The probe binds 127.0.0.1 only. Publish it with:
    tailscale serve --bg --https=443 http://127.0.0.1:8737
";

/// How this machine names itself, when nobody said.
///
/// No dependency for this: `COMPUTERNAME` on Windows, `HOSTNAME` when a
/// shell exported it, and `/etc/hostname` on the Pi, which is the case
/// the first two miss.
fn default_peer() -> String {
    if let Ok(name) = std::env::var("COMPUTERNAME") {
        return name;
    }
    if let Ok(name) = std::env::var("HOSTNAME") {
        return name;
    }
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// What the command line asked for.
struct Args {
    projects_root: Option<PathBuf>,
    registry: Option<PathBuf>,
    port: u16,
    peer: Option<String>,
    github_token: Option<String>,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut args = Args {
        projects_root: None,
        registry: None,
        port: DEFAULT_PORT,
        peer: None,
        github_token: None,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        let mut value = || argv.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "-h" | "--help" => return Ok(None),
            "--projects-root" => args.projects_root = Some(PathBuf::from(value()?)),
            "--registry" => args.registry = Some(PathBuf::from(value()?)),
            "--peer" => args.peer = Some(value()?),
            "--github-token" => args.github_token = Some(value()?),
            "--port" => {
                let raw = value()?;
                args.port = raw.parse().map_err(|_| format!("`{raw}` is not a port"))?;
            }
            other => return Err(format!("unknown flag `{other}`")),
        }
    }
    Ok(Some(args))
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(problem) => {
            eprintln!("parallax-probe: {problem}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let registry = match (&args.projects_root, &args.registry) {
        (Some(root), _) => Registry::scan(root),
        (None, Some(file)) => match Registry::from_file(file) {
            Ok(registry) => registry,
            Err(e) => {
                // A registry file that cannot be read is no answer, not
                // a partial one — the rule the constructor already holds.
                eprintln!("parallax-probe: {e}");
                return ExitCode::FAILURE;
            }
        },
        // Serving nothing is a legitimate answer and a much better one
        // than refusing to start: a machine with no projects yet is
        // still a machine the cockpit should be able to reach.
        (None, None) => Registry::default(),
    };

    for failure in registry.failures() {
        eprintln!("parallax-probe: {failure}");
    }

    // Credential discovery is a frontend's job: `parallax-baseline` takes
    // a token and never reads the environment, so it runs identically in
    // a test. Somebody still has to look — without it every private
    // repository degrades to a 404, and a cockpit two machines away
    // reports a project this one simply is not allowed to see.
    let config = AdapterConfig {
        poll_interval: DEFAULT_POLL_INTERVAL,
        github_token: args
            .github_token
            .or_else(|| non_empty("GITHUB_TOKEN"))
            .or_else(|| non_empty("GH_TOKEN")),
    };

    let peer = args.peer.unwrap_or_else(default_peer);
    let probe = match Probe::bind(args.port) {
        Ok(probe) => probe,
        Err(problem) => {
            eprintln!("parallax-probe: {problem}");
            return ExitCode::FAILURE;
        }
    };

    eprintln!(
        "parallax-probe: {peer} serving {} project(s) on {}",
        registry.projects().len(),
        probe.url()
    );
    probe.serve(&registry, &config, &peer);
    ExitCode::SUCCESS
}

/// An environment variable, when it is set to something.
fn non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}
