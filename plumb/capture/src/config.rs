//! Parses and validates `.plumb/config.yaml`: the scenario list that
//! defines what gets captured, what each capture is for, and which
//! source paths make it relevant. Deliberately holds no runtime state.

use serde::Deserialize;
use std::path::Path;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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
}

/// A parsed `.plumb/config.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Every declared scenario, in file order.
    pub scenarios: Vec<Scenario>,
}

/// Failure reading, parsing, or validating a config file.
#[derive(Debug)]
pub enum ConfigError {
    /// Filesystem failure reading the file.
    Io(std::io::Error),
    /// The file is not valid YAML, or not this schema.
    Yaml(serde_yaml::Error),
    /// Two scenarios share a name.
    DuplicateScenario(String),
    /// A scenario has an empty name.
    EmptyName,
    /// A `command` scenario's `args` has no `{out}` placeholder.
    MissingOutPlaceholder(String),
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}
impl From<serde_yaml::Error> for ConfigError {
    fn from(e: serde_yaml::Error) -> Self {
        ConfigError::Yaml(e)
    }
}
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "reading .plumb/config.yaml: {e}"),
            ConfigError::Yaml(e) => write!(f, "parsing .plumb/config.yaml: {e}"),
            ConfigError::DuplicateScenario(n) => write!(f, "duplicate scenario name: {n}"),
            ConfigError::EmptyName => write!(f, "a scenario has an empty name"),
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
    let text = std::fs::read_to_string(path)?;
    let config: Config = serde_yaml::from_str(&text)?;
    let mut seen: Vec<&str> = Vec::new();
    for s in &config.scenarios {
        if s.name.trim().is_empty() {
            return Err(ConfigError::EmptyName);
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
}
