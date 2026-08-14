//! `plumb init`: scaffolds `.plumb/` from the bundled templates. A
//! missing `.plumb/` is the expected first-run state, not a failure,
//! and an existing `config.yaml`/`taste.md` is never overwritten.

use super::IoFailure;
use std::path::{Path, PathBuf};

/// The bundled `config.yaml` template, embedded so a cached binary can
/// scaffold `.plumb/` with no plugin directory in scope.
const CONFIG_TEMPLATE: &str = include_str!("../../../templates/config.example.yaml");
/// The bundled `taste.md` template; same embedding rationale as above.
const TASTE_TEMPLATE: &str = include_str!("../../../templates/taste.md");

/// One action taken while scaffolding `.plumb/`, reported to the
/// operator once `run_init` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InitAction {
    /// A template was written because the target did not exist.
    Wrote(PathBuf),
    /// The target already existed and was left untouched.
    Kept(PathBuf),
}

/// Scaffolds `dir` from the bundled templates: creates `scripts/` and
/// `runs/` subdirectories unconditionally, and writes `config.yaml`/
/// `taste.md` only when absent. A missing `.plumb/` is the expected
/// first-run state, not a failure — this never errors on that account,
/// and never overwrites a file the operator may have already edited.
pub(super) fn run_init(dir: &Path) -> Result<Vec<InitAction>, IoFailure> {
    for sub in ["scripts", "runs"] {
        let path = dir.join(sub);
        std::fs::create_dir_all(&path).map_err(|source| IoFailure { path, source })?;
    }
    let mut actions = Vec::new();
    for (template, target) in [
        (CONFIG_TEMPLATE, "config.yaml"),
        (TASTE_TEMPLATE, "taste.md"),
    ] {
        let dest = dir.join(target);
        if dest.exists() {
            actions.push(InitAction::Kept(dest));
            continue;
        }
        std::fs::write(&dest, template).map_err(|source| IoFailure {
            path: dest.clone(),
            source,
        })?;
        actions.push(InitAction::Wrote(dest));
    }
    Ok(actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_scaffolds_a_missing_plumb_dir_rather_than_erroring() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".plumb");
        assert!(!dir.exists());

        let actions = run_init(&dir).unwrap();

        assert!(dir.join("scripts").is_dir());
        assert!(dir.join("runs").is_dir());
        assert!(dir.join("config.yaml").is_file());
        assert!(dir.join("taste.md").is_file());
        assert_eq!(
            actions,
            vec![
                InitAction::Wrote(dir.join("config.yaml")),
                InitAction::Wrote(dir.join("taste.md")),
            ]
        );
        let written = std::fs::read_to_string(dir.join("config.yaml")).unwrap();
        assert_eq!(written, CONFIG_TEMPLATE);
    }

    #[test]
    fn init_never_overwrites_an_existing_config_or_taste_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".plumb");
        run_init(&dir).unwrap();
        std::fs::write(dir.join("config.yaml"), "# hand-edited\n").unwrap();

        let actions = run_init(&dir).unwrap();

        assert_eq!(
            actions,
            vec![
                InitAction::Kept(dir.join("config.yaml")),
                InitAction::Kept(dir.join("taste.md")),
            ]
        );
        let contents = std::fs::read_to_string(dir.join("config.yaml")).unwrap();
        assert_eq!(contents, "# hand-edited\n", "must not clobber edits");
    }
}
