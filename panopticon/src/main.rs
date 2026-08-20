//! Entry point: resolves the manifests directory and an optional
//! GitHub token from the environment, then hands off to
//! `parallax_panopticon::run`, the only place in this crate a terminal
//! is touched.

use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let manifests_dir = std::env::var("PANOPTICON_MANIFESTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("manifests"));
    let github_token = std::env::var("GITHUB_TOKEN").ok();
    parallax_panopticon::run(&manifests_dir, github_token.as_deref())
}
