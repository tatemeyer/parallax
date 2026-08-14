//! Chooses which scenarios a change actually warrants reviewing, by
//! matching changed paths against each scenario's `touches` globs.
//! Deliberately never falls back to "review everything" on no match.

use crate::config::Config;
use globset::{Glob, GlobSetBuilder};

/// A scenario chosen for review, with the changed paths that chose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    /// The scenario's name.
    pub name: String,
    /// Changed paths that matched its `touches` globs; empty when the
    /// scenario was named explicitly rather than matched.
    pub matched: Vec<String>,
}

/// The outcome of a selection pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Scenarios to capture, in config order.
    pub selected: Vec<Selected>,
    /// Changed paths no scenario claimed — reported, never ignored.
    pub unmatched: Vec<String>,
}

/// Failure building or applying a selection.
#[derive(Debug)]
pub enum SelectError {
    /// A scenario's `touches` entry is not a valid glob.
    BadGlob {
        /// The scenario that declared it.
        scenario: String,
        /// The offending glob.
        glob: String,
    },
    /// `--scenario` named something the config does not declare.
    UnknownScenario(String),
}

impl std::fmt::Display for SelectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectError::BadGlob { scenario, glob } => {
                write!(f, "scenario {scenario}: invalid touches glob {glob:?}")
            }
            SelectError::UnknownScenario(n) => write!(f, "no scenario named {n:?} in config"),
        }
    }
}
impl std::error::Error for SelectError {}

/// Normalizes a path for glob matching: backslashes to forward slashes,
/// leading `./` stripped. `touches` globs are always written POSIX-style.
fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
}

/// Selects every scenario whose `touches` globs match a changed path.
/// An empty `selected` is a legitimate, reportable result — callers
/// must stop and say so, never widen to all scenarios.
pub fn select_by_paths(config: &Config, changed: &[String]) -> Result<Selection, SelectError> {
    let normalized: Vec<String> = changed.iter().map(|p| normalize(p)).collect();
    let mut selected = Vec::new();
    let mut claimed = vec![false; normalized.len()];

    for scenario in &config.scenarios {
        let mut builder = GlobSetBuilder::new();
        for g in &scenario.touches {
            let glob = Glob::new(g).map_err(|_| SelectError::BadGlob {
                scenario: scenario.name.clone(),
                glob: g.clone(),
            })?;
            builder.add(glob);
        }
        let set = builder.build().map_err(|_| SelectError::BadGlob {
            scenario: scenario.name.clone(),
            glob: scenario.touches.join(", "),
        })?;

        let mut matched = Vec::new();
        for (i, path) in normalized.iter().enumerate() {
            if set.is_match(path) {
                matched.push(path.clone());
                claimed[i] = true;
            }
        }
        if !matched.is_empty() {
            selected.push(Selected {
                name: scenario.name.clone(),
                matched,
            });
        }
    }

    let unmatched = normalized
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !claimed[*i])
        .map(|(_, p)| p)
        .collect();

    Ok(Selection {
        selected,
        unmatched,
    })
}

/// Selects exactly one named scenario, ignoring `touches` — the
/// `--scenario <name>` path for a targeted look while iterating.
pub fn select_by_name(config: &Config, name: &str) -> Result<Selection, SelectError> {
    if !config.scenarios.iter().any(|s| s.name == name) {
        return Err(SelectError::UnknownScenario(name.to_string()));
    }
    Ok(Selection {
        selected: vec![Selected {
            name: name.to_string(),
            matched: Vec::new(),
        }],
        unmatched: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AdapterKind, Config, Scenario};

    fn scn(name: &str, touches: &[&str]) -> Scenario {
        Scenario {
            name: name.into(),
            adapter: AdapterKind::Command,
            args: "x {out}.png".into(),
            touches: touches.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn cfg() -> Config {
        Config {
            scenarios: vec![
                scn("dial", &["src/widgets/dial.rs", "examples/omnitrix/**"]),
                scn("glitch", &["src/glitch.rs"]),
            ],
        }
    }

    #[test]
    fn an_exact_path_selects_its_scenario_only() {
        let s = select_by_paths(&cfg(), &["src/glitch.rs".into()]).unwrap();
        assert_eq!(s.selected.len(), 1);
        assert_eq!(s.selected[0].name, "glitch");
        assert_eq!(s.selected[0].matched, vec!["src/glitch.rs".to_string()]);
    }

    #[test]
    fn a_double_star_glob_matches_nested_paths() {
        let s = select_by_paths(&cfg(), &["examples/omnitrix/faceplate.rs".into()]).unwrap();
        assert_eq!(s.selected.len(), 1);
        assert_eq!(s.selected[0].name, "dial");
    }

    #[test]
    fn no_match_selects_nothing_rather_than_everything() {
        // The whole contract: never silently review everything.
        let s = select_by_paths(&cfg(), &["README.md".into()]).unwrap();
        assert!(s.selected.is_empty());
        assert_eq!(s.unmatched, vec!["README.md".to_string()]);
    }

    #[test]
    fn an_empty_changed_list_selects_nothing() {
        let s = select_by_paths(&cfg(), &[]).unwrap();
        assert!(s.selected.is_empty());
    }

    #[test]
    fn one_path_can_select_several_scenarios() {
        let mut c = cfg();
        c.scenarios.push(scn("both", &["src/glitch.rs"]));
        let s = select_by_paths(&c, &["src/glitch.rs".into()]).unwrap();
        assert_eq!(s.selected.len(), 2);
    }

    #[test]
    fn select_by_name_ignores_touches_entirely() {
        let s = select_by_name(&cfg(), "dial").unwrap();
        assert_eq!(s.selected.len(), 1);
        assert_eq!(s.selected[0].name, "dial");
        assert!(s.selected[0].matched.is_empty());
    }

    #[test]
    fn select_by_name_rejects_an_unknown_scenario() {
        assert!(matches!(
            select_by_name(&cfg(), "nope"),
            Err(SelectError::UnknownScenario(n)) if n == "nope"
        ));
    }

    #[test]
    fn a_malformed_glob_names_its_scenario() {
        let c = Config {
            scenarios: vec![scn("bad", &["src/[unclosed"])],
        };
        assert!(matches!(
            select_by_paths(&c, &["src/a.rs".into()]),
            Err(SelectError::BadGlob { scenario, .. }) if scenario == "bad"
        ));
    }

    #[test]
    fn windows_style_separators_in_changed_paths_still_match() {
        let s = select_by_paths(&cfg(), &["examples\\omnitrix\\boot.rs".into()]).unwrap();
        assert_eq!(s.selected.len(), 1, "backslashes must normalize to /");
    }
}
