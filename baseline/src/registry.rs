//! Which projects are registered.
//!
//! `manifest::parse_manifest_file` reads *a* manifest from *a* path;
//! this answers the question one level up. Three ways in — an explicit
//! list of roots, a registry file, or a scan of a directory — and one
//! type out.
//!
//! A registered project that fails to load degrades itself and nothing
//! else, which is the rule `state::aggregate` already follows for a
//! failing adapter: a blank list is a worse failure than one row
//! labelled broken. That is why two of the three constructors return
//! `Self` rather than `Result` — a registry *file* that cannot be read
//! is not a partial answer, but one bad project among five is.

use crate::manifest::parse_manifest_file;
use crate::validate::{validate, Validated};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The file a project drops in its root to join the platform.
pub const MANIFEST_FILENAME: &str = "parallax.yaml";

/// One project the platform knows about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredProject {
    /// The project's short name, from its own manifest.
    pub name: String,
    /// Where the project actually is on this machine.
    pub root: PathBuf,
    /// The manifest that was read.
    pub manifest_path: PathBuf,
    /// That manifest, validated.
    pub manifest: Validated,
}

/// A registered project that could not be loaded, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError {
    /// The file that could not be read or understood.
    pub source: PathBuf,
    /// What was wrong with it, in one sentence.
    pub problem: String,
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.source.display(), self.problem)
    }
}
impl std::error::Error for RegistryError {}

/// Every registered project, and every one that could not be loaded.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Registry {
    projects: Vec<RegisteredProject>,
    failures: Vec<RegistryError>,
}

impl Registry {
    /// Loads every root's `parallax.yaml`.
    ///
    /// Infallible by design: a root that cannot be loaded becomes a
    /// [`RegistryError`] in [`Registry::failures`] and every other root
    /// still loads.
    pub fn from_roots(roots: &[PathBuf]) -> Self {
        let mut registry = Self::default();
        for root in roots {
            match load_one(root) {
                Ok(project) => registry.projects.push(project),
                Err(failure) => registry.failures.push(failure),
            }
        }
        registry
    }

    /// Loads the roots a registry file lists.
    ///
    /// The one fallible constructor: a registry file that cannot be read
    /// or parsed is not a partial answer, it is no answer.
    pub fn from_file(path: &Path) -> Result<Self, RegistryError> {
        let text = std::fs::read_to_string(path).map_err(|e| RegistryError {
            source: path.to_path_buf(),
            problem: format!("reading registry: {e}"),
        })?;
        let file: RegistryFile = serde_yaml::from_str(&text).map_err(|e| RegistryError {
            source: path.to_path_buf(),
            problem: format!("parsing registry: {e}"),
        })?;
        let roots: Vec<PathBuf> = file.projects.into_iter().map(|p| p.root).collect();
        Ok(Self::from_roots(&roots))
    }

    /// Treats every immediate child of `projects_root` that contains a
    /// manifest as registered. The zero-configuration path for projects
    /// that are siblings on disk.
    ///
    /// A directory with no manifest is not registered and is not a
    /// failure — it is simply not a project.
    pub fn scan(projects_root: &Path) -> Self {
        let Ok(entries) = std::fs::read_dir(projects_root) else {
            return Self::default();
        };
        // `read_dir` order is filesystem-defined, and
        // `PlatformState::projects` is documented as being in
        // registration order — so a scan sorts to have one.
        let mut roots: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join(MANIFEST_FILENAME).is_file())
            .collect();
        roots.sort();
        Self::from_roots(&roots)
    }

    /// Every project that loaded, in registration order.
    pub fn projects(&self) -> &[RegisteredProject] {
        &self.projects
    }

    /// Every registered project that could not be loaded.
    pub fn failures(&self) -> &[RegistryError] {
        &self.failures
    }

    /// Whether nothing at all loaded.
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }
}

/// The registry file's schema. Roots and nothing else: a project's name
/// comes from its own manifest, so a rename in `parallax.yaml` cannot
/// desynchronize from a list somewhere else.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RegistryFile {
    /// Schema version, e.g. `parallax/v1`. Parsed and unused, so a file
    /// can declare one before anything reads it.
    #[serde(default)]
    #[allow(dead_code)]
    api_version: Option<String>,
    /// The registered project roots, in order.
    projects: Vec<RegistryEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryEntry {
    root: PathBuf,
}

