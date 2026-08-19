//! The read-only claim, checked rather than remembered.
//!
//! This sub-project observes and mutates nothing: no labels set, no
//! pull requests merged, no runs dispatched, nothing written to any
//! repository. `parallax-baseline` ships a whole `actions` module that
//! could do all of it, and the enforcement is simply that this crate
//! never names it.
//!
//! A grep is crude, and it is exactly proportionate. The claim is "this
//! crate cannot mutate anything", the thing that would actually happen
//! is somebody reaching for `actions::`, and a type-level guarantee
//! would need a facade crate to say the same thing.

use std::path::{Path, PathBuf};

/// The shipped code of one file: comments stripped, and everything from
/// the first `#[cfg(test)]` dropped.
///
/// Both exclusions are load-bearing rather than convenient. A doc
/// comment saying "nothing here calls `parallax_baseline::actions`" is
/// the crate stating this very promise, and a test that writes a
/// temporary fixture directory is not the cockpit holding state. What
/// is being checked is what runs when someone launches it.
fn shipped_code(text: &str) -> String {
    let code = match text.find("#[cfg(test)]") {
        Some(at) => &text[..at],
        None => text,
    };
    code.lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

/// Every `.rs` file under `panopticon/src`.
fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out.sort();
    out
}

#[test]
fn no_source_file_reaches_for_baselines_actions_module() {
    let files = sources();
    assert!(files.len() > 5, "the walk found the crate: {files:?}");

    for path in files {
        let text = shipped_code(&std::fs::read_to_string(&path).expect("readable"));
        for needle in ["parallax_baseline::actions", "actions::"] {
            assert!(
                !text.contains(needle),
                "{} names `{needle}` — this sub-project is read-only, and \
                 control is sub-project #5",
                path.display()
            );
        }
    }
}

/// The other half of read-only: nothing writes to disk either. The
/// cockpit holds no state across runs, so a `File::create` or a
/// `write` would be a surprise worth arguing about in review rather
/// than discovering later.
#[test]
fn no_source_file_writes_to_disk() {
    for path in sources() {
        let text = shipped_code(&std::fs::read_to_string(&path).expect("readable"));
        for needle in [
            "fs::write",
            "File::create",
            "create_dir",
            "OpenOptions",
            "remove_file",
        ] {
            assert!(
                !text.contains(needle),
                "{} calls `{needle}` — the cockpit reads, and holds no state \
                 across runs",
                path.display()
            );
        }
    }
}
