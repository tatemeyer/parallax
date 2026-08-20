//! The read-only boundary, checked rather than remembered.
//!
//! Sub-project #5 gave the cockpit the ability to act, which makes this
//! test more necessary rather than less. It used to say "no file in
//! this crate names `actions`". Deleting it when control arrived would
//! have quietly discarded the guarantee it encodes, so it moved
//! instead: **only `control` may name an action**, and every module
//! that observes or renders still may not.
//!
//! That is the property worth keeping. A render path that can reach an
//! action is a render path that can merge a pull request while drawing
//! a frame, and the reason the rest of the screen is safe to leave
//! running is that it structurally cannot.
//!
//! A grep is crude, and it is exactly proportionate. The claim is
//! "observation cannot mutate anything", the thing that would actually
//! happen is somebody reaching for `actions::` from inside `view/`, and
//! a type-level guarantee would need a facade crate to say the same.

use std::path::{Path, PathBuf, MAIN_SEPARATOR};

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

/// The four files allowed to name an action, and why each one is.
///
/// `control` performs them. `app.rs` is the event loop, where a
/// keypress becomes an intent - it names actions and performs none,
/// which is the same split `control` itself keeps. `main.rs` is the
/// composition root, and the one place that decides whether this run
/// can act at all: it is where fixture mode is denied executors.
///
/// `courier.rs` carries an action to another machine, and was added
/// when control crossed the wire. The argument for it is the argument
/// against putting it in the refresh thread: a submission is a network
/// call that can block, the refresh thread is the other one, and
/// merging them would have meant either freezing the UI or letting
/// observation name an action. It decides nothing - the prompt is
/// `control`'s and the authorization is the far machine's - and it is
/// the reason `refresh.rs` is still on the other side of this line.
///
/// Everything else - every view module, the refresh thread, the bell,
/// the fixtures - is observation, and stays structurally unable to act.
const MAY_ACT: [&str; 4] = ["control", "app.rs", "main.rs", "courier.rs"];

fn may_act(path: &Path) -> bool {
    let text = path.to_string_lossy().replace(MAIN_SEPARATOR, "/");
    MAY_ACT
        .iter()
        .any(|allowed| text.contains(&format!("/{allowed}/")) || text.ends_with(allowed))
}

#[test]
fn nothing_outside_control_reaches_for_baselines_actions_module() {
    let files = sources();
    assert!(files.len() > 5, "the walk found the crate: {files:?}");

    let mut checked = 0;
    for path in files {
        if may_act(&path) {
            continue;
        }
        checked += 1;
        let text = shipped_code(&std::fs::read_to_string(&path).expect("readable"));
        for needle in ["parallax_baseline::actions", "actions::"] {
            assert!(
                !text.contains(needle),
                "{} names `{needle}` - observation may not act. Only `control`                  and the event loop may, and a render path that can reach an                  action can merge a pull request while drawing a frame",
                path.display()
            );
        }
    }
    assert!(
        checked > 5,
        "the exemption swallowed the crate: only {checked} files were checked"
    );
}

/// The exemption is a list, not a hole. If `control` ever stops naming
/// actions the list is stale and should shrink - and if a second module
/// is ever added to it, this test is where that argument happens.
#[test]
fn the_module_allowed_to_act_actually_does() {
    let control = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("control")
        .join("mod.rs");
    let text = std::fs::read_to_string(&control).expect("control/mod.rs exists");
    assert!(
        text.contains("parallax_baseline::actions"),
        "the exemption list names a module that does not use it"
    );
}

/// The other half: the cockpit itself still writes nothing to disk.
///
/// Control changes this less than it looks. A ruling is appended by
/// `LocalExecutor`, in the library, through a path the manifest
/// declares — the cockpit asks for it and does not do it. So a
/// `File::create` in this crate is still a surprise worth arguing about
/// in review rather than discovering later.
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
