//! Discovers and validates every `manifests/*.yaml` file. A single bad
//! or missing manifest is reported, not fatal to the rest -- one
//! misconfigured project must not blank the cockpit for every other
//! one, the same principle Baseline's own aggregation already applies
//! to a single failing adapter. A missing manifests directory is not
//! an error at all: it is the "no configuration" offline path, and
//! yields zero projects rather than a failure.

use parallax_baseline::manifest::parse_manifest_file;
use parallax_baseline::validate::{validate, Validated};
use std::path::{Path, PathBuf};

/// One manifest file that failed to parse or validate.
#[derive(Debug)]
pub struct LoadFailure {
    /// The file that failed.
    pub path: PathBuf,
    /// Why, in one line.
    pub reason: String,
}

/// Parses and validates every `.yaml`/`.yml` file directly under `dir`,
/// sorted by filename for a stable, reproducible project order. A
/// missing `dir` yields no manifests and no failures.
pub fn load_manifests(dir: &Path) -> (Vec<Validated>, Vec<LoadFailure>) {
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                matches!(
                    p.extension().and_then(|ext| ext.to_str()),
                    Some("yaml") | Some("yml")
                )
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    paths.sort();

    let mut validated = Vec::new();
    let mut failures = Vec::new();
    for path in paths {
        match parse_manifest_file(&path) {
            Ok(manifest) => match validate(manifest) {
                Ok(v) => validated.push(v),
                Err(errors) => failures.push(LoadFailure {
                    path,
                    reason: errors
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join("; "),
                }),
            },
            Err(e) => failures.push(LoadFailure {
                path,
                reason: e.to_string(),
            }),
        }
    }
    (validated, failures)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, contents) in files {
            std::fs::write(dir.path().join(name), contents).unwrap();
        }
        dir
    }

    #[test]
    fn a_missing_directory_yields_no_manifests_and_no_failures() {
        let (validated, failures) = load_manifests(Path::new("does/not/exist"));
        assert!(validated.is_empty());
        assert!(failures.is_empty());
    }

    #[test]
    fn valid_manifests_are_parsed_and_validated() {
        let dir = tree(&[
            ("a.yaml", "project:\n  name: a\n"),
            ("b.yaml", "project:\n  name: b\n"),
        ]);
        let (validated, failures) = load_manifests(dir.path());
        assert_eq!(validated.len(), 2);
        assert!(failures.is_empty());
    }

    #[test]
    fn manifests_load_in_filename_order() {
        let dir = tree(&[
            ("z.yaml", "project:\n  name: z\n"),
            ("a.yaml", "project:\n  name: a\n"),
        ]);
        let (validated, _) = load_manifests(dir.path());
        assert_eq!(validated[0].manifest().project.name, "a");
        assert_eq!(validated[1].manifest().project.name, "z");
    }

    #[test]
    fn non_yaml_files_are_ignored() {
        let dir = tree(&[
            ("readme.md", "not a manifest"),
            ("a.yaml", "project:\n  name: a\n"),
        ]);
        let (validated, failures) = load_manifests(dir.path());
        assert_eq!(validated.len(), 1);
        assert!(failures.is_empty());
    }

    #[test]
    fn a_bad_manifest_is_reported_but_does_not_block_the_others() {
        let dir = tree(&[
            ("bad.yaml", "project:\n  name: ''\n"), // fails validation: empty name
            ("good.yaml", "project:\n  name: good\n"),
        ]);
        let (validated, failures) = load_manifests(dir.path());
        assert_eq!(validated.len(), 1);
        assert_eq!(validated[0].manifest().project.name, "good");
        assert_eq!(failures.len(), 1);
        assert!(failures[0].path.ends_with("bad.yaml"));
    }

    #[test]
    fn unparseable_yaml_is_reported_as_a_failure_not_a_panic() {
        let dir = tree(&[("broken.yaml", "not: [valid")]);
        let (validated, failures) = load_manifests(dir.path());
        assert!(validated.is_empty());
        assert_eq!(failures.len(), 1);
    }
}
