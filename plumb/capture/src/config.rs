//! Parses and validates `.plumb/config.yaml`: the scenario list that
//! defines what gets captured, what each capture is for, and which
//! source paths make it relevant. Deliberately holds no runtime state.

use crate::glyph::GlyphMode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Which capture adapter runs a scenario.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdapterKind {
    /// Spawn a command under a pseudo-console and rasterize its output.
    Pty,
    /// Capture a native OS window by title. Deferred — no v1 consumer.
    Window,
    /// Run any shell command that writes images to a declared path.
    #[default]
    Command,
}

/// A distortion a scenario declares as intentional, exempting it from
/// the breakage lens. Unknown values are a parse error by design.
///
/// `Serialize` is derived (alongside `Deserialize`) so this can appear
/// in `RunManifest`, which the manifest module writes out as JSON for
/// a lens agent to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Expectation {
    /// Glyph garbling and region displacement are this scenario's point.
    VisualCorruption,
}

/// One scenario: how to capture it, what it is for, and what it touches.
///
/// `Default` is derived deliberately: Arc 5 adds three `pty`-only
/// fields (`size`, `script`, `on_unmapped_glyph`), and every test
/// helper in this crate builds a `Scenario` with `..Default::default()`
/// so that addition needs no edits to earlier tasks' tests.
/// `on_unmapped_glyph` landed in Task 20; `size`/`script` remain
/// pending.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Scenario {
    /// Unique name; also the captured image's filename stem.
    pub name: String,
    /// Which adapter runs it.
    pub adapter: AdapterKind,
    /// Adapter arguments; `{out}` is substituted with the run's stem.
    pub args: String,
    /// What the capture is supposed to show — the intent lens's input.
    #[serde(default)]
    pub intent: Option<String>,
    /// Distortion declared intentional; the breakage lens's exemptions.
    #[serde(default)]
    pub expects: Vec<Expectation>,
    /// Scenario-scoped addition to `taste.md`; design lens only.
    #[serde(default)]
    pub taste_override: Option<String>,
    /// Globs whose modification makes this scenario worth reviewing.
    #[serde(default)]
    pub touches: Vec<String>,
    /// How this scenario's `pty` capture reacts to an unmapped
    /// codepoint: hard-error (the default, `GlyphMode::Error`) or
    /// substitute a placeholder and disclose it as a manifest caveat
    /// (`GlyphMode::Substitute`). A per-scenario field rather than only
    /// a CLI flag, since only some scenarios hit unmapped glyphs — a
    /// scenario that never draws one never needs to opt in. No effect
    /// on a `command`/`window` scenario; meaningful for `pty` only.
    #[serde(default)]
    pub on_unmapped_glyph: GlyphMode,
}

/// A parsed `.plumb/config.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Every declared scenario, in file order.
    pub scenarios: Vec<Scenario>,
}

/// An I/O failure reading a config file, together with the path that
/// caused it — kept as a single field so `ConfigError::Io(_)` stays an
/// opaque, one-wildcard match for callers that only care *that* it
/// failed, not why.
#[derive(Debug)]
pub struct IoFailure {
    /// The path `load_config` was asked to read.
    pub path: PathBuf,
    /// The underlying I/O error.
    pub source: std::io::Error,
}

/// A YAML parse failure, together with the path that caused it — kept
/// as a single field for the same reason as [`IoFailure`]: it keeps
/// `ConfigError::Yaml(_)` an opaque match, which is what lets
/// `serde_yaml` stay swappable behind this module alone.
#[derive(Debug)]
pub struct YamlFailure {
    /// The path `load_config` was asked to read.
    pub path: PathBuf,
    /// The underlying YAML error.
    pub source: serde_yaml::Error,
}

/// Failure reading, parsing, or validating a config file.
#[derive(Debug)]
pub enum ConfigError {
    /// Filesystem failure reading the file, and the path that caused it.
    Io(IoFailure),
    /// The file is not valid YAML, or not this schema; and the path
    /// that caused it.
    Yaml(YamlFailure),
    /// Two scenarios share a name.
    DuplicateScenario(String),
    /// A scenario has an empty name; carries its zero-based index in
    /// the file, since the name itself can't identify it.
    EmptyName(usize),
    /// A `command` scenario's `args` has no `{out}` placeholder.
    MissingOutPlaceholder(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "reading {}: {}", e.path.display(), e.source),
            ConfigError::Yaml(e) => write!(f, "parsing {}: {}", e.path.display(), e.source),
            ConfigError::DuplicateScenario(n) => write!(f, "duplicate scenario name: {n}"),
            ConfigError::EmptyName(i) => write!(f, "scenario at index {i} has an empty name"),
            ConfigError::MissingOutPlaceholder(n) => write!(
                f,
                "scenario {n}: `command` adapter args must contain the {{out}} placeholder"
            ),
        }
    }
}
impl std::error::Error for ConfigError {}

