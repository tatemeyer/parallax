//! The registry, from a file and from a scan, and the whole path a
//! frontend actually takes: a directory on disk to a `PlatformState`
//! with nothing hand-wired.

use parallax_baseline::adapters::factory::{from_manifest_with, AdapterConfig};
use parallax_baseline::adapters::http::FixtureTransport;
use parallax_baseline::adapters::verification::ScriptedShellRunner;
use parallax_baseline::registry::Registry;
use parallax_baseline::state::{aggregate, ProjectAdapters};
use parallax_baseline::validate::Validated;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn manifest(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/manifests")
        .join(name)
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// A projects directory holding copies of both real manifests, one
/// project whose manifest is broken, and one directory that is not a
/// project at all.
fn projects_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (folder, fixture) in [
        ("TTUI", "ttui.yaml"),
        ("Model-Experiments", "model-experiments.yaml"),
    ] {
        let root = dir.path().join(folder);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::copy(manifest(fixture), root.join("parallax.yaml")).unwrap();
    }
    let broken = dir.path().join("Broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("parallax.yaml"), "project:\n  name: ''\n").unwrap();

    let not_a_project = dir.path().join("notes");
    std::fs::create_dir_all(&not_a_project).unwrap();
    std::fs::write(not_a_project.join("README.md"), "just a folder\n").unwrap();
    dir
}

fn write_registry(dir: &Path, roots: &[PathBuf]) -> PathBuf {
    let mut text = String::from("apiVersion: parallax/v1\nprojects:\n");
    for root in roots {
        text.push_str(&format!(
            "  - root: {}\n",
            root.display().to_string().replace('\\', "/")
        ));
    }
    let path = dir.join("registry.yaml");
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_registry_file_loads_the_roots_it_lists() {
    let tree = projects_tree();
    let file = write_registry(
        tree.path(),
        &[
            tree.path().join("TTUI"),
            tree.path().join("Model-Experiments"),
        ],
    );

    let registry = Registry::from_file(&file).expect("the registry file reads");
    let names: Vec<&str> = registry
        .projects()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, vec!["ttui", "model-experiments"]);
    assert!(registry.failures().is_empty());
}

/// A broken project degrades itself. The registry is not the place a
/// single bad manifest takes the whole platform down.
#[test]
fn a_broken_project_in_the_file_leaves_the_rest_loaded() {
    let tree = projects_tree();
    let file = write_registry(
        tree.path(),
        &[tree.path().join("TTUI"), tree.path().join("Broken")],
    );

    let registry = Registry::from_file(&file).expect("the registry file reads");
    assert_eq!(registry.projects().len(), 1);
    assert_eq!(registry.failures().len(), 1);
    assert_eq!(
        registry.failures()[0].source,
        tree.path().join("Broken").join("parallax.yaml")
    );
}

#[test]
fn a_registry_file_with_an_unknown_key_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.yaml");
    std::fs::write(
        &path,
        "apiVersion: parallax/v1\nprojekts:\n  - root: /tmp/x\n",
    )
    .unwrap();
    assert!(Registry::from_file(&path).is_err());
}

/// A registry file that names a project's display name is refused, not
/// quietly accepted: identity has one source, and it is the manifest.
#[test]
fn a_registry_entry_carrying_anything_but_a_root_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.yaml");
    std::fs::write(&path, "projects:\n  - root: /tmp/x\n    name: pretty\n").unwrap();
    assert!(Registry::from_file(&path).is_err());
}

#[test]
fn a_missing_registry_file_is_an_error_naming_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nope.yaml");
    let err = Registry::from_file(&path).unwrap_err();
    assert_eq!(err.source, path);
}

#[test]
fn a_scan_finds_every_child_with_a_manifest_and_ignores_the_rest() {
    let tree = projects_tree();
    let registry = Registry::scan(tree.path());

    let names: Vec<&str> = registry
        .projects()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    // Sorted by directory name: Broken, Model-Experiments, TTUI — and
    // `notes/` is not a project, so it is neither loaded nor a failure.
    assert_eq!(names, vec!["model-experiments", "ttui"]);
    assert_eq!(registry.failures().len(), 1, "Broken/ is a failure");
}

#[test]
fn a_scan_of_a_directory_that_does_not_exist_is_empty_rather_than_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::scan(&dir.path().join("nowhere"));
    assert!(registry.is_empty());
    assert!(registry.failures().is_empty());
}

/// Every loaded project points at the directory it was found in, not at
/// the `<projects-root>/TTUI` placeholder its manifest declares.
#[test]
fn loaded_projects_point_at_the_tree_they_were_found_in() {
    let tree = projects_tree();
    let registry = Registry::scan(tree.path());
    for project in registry.projects() {
        assert!(
            project.root.starts_with(tree.path()),
            "{} points outside the tree: {}",
            project.name,
            project.root.display()
        );
        assert_eq!(
            project.manifest.manifest().project.root.as_deref(),
            Some(project.root.as_path())
        );
    }
}

/// The whole path, end to end: a directory on disk becomes a
/// `PlatformState`, with no adapter constructed by hand anywhere.
#[test]
fn a_directory_on_disk_aggregates_into_platform_state() {
    let tree = projects_tree();
    let registry = Registry::scan(tree.path());

    let mut inputs: Vec<(Validated, ProjectAdapters)> = registry
        .projects()
        .iter()
        .map(|p| {
            let adapters = from_manifest_with(
                &p.manifest,
                &AdapterConfig::default(),
                FixtureTransport::new,
                ScriptedShellRunner::new,
            );
            (p.manifest.clone(), adapters)
        })
        .collect();

    let platform = aggregate(&mut inputs, at(0));

    assert_eq!(platform.projects.len(), 2);
    assert_eq!(
        platform
            .project("ttui")
            .and_then(|p| p.methodology.as_deref()),
        Some("methodology-first")
    );
    assert_eq!(
        platform
            .project("model-experiments")
            .and_then(|p| p.methodology.as_deref()),
        Some("outcome-first")
    );

    // The work adapters have no fixture responses, so every work source
    // degrades — and says so per project rather than blanking the view.
    for project in &platform.projects {
        assert!(project.work.is_none());
        assert_eq!(project.degradations.len(), 1, "{}", project.name);
        assert_eq!(project.degradations[0].source, "work:github");
    }

    // The broken project is a failure, not a row.
    assert!(platform.project("").is_none());
    assert_eq!(registry.failures().len(), 1);
}