/// Loads one project, or says why it could not be loaded.
fn load_one(root: &Path) -> Result<RegisteredProject, RegistryError> {
    let manifest_path = root.join(MANIFEST_FILENAME);
    let mut parsed = parse_manifest_file(&manifest_path).map_err(|e| RegistryError {
        source: manifest_path.clone(),
        problem: e.to_string(),
    })?;

    // The registry knows where the project actually is; the manifest's
    // `root:` is checked into a repository that gets cloned to different
    // paths, and `manifests/ttui.yaml` declares `<projects-root>/TTUI` —
    // a placeholder that exists on no machine.
    parsed.project.root = Some(root.to_path_buf());

    let manifest = validate(parsed).map_err(|errors| RegistryError {
        source: manifest_path.clone(),
        problem: errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; "),
    })?;

    Ok(RegisteredProject {
        name: manifest.manifest().project.name.clone(),
        root: root.to_path_buf(),
        manifest_path,
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a project directory holding the given manifest text.
    fn project(dir: &Path, name: &str, manifest: &str) -> PathBuf {
        let root = dir.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(MANIFEST_FILENAME), manifest).unwrap();
        root
    }

    const GOOD: &str = "project:\n  name: ttui\n  root: <projects-root>/TTUI\n  language: rust\n";
    const INVALID: &str = "project:\n  name: ''\n";
    const UNPARSEABLE: &str = "project:\n  name: [unclosed\n";

    #[test]
    fn the_manifest_filename_is_parallax_yaml() {
        assert_eq!(MANIFEST_FILENAME, "parallax.yaml");
    }

    #[test]
    fn a_loaded_project_takes_its_name_from_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), "ttui-checkout", GOOD);
        let registry = Registry::from_roots(std::slice::from_ref(&root));

        assert_eq!(registry.projects().len(), 1);
        assert!(registry.failures().is_empty());
        assert_eq!(registry.projects()[0].name, "ttui");
        assert_eq!(
            registry.projects()[0].manifest_path,
            root.join("parallax.yaml")
        );
    }

    /// The manifest declares `<projects-root>/TTUI`, which exists on no
    /// machine. The registry knows where the clone actually is, and its
    /// answer is the one every adapter then scans.
    #[test]
    fn the_registrys_root_overrides_the_manifests_declared_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), "ttui-checkout", GOOD);
        let registry = Registry::from_roots(std::slice::from_ref(&root));

        let loaded = &registry.projects()[0];
        assert_eq!(loaded.root, root);
        assert_eq!(
            loaded.manifest.manifest().project.root.as_deref(),
            Some(root.as_path()),
            "the validated manifest carries the real root, not the placeholder"
        );
    }

    #[test]
    fn a_root_with_no_manifest_is_one_failure_and_loads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("empty");
        std::fs::create_dir_all(&root).unwrap();

        let registry = Registry::from_roots(std::slice::from_ref(&root));
        assert!(registry.projects().is_empty());
        assert_eq!(registry.failures().len(), 1);
        assert_eq!(registry.failures()[0].source, root.join("parallax.yaml"));
    }

    #[test]
    fn a_manifest_that_fails_validation_carries_the_validators_own_message() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), "broken", INVALID);

        let registry = Registry::from_roots(std::slice::from_ref(&root));
        assert_eq!(registry.failures().len(), 1);
        assert!(
            registry.failures()[0].problem.contains("project.name"),
            "got {}",
            registry.failures()[0].problem
        );
    }

    /// The rule aggregation already follows for adapters: one broken
    /// source degrades itself and nothing else.
    #[test]
    fn one_broken_project_does_not_stop_the_others_loading() {
        let dir = tempfile::tempdir().unwrap();
        let a = project(dir.path(), "a", GOOD);
        let bad = project(dir.path(), "bad", UNPARSEABLE);
        let c = project(dir.path(), "c", "project:\n  name: other\n");

        let registry = Registry::from_roots(&[a, bad.clone(), c]);
        assert_eq!(registry.projects().len(), 2);
        assert_eq!(registry.failures().len(), 1);
        assert_eq!(registry.failures()[0].source, bad.join("parallax.yaml"));
    }

    #[test]
    fn projects_stay_in_the_order_they_were_registered() {
        let dir = tempfile::tempdir().unwrap();
        let z = project(dir.path(), "z", "project:\n  name: zebra\n");
        let a = project(dir.path(), "a", "project:\n  name: aardvark\n");

        let registry = Registry::from_roots(&[z, a]);
        let names: Vec<&str> = registry
            .projects()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, vec!["zebra", "aardvark"], "input order, not sorted");
    }

    #[test]
    fn an_empty_registry_says_so() {
        assert!(Registry::default().is_empty());
    }
}