/// Reads, parses, and validates a `.plumb/config.yaml`.
pub fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| {
        ConfigError::Io(IoFailure {
            path: path.to_path_buf(),
            source,
        })
    })?;
    let config: Config = serde_yaml::from_str(&text).map_err(|source| {
        ConfigError::Yaml(YamlFailure {
            path: path.to_path_buf(),
            source,
        })
    })?;
    let mut seen: Vec<&str> = Vec::new();
    for (i, s) in config.scenarios.iter().enumerate() {
        if s.name.trim().is_empty() {
            return Err(ConfigError::EmptyName(i));
        }
        if seen.contains(&s.name.as_str()) {
            return Err(ConfigError::DuplicateScenario(s.name.clone()));
        }
        seen.push(&s.name);
        if s.adapter == AdapterKind::Command && !s.args.contains("{out}") {
            return Err(ConfigError::MissingOutPlaceholder(s.name.clone()));
        }
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(yaml: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, yaml).unwrap();
        (dir, path)
    }

    #[test]
    fn parses_the_spec_example_scenario() {
        let (_d, p) = write(
            r#"
scenarios:
  - name: omnitrix-dial-rotate
    adapter: command
    args: >
      cargo run -p visual-snapshot -- --example omnitrix
      --size 120x40 --script .plumb/scripts/dial-rotate.json --out {out}.gif
    intent: >
      The dial rotates through four alien modes.
    expects: []
    touches:
      - src/widgets/dial.rs
      - examples/omnitrix/**
"#,
        );
        let cfg = load_config(&p).unwrap();
        let s = &cfg.scenarios[0];
        assert_eq!(s.name, "omnitrix-dial-rotate");
        assert_eq!(s.adapter, AdapterKind::Command);
        assert!(s.args.contains("{out}.gif"));
        assert!(s.intent.as_deref().unwrap().contains("four alien modes"));
        assert_eq!(s.expects, Vec::new());
        assert_eq!(s.touches.len(), 2);
        assert!(s.taste_override.is_none());
    }

    #[test]
    fn intent_expects_and_taste_override_are_all_optional() {
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: command\n    args: 'x {out}.png'\n    touches: ['src/**']\n",
        );
        let s = &load_config(&p).unwrap().scenarios[0];
        assert!(s.intent.is_none());
        assert!(s.taste_override.is_none());
        assert_eq!(
            s.expects,
            Vec::new(),
            "an undeclared scenario expects nothing"
        );
    }

    #[test]
    fn declared_visual_corruption_parses() {
        let (_d, p) = write(
            "scenarios:\n  - name: falcon-glitch-burst\n    adapter: command\n    args: 'x {out}.gif'\n    expects: [visual-corruption]\n    touches: ['src/glitch.rs']\n",
        );
        let s = &load_config(&p).unwrap().scenarios[0];
        assert_eq!(s.expects, vec![Expectation::VisualCorruption]);
    }

    #[test]
    fn taste_override_parses_when_present() {
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: command\n    args: 'x {out}.png'\n    taste_override: 'Falcon is the scruffiest machine in the set.'\n    touches: ['src/**']\n",
        );
        let s = &load_config(&p).unwrap().scenarios[0];
        assert!(s.taste_override.as_deref().unwrap().contains("scruffiest"));
    }

    #[test]
    fn an_unknown_expectation_is_a_parse_error() {
        // The burden is on the scenario to claim a *known* exemption;
        // a typo must never silently degrade to "expects nothing".
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: command\n    args: 'x {out}.png'\n    expects: [visual-corrupton]\n    touches: ['src/**']\n",
        );
        assert!(matches!(load_config(&p), Err(ConfigError::Yaml(_))));
    }

    #[test]
    fn duplicate_scenario_names_are_rejected() {
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: command\n    args: 'x {out}.png'\n    touches: ['src/**']\n  - name: a\n    adapter: command\n    args: 'y {out}.png'\n    touches: ['src/**']\n",
        );
        assert!(matches!(load_config(&p), Err(ConfigError::DuplicateScenario(n)) if n == "a"));
    }

    #[test]
    fn a_command_scenario_without_an_out_placeholder_is_rejected() {
        // Without {out} the adapter has no idea where images land.
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: command\n    args: 'cargo run -p thing --out fixed.png'\n    touches: ['src/**']\n",
        );
        assert!(matches!(load_config(&p), Err(ConfigError::MissingOutPlaceholder(n)) if n == "a"));
    }

    #[test]
    fn on_unmapped_glyph_defaults_to_error_when_omitted() {
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: pty\n    args: 'x'\n    touches: ['src/**']\n",
        );
        let s = &load_config(&p).unwrap().scenarios[0];
        assert_eq!(s.on_unmapped_glyph, GlyphMode::Error);
    }

    #[test]
    fn on_unmapped_glyph_substitute_parses() {
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: pty\n    args: 'x'\n    on_unmapped_glyph: substitute\n    touches: ['src/**']\n",
        );
        let s = &load_config(&p).unwrap().scenarios[0];
        assert_eq!(s.on_unmapped_glyph, GlyphMode::Substitute);
    }

    #[test]
    fn window_adapter_parses_even_though_it_is_deferred() {
        // Deferral lives in the adapter, not the schema — the contract
        // admits it so a later implementation needs no schema change.
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: window\n    args: 'Some Window Title'\n    touches: ['src/**']\n",
        );
        assert_eq!(
            load_config(&p).unwrap().scenarios[0].adapter,
            AdapterKind::Window
        );
    }

    #[test]
    fn an_empty_scenario_name_is_rejected_with_its_index() {
        // The second scenario (index 1) is the offender; the message
        // must be able to say *which* one since the name itself, by
        // definition, can't be used to identify it.
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: command\n    args: 'x {out}.png'\n    touches: ['src/**']\n  - name: ''\n    adapter: command\n    args: 'y {out}.png'\n    touches: ['src/**']\n",
        );
        assert!(matches!(load_config(&p), Err(ConfigError::EmptyName(1))));
    }

    #[test]
    fn io_error_message_names_the_actual_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.yaml");
        let err = load_config(&missing).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(&missing.display().to_string()),
            "message did not name the real path: {msg}"
        );
    }

    #[test]
    fn yaml_error_message_names_the_actual_path() {
        let (_d, p) = write(
            "scenarios:\n  - name: a\n    adapter: command\n    args: 'x {out}.png'\n    expects: [visual-corrupton]\n    touches: ['src/**']\n",
        );
        let err = load_config(&p).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(&p.display().to_string()),
            "message did not name the real path: {msg}"
        );
    }
}
