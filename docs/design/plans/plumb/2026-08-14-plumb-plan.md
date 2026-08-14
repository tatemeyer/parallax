# Plumb Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Structure note:** This plan is organized as **Arcs → Slices → Tasks**
> per `docs/design/README.md`, not the flat "Task N" list the
> `writing-plans` skill defaults to. Tasks still follow the skill's
> bite-sized TDD step structure; Arc/Slice headings are pure grouping.

**Goal:** Build **Plumb** — sub-project #1 of the **Parallax** platform,
a git-installable Claude Code plugin that gives the visual-review step
eyes and opinions: portable capture adapters plus an adversarial
multi-lens reviewer that never sees the code it is judging, rendering a
**GO / NO-GO / HOLD** verdict against a per-project taste profile and a
declared per-scenario intent.

**Architecture:** A Rust CLI (`plumb`, package `parallax-plumb`, living
in the plugin's `capture/` directory) owns everything deterministic —
config parsing, scenario selection from `touches` globs, running capture
adapters, writing a run manifest, **building the blinded lens prompts**,
merging and deduping findings, applying ruling suppression, and
rendering `verdict.md`. A Claude Code skill
(`skills/visual-review/SKILL.md`) owns the one thing only the harness
can do — dispatching the lens subagents in parallel and feeding their
JSON back to the CLI. Putting prompt *construction* in the CLI rather
than in skill prose is what makes the blinding property a unit test
instead of a hope.

**Tech Stack:** Rust (stable, 2021 edition), `clap` (CLI), `serde` +
`serde_yaml` (config) + `serde_json` (findings, rulings, manifests),
`globset` (`touches` matching), `sha2` (taste-profile hashing, finding
fingerprints), `chrono` (run timestamps), and — from Arc 5 onward, all
already proven in `tools/visual-snapshot` — `portable-pty`, `vt100`,
`font8x8`, `image`.

---

## Global Constraints

Copied verbatim from
`docs/design/specs/plumb/2026-08-14-plumb-design.md` and
`docs/design/specs/parallax/2026-08-14-parallax-platform-design.md`.
Every task's requirements implicitly include this section.

**Where the work happens.** Arcs 1-5 and 7 execute in a **new
repository**, `plumb`, created in Arc 1 Slice 1.1 as a sibling of
`D:/Dev/Projects/TTUI`. Arc 6 (and the single seed-scenario task at the
end of Arc 2) executes **inside the TTUI repo**, on a TTUI worktree
branch, and lands through TTUI's normal Gated PR flow. No task in this
plan modifies `tools/visual-snapshot`.

**Naming (from Parallax's Naming table — use these exact words).**
Platform: **Parallax**. This sub-project: **Plumb**, crate
`parallax-plumb`. Reviewer persona: **Sim Sup**. Verdict states:
**GO / NO-GO / HOLD**. Never "pass/fail", never "approved/rejected".

**Blinding — the single most important property in this system.** Lens
agents receive the image, the run manifest, the taste profile, and —
for the intent lens only — the scenario's `intent` string. They do
**not** receive the diff, the source, the scenario's `args`, the
scenario's `touches` globs, or the fact that anything changed. No task
may add a parameter, manifest field, or prompt section that carries any
of those. Lens agent definitions declare `tools: Read` and nothing else,
so a lens physically cannot `Grep` its way to the source.

**Third-party framing.** Every agent prompt states it is reviewing
someone else's work submitted for critique. Never "verify my change",
"confirm this looks right", or any second-person-authorship phrasing.

**No quota.** An empty findings list is a legitimate, expected outcome,
stated explicitly in every agent prompt.

**Confidence governs voice.** High-confidence findings assert.
Low-confidence design findings must be phrased as questions.
Prompt-enforced only.

**`region` is mandatory and load-bearing.** A finding that cannot name
where on screen it lives is **dropped by the orchestrator** — dropped,
counted, and reported as dropped, never silently kept.

**Lens applicability is checked, never assumed:**

| Lens | Applies when | Max severity |
|---|---|---|
| `breakage` | always | **blocker** |
| `intent` | scenario declares `intent` | **blocker** |
| `design` | `taste.md` exists | major |
| `motion` | capture is multi-frame | major |

A design lens with no taste profile is **skipped with a notice**, not
run generically.

**Gate semantics.** **GO** — no findings, or advisory findings only.
**NO-GO** — at least one unresolved `blocker` from a blocker-capable
lens. **HOLD** — the lens could not reach a verdict (capture failed, or
the agent returned unparseable output twice). **A HOLD is never
upgraded to a GO.** Aggregate precedence: any NO-GO → NO-GO; else any
HOLD → HOLD; else GO. Capture failure is never a GO.

**Intentional distortion is resolved by declaration, never by
inspection.** A scenario may declare `expects: [visual-corruption]`.
The breakage lens receives that list and does not raise findings for
declared distortion. **The burden is on the scenario to claim the
exemption, never on the lens to guess at one** — a scenario declaring
nothing gets default treatment and garbled output is a defect. Two
bounds, both prompt-enforced and both covered by tests: the exemption
**suppresses a category, not a region** (`visual-corruption` excuses
garbling; it does not excuse a panel that failed to draw), and
**declared distortion is still bound by legibility** (a glitch that
momentarily disturbs a reading is the feature; one that permanently
destroys it is a defect, and the lens still reports it).

**`taste_override:`** is an optional per-scenario string, additive to
`taste.md` and scoped to that scenario, delivered to the **design lens
only**. TTUI authors none yet, by design — they are added when the
design lens actually misfires on a specific app, not in advance of
evidence.

**Rulings are a post-hoc suppression filter and are never fed to the
agents.** No task may add a ruling, a "the user likes X" note, or any
prior-run context to a prompt. Suppressed findings still appear in the
verdict as a collapsed `previously overruled (N)` line.

**Concurrency is capped (default 8).** If selection would exceed the
cap, the orchestrator batches and **reports what it deferred**.

**Selection contract.** No `touches` matches and no explicit scenario
named → say so and stop. Never silently review everything, never
silently review nothing.

**Capture adapters:** `pty`, `window`, `command`.
- `command` ships first (Arc 1) and is what TTUI adopts — TTUI keeps
  `tools/visual-snapshot` verbatim, unmodified.
- `pty` is an **extraction, not an invention** (Arc 5), from
  `tools/visual-snapshot`'s already-proven `portable-pty` + `vt100` +
  `font8x8` + `image` stack, with `--example <name>` replaced by an
  arbitrary `command`.
- **`window` is explicitly deferred and out of scope for this plan.**
  It has no consumer: TTUI is a TUI, Model-Experiments is Python/CLI,
  neither is a desktop app. It is planned only as far as the adapter
  contract admitting it — `adapter: window` parses, validates, and
  fails with a typed, actionable "no v1 implementation" error (Arc 1
  Slice 1.4). Do not implement window capture under this plan.

**`--on-unmapped-glyph {error,substitute}`** (Arc 5, `pty` adapter
only). `error` remains the default, preserving `visual-snapshot`'s
existing behavior. `substitute` renders a visible placeholder box,
records every substitution in the run manifest, and the lens agents
receive that manifest as a **disclosed caveat**: these cells are
placeholders, do not judge them.

**Failure modes, all of which must be named in the verdict rather than
degrading to silent success:** subagent malformed output → one retry,
then that lens reports `HOLD`; no `.plumb/` directory → offer to
scaffold from `templates/`, do not error; capture binary not built →
build and cache it, and a missing Rust toolchain is a clear actionable
message, not a stack trace.

**Non-goals — no task may drift into these:** golden-image diffing or
baseline regression; a web/browser adapter (`claude-in-chrome` covers
it); macOS/Linux window capture; CI integration (the gate is
harness-level and human-overridable, not a required status check);
prebuilt release binaries (v1 builds from bundled source on first use
and caches); replacing `tools/visual-snapshot`.

**Repo conventions (both repos).** Conventional Commits
(`type(scope): description`, imperative subject, body required on any
non-obvious `feat`/`fix`), one commit per task. TDD is mandatory for
every `coding`-tagged task except the four named exceptions in
`.claude/rules/development-conventions.md` — each such task below says
so explicitly and why. Soft ceiling 500 lines per file. Every `pub`
item gets a one-line `///`; every module gets a `//!` header;
`#![warn(missing_docs)]` in `capture/src/lib.rs`. Every task's commit
must pass `cargo build`, `cargo test`, `cargo fmt --check`, and
`cargo clippy --all-targets -- -D warnings` locally first.

---

## File Structure

The plugin repository:

```
plumb/
  .claude-plugin/plugin.json      — plugin manifest
  README.md
  commands/review.md              — /plumb:review
  skills/visual-review/SKILL.md   — the orchestrator
  agents/
    critic-breakage.md            — blocker-capable
    critic-intent.md              — blocker-capable
    critic-design.md              — advisory
    critic-motion.md              — advisory
  templates/
    taste.md                      — scaffolded into a consumer's .plumb/
    config.example.yaml
  capture/                        — package `parallax-plumb`, binary `plumb`
    Cargo.toml
    src/
      lib.rs        — module re-exports, #![warn(missing_docs)]
      main.rs       — clap CLI: init/select/capture/plan/merge/rule
      config.rs     — Config/Scenario/Adapter, YAML parse + validation
      select.rs     — touches-glob matching, the never-all/never-none contract
      manifest.rs   — RunManifest: what a lens agent is allowed to know
      adapter/
        mod.rs      — the capture contract + typed CaptureError
        command.rs  — the `command` adapter (Arc 1)
        pty.rs      — the `pty` adapter (Arc 5)
        window.rs   — deferred: parses, validates, refuses (Arc 1)
      finding.rs    — Finding/Severity/Confidence/Lens, parse + region drop
      prompt.rs     — blinded prompt construction + lens applicability
      merge.rs      — dedupe, normalize, fingerprint, severity tiering
      rulings.rs    — ruling records, suppression filter, staleness
      verdict.rs    — aggregate GO/NO-GO/HOLD, render verdict.md
      script.rs     — (Arc 5) ported from visual-snapshot
      keys.rs       — (Arc 5) ported from visual-snapshot
      color.rs      — (Arc 5) ported from visual-snapshot
      glyph.rs      — (Arc 5) ported, plus the substitute path
      render.rs     — (Arc 5) ported from visual-snapshot
      encode.rs     — (Arc 5) ported from visual-snapshot
    examples/
      echo_key.rs   — (Arc 5) PTY test fixture, ported
    tests/
      command_adapter.rs
      pty_roundtrip.rs            — (Arc 5)
      corpus.rs                   — (Arc 7) threshold suite
    corpus/                       — (Arc 7) fixture images + ground truth
```

In TTUI, on adoption (Arc 6):

```
.plumb/
  config.yaml
  taste.md                        — authored by the human, separately
  scripts/*.json                  — the scenario key/wait scripts
  runs/                           — gitignored
.claude/rules/development-conventions.md   — additive note
```

---

## Milestones

- **End of Arc 1** — `plumb capture` runs a TTUI scenario through
  `tools/visual-snapshot` via the `command` adapter and writes an image
  plus a run manifest. No judgment yet.
- **End of Arc 2** — **first working version.** `/plumb:review` selects
  scenarios from a diff, captures, fans out the blinded `breakage` and
  `intent` lenses, merges, and writes a `verdict.md` carrying a real
  GO / NO-GO / HOLD. `design` and `motion` resolve as *skipped with
  notice*. Nothing here waits on `taste.md`.
- **End of Arc 3** — the two advisory lenses land. `design` activates
  the moment `taste.md` is present, with no further code change.
  TTUI's `.plumb/taste.md` already exists (framework-level profile:
  two exemptions — constant motion, saturation/glow; four
  non-negotiables — legibility survives effects, cell-grid discipline,
  colour carries state, reads as a machine; density and ornament
  explicitly left open to critique), so the design lens is
  implementable here rather than speculative. The **sequencing still
  does not depend on it**: Arcs 1-2 deliver breakage and intent value
  with `design` resolving as *skipped with notice* if the file is
  absent for any consumer.
- **End of Arc 4** — rulings and the calcification guard.
- **End of Arc 5** — the `pty` adapter, making Plumb useful to projects
  that do not already own a capture tool.
- **End of Arc 6** — TTUI's scenario library, built incrementally.
- **End of Arc 7** — the reviewer regression corpus.

---

## Arc 1: Plugin skeleton and the capture contract

Ends with `plumb capture` producing a real image and a run manifest for
a TTUI scenario, through `tools/visual-snapshot`, with that tool
unmodified.

### Slice 1.1: Repository and plugin scaffolding

**Tags:** git-adjacent, admin

#### Task 1: Create the `plumb` repository and the plugin manifest

**Files:**
- Create: `plumb/.claude-plugin/plugin.json`
- Create: `plumb/README.md`
- Create: `plumb/.gitignore`
- Create: `plumb/capture/Cargo.toml`
- Create: `plumb/capture/src/lib.rs`
- Create: `plumb/capture/src/main.rs`
- Create: `plumb/.github/workflows/ci.yml`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: an installable, empty Claude Code plugin and a buildable
  `parallax-plumb` crate whose binary is named `plumb`, which every
  later task adds modules to.

**TDD exception: pure scaffolding/config, no application logic** — one
of the four named exceptions in
`.claude/rules/development-conventions.md`. Verified by building, not
by asserting.

- [ ] **Step 1: Create the repository**

```bash
mkdir -p /d/Dev/Projects/plumb && cd /d/Dev/Projects/plumb
git init -b main
```

- [ ] **Step 2: Write `.claude-plugin/plugin.json`**

```json
{
  "name": "plumb",
  "version": "0.1.0",
  "description": "Perceptual verification for terminal and image output: portable capture adapters plus an adversarial, blinded multi-lens reviewer that renders a GO / NO-GO / HOLD verdict. Sub-project #1 of the Parallax platform.",
  "author": { "name": "Tate Meyer" }
}
```

`commands/`, `skills/`, and `agents/` are discovered by convention from
the plugin root — they need no entry here.

- [ ] **Step 3: Write `capture/Cargo.toml`**

```toml
[package]
name = "parallax-plumb"
version = "0.1.0"
edition = "2021"
description = "Capture adapters, blinded prompt construction, and verdict merging for the Plumb visual reviewer."

[[bin]]
name = "plumb"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
globset = "0.4"
sha2 = "0.10"
chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }
image = "0.25"

[dev-dependencies]
tempfile = "3"
```

`serde_yaml = "0.9"` is archived upstream but functional and by far the
most widely used YAML serde binding; it is isolated behind `config.rs`
and swappable in one file if that ever matters. `image` is needed from
Arc 1 onward to count frames in a GIF the `command` adapter produced —
not only from Arc 5.

- [ ] **Step 4: Write `capture/src/lib.rs`**

```rust
//! Plumb's deterministic half: config parsing, scenario selection,
//! capture adapters, blinded prompt construction, finding merge,
//! ruling suppression, and verdict rendering. Deliberately owns
//! everything that can be unit-tested; subagent dispatch belongs to
//! the orchestrating skill, not here.
#![warn(missing_docs)]
```

(Module declarations are added by each later task as its module lands.)

- [ ] **Step 5: Write a placeholder `capture/src/main.rs`**

```rust
//! CLI entry point for `plumb`. Subcommands are added by later tasks.

fn main() {
    println!("plumb: not yet implemented");
}
```

- [ ] **Step 6: Write `.gitignore`**

```
/capture/target
.plumb/runs/
```

- [ ] **Step 7: Write `README.md`**

A short file naming: what Plumb is (one paragraph from the spec's
Overview), that it is sub-project #1 of Parallax, the three adapters
with `window` marked **deferred — no consumer yet**, and a pointer to
`docs/design/specs/plumb/2026-08-14-plumb-design.md` in the TTUI repo
as the design of record until it moves here.

- [ ] **Step 8: Write `.github/workflows/ci.yml`**

Four jobs mirroring TTUI's, scoped to `capture/`: `cargo build`,
`cargo test`, `cargo fmt --check`, `cargo clippy --all-targets -- -D
warnings`, on `windows-latest` and `ubuntu-latest`.

- [ ] **Step 9: Verify it builds**

Run: `cargo build --manifest-path capture/Cargo.toml`
Expected: compiles clean; `cargo run --manifest-path capture/Cargo.toml`
prints the placeholder line.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "chore(plumb): scaffold the plugin repository and capture crate"
```

---

### Slice 1.2: Config schema, `expects`, and validation

**Tags:** coding

#### Task 2: Parse and validate `.plumb/config.yaml`

**Files:**
- Create: `capture/src/config.rs`
- Modify: `capture/src/lib.rs` (add `pub mod config;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct Config { pub scenarios: Vec<Scenario> }`
  - `pub struct Scenario { pub name: String, pub adapter: AdapterKind, pub args: String, pub intent: Option<String>, pub expects: Vec<Expectation>, pub taste_override: Option<String>, pub touches: Vec<String> }`
  - `pub enum AdapterKind { Pty, Window, Command }`
  - `pub enum Expectation { VisualCorruption }`
  - `pub enum ConfigError { Io(std::io::Error), Yaml(serde_yaml::Error), DuplicateScenario(String), EmptyName, MissingOutPlaceholder(String) }`
  - `pub fn load_config(path: &Path) -> Result<Config, ConfigError>`

  Consumed by `select.rs` (Task 3), `adapter::command` (Task 5), and
  `prompt.rs` (Task 11).

- [ ] **Step 1: Write the failing tests**

```rust
// capture/src/config.rs
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
        assert_eq!(s.expects, Vec::new(), "an undeclared scenario expects nothing");
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
        assert_eq!(load_config(&p).unwrap().scenarios[0].adapter, AdapterKind::Window);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path capture/Cargo.toml config::`
Expected: FAIL — `config` module does not exist.

- [ ] **Step 3: Write the implementation**

```rust
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
```

Add `pub mod config;` to `capture/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml config::`
Expected: PASS, 8 tests.

- [ ] **Step 5: Commit**

```bash
git add capture/src/config.rs capture/src/lib.rs
git commit -m "feat(config): parse and validate .plumb/config.yaml

An unknown \`expects\` value is a hard parse error rather than a silent
empty list, so a typo can never quietly grant a breakage-lens exemption
the scenario did not actually claim."
```

---

### Slice 1.3: Scenario selection

**Tags:** coding

#### Task 3: Match changed paths against `touches` globs

**Files:**
- Create: `capture/src/select.rs`
- Modify: `capture/src/lib.rs` (add `pub mod select;`)

**Interfaces:**
- Consumes: `config::{Config, Scenario}` (Task 2).
- Produces:
  - `pub struct Selected { pub name: String, pub matched: Vec<String> }`
  - `pub struct Selection { pub selected: Vec<Selected>, pub unmatched: Vec<String> }`
  - `pub fn select_by_paths(config: &Config, changed: &[String]) -> Result<Selection, SelectError>`
  - `pub fn select_by_name(config: &Config, name: &str) -> Result<Selection, SelectError>`
  - `pub enum SelectError { BadGlob { scenario: String, glob: String }, UnknownScenario(String) }`

  Consumed by `main.rs`'s `select` subcommand (Task 6).

- [ ] **Step 1: Write the failing tests**

```rust
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
        let c = Config { scenarios: vec![scn("bad", &["src/[unclosed"])] };
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path capture/Cargo.toml select::`
Expected: FAIL — `select` module does not exist.

- [ ] **Step 3: Write the implementation**

```rust
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

    Ok(Selection { selected, unmatched })
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
```

Add `pub mod select;` to `capture/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml select::`
Expected: PASS, 9 tests.

- [ ] **Step 5: Commit**

```bash
git add capture/src/select.rs capture/src/lib.rs
git commit -m "feat(select): match changed paths against scenario touches globs

Selecting nothing is a reportable outcome, not a cue to widen to every
scenario — a review that quietly covered everything is as untrustworthy
as one that quietly covered nothing."
```

---

### Slice 1.4: The run manifest — what a lens is allowed to know

**Tags:** coding

#### Task 4: The run manifest

**Files:**
- Create: `capture/src/manifest.rs`
- Modify: `capture/src/lib.rs` (add `pub mod manifest;`)

**Interfaces:**
- Consumes: `config::Expectation` (Task 2).
- Produces:
  - `pub struct RunManifest { pub run_id: String, pub scenario: String, pub adapter: String, pub image: PathBuf, pub frame_count: usize, pub size: Option<String>, pub intent: Option<String>, pub expects: Vec<Expectation>, pub caveats: Vec<Caveat> }`
  - `pub enum Caveat { UnmappedGlyphSubstituted { codepoint: String, count: usize } }`
  - `pub fn write_manifest(m: &RunManifest, dir: &Path) -> std::io::Result<PathBuf>`
  - `pub fn read_manifest(path: &Path) -> Result<RunManifest, ManifestError>`
  - `pub fn new_run_id() -> String`

  Consumed by `adapter::*` (Task 5), `prompt.rs` (Task 11), and
  `verdict.rs` (Task 14).

**This struct is the blinding boundary.** It is the *only* per-run data
a lens agent ever sees. It therefore deliberately carries **no `args`
field and no `touches` field** — the adapter's command line names
`--example omnitrix` and source paths, and `touches` is a list of source
files. Neither may ever reach a prompt. Do not add them here for
convenience; a later task will assert their absence.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Expectation;

    fn sample() -> RunManifest {
        RunManifest {
            run_id: "20260814T101500Z".into(),
            scenario: "omnitrix-dial-rotate".into(),
            adapter: "command".into(),
            image: std::path::PathBuf::from("omnitrix-dial-rotate.gif"),
            frame_count: 5,
            size: Some("120x40".into()),
            intent: Some("The dial rotates through four alien modes.".into()),
            expects: vec![Expectation::VisualCorruption],
            caveats: vec![Caveat::UnmappedGlyphSubstituted {
                codepoint: "U+2726".into(),
                count: 3,
            }],
        }
    }

    #[test]
    fn round_trips_through_json_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(&sample(), dir.path()).unwrap();
        let back = read_manifest(&path).unwrap();
        assert_eq!(back.scenario, "omnitrix-dial-rotate");
        assert_eq!(back.frame_count, 5);
        assert_eq!(back.expects, vec![Expectation::VisualCorruption]);
        assert_eq!(back.caveats.len(), 1);
    }

    #[test]
    fn manifest_lands_beside_the_images_as_manifest_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_manifest(&sample(), dir.path()).unwrap();
        assert_eq!(path.file_name().unwrap(), "omnitrix-dial-rotate.manifest.json");
    }

    #[test]
    fn a_run_id_is_a_sortable_utc_timestamp() {
        let id = new_run_id();
        assert_eq!(id.len(), 16, "YYYYMMDDTHHMMSSZ");
        assert!(id.ends_with('Z') && id.contains('T'));
    }

    /// Guards the blinding boundary: the serialized manifest must not
    /// contain the adapter's command line or any source path. This is
    /// asserted on the serialized form, not just the struct, because
    /// the serialized form is what a lens agent literally reads.
    #[test]
    fn serialized_manifest_carries_no_command_line_and_no_source_paths() {
        let json = serde_json::to_string(&sample()).unwrap();
        for forbidden in ["cargo", "--example", "src/", "examples/", "touches", "args"] {
            assert!(
                !json.contains(forbidden),
                "manifest leaked {forbidden:?} to the reviewer: {json}"
            );
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path capture/Cargo.toml manifest::`
Expected: FAIL — `manifest` module does not exist.

- [ ] **Step 3: Write the implementation**

```rust
//! The run manifest: everything a lens agent is permitted to know about
//! a capture, and nothing else. Deliberately carries no command line,
//! no source paths, and no statement that anything changed — see the
//! blinding constraint in the Plumb design.

use crate::config::Expectation;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A limitation of the capture, disclosed to the lens agents so they
/// do not report it as a defect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Caveat {
    /// Cells rendering this codepoint show a placeholder box instead.
    UnmappedGlyphSubstituted {
        /// The codepoint, as `U+XXXX`.
        codepoint: String,
        /// How many cells were substituted.
        count: usize,
    },
}

/// One captured scenario, as described to the reviewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunManifest {
    /// The run's timestamp id, shared by every scenario in the run.
    pub run_id: String,
    /// The scenario's name.
    pub scenario: String,
    /// Which adapter produced the image (`pty`/`window`/`command`).
    pub adapter: String,
    /// The captured image, relative to the run directory.
    pub image: PathBuf,
    /// 1 for a PNG, 2+ for an animated GIF.
    pub frame_count: usize,
    /// Terminal size as `COLSxROWS`, when the adapter knows it.
    pub size: Option<String>,
    /// The scenario's declared intent, for the intent lens.
    pub intent: Option<String>,
    /// Distortion this scenario declares intentional.
    pub expects: Vec<Expectation>,
    /// Disclosed limitations of this capture.
    pub caveats: Vec<Caveat>,
}

/// Failure reading or parsing a manifest.
#[derive(Debug)]
pub enum ManifestError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// Not valid JSON, or not this schema.
    Json(serde_json::Error),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Io(e) => write!(f, "reading run manifest: {e}"),
            ManifestError::Json(e) => write!(f, "parsing run manifest: {e}"),
        }
    }
}
impl std::error::Error for ManifestError {}

/// A sortable UTC run id: `YYYYMMDDTHHMMSSZ`.
pub fn new_run_id() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Writes `m` to `<dir>/<scenario>.manifest.json`.
pub fn write_manifest(m: &RunManifest, dir: &Path) -> std::io::Result<PathBuf> {
    let path = dir.join(format!("{}.manifest.json", m.scenario));
    let json = serde_json::to_string_pretty(m)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Reads a manifest written by `write_manifest`.
pub fn read_manifest(path: &Path) -> Result<RunManifest, ManifestError> {
    let text = std::fs::read_to_string(path).map_err(ManifestError::Io)?;
    serde_json::from_str(&text).map_err(ManifestError::Json)
}
```

Add `pub mod manifest;` to `capture/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml manifest::`
Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add capture/src/manifest.rs capture/src/lib.rs
git commit -m "feat(manifest): define what a lens agent is allowed to know

The manifest is the blinding boundary: it deliberately omits the
adapter's command line and the scenario's touches globs, and a test
asserts their absence from the serialized form a lens actually reads."
```

---

### Slice 1.5: The capture contract, the `command` adapter, and the deferred `window` adapter

**Tags:** coding

#### Task 5: The adapter contract and the `command` adapter

**Files:**
- Create: `capture/src/adapter/mod.rs`
- Create: `capture/src/adapter/command.rs`
- Create: `capture/src/adapter/window.rs`
- Modify: `capture/src/lib.rs` (add `pub mod adapter;`)
- Test: `capture/tests/command_adapter.rs`

**Interfaces:**
- Consumes: `config::{Scenario, AdapterKind}` (Task 2),
  `manifest::{RunManifest, new_run_id}` (Task 4).
- Produces:
  - `pub fn substitute_out(args: &str, out_stem: &Path) -> String`
  - `pub fn capture(scenario: &Scenario, run_dir: &Path, run_id: &str) -> Result<RunManifest, CaptureError>`
  - `pub enum CaptureError { NotImplemented { adapter: &'static str, reason: &'static str }, Spawn(std::io::Error), CommandFailed { status: String, stderr: String }, NoOutput { expected_stem: PathBuf }, AmbiguousOutput(Vec<PathBuf>), UnreadableImage { path: PathBuf, source: String } }`
  - `pub fn frame_count(path: &Path) -> Result<usize, CaptureError>`

  Consumed by `main.rs`'s `capture` subcommand (Task 6).

**The contract, stated once:** *given args, write one or more images to
a declared path, or fail with a typed error.* Nothing downstream knows
or cares which adapter produced a frame. Adding a surface later means
one new module behind this signature and no change anywhere else.

- [ ] **Step 1: Write the failing unit tests in `adapter/mod.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_placeholder_is_replaced_with_the_run_stem() {
        let args = "cargo run -p visual-snapshot -- --out {out}.gif";
        let got = substitute_out(args, std::path::Path::new("/runs/20260814/dial"));
        assert!(got.ends_with("dial.gif"), "got {got}");
        assert!(!got.contains("{out}"));
    }

    #[test]
    fn every_occurrence_of_the_placeholder_is_replaced() {
        let got = substitute_out("a {out}.png b {out}.log", std::path::Path::new("/r/s"));
        assert_eq!(got.matches("/r/s").count(), 2);
    }

    #[test]
    fn a_png_is_one_frame() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        image::RgbaImage::new(4, 4).save(&p).unwrap();
        assert_eq!(frame_count(&p).unwrap(), 1);
    }

    #[test]
    fn window_adapter_fails_with_a_typed_not_implemented_error() {
        // Deferred by design: no consumer exists. It must fail loudly
        // and specifically, never silently produce nothing.
        let e = window::capture_window("Some Title", std::path::Path::new("/tmp/x")).unwrap_err();
        match e {
            CaptureError::NotImplemented { adapter, reason } => {
                assert_eq!(adapter, "window");
                assert!(reason.contains("no consumer"), "reason must say why: {reason}");
            }
            other => panic!("expected NotImplemented, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Write the failing integration test**

```rust
// capture/tests/command_adapter.rs
//! Exercises the `command` adapter end to end against a real
//! subprocess, the way a consumer's `cargo run -p visual-snapshot`
//! line will be run.

use parallax_plumb::adapter::{capture, CaptureError};
use parallax_plumb::config::{AdapterKind, Scenario};

fn scenario(args: &str) -> Scenario {
    Scenario {
        name: "fixture".into(),
        adapter: AdapterKind::Command,
        args: args.into(),
        intent: Some("a 4x4 image exists".into()),
        touches: vec!["src/**".into()],
        ..Default::default()
    }
}

#[test]
fn a_command_that_writes_a_png_yields_a_one_frame_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("fixture.png");
    image::RgbaImage::new(4, 4).save(&src).unwrap();
    let copy = if cfg!(windows) {
        format!("copy \"{}\" \"{{out}}.png\"", src.display())
    } else {
        format!("cp '{}' '{{out}}.png'", src.display())
    };

    let m = capture(&scenario(&copy), dir.path(), "20260814T101500Z").unwrap();

    assert_eq!(m.adapter, "command");
    assert_eq!(m.frame_count, 1);
    assert_eq!(m.image, std::path::PathBuf::from("fixture.png"));
    assert_eq!(m.intent.as_deref(), Some("a 4x4 image exists"));
}

#[test]
fn a_command_that_writes_nothing_is_a_typed_no_output_error() {
    let dir = tempfile::tempdir().unwrap();
    let s = scenario("cd . {out}");
    assert!(matches!(capture(&s, dir.path(), "r").unwrap_err(), CaptureError::NoOutput { .. }));
}

#[test]
fn a_failing_command_reports_its_status_and_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let s = scenario("this-command-does-not-exist --out {out}.png");
    match capture(&s, dir.path(), "r").unwrap_err() {
        CaptureError::CommandFailed { status, .. } => assert!(!status.is_empty()),
        CaptureError::Spawn(_) => {}
        other => panic!("expected a command failure, got {other:?}"),
    }
}
```

Note the fixture strategy: the test writes a real 4x4 PNG itself and
declares a shell `copy`/`cp` as the scenario's `args`, so the
integration test exercises the full spawn-and-discover path without
depending on any external tool being installed.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --manifest-path capture/Cargo.toml`
Expected: FAIL — `adapter` module does not exist.

- [ ] **Step 4: Write `adapter/mod.rs`**

```rust
//! The capture contract: given args, write one or more images to a
//! declared path, or fail with a typed error. Nothing downstream knows
//! which adapter produced a frame.

pub mod command;
pub mod pty;
pub mod window;

use crate::config::{AdapterKind, Scenario};
use crate::manifest::RunManifest;
use std::path::{Path, PathBuf};

/// A capture that did not produce a usable image.
#[derive(Debug)]
pub enum CaptureError {
    /// This adapter has no v1 implementation.
    NotImplemented {
        /// Adapter name.
        adapter: &'static str,
        /// Why it is deferred, in words a reader can act on.
        reason: &'static str,
    },
    /// The adapter's process could not be spawned at all.
    Spawn(std::io::Error),
    /// The adapter's process ran and exited non-zero.
    CommandFailed {
        /// Exit status, rendered.
        status: String,
        /// Captured stderr, truncated to something readable.
        stderr: String,
    },
    /// The command succeeded but wrote no image at the declared stem.
    NoOutput {
        /// The stem images were expected at.
        expected_stem: PathBuf,
    },
    /// More than one image landed at the declared stem.
    AmbiguousOutput(Vec<PathBuf>),
    /// An image was produced but could not be decoded.
    UnreadableImage {
        /// The offending file.
        path: PathBuf,
        /// Decoder message.
        source: String,
    },
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::NotImplemented { adapter, reason } => {
                write!(f, "the `{adapter}` adapter is not implemented: {reason}")
            }
            CaptureError::Spawn(e) => write!(f, "could not start the capture command: {e}"),
            CaptureError::CommandFailed { status, stderr } => {
                write!(f, "capture command exited {status}\n{stderr}")
            }
            CaptureError::NoOutput { expected_stem } => write!(
                f,
                "capture command succeeded but wrote no image at {}.png/.gif",
                expected_stem.display()
            ),
            CaptureError::AmbiguousOutput(paths) => {
                write!(f, "capture wrote several images: {paths:?}")
            }
            CaptureError::UnreadableImage { path, source } => {
                write!(f, "could not decode {}: {source}", path.display())
            }
        }
    }
}
impl std::error::Error for CaptureError {}

/// Substitutes `{out}` with the run's output stem (a path with no
/// extension) everywhere it appears in `args`.
pub fn substitute_out(args: &str, out_stem: &Path) -> String {
    args.replace("{out}", &out_stem.display().to_string())
}

/// Counts frames in a captured image: 1 for a PNG, the decoded frame
/// count for a GIF.
pub fn frame_count(path: &Path) -> Result<usize, CaptureError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "gif" {
        use image::AnimationDecoder;
        let file = std::fs::File::open(path).map_err(CaptureError::Spawn)?;
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file))
            .map_err(|e| CaptureError::UnreadableImage {
                path: path.to_path_buf(),
                source: e.to_string(),
            })?;
        return Ok(decoder.into_frames().count());
    }
    image::open(path).map_err(|e| CaptureError::UnreadableImage {
        path: path.to_path_buf(),
        source: e.to_string(),
    })?;
    Ok(1)
}

/// Runs `scenario`'s adapter, writing into `run_dir`, and returns the
/// manifest describing what was captured.
pub fn capture(
    scenario: &Scenario,
    run_dir: &Path,
    run_id: &str,
) -> Result<RunManifest, CaptureError> {
    match scenario.adapter {
        AdapterKind::Command => command::capture_command(scenario, run_dir, run_id),
        AdapterKind::Pty => Err(CaptureError::NotImplemented {
            adapter: "pty",
            reason: "landing in Arc 5; use the `command` adapter meanwhile",
        }),
        AdapterKind::Window => Err(CaptureError::NotImplemented {
            adapter: "window",
            reason: "deferred — no consumer exists yet (TTUI is a TUI, \
                     Model-Experiments is Python/CLI); the contract admits \
                     it, the implementation is out of v1 scope",
        }),
    }
}
```

- [ ] **Step 5: Write `adapter/command.rs`**

```rust
//! The `command` adapter: runs any shell command that writes images to
//! a declared path. The escape hatch that makes adoption free — TTUI
//! adopts Plumb by declaring its existing `visual-snapshot` invocation
//! here and changing nothing about that tool.

use super::{frame_count, substitute_out, CaptureError};
use crate::config::Scenario;
use crate::manifest::RunManifest;
use std::path::Path;
use std::process::Command;

/// Runs `scenario.args` (with `{out}` substituted) through the platform
/// shell and reports the single image it produced.
pub fn capture_command(
    scenario: &Scenario,
    run_dir: &Path,
    run_id: &str,
) -> Result<RunManifest, CaptureError> {
    let stem = run_dir.join(&scenario.name);
    let line = substitute_out(&scenario.args, &stem);

    let output = if cfg!(windows) {
        Command::new("cmd").args(["/C", &line]).output()
    } else {
        Command::new("sh").args(["-c", &line]).output()
    }
    .map_err(CaptureError::Spawn)?;

    if !output.status.success() {
        return Err(CaptureError::CommandFailed {
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(4000)
                .collect(),
        });
    }

    let mut produced = Vec::new();
    for ext in ["png", "gif"] {
        let candidate = stem.with_extension(ext);
        if candidate.exists() {
            produced.push(candidate);
        }
    }
    match produced.len() {
        0 => Err(CaptureError::NoOutput { expected_stem: stem }),
        1 => {
            let image = produced.remove(0);
            let frames = frame_count(&image)?;
            Ok(RunManifest {
                run_id: run_id.to_string(),
                scenario: scenario.name.clone(),
                adapter: "command".into(),
                image: image.file_name().map(Into::into).unwrap_or(image.clone()),
                frame_count: frames,
                size: None,
                intent: scenario.intent.clone(),
                expects: scenario.expects.clone(),
                caveats: Vec::new(),
            })
        }
        _ => Err(CaptureError::AmbiguousOutput(produced)),
    }
}
```

`size` is `None` here: the `command` adapter deliberately knows nothing
about what it ran. The `pty` adapter fills it in (Arc 5).

- [ ] **Step 6: Write `adapter/window.rs` and a placeholder `adapter/pty.rs`**

```rust
//! The `window` adapter — capturing a native OS window by title.
//!
//! **Deliberately unimplemented.** The Plumb design draws the adapter
//! boundary so this can slot in behind the same contract later, but no
//! consumer for it exists: TTUI is a terminal UI, Model-Experiments is
//! Python/CLI, and neither is a desktop app. Implementing it now would
//! be speculative surface with no caller to shape it. This module
//! exists to make the deferral explicit and typed rather than a gap.

use super::CaptureError;
use std::path::Path;

/// Always fails with a typed, actionable `NotImplemented`.
pub fn capture_window(_title: &str, _out_stem: &Path) -> Result<(), CaptureError> {
    Err(CaptureError::NotImplemented {
        adapter: "window",
        reason: "deferred — no consumer exists yet (TTUI is a TUI, \
                 Model-Experiments is Python/CLI); the contract admits \
                 it, the implementation is out of v1 scope",
    })
}
```

```rust
//! The `pty` adapter — generalized from `tools/visual-snapshot`.
//! Lands in Arc 5; until then `adapter::capture` returns a typed
//! `NotImplemented` naming that.
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml`
Expected: PASS — 4 unit tests in `adapter::tests`, 3 integration tests
in `tests/command_adapter.rs`.

- [ ] **Step 8: Commit**

```bash
git add capture/src/adapter capture/src/lib.rs capture/tests/command_adapter.rs
git commit -m "feat(adapter): add the capture contract and the command adapter

The command adapter is what makes adoption free: TTUI declares its
existing visual-snapshot invocation and keeps that tool verbatim, so
extracting the pty capture crate is not on the critical path. \`window\`
ships as an explicit typed deferral — it has no consumer yet."
```

---

#### Task 6: Wire `init`, `select`, and `capture` into the CLI

**Files:**
- Modify: `capture/src/main.rs`
- Create: `templates/config.example.yaml`
- Create: `templates/taste.md`

**Interfaces:**
- Consumes: `config::load_config`, `select::{select_by_paths, select_by_name}`, `adapter::capture`, `manifest::{new_run_id, write_manifest}`.
- Produces: the CLI surface every later task and the orchestrating skill
  calls:
  - `plumb init [--dir .plumb]` — scaffold from `templates/`.
  - `plumb select --config <path> [--changed <file>|--scenario <name>]`
    — prints a JSON `Selection`; exit 3 when `selected` is empty.
  - `plumb capture --config <path> --run-dir <dir> --scenario <name>`
    — runs one adapter, writes the manifest, prints its path.

**TDD:** the argument-parsing surface is unit-tested with
`Args::try_parse_from` (the pattern `tools/visual-snapshot/src/main.rs`
already uses); the subcommand bodies are thin wiring over functions
already tested in Tasks 2-5.

- [ ] **Step 1: Write the failing CLI-parsing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_parses_a_changed_file_list_path() {
        let a = Args::try_parse_from([
            "plumb", "select", "--config", ".plumb/config.yaml", "--changed", "changed.txt",
        ])
        .unwrap();
        match a.command {
            Command::Select { config, changed, scenario } => {
                assert_eq!(config, std::path::PathBuf::from(".plumb/config.yaml"));
                assert_eq!(changed, Some(std::path::PathBuf::from("changed.txt")));
                assert!(scenario.is_none());
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn select_rejects_naming_both_changed_and_scenario() {
        assert!(Args::try_parse_from([
            "plumb", "select", "--config", "c.yaml", "--changed", "f.txt", "--scenario", "dial",
        ])
        .is_err());
    }

    #[test]
    fn capture_requires_a_run_dir_and_a_scenario() {
        assert!(Args::try_parse_from(["plumb", "capture", "--config", "c.yaml"]).is_err());
    }

    #[test]
    fn init_defaults_its_target_directory_to_dot_plumb() {
        let a = Args::try_parse_from(["plumb", "init"]).unwrap();
        match a.command {
            Command::Init { dir } => assert_eq!(dir, std::path::PathBuf::from(".plumb")),
            _ => panic!("expected Init"),
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path capture/Cargo.toml --bin plumb`
Expected: FAIL — no `Args`/`Command` types.

- [ ] **Step 3: Implement the CLI**

```rust
//! CLI entry point for `plumb`: scaffolding, scenario selection, and
//! capture. Judgment is a fan-out of subagents the orchestrating skill
//! dispatches — this binary never calls a model.

use clap::{Parser, Subcommand};
use parallax_plumb::{adapter, config, manifest, select};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "plumb", about = "Perceptual verification: capture, then judge.")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a `.plumb/` directory from the bundled templates.
    Init {
        /// Where to scaffold.
        #[arg(long, default_value = ".plumb")]
        dir: PathBuf,
    },
    /// Choose which scenarios a change warrants reviewing.
    Select {
        /// Path to `.plumb/config.yaml`.
        #[arg(long)]
        config: PathBuf,
        /// File holding one changed path per line (`-` for stdin).
        #[arg(long, conflicts_with = "scenario")]
        changed: Option<PathBuf>,
        /// Review exactly this scenario, ignoring `touches`.
        #[arg(long)]
        scenario: Option<String>,
    },
    /// Run one scenario's adapter and write its run manifest.
    Capture {
        /// Path to `.plumb/config.yaml`.
        #[arg(long)]
        config: PathBuf,
        /// Directory to write images and manifests into.
        #[arg(long)]
        run_dir: PathBuf,
        /// Which scenario to capture.
        #[arg(long)]
        scenario: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Args::parse().command {
        Command::Init { dir } => {
            std::fs::create_dir_all(dir.join("scripts"))?;
            std::fs::create_dir_all(dir.join("runs"))?;
            for (template, target) in [
                ("config.example.yaml", "config.yaml"),
                ("taste.md", "taste.md"),
            ] {
                let dest = dir.join(target);
                if dest.exists() {
                    println!("kept existing {}", dest.display());
                    continue;
                }
                let body = match template {
                    "config.example.yaml" => {
                        include_str!("../../templates/config.example.yaml")
                    }
                    _ => include_str!("../../templates/taste.md"),
                };
                std::fs::write(&dest, body)?;
                println!("wrote {}", dest.display());
            }
        }
        Command::Select { config, changed, scenario } => {
            let cfg = config::load_config(&config)?;
            let selection = match (changed, scenario) {
                (_, Some(name)) => select::select_by_name(&cfg, &name)?,
                (Some(path), None) => {
                    let text = if path == PathBuf::from("-") {
                        std::io::read_to_string(std::io::stdin())?
                    } else {
                        std::fs::read_to_string(&path)?
                    };
                    let paths: Vec<String> =
                        text.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
                    select::select_by_paths(&cfg, &paths)?
                }
                (None, None) => return Err("one of --changed or --scenario is required".into()),
            };
            println!("{}", serde_json::to_string_pretty(&selection)?);
            if selection.selected.is_empty() {
                eprintln!(
                    "no scenario's `touches` globs matched the changed paths, and no \
                     --scenario was named: nothing to review. Stopping rather than \
                     reviewing everything."
                );
                std::process::exit(3);
            }
        }
        Command::Capture { config, run_dir, scenario } => {
            let cfg = config::load_config(&config)?;
            let s = cfg
                .scenarios
                .iter()
                .find(|s| s.name == scenario)
                .ok_or_else(|| format!("no scenario named {scenario:?}"))?;
            std::fs::create_dir_all(&run_dir)?;
            let run_id = run_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from)
                .unwrap_or_else(manifest::new_run_id);
            let m = adapter::capture(s, &run_dir, &run_id)?;
            let path = manifest::write_manifest(&m, &run_dir)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}
```

Two things this needs to compile, both intentional:

- `Selection` and `Selected` gain `#[derive(Serialize)]` in
  `select.rs` — add it there.
- The two `include_str!` calls read the template files written in Steps
  4 and 5, so write those files first, then this. Embedding the
  templates at compile time is deliberate: it is what lets a cached
  binary scaffold a fresh `.plumb/` with no plugin directory in scope.

- [ ] **Step 4: Write `templates/config.example.yaml`**

```yaml
# .plumb/config.yaml — what to capture, and what each capture is for.
scenarios:
  - name: example-scenario
    # One of: command | pty | window(deferred, no v1 implementation)
    adapter: command
    # {out} is substituted with this run's output stem (no extension).
    args: >
      your-capture-command --out {out}.png
    # What this capture is supposed to show. The intent lens checks
    # against this, and only this. Omit it and the intent lens is
    # skipped with a notice.
    intent: >
      Describe the screen as someone who has never seen it would need.
    # Distortion this scenario declares intentional. Only
    # `visual-corruption` is defined. Omit it and garbled output is a
    # defect — the burden is on the scenario to claim the exemption.
    expects: []
    # Optional, design lens only: an additive, scenario-scoped note on
    # top of taste.md.
    # taste_override: >
    #   This screen is deliberately scruffier than the house grammar.
    # Changed paths that make this scenario worth reviewing.
    touches:
      - src/**
```

- [ ] **Step 5: Write `templates/taste.md`**

A skeleton with the four headings the design lens reads, each with one
line of instruction and no content: `## Aesthetic intent`,
`## Non-negotiables`, `## Deliberate violations of generic UI norms`
(with a note that **this is the most important section — without it the
design lens relitigates the entire aesthetic every run**), and
`## Explicitly still open to critique`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml`
Expected: PASS.

- [ ] **Step 7: Verify the Arc 1 milestone by hand, against TTUI**

From a TTUI checkout, with a temporary `config.yaml`:

```bash
plumb capture --config /tmp/plumb-config.yaml --run-dir /tmp/run1 \
  --scenario omnitrix-dial-rotate
```

Expected: `/tmp/run1/omnitrix-dial-rotate.gif` exists,
`/tmp/run1/omnitrix-dial-rotate.manifest.json` records the right frame
count, and `tools/visual-snapshot` was not modified.

- [ ] **Step 8: Commit**

```bash
git add capture/src/main.rs capture/src/select.rs templates
git commit -m "feat(cli): add plumb init, select, and capture

Selecting nothing exits 3 with an explicit message rather than widening
to every scenario, so a caller can never mistake 'nothing matched' for
'everything passed'."
```

---

## Arc 2: The blinded reviewer — first working verdict

Ends with `/plumb:review` producing a real GO / NO-GO / HOLD on a TTUI
scenario, using the two **blocker-capable** lenses. `design` and
`motion` resolve as *skipped with notice* until Arc 3. Nothing in this
Arc waits on `taste.md`.

### Slice 2.1: The finding contract

**Tags:** coding

#### Task 7: Finding schema, region enforcement, and severity clamping

**Files:**
- Create: `capture/src/finding.rs`
- Modify: `capture/src/lib.rs` (add `pub mod finding;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum Lens { Breakage, Intent, Design, Motion }` with `pub fn agent_name(self) -> &'static str`, `pub fn max_severity(self) -> Severity`, `pub fn is_blocker_capable(self) -> bool`
  - `pub enum Severity { Blocker, Major, Minor, Nit }` (ordered, `Blocker` most severe)
  - `pub enum Confidence { High, Medium, Low }`
  - `pub struct Finding { pub lens: Lens, pub scenario: String, pub severity: Severity, pub region: String, pub claim: String, pub evidence: String, pub confidence: Confidence }`
  - `pub struct ParsedFindings { pub kept: Vec<Finding>, pub dropped_no_region: usize, pub clamped: usize }`
  - `pub fn parse_findings(lens: Lens, scenario: &str, json: &str) -> Result<ParsedFindings, FindingParseError>`

  Consumed by `merge.rs` (Task 11) and `verdict.rs` (Task 12).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"[{"lens":"breakage","scenario":"dial","severity":"blocker",
      "region":"upper-right quadrant","claim":"the border does not close",
      "evidence":"the top-right corner glyph is a space","confidence":"high"}]"#;

    #[test]
    fn parses_a_well_formed_finding() {
        let p = parse_findings(Lens::Breakage, "dial", ONE).unwrap();
        assert_eq!(p.kept.len(), 1);
        assert_eq!(p.kept[0].severity, Severity::Blocker);
        assert_eq!(p.kept[0].confidence, Confidence::High);
        assert_eq!(p.dropped_no_region, 0);
    }

    #[test]
    fn an_empty_array_is_a_legitimate_result_not_an_error() {
        // No quota: finding nothing is expected and must never be an error.
        let p = parse_findings(Lens::Breakage, "dial", "[]").unwrap();
        assert!(p.kept.is_empty());
    }

    #[test]
    fn a_finding_with_no_region_is_dropped_and_counted() {
        let json = r#"[{"lens":"design","scenario":"dial","severity":"minor","region":"",
          "claim":"the layout feels unbalanced","evidence":"vibes","confidence":"low"}]"#;
        let p = parse_findings(Lens::Design, "dial", json).unwrap();
        assert!(p.kept.is_empty());
        assert_eq!(p.dropped_no_region, 1);
    }

    #[test]
    fn a_whitespace_only_region_is_also_dropped() {
        let json = ONE.replace("upper-right quadrant", "   ");
        let p = parse_findings(Lens::Breakage, "dial", &json).unwrap();
        assert_eq!(p.dropped_no_region, 1);
    }

    #[test]
    fn an_advisory_lens_cannot_emit_a_blocker() {
        // design/motion are capped at major, whatever they claim.
        let json = ONE.replace("breakage", "design");
        let p = parse_findings(Lens::Design, "dial", &json).unwrap();
        assert_eq!(p.kept[0].severity, Severity::Major);
        assert_eq!(p.clamped, 1);
    }

    #[test]
    fn a_blocker_capable_lens_keeps_its_blocker() {
        let p = parse_findings(Lens::Intent, "dial", &ONE.replace("breakage", "intent")).unwrap();
        assert_eq!(p.kept[0].severity, Severity::Blocker);
        assert_eq!(p.clamped, 0);
    }

    #[test]
    fn the_scenario_is_forced_to_the_one_actually_dispatched() {
        // An agent that mislabels its scenario must not corrupt the merge.
        let p = parse_findings(Lens::Breakage, "actual", &ONE.replace("dial", "hallucinated")).unwrap();
        assert_eq!(p.kept[0].scenario, "actual");
    }

    #[test]
    fn unparseable_output_is_an_error_the_caller_can_retry_on() {
        assert!(parse_findings(Lens::Breakage, "dial", "I looked at it and it's fine!").is_err());
    }

    #[test]
    fn prose_wrapped_around_a_json_array_is_recovered() {
        // Models pad. One recovery attempt is cheaper than a HOLD.
        let padded = format!("Here is my report:\n```json\n{ONE}\n```\n");
        assert_eq!(parse_findings(Lens::Breakage, "dial", &padded).unwrap().kept.len(), 1);
    }

    #[test]
    fn severity_orders_blocker_above_nit() {
        assert!(Severity::Blocker > Severity::Major);
        assert!(Severity::Major > Severity::Minor);
        assert!(Severity::Minor > Severity::Nit);
    }

    #[test]
    fn only_breakage_and_intent_are_blocker_capable() {
        assert!(Lens::Breakage.is_blocker_capable());
        assert!(Lens::Intent.is_blocker_capable());
        assert!(!Lens::Design.is_blocker_capable());
        assert!(!Lens::Motion.is_blocker_capable());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path capture/Cargo.toml finding::`
Expected: FAIL — `finding` module does not exist.

- [ ] **Step 3: Write the implementation**

```rust
//! The finding contract every lens agent reports against, plus the two
//! rules the orchestrator enforces on the way in: a finding that cannot
//! name where on screen it lives is dropped, and an advisory lens's
//! severity is clamped to its ceiling regardless of what it claimed.

use serde::{Deserialize, Serialize};

/// One of the four review lenses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lens {
    /// Rendering corruption, clipping, overlap, misalignment.
    Breakage,
    /// Conformance to the scenario's declared intent.
    Intent,
    /// Conformance to the project's taste profile.
    Design,
    /// Pacing, continuity, and readability across frames.
    Motion,
}

impl Lens {
    /// The agent definition file's `name` this lens dispatches to.
    pub fn agent_name(self) -> &'static str {
        match self {
            Lens::Breakage => "critic-breakage",
            Lens::Intent => "critic-intent",
            Lens::Design => "critic-design",
            Lens::Motion => "critic-motion",
        }
    }

    /// The most severe finding this lens is permitted to report.
    pub fn max_severity(self) -> Severity {
        match self {
            Lens::Breakage | Lens::Intent => Severity::Blocker,
            Lens::Design | Lens::Motion => Severity::Major,
        }
    }

    /// Whether an unresolved finding from this lens can hold the run.
    pub fn is_blocker_capable(self) -> bool {
        self.max_severity() == Severity::Blocker
    }
}

/// How bad a finding is. Ordered: `Blocker` is the most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Least severe.
    Nit,
    /// Small, worth knowing.
    Minor,
    /// Substantial, not run-holding.
    Major,
    /// Holds the run.
    Blocker,
}

/// How sure the lens is. Governs voice, not weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Assert it.
    High,
    /// State it plainly.
    Medium,
    /// Phrase it as a question.
    Low,
}

/// One reported observation about one scenario's capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Which lens reported it.
    pub lens: Lens,
    /// Which scenario it is about.
    pub scenario: String,
    /// How bad it is, after clamping.
    pub severity: Severity,
    /// Where on screen it lives. Mandatory and load-bearing.
    pub region: String,
    /// One sentence: what is wrong.
    pub claim: String,
    /// What in the image supports the claim.
    pub evidence: String,
    /// How sure the lens is.
    pub confidence: Confidence,
}

/// The result of ingesting one lens's report, with what was discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFindings {
    /// Findings that survived enforcement.
    pub kept: Vec<Finding>,
    /// How many were dropped for naming no region.
    pub dropped_no_region: usize,
    /// How many had their severity clamped to the lens's ceiling.
    pub clamped: usize,
}

/// A lens report that could not be read as the finding schema.
#[derive(Debug)]
pub struct FindingParseError(pub String);

impl std::fmt::Display for FindingParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lens report was not a JSON finding array: {}", self.0)
    }
}
impl std::error::Error for FindingParseError {}

/// Extracts the outermost `[...]` from text a model may have padded
/// with prose or a fenced code block. One recovery attempt only.
fn extract_array(text: &str) -> Option<&str> {
    let start = text.find('[')?;
    let end = text.rfind(']')?;
    if end > start {
        Some(&text[start..=end])
    } else {
        None
    }
}

/// Parses one lens's report, enforcing the mandatory `region` and the
/// lens's severity ceiling, and forcing `lens`/`scenario` to what was
/// actually dispatched rather than what the agent claimed.
pub fn parse_findings(
    lens: Lens,
    scenario: &str,
    json: &str,
) -> Result<ParsedFindings, FindingParseError> {
    let array = extract_array(json).ok_or_else(|| FindingParseError(json.chars().take(200).collect()))?;
    let raw: Vec<Finding> =
        serde_json::from_str(array).map_err(|e| FindingParseError(e.to_string()))?;

    let ceiling = lens.max_severity();
    let mut kept = Vec::new();
    let mut dropped_no_region = 0;
    let mut clamped = 0;
    for mut f in raw {
        if f.region.trim().is_empty() {
            dropped_no_region += 1;
            continue;
        }
        f.lens = lens;
        f.scenario = scenario.to_string();
        if f.severity > ceiling {
            f.severity = ceiling;
            clamped += 1;
        }
        kept.push(f);
    }
    Ok(ParsedFindings { kept, dropped_no_region, clamped })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml finding::`
Expected: PASS, 11 tests.

- [ ] **Step 5: Commit**

```bash
git add capture/src/finding.rs capture/src/lib.rs
git commit -m "feat(finding): enforce mandatory region and per-lens severity ceilings

A finding that cannot point at somewhere on screen is dropped and
counted rather than kept — that single requirement is what stops 'the
layout feels unbalanced' from surviving into a verdict."
```

---

### Slice 2.2: Blinded prompt construction

**Tags:** coding

#### Task 8: Lens applicability and the blinded prompt builder

**Files:**
- Create: `capture/src/prompt.rs`
- Modify: `capture/src/lib.rs` (add `pub mod prompt;`)

**Interfaces:**
- Consumes: `finding::Lens` (Task 7), `manifest::{RunManifest, Caveat}` (Task 4), `config::Expectation` (Task 2).
- Produces:
  - `pub struct LensInputs<'a> { pub lens: Lens, pub manifest: &'a RunManifest, pub taste: Option<&'a str>, pub taste_override: Option<&'a str> }`
  - `pub enum Skip { NoIntentDeclared, NoTasteProfile, SingleFrame }`
  - `pub fn applicable_lenses(m: &RunManifest, taste_present: bool) -> (Vec<Lens>, Vec<(Lens, Skip)>)`
  - `pub fn build_prompt(inputs: &LensInputs) -> String`
  - `pub struct Dispatch { pub lens: Lens, pub agent: String, pub scenario: String, pub image: PathBuf, pub prompt: String }`
  - `pub fn plan_dispatch(manifests: &[RunManifest], taste: Option<&str>, overrides: &HashMap<String, String>, cap: usize) -> DispatchPlan`
  - `pub struct DispatchPlan { pub batches: Vec<Vec<Dispatch>>, pub skipped: Vec<(String, Lens, Skip)>, pub cap: usize }`
  - `pub const DEFAULT_CONCURRENCY_CAP: usize = 8;`

  Consumed by `main.rs`'s `plan` subcommand (Task 12) and the skill.

**Why prompt construction lives in Rust and not in skill prose:** it
makes the blinding property a unit test. `build_prompt`'s only inputs
are the lens, the manifest, and the taste text. There is no parameter
through which a diff, a source file, an `args` string, or a `touches`
glob could arrive — and the tests below assert that on the rendered
output as well as by construction.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Expectation;
    use crate::manifest::{Caveat, RunManifest};

    fn m(frames: usize, intent: Option<&str>, expects: Vec<Expectation>) -> RunManifest {
        RunManifest {
            run_id: "r".into(),
            scenario: "falcon-glitch-burst".into(),
            adapter: "command".into(),
            image: "falcon-glitch-burst.gif".into(),
            frame_count: frames,
            size: Some("120x40".into()),
            intent: intent.map(String::from),
            expects,
            caveats: Vec::new(),
        }
    }

    // --- applicability -------------------------------------------------

    #[test]
    fn breakage_always_applies() {
        let (apply, _) = applicable_lenses(&m(1, None, vec![]), false);
        assert!(apply.contains(&Lens::Breakage));
    }

    #[test]
    fn intent_is_skipped_with_a_notice_when_no_intent_is_declared() {
        let (apply, skipped) = applicable_lenses(&m(1, None, vec![]), true);
        assert!(!apply.contains(&Lens::Intent));
        assert!(skipped.contains(&(Lens::Intent, Skip::NoIntentDeclared)));
    }

    #[test]
    fn design_is_skipped_with_a_notice_when_no_taste_profile_exists() {
        // A generic aesthetic opinion is worse than none.
        let (apply, skipped) = applicable_lenses(&m(1, Some("i"), vec![]), false);
        assert!(!apply.contains(&Lens::Design));
        assert!(skipped.contains(&(Lens::Design, Skip::NoTasteProfile)));
    }

    #[test]
    fn motion_is_skipped_with_a_notice_on_a_single_frame_capture() {
        let (apply, skipped) = applicable_lenses(&m(1, Some("i"), vec![]), true);
        assert!(!apply.contains(&Lens::Motion));
        assert!(skipped.contains(&(Lens::Motion, Skip::SingleFrame)));
    }

    #[test]
    fn all_four_apply_on_a_multiframe_capture_with_intent_and_taste() {
        let (apply, skipped) = applicable_lenses(&m(5, Some("i"), vec![]), true);
        assert_eq!(apply.len(), 4);
        assert!(skipped.is_empty());
    }

    // --- blinding ------------------------------------------------------

    /// The single most important test in this crate.
    #[test]
    fn no_prompt_carries_a_diff_source_authorship_or_change_framing() {
        for lens in [Lens::Breakage, Lens::Intent, Lens::Design, Lens::Motion] {
            let manifest = m(5, Some("The panel stays legible."), vec![]);
            let p = build_prompt(&LensInputs {
                lens,
                manifest: &manifest,
                taste: Some("Loud in colour, disciplined in structure."),
                taste_override: None,
            })
            .to_lowercase();
            for forbidden in [
                "diff", "git ", "commit", "source code", "the code", "your change",
                "you changed", "verify", "confirm this", "looks right", "regression",
                "src/", "examples/", "cargo", "--example", "touches",
            ] {
                assert!(!p.contains(forbidden), "{lens:?} prompt leaked {forbidden:?}");
            }
        }
    }

    #[test]
    fn every_prompt_frames_the_work_as_someone_elses() {
        for lens in [Lens::Breakage, Lens::Intent, Lens::Design, Lens::Motion] {
            let manifest = m(5, Some("i"), vec![]);
            let p = build_prompt(&LensInputs { lens, manifest: &manifest, taste: Some("t"), taste_override: None });
            assert!(p.contains("Sim Sup"), "{lens:?} must carry the persona");
            assert!(p.contains("someone else"), "{lens:?} must use third-party framing");
        }
    }

    #[test]
    fn every_prompt_states_that_an_empty_list_is_a_correct_outcome() {
        let manifest = m(5, Some("i"), vec![]);
        let p = build_prompt(&LensInputs { lens: Lens::Breakage, manifest: &manifest, taste: None, taste_override: None });
        assert!(p.contains("[]"));
        assert!(p.to_lowercase().contains("expected outcome"));
    }

    // --- per-lens payloads ---------------------------------------------

    #[test]
    fn only_the_intent_lens_receives_the_declared_intent() {
        let manifest = m(5, Some("THE DIAL ROTATES"), vec![]);
        let intent_prompt = build_prompt(&LensInputs { lens: Lens::Intent, manifest: &manifest, taste: Some("t"), taste_override: None });
        assert!(intent_prompt.contains("THE DIAL ROTATES"));
        for lens in [Lens::Breakage, Lens::Design, Lens::Motion] {
            let p = build_prompt(&LensInputs { lens, manifest: &manifest, taste: Some("t"), taste_override: None });
            assert!(!p.contains("THE DIAL ROTATES"), "{lens:?} must not see the intent");
        }
    }

    #[test]
    fn only_the_design_lens_receives_the_taste_profile_and_its_override() {
        let manifest = m(5, Some("i"), vec![]);
        let design = build_prompt(&LensInputs {
            lens: Lens::Design,
            manifest: &manifest,
            taste: Some("DENSITY IS INTENTIONAL"),
            taste_override: Some("SCRUFFIER THAN THE HOUSE GRAMMAR"),
        });
        assert!(design.contains("DENSITY IS INTENTIONAL"));
        assert!(design.contains("SCRUFFIER THAN THE HOUSE GRAMMAR"));
        for lens in [Lens::Breakage, Lens::Intent, Lens::Motion] {
            let p = build_prompt(&LensInputs { lens, manifest: &manifest, taste: Some("DENSITY IS INTENTIONAL"), taste_override: Some("SCRUFFIER THAN THE HOUSE GRAMMAR") });
            assert!(!p.contains("DENSITY IS INTENTIONAL"), "{lens:?} must not see taste.md");
        }
    }

    // --- intentional distortion ----------------------------------------

    #[test]
    fn declared_visual_corruption_reaches_the_breakage_lens_as_an_exemption() {
        let manifest = m(5, Some("i"), vec![Expectation::VisualCorruption]);
        let p = build_prompt(&LensInputs { lens: Lens::Breakage, manifest: &manifest, taste: None, taste_override: None });
        assert!(p.contains("visual-corruption"));
        assert!(p.contains("Do not raise findings for it"));
        // Bound 1: a category, not a region.
        assert!(p.contains("does not excuse a panel that failed to draw"));
        // Bound 2: still bound by legibility.
        assert!(p.contains("permanently destroys a reading"));
    }

    #[test]
    fn an_undeclared_scenario_gets_the_default_garbling_is_a_defect_treatment() {
        let manifest = m(5, Some("i"), vec![]);
        let p = build_prompt(&LensInputs { lens: Lens::Breakage, manifest: &manifest, taste: None, taste_override: None });
        assert!(!p.contains("visual-corruption"));
        assert!(p.contains("This scenario declares no intentional distortion"));
    }

    #[test]
    fn expects_is_a_breakage_lens_input_only() {
        let manifest = m(5, Some("i"), vec![Expectation::VisualCorruption]);
        for lens in [Lens::Intent, Lens::Design, Lens::Motion] {
            let p = build_prompt(&LensInputs { lens, manifest: &manifest, taste: Some("t"), taste_override: None });
            assert!(!p.contains("visual-corruption"), "{lens:?} must not receive expects");
        }
    }

    // --- caveats and batching -------------------------------------------

    #[test]
    fn a_disclosed_caveat_reaches_every_lens() {
        let mut manifest = m(5, Some("i"), vec![]);
        manifest.caveats = vec![Caveat::UnmappedGlyphSubstituted { codepoint: "U+2726".into(), count: 3 }];
        for lens in [Lens::Breakage, Lens::Intent, Lens::Design, Lens::Motion] {
            let p = build_prompt(&LensInputs { lens, manifest: &manifest, taste: Some("t"), taste_override: None });
            assert!(p.contains("U+2726"), "{lens:?} must be told about placeholders");
            assert!(p.contains("do not judge"), "{lens:?} must be told not to judge them");
        }
    }

    #[test]
    fn dispatch_batches_at_the_concurrency_cap_and_reports_the_cap() {
        let manifests: Vec<_> = (0..3).map(|i| {
            let mut mm = m(5, Some("i"), vec![]);
            mm.scenario = format!("s{i}");
            mm
        }).collect();
        // 3 scenarios x 4 applicable lenses = 12 dispatches, cap 8.
        let plan = plan_dispatch(&manifests, Some("t"), &Default::default(), 8);
        assert_eq!(plan.batches.len(), 2);
        assert_eq!(plan.batches[0].len(), 8);
        assert_eq!(plan.batches[1].len(), 4);
        assert_eq!(plan.cap, 8);
    }

    #[test]
    fn the_default_concurrency_cap_is_eight() {
        assert_eq!(DEFAULT_CONCURRENCY_CAP, 8);
    }

    #[test]
    fn a_taste_override_is_matched_to_its_scenario_only() {
        let manifests = vec![m(5, Some("i"), vec![])];
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("some-other-scenario".to_string(), "NOT THIS ONE".to_string());
        let plan = plan_dispatch(&manifests, Some("t"), &overrides, 8);
        for d in plan.batches.iter().flatten() {
            assert!(!d.prompt.contains("NOT THIS ONE"));
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path capture/Cargo.toml prompt::`
Expected: FAIL — `prompt` module does not exist.

- [ ] **Step 3: Write the implementation**

`build_prompt` composes a fixed skeleton plus per-lens sections. The
skeleton, verbatim (this text is the contract the tests above assert
against — do not paraphrase it):

```text
You are Sim Sup.

NASA's Simulation Supervisor spent every training run inventing failures
to find out whether the flight controllers caught them. That is your
stance. You are looking at someone else's work, submitted for critique.
You did not produce it and it does not need your approval.

## What you can see

One image and the run manifest below. Read the image. That is your
entire evidence base. You cannot see how it was produced, and you must
not reason about how it was probably produced. Reason only from pixels.

Image: <image path>
Frames: <n> (<single still | animated sequence>)
Terminal size: <cols>x<rows>            [omitted when unknown]

## Disclosed caveats                     [section omitted when none]

- <n> cells render a placeholder box in place of <codepoint>. These are
  a known limitation of the capture, not a defect: do not judge them.

<PER-LENS SECTION — see below>

## No quota

An empty findings list is a correct and expected outcome. You are not
graded on finding something. A manufactured finding is worse than none,
because it teaches the reader to skim you.

## Confidence governs voice

High confidence asserts. Low confidence asks: phrase a low-confidence
observation as a question, because that is what it actually is.

## Reporting

Return a JSON array and nothing else.

[
  {
    "lens": "<lens>",
    "scenario": "<scenario>",
    "severity": "blocker|major|minor|nit",
    "region": "where on screen, in words a reader can find unaided",
    "claim": "one sentence: what is wrong",
    "evidence": "what in the image supports this",
    "confidence": "high|medium|low"
  }
]

If you have nothing to report, return exactly:

[]

`region` is mandatory. A finding whose region you cannot fill in
concretely is dropped before anyone reads it — so do not submit it.
```

Per-lens sections:

- **`Lens::Breakage`** — the domain list (corruption, clipping,
  overlap, misalignment, dead frames, unreadable contrast), the
  out-of-bounds list (attractiveness, proportion, pacing, whether it
  did what it was meant to), the severity ceiling (`blocker`), and the
  **intentional-distortion block**, which has exactly two forms:

  With `expects: [visual-corruption]` present:

  ```text
  ## Intentional distortion

  This scenario declares `visual-corruption`: glyph garbling and region
  displacement are the point here, not a defect. Do not raise findings
  for it.

  Two bounds still hold:

  - This excuses a *category*, not a *region*. It excuses garbling; it
    does not excuse a panel that failed to draw, a border that does not
    close, or content clipped by an edge.
  - It is still bound by legibility. A glitch that momentarily disturbs
    a reading is the feature; one that permanently destroys a reading
    across the whole capture is a defect, and you must still report it.
  ```

  With no declaration:

  ```text
  ## Intentional distortion

  This scenario declares no intentional distortion. Garbled glyphs and
  displaced regions are defects here. Report them.
  ```

- **`Lens::Intent`** — the declared intent verbatim, an instruction to
  check the image against *that statement only* and not against general
  quality, and the severity ceiling (`blocker`, reserved for an intent
  the image plainly does not satisfy).
- **`Lens::Design`** — `taste.md` verbatim, then the `taste_override`
  (labelled as additive and scenario-scoped) when present, then: *where
  this profile and generic UI advice conflict, the profile wins*, and
  the severity ceiling (`major` — this lens is advisory and cannot hold
  a run).
- **`Lens::Motion`** — the frame count, an instruction to judge pacing,
  continuity, and whether anything is legible only in frames a viewer
  would not pause on, and the severity ceiling (`major`).

`plan_dispatch` maps each manifest through `applicable_lenses`, builds
a `Dispatch` per applicable lens, and chunks the flat list into batches
of `cap`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml prompt::`
Expected: PASS, 17 tests.

- [ ] **Step 5: Commit**

```bash
git add capture/src/prompt.rs capture/src/lib.rs
git commit -m "feat(prompt): build blinded lens prompts in code, not in skill prose

Putting prompt construction here makes the blinding property a unit
test rather than a hope: build_prompt has no parameter through which a
diff, a source path, or the adapter's command line could arrive, and
the tests assert their absence from every rendered lens prompt."
```

---

### Slice 2.3: The two blocker-capable lens agents

**Tags:** coding

The agent definition file is the **durable** statement of a lens's
stance and tool grant; the dispatched prompt (Task 8) carries the
per-run payload and repeats the framing so a single run is
self-contained. The overlap is deliberate — the framing is the one
thing that must not go missing.

**`tools: Read` is a blinding enforcement, not a performance tweak.** A
lens with no `Grep`, `Glob`, or `Bash` physically cannot go and read
the source it is being kept from.

#### Task 9: `critic-breakage`

**Files:**
- Create: `agents/critic-breakage.md`

**Interfaces:**
- Consumes: the prompt built by `prompt::build_prompt` for
  `Lens::Breakage` (Task 8).
- Produces: the agent type `critic-breakage`, dispatched by name from
  `skills/visual-review/SKILL.md` (Task 13).

**TDD exception: this is a prompt, not code.** Its behavior is verified
by the reviewer regression corpus in Arc 7, which is exactly the
mechanism the spec names for "tuning agent prompts against evidence
rather than vibes."

- [ ] **Step 1: Write `agents/critic-breakage.md`**

```markdown
---
name: critic-breakage
description: Sim Sup's breakage lens. Reports rendering corruption, clipping, overlap, misalignment, and dead frames visible in a submitted screen capture. Blocker-capable. Receives the image and run manifest only — never source, never a diff.
tools: Read
---

You are **Sim Sup**.

NASA's Simulation Supervisor spent every training run inventing failures
to find out whether the flight controllers caught them. That is your
stance. You are reviewing someone else's work, submitted for critique.
You did not produce it, and it does not need your approval.

## What you can see

One image and one run manifest. Read the image. That is your entire
evidence base. You do not have the source, and you must not reason
about what the source probably does — an agent that can read the code
concludes "it draws three panes, so there are three panes" instead of
looking. Reason only from pixels.

## Your lens

Report only these:

- Corrupted or garbled glyphs, mojibake, replacement characters.
- Content clipped by a pane or screen edge mid-word or mid-shape.
- Panels, borders, or text overlapping such that one destroys the other.
- Misalignment: a border that does not close, a column that does not
  line up with its header, a panel drifted by a cell.
- Dead frames: entirely blank, entirely black, or entirely uniform
  where structure is plainly expected.
- Contrast that renders a region unreadable.

Out of bounds even when you notice it: whether it is attractive,
whether the proportions are good, whether the animation paces well,
whether it accomplishes what it was meant to. Other lenses own those,
and you may not clear or overrule them.

## Intentional distortion

Some interfaces corrupt themselves on purpose. Your dispatched prompt
states whether this scenario declares that. If it does, do not raise
findings for the declared distortion — but the declaration excuses a
*category*, not a *region*, and it never excuses a glitch that
permanently destroys a reading rather than momentarily disturbing it.
If it does not declare it, garbling is a defect and you report it.

## Disclosed caveats

The manifest may disclose placeholders or regions the capture could not
reproduce. Those are stated limitations, not defects. Do not report
them.

## No quota

An empty findings list is a correct and expected outcome. You are not
graded on finding something. A manufactured finding is worse than none,
because it teaches the reader to skim you.

## Severity

You are blocker-capable. Reserve `blocker` for damage that makes the
interface wrong or unusable, not for something merely untidy.

## Reporting

Return a JSON array and nothing else — no prose before or after.

[
  {
    "lens": "breakage",
    "scenario": "<the scenario name you were given>",
    "severity": "blocker|major|minor|nit",
    "region": "where on screen, in words a reader can find unaided",
    "claim": "one sentence: what is wrong",
    "evidence": "what in the image supports this",
    "confidence": "high|medium|low"
  }
]

If you have nothing to report, return exactly:

[]

`region` is mandatory. A finding whose region you cannot fill in
concretely is dropped before anyone reads it — so do not submit it.
```

- [ ] **Step 2: Verify the file parses as an agent definition**

Run: `plumb`'s repo installed as a local plugin; confirm
`critic-breakage` appears in the available agent list and that its
`tools` grant is `Read` only.

- [ ] **Step 3: Commit**

```bash
git add agents/critic-breakage.md
git commit -m "feat(agents): add the blocker-capable breakage lens

Granting only Read is a blinding enforcement rather than a performance
choice: with no Grep, Glob, or Bash the lens physically cannot reach
the source it is being kept from."
```

---

#### Task 10: `critic-intent`

**Files:**
- Create: `agents/critic-intent.md`

**Interfaces:**
- Consumes: the prompt built for `Lens::Intent` (Task 8), which carries
  the scenario's declared `intent` string verbatim.
- Produces: the agent type `critic-intent`.

**TDD exception: prompt, not code** — same as Task 9.

- [ ] **Step 1: Write `agents/critic-intent.md`**

Same frontmatter shape (`name: critic-intent`, `tools: Read`,
description naming it blocker-capable and blinded), the identical Sim
Sup / "What you can see" / "No quota" / "Reporting" sections as Task 9
with `"lens": "intent"`, and this lens section in place of Task 9's:

```markdown
## Your lens

Your dispatched prompt contains a **declared intent**: one statement of
what this capture is supposed to show, written by whoever built the
scenario. Check the image against that statement and nothing else.

- Does the image show what the intent says it shows?
- Is anything the intent names absent, wrong, or in a different place
  than the intent describes?
- Is there something present that plainly contradicts the intent?

Out of bounds even when you notice it: rendering defects (another lens
owns those), whether it looks good, whether it moves well, and any
opinion about whether the intent itself is a good idea. You check
conformance to the stated intent. That is the whole job.

The intent is written in prose and will not be exhaustive. Do not
report the absence of something the intent never claimed — silence in
the intent is not a claim.

## Severity

You are blocker-capable. Reserve `blocker` for an intent the image
plainly does not satisfy — a named element missing, or the described
state not the one on screen. A partial or arguable mismatch is `major`
at most.
```

- [ ] **Step 2: Verify and commit**

```bash
git add agents/critic-intent.md
git commit -m "feat(agents): add the blocker-capable intent lens"
```

---

### Slice 2.4: Merging findings

**Tags:** coding

#### Task 11: Dedupe, normalize, and fingerprint

**Files:**
- Create: `capture/src/merge.rs`
- Modify: `capture/src/lib.rs` (add `pub mod merge;`)

**Interfaces:**
- Consumes: `finding::{Finding, Lens, Severity}` (Task 7).
- Produces:
  - `pub fn normalize_claim(claim: &str) -> String`
  - `pub fn fingerprint(scenario: &str, region: &str, claim: &str) -> String`
  - `pub struct MergedFinding { pub finding: Finding, pub also_raised_by: Vec<Lens>, pub fingerprint: String }`
  - `pub fn merge(findings: Vec<Finding>) -> Vec<MergedFinding>`

  Consumed by `rulings.rs` (Task 16) and `verdict.rs` (Task 12).

**Fingerprint definition (used identically by rulings in Arc 4):** the
first 16 hex characters of `sha256(scenario \n normalized_region \n
normalized_claim)`. Note it deliberately **excludes the lens** —
otherwise the same observation raised by a second lens would evade a
ruling made against the first.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Confidence, Finding, Lens, Severity};

    fn f(lens: Lens, sev: Severity, region: &str, claim: &str) -> Finding {
        Finding {
            lens,
            scenario: "dial".into(),
            severity: sev,
            region: region.into(),
            claim: claim.into(),
            evidence: "e".into(),
            confidence: Confidence::High,
        }
    }

    #[test]
    fn normalization_ignores_case_punctuation_and_spacing() {
        assert_eq!(
            normalize_claim("The  border does NOT close."),
            normalize_claim("the border does not close")
        );
    }

    #[test]
    fn a_fingerprint_is_sixteen_stable_hex_characters() {
        let a = fingerprint("dial", "upper right", "the border does not close");
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, fingerprint("dial", "upper right", "The border does not close!"));
    }

    #[test]
    fn different_scenarios_fingerprint_differently() {
        assert_ne!(fingerprint("a", "r", "c"), fingerprint("b", "r", "c"));
    }

    #[test]
    fn two_lenses_raising_the_same_thing_merge_into_one_finding() {
        let merged = merge(vec![
            f(Lens::Breakage, Severity::Major, "upper right", "the border does not close"),
            f(Lens::Design, Severity::Minor, "upper right", "The border does not close."),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].also_raised_by, vec![Lens::Design]);
    }

    #[test]
    fn a_merged_finding_keeps_the_most_severe_report() {
        let merged = merge(vec![
            f(Lens::Design, Severity::Minor, "r", "c"),
            f(Lens::Breakage, Severity::Blocker, "r", "c"),
        ]);
        assert_eq!(merged[0].finding.severity, Severity::Blocker);
        assert_eq!(merged[0].finding.lens, Lens::Breakage, "the severest report owns it");
    }

    #[test]
    fn distinct_regions_do_not_merge() {
        let merged = merge(vec![
            f(Lens::Breakage, Severity::Major, "upper right", "c"),
            f(Lens::Breakage, Severity::Major, "lower left", "c"),
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn output_is_sorted_most_severe_first() {
        let merged = merge(vec![
            f(Lens::Breakage, Severity::Nit, "a", "x"),
            f(Lens::Breakage, Severity::Blocker, "b", "y"),
            f(Lens::Breakage, Severity::Minor, "c", "z"),
        ]);
        let sevs: Vec<_> = merged.iter().map(|m| m.finding.severity).collect();
        assert_eq!(sevs, vec![Severity::Blocker, Severity::Minor, Severity::Nit]);
    }

    #[test]
    fn merging_an_empty_list_yields_an_empty_list() {
        assert!(merge(Vec::new()).is_empty());
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test --manifest-path capture/Cargo.toml merge::` → FAIL.

Implementation notes: `normalize_claim` lowercases, strips all
non-alphanumeric characters to spaces, and collapses runs of
whitespace; `fingerprint` hashes with `sha2::Sha256` and hex-truncates
to 16; `merge` groups by fingerprint, keeps the most severe member as
the representative (ties broken by blocker-capable lens first), records
every other contributing lens in `also_raised_by`, and sorts descending
by severity then by scenario then by region for a stable order.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml merge::`
Expected: PASS, 8 tests.

- [ ] **Step 4: Commit**

```bash
git add capture/src/merge.rs capture/src/lib.rs
git commit -m "feat(merge): dedupe findings across lenses by scenario, region, and claim

The fingerprint deliberately excludes the lens: otherwise the same
observation raised by a second lens would slip past a ruling already
made against the first. Suppression is bounded by a severity ceiling
in Slice 4.1 so that waiving a cosmetic finding cannot silence a
blocker about the same region."
```

---

### Slice 2.5: Gate semantics and `verdict.md`

**Tags:** coding

#### Task 12: Aggregate GO / NO-GO / HOLD and render the verdict

**Files:**
- Create: `capture/src/verdict.rs`
- Modify: `capture/src/lib.rs` (add `pub mod verdict;`)
- Modify: `capture/src/main.rs` (add the `plan` and `merge` subcommands)

**Interfaces:**
- Consumes: `merge::MergedFinding` (Task 11), `finding::{Lens, Severity}` (Task 7), `prompt::{Skip, DispatchPlan}` (Task 8).
- Produces:
  - `pub enum Verdict { Go, NoGo, Hold }` with `pub fn exit_code(self) -> i32` (0/1/2)
  - `pub struct LensReport { pub scenario: String, pub lens: Lens, pub outcome: LensOutcome }`
  - `pub enum LensOutcome { Reported, Skipped(Skip), Held(String) }`
  - `pub fn aggregate(reports: &[LensReport], findings: &[MergedFinding]) -> Verdict`
  - `pub struct VerdictInput { pub run_id: String, pub reports: Vec<LensReport>, pub findings: Vec<MergedFinding>, pub suppressed: Vec<MergedFinding>, pub stale_rulings: Vec<String>, pub dropped_no_region: usize, pub deferred: Vec<String>, pub capture_failures: Vec<(String, String)> }`
  - `pub fn render_verdict(input: &VerdictInput) -> String`
  - `pub fn write_verdict(input: &VerdictInput, run_dir: &Path) -> std::io::Result<PathBuf>`

  Consumed by `main.rs`'s `merge` subcommand and the skill (Task 13).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Confidence, Finding, Lens, Severity};
    use crate::merge::{merge, MergedFinding};
    use crate::prompt::Skip;

    fn reported(lens: Lens) -> LensReport {
        LensReport { scenario: "dial".into(), lens, outcome: LensOutcome::Reported }
    }

    fn finding(lens: Lens, sev: Severity) -> Vec<MergedFinding> {
        merge(vec![Finding {
            lens,
            scenario: "dial".into(),
            severity: sev,
            region: "upper right".into(),
            claim: "the border does not close".into(),
            evidence: "e".into(),
            confidence: Confidence::High,
        }])
    }

    #[test]
    fn no_findings_is_a_go() {
        let v = aggregate(&[reported(Lens::Breakage), reported(Lens::Intent)], &[]);
        assert_eq!(v, Verdict::Go);
    }

    #[test]
    fn advisory_findings_only_is_still_a_go() {
        let v = aggregate(&[reported(Lens::Design)], &finding(Lens::Design, Severity::Major));
        assert_eq!(v, Verdict::Go, "advisory findings are reported and never block");
    }

    #[test]
    fn a_blocker_from_a_blocker_capable_lens_is_a_no_go() {
        let v = aggregate(&[reported(Lens::Breakage)], &finding(Lens::Breakage, Severity::Blocker));
        assert_eq!(v, Verdict::NoGo);
    }

    #[test]
    fn a_single_no_go_holds_the_run_however_many_lenses_reported_clean() {
        let reports = vec![reported(Lens::Breakage), reported(Lens::Intent), reported(Lens::Design), reported(Lens::Motion)];
        let v = aggregate(&reports, &finding(Lens::Intent, Severity::Blocker));
        assert_eq!(v, Verdict::NoGo, "one console's no-go holds the launch");
    }

    #[test]
    fn a_held_lens_is_never_upgraded_to_a_go() {
        // The single most important gate rule.
        let reports = vec![
            reported(Lens::Breakage),
            LensReport { scenario: "dial".into(), lens: Lens::Intent, outcome: LensOutcome::Held("unparseable output twice".into()) },
        ];
        assert_eq!(aggregate(&reports, &[]), Verdict::Hold);
    }

    #[test]
    fn a_skipped_lens_does_not_hold_the_run() {
        // Skipped is a checked non-applicability, not an unknown.
        let reports = vec![
            reported(Lens::Breakage),
            LensReport { scenario: "dial".into(), lens: Lens::Design, outcome: LensOutcome::Skipped(Skip::NoTasteProfile) },
        ];
        assert_eq!(aggregate(&reports, &[]), Verdict::Go);
    }

    #[test]
    fn a_no_go_outranks_a_hold() {
        let reports = vec![
            reported(Lens::Breakage),
            LensReport { scenario: "dial".into(), lens: Lens::Motion, outcome: LensOutcome::Held("x".into()) },
        ];
        assert_eq!(aggregate(&reports, &finding(Lens::Breakage, Severity::Blocker)), Verdict::NoGo);
    }

    #[test]
    fn exit_codes_are_zero_one_two() {
        assert_eq!(Verdict::Go.exit_code(), 0);
        assert_eq!(Verdict::NoGo.exit_code(), 1);
        assert_eq!(Verdict::Hold.exit_code(), 2);
    }

    // --- rendering ------------------------------------------------------

    fn input() -> VerdictInput {
        VerdictInput {
            run_id: "20260814T101500Z".into(),
            reports: vec![reported(Lens::Breakage)],
            findings: finding(Lens::Breakage, Severity::Blocker),
            suppressed: Vec::new(),
            stale_rulings: Vec::new(),
            dropped_no_region: 0,
            deferred: Vec::new(),
            capture_failures: Vec::new(),
        }
    }

    #[test]
    fn the_verdict_states_go_no_go_or_hold_in_those_exact_words() {
        assert!(render_verdict(&input()).contains("NO-GO"));
    }

    #[test]
    fn a_capture_failure_is_reported_as_hold_and_is_never_a_go() {
        let mut i = input();
        i.findings = Vec::new();
        i.capture_failures = vec![("tardis-idle".into(), "unmapped glyph U+2726".into())];
        let text = render_verdict(&i);
        assert!(text.contains("tardis-idle"));
        assert!(text.contains("U+2726"));
        assert!(text.contains("HOLD"));
    }

    #[test]
    fn suppressed_findings_appear_as_a_collapsed_previously_overruled_line() {
        let mut i = input();
        i.suppressed = finding(Lens::Design, Severity::Major);
        assert!(render_verdict(&i).contains("previously overruled (1)"));
    }

    #[test]
    fn dropped_regionless_findings_are_counted_not_hidden() {
        let mut i = input();
        i.dropped_no_region = 2;
        assert!(render_verdict(&i).contains("2 finding(s) dropped for naming no region"));
    }

    #[test]
    fn deferred_scenarios_are_named_rather_than_silently_omitted() {
        // A review that quietly covered half its scenarios reads as a
        // pass it did not earn.
        let mut i = input();
        i.deferred = vec!["smash-crabs-explosion".into()];
        let text = render_verdict(&i);
        assert!(text.contains("deferred"));
        assert!(text.contains("smash-crabs-explosion"));
    }

    #[test]
    fn a_skipped_lens_is_named_with_its_reason() {
        let mut i = input();
        i.reports.push(LensReport { scenario: "dial".into(), lens: Lens::Design, outcome: LensOutcome::Skipped(Skip::NoTasteProfile) });
        let text = render_verdict(&i);
        assert!(text.contains("design"));
        assert!(text.contains("no taste.md"));
    }

    #[test]
    fn a_held_lens_is_named_with_why_it_could_not_report() {
        let mut i = input();
        i.reports.push(LensReport { scenario: "dial".into(), lens: Lens::Motion, outcome: LensOutcome::Held("unparseable output twice".into()) });
        assert!(render_verdict(&i).contains("unparseable output twice"));
    }

    #[test]
    fn stale_rulings_are_surfaced_for_revalidation() {
        let mut i = input();
        i.stale_rulings = vec!["a1b2c3d4e5f60718".into()];
        let text = render_verdict(&i);
        assert!(text.contains("stale"));
        assert!(text.contains("a1b2c3d4e5f60718"));
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test --manifest-path capture/Cargo.toml verdict::` → FAIL.

`aggregate` implements exactly one precedence rule, and no other:

```rust
/// Aggregates the poll. Every lens reports on its own domain only, no
/// lens can clear another's, and the run carries the most severe report
/// received. A `Hold` is never upgraded to a `Go`.
pub fn aggregate(reports: &[LensReport], findings: &[MergedFinding]) -> Verdict {
    let blocked = findings
        .iter()
        .any(|m| m.finding.severity == Severity::Blocker && m.finding.lens.is_blocker_capable());
    if blocked {
        return Verdict::NoGo;
    }
    if reports.iter().any(|r| matches!(r.outcome, LensOutcome::Held(_))) {
        return Verdict::Hold;
    }
    Verdict::Go
}
```

`render_verdict` writes, in order: a header line carrying the run id and
the overall verdict in the exact words `GO` / `NO-GO` / `HOLD`; a
per-scenario, per-lens poll table (`reported` / `skipped — <reason>` /
`HOLD — <why>`); capture failures with their adapter errors; findings
sorted most-severe-first with lens, region, claim, evidence, confidence,
and `also raised by` when non-empty; then the accounting lines —
`previously overruled (N)`, `N finding(s) dropped for naming no region`,
`deferred to a later batch: <names>`, and `stale ruling(s) needing
re-validation: <fingerprints>`. `Skip` renders as `no intent declared`,
`no taste.md`, and `single-frame capture`.

Wire two new subcommands into `main.rs`:
- `plumb plan --run-dir <dir> [--taste <path>] [--cap 8]` — reads every
  `*.manifest.json` in the run dir, prints the `DispatchPlan` as JSON
  for the skill to dispatch.
- `plumb merge --run-dir <dir> --report <lens>:<scenario>:<file> ...`
  — ingests each lens's raw output through `finding::parse_findings`,
  merges, renders `verdict.md` into the run dir, and **exits with the
  verdict's exit code**.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml`
Expected: PASS, 16 new tests in `verdict::`.

- [ ] **Step 4: Commit**

```bash
git add capture/src/verdict.rs capture/src/main.rs capture/src/lib.rs
git commit -m "feat(verdict): aggregate the lens poll into GO / NO-GO / HOLD

A HOLD is never upgraded to a GO, and everything that could not be
checked — a held lens, a failed capture, a deferred batch, a dropped
regionless finding — is named in verdict.md rather than degrading to
silent success."
```

---

### Slice 2.6: The orchestrating skill

**Tags:** coding, admin

#### Task 13: `/plumb:review` and `skills/visual-review/SKILL.md`

**Files:**
- Create: `skills/visual-review/SKILL.md`
- Create: `commands/review.md`

**Interfaces:**
- Consumes: every CLI subcommand from Tasks 6 and 12 (`init`, `select`,
  `capture`, `plan`, `merge`) and the agent types from Tasks 9-10.
- Produces: `/plumb:review`, the trigger a project convention or a
  pre-PR check invokes.

**TDD exception: this is a skill definition, not code** — its
deterministic parts already have tests, and its one non-deterministic
part (subagent dispatch) is verified end-to-end in Task 14.

- [ ] **Step 1: Write `skills/visual-review/SKILL.md`**

The orchestration procedure, in this exact order, with the failure
behavior stated at each step:

1. **Build the capture binary if needed.** `cargo build --release
   --manifest-path <plugin>/capture/Cargo.toml`, cached thereafter. If
   `cargo` is absent, stop with: *"Plumb's capture binary needs a Rust
   toolchain (rustup.rs). Nothing was captured and no verdict was
   produced."* — a clear, actionable message, never a stack trace.
2. **Scaffold if absent.** No `.plumb/` → **offer** to run `plumb init`
   and stop for an answer. Never error, never scaffold silently.
3. **Select.** `git diff --name-only <merge-base>..HEAD` piped to
   `plumb select --changed -`, or `plumb select --scenario <name>` when
   one was named. **Exit 3 means stop and say so** — report that
   nothing matched, list the changed paths that matched nothing, and
   end the run. Never widen to every scenario.
4. **Capture.** `plumb capture` per selected scenario into
   `.plumb/runs/<run-id>/`. A failed capture is recorded and the other
   scenarios continue; it becomes a `HOLD` in the verdict and the run
   is not a GO.
5. **Plan the fan-out.** `plumb plan --run-dir <dir> --taste
   .plumb/taste.md`. Dispatch each batch's entries **in parallel, one
   subagent per entry**, each to the `agent` the plan names, with the
   `prompt` the plan supplies **verbatim**.
   **You must not add to that prompt.** Do not paste the diff, name the
   files that changed, mention that anything changed, say the work is
   yours, or ask the agent to confirm anything. The prompt is
   constructed to be blind; appending to it destroys the one property
   the whole tool rests on.
6. **Retry once, then HOLD.** A lens whose output does not parse as a
   JSON finding array is re-dispatched **once** with the identical
   prompt. A second failure is a `HOLD` for that lens, recorded with
   the reason. Never guess at what it meant.
7. **Merge.** `plumb merge --run-dir <dir> --report ...` writes
   `verdict.md` and exits 0/1/2.
8. **Report.** Show the verdict. On **NO-GO**, state plainly that the
   task may not be claimed complete and no PR may be opened until each
   blocker is **fixed**, **overruled** (which writes a ruling — Arc 4),
   or **deferred with a note**. On **HOLD**, name which lens could not
   report and why, and state that this is not a GO.

Plus a **Do not** section repeating, as the skill's own last word: never
review everything on no match; never omit a deferred scenario; never
upgrade a HOLD; never add to a dispatched prompt; never feed rulings to
an agent.

- [ ] **Step 2: Write `commands/review.md`**

Frontmatter `description: Capture the scenarios this change touches and
run the blinded multi-lens review, producing a GO / NO-GO / HOLD
verdict.`, accepting an optional `--scenario <name>`, whose body invokes
the `visual-review` skill.

- [ ] **Step 3: Commit**

```bash
git add skills commands
git commit -m "feat(skill): add the /plumb:review orchestrator

The skill owns only what the harness alone can do — parallel subagent
dispatch — and is instructed to pass the CLI's constructed prompts
verbatim, because appending to them is the one way to destroy the
blinding the rest of the design protects."
```

---

### Slice 2.7: First end-to-end verdict

**Tags:** coding, admin

#### Task 14: A seed scenario in TTUI and the first real run

**Files (in the TTUI repo, on a worktree branch):**
- Create: `.plumb/config.yaml`
- Create: `.plumb/scripts/omnitrix-dial-rotate.json`
- Modify: `.gitignore` (add `.plumb/runs/`)

**Interfaces:**
- Consumes: the whole of Arcs 1-2.
- Produces: the first two committed capture scripts this repository has
  ever had, and the shape Arc 6 expands.

**Deliberately one scenario, not eight.** This task exists to prove the
pipeline end-to-end, not to build the library — that is Arc 6, and its
estimate is genuinely uncertain.

- [ ] **Step 1: Write `.plumb/scripts/omnitrix-dial-rotate.json`**

```json
[
  { "wait_ms": 400 },
  { "key": "Right" },
  { "wait_ms": 250 },
  { "key": "Right" },
  { "wait_ms": 250 },
  { "key": "Enter" },
  { "wait_ms": 400 }
]
```

Every `wait_ms` is real wall-clock time — it is what actually drives an
app's `tick_rate()` animation, since nothing calls `on_tick()` directly.
7 steps yields 8 frames, so `--out` must end in `.gif`.

- [ ] **Step 2: Write `.plumb/config.yaml`**

```yaml
scenarios:
  - name: omnitrix-dial-rotate
    adapter: command
    args: >
      cargo run -p visual-snapshot --
      --example omnitrix --size 120x40
      --script .plumb/scripts/omnitrix-dial-rotate.json
      --out {out}.gif
    intent: >
      The Omnitrix dial rotates through its alien modes as the selection
      advances; the selected mode's label sits beneath the dial, and the
      dial's glow border takes the selected mode's colour. Confirming a
      selection transitions to that mode's screen.
    expects: []
    touches:
      - src/widgets/**
      - src/effects.rs
      - src/canvas.rs
      - examples/omnitrix/**
```

- [ ] **Step 3: Run the real review**

```bash
/plumb:review --scenario omnitrix-dial-rotate
```

Expected: a GIF and manifest under `.plumb/runs/<id>/`; two lens
subagents dispatched (`critic-breakage`, `critic-intent`); `design` and
`motion` present in the verdict as *skipped* — `design` only if
`.plumb/taste.md` is absent, `motion` never, since 8 frames makes it
applicable; a `verdict.md` carrying a real GO / NO-GO / HOLD.

- [ ] **Step 4: Verify the blinding by inspection, once, by hand**

Read the two dispatched prompts in the transcript. Confirm neither
contains a diff, a source path, the `cargo run` line, the `touches`
globs, or any statement that something changed. This is a one-time
manual confirmation that the unit tests in Task 8 assert the right
thing about the real dispatch path.

- [ ] **Step 5: Commit (in TTUI)**

```bash
git add .plumb .gitignore
git commit -m "feat(design): adopt Plumb with a first capture scenario

Adoption is via the command adapter pointed at the existing
tools/visual-snapshot, which is unmodified — one scenario to prove the
pipeline, with the library itself built out separately."
```

---

## Arc 3: The advisory lenses

Both are capped at `major` and can never hold a run. The applicability
gate and both prompt bodies already exist (Task 8); this Arc adds the
two agent definitions that make them dispatchable.

`design` was sequenced after `breakage` and `intent` because
`taste.md` was being authored separately and nothing was allowed to
block on it. **It now exists** — TTUI's `.plumb/taste.md` grants exactly
two exemptions (constant motion; saturation and glow), holds four
non-negotiables (legibility survives the effects, cell-grid discipline,
colour carries state, it reads as a machine), and explicitly leaves
density and ornament open to critique — so this lens is implementable
rather than speculative. The sequencing stands regardless: a consumer
without a taste profile still gets `design` skipped with a notice, and
Arc 2 delivers value without it.

### Slice 3.1: `critic-motion`

**Tags:** coding

#### Task 15: The motion lens

**Files:**
- Create: `agents/critic-motion.md`

**Interfaces:**
- Consumes: the prompt built for `Lens::Motion` (Task 8), dispatched
  only when `frame_count > 1`.
- Produces: the agent type `critic-motion`.

**TDD exception: prompt, not code** — verified by the Arc 7 corpus.

- [ ] **Step 1: Write `agents/critic-motion.md`**

Frontmatter `name: critic-motion`, `tools: Read`, description naming it
**advisory (capped at major)** and blinded. The identical Sim Sup /
"What you can see" / "No quota" / "Confidence governs voice" /
"Reporting" sections as Task 9, with `"lens": "motion"`, and this lens
section:

```markdown
## Your lens

You are looking at an animated sequence, not a still. Judge only what
the passage of frames reveals:

- **Pacing.** Does a transition read, or does it snap through a state
  too fast to perceive? Does something linger long enough to feel
  stalled?
- **Continuity.** Does anything jump discontinuously between frames —
  a panel that relocates, a value that skips, an element that vanishes
  and returns without a transition?
- **Frame-dependent legibility.** Is anything readable only in a frame
  a viewer would not pause on? Text that is legible in frame 1 and
  smeared in frames 2-8 is a finding; the reverse usually is not.
- **Dead motion.** Does a sequence that should move not move at all?

Out of bounds even when you notice it: static rendering defects, the
palette, the layout, and whether it accomplishes any stated goal. Other
lenses own those, and a defect visible in a single frame is not yours
just because you saw it in eight.

Constant motion is not itself a finding. Many interfaces animate
continuously on purpose.

## Severity

You are **advisory**: `major` is your ceiling and your findings never
hold a run. Report them plainly anyway.
```

- [ ] **Step 2: Commit**

```bash
git add agents/critic-motion.md
git commit -m "feat(agents): add the advisory motion lens"
```

---

### Slice 3.2: `critic-design`

**Tags:** coding

#### Task 16: The design lens

**Files:**
- Create: `agents/critic-design.md`

**Interfaces:**
- Consumes: the prompt built for `Lens::Design` (Task 8), which carries
  `taste.md` verbatim plus the scenario's `taste_override` when
  present. Dispatched only when a taste profile exists.
- Produces: the agent type `critic-design`.

**TDD exception: prompt, not code** — verified by the Arc 7 corpus.

- [ ] **Step 1: Write `agents/critic-design.md`**

Frontmatter `name: critic-design`, `tools: Read`, description naming it
**advisory (capped at major)** and blinded. Same shared sections, with
`"lens": "design"`, and this lens section:

```markdown
## Your lens

Your dispatched prompt contains a **taste profile**: the project's
declared aesthetic, written by the person whose project it is. It is
the standard. Where it and generic UI advice conflict, **it wins** —
without exception and without argument.

Read it for four things and judge against them:

- **Aesthetic intent** — what this interface is trying to be.
- **Non-negotiables** — breaches of these are your most serious
  findings.
- **Deliberate violations of generic UI norms** — the list of places
  where standard advice is *wrong here*. Do not raise findings on
  anything this section claims. An objection the profile has already
  answered is not a finding; it is a cost the reader must pay to argue
  you down.
- **Explicitly open to critique** — what the profile declines to claim.
  This is where your findings are most useful.

If your prompt also contains a **scenario-scoped override**, it is
additive to the profile and applies to this screen only.

Out of bounds even when you notice it: rendering defects, conformance
to any stated goal, and pacing. Other lenses own those.

The most common failure of a design lens is regressing to stock advice
— more whitespace, less density, calmer colour. If a finding you are
about to write would apply unchanged to any interface you have ever
seen, it is stock advice, and you should not write it.

## Severity

You are **advisory**: `major` is your ceiling and your findings never
hold a run. A clear breach of a stated non-negotiable is the only thing
that earns `major`.
```

- [ ] **Step 2: Run `/plumb:review --scenario omnitrix-dial-rotate` in TTUI**

Expected: all four lenses now dispatch (8 frames, declared intent,
`.plumb/taste.md` present); the design lens's findings, if any, cite
sections of the taste profile rather than generic UI heuristics. Read
the verdict and confirm it does not argue for calmer colour or less
motion — the profile exempts both, and a finding on either is a prompt
regression to fix here, not to argue down.

- [ ] **Step 3: Commit**

```bash
git add agents/critic-design.md
git commit -m "feat(agents): add the advisory design lens

A critique with no declared target regresses to stock advice — more
whitespace, calmer colour — which is actively wrong for a project whose
taste profile exempts exactly those things."
```

---

## Arc 4: Rulings and the calcification guard

### Slice 4.1: Ruling records and post-hoc suppression

**Tags:** coding

#### Task 17: Rulings, scoping, and the suppression filter

**Files:**
- Create: `capture/src/rulings.rs`
- Modify: `capture/src/lib.rs` (add `pub mod rulings;`)
- Modify: `capture/src/main.rs` (add the `rule` subcommand; wire
  suppression into `merge`)

**Interfaces:**
- Consumes: `merge::{MergedFinding, fingerprint}` (Task 11),
  `finding::Lens` (Task 7).
- Produces:
  - `pub enum Scope { Scenario, ProjectWide }`
  - `pub struct Ruling { pub fingerprint: String, pub lens: Lens, pub severity: Severity, pub scenario: String, pub region: String, pub claim: String, pub reason: String, pub date: String, pub taste_hash: String, pub scope: Scope }`
  - `pub fn taste_hash(taste: Option<&str>) -> String`
  - `pub fn load_rulings(path: &Path) -> Result<Vec<Ruling>, RulingError>`
  - `pub fn append_ruling(path: &Path, r: &Ruling) -> std::io::Result<()>`
  - `pub struct Suppression { pub kept: Vec<MergedFinding>, pub suppressed: Vec<MergedFinding>, pub stale: Vec<String> }`
  - `pub fn suppress(findings: Vec<MergedFinding>, rulings: &[Ruling], current_taste_hash: &str) -> Suppression`

  Consumed by `verdict::VerdictInput` (Task 12).

**Rulings are never passed to `prompt::build_prompt`.** `suppress` runs
*after* every lens has reported. Feeding "the user likes X" into a
prompt would bias the whole review and quietly blind it to real
regressions in that region; this way the eyes never learn to stop
seeing, and only the report learns to stop repeating itself.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Confidence, Finding, Lens, Severity};
    use crate::merge::{fingerprint, merge, MergedFinding};

    fn f(scenario: &str, region: &str, claim: &str) -> Vec<MergedFinding> {
        merge(vec![Finding {
            lens: Lens::Design,
            scenario: scenario.into(),
            severity: Severity::Major,
            region: region.into(),
            claim: claim.into(),
            evidence: "e".into(),
            confidence: Confidence::Medium,
        }])
    }

    fn ruling(scenario: &str, region: &str, claim: &str, hash: &str, scope: Scope) -> Ruling {
        Ruling {
            fingerprint: fingerprint(scenario, region, claim),
            lens: Lens::Design,
            scenario: scenario.into(),
            region: region.into(),
            claim: claim.into(),
            reason: "density is the point here".into(),
            date: "2026-08-14".into(),
            taste_hash: hash.into(),
            scope,
        }
    }

    #[test]
    fn a_matching_ruling_suppresses_its_finding() {
        let s = suppress(f("dial", "left column", "too dense"), &[ruling("dial", "left column", "too dense", "H", Scope::Scenario)], "H");
        assert!(s.kept.is_empty());
        assert_eq!(s.suppressed.len(), 1);
    }

    #[test]
    fn suppression_is_scoped_to_its_scenario_by_default() {
        // Overruling one screen's density must not mute density everywhere.
        let s = suppress(f("tardis", "left column", "too dense"), &[ruling("dial", "left column", "too dense", "H", Scope::Scenario)], "H");
        assert_eq!(s.kept.len(), 1, "another scenario's ruling must not apply");
    }

    #[test]
    fn project_wide_scope_is_opt_in_and_crosses_scenarios() {
        let r = ruling("dial", "left column", "too dense", "H", Scope::ProjectWide);
        let s = suppress(f("tardis", "left column", "too dense"), &[r], "H");
        assert_eq!(s.suppressed.len(), 1);
    }

    #[test]
    fn a_ruling_made_under_a_different_taste_hash_is_stale_and_does_not_suppress() {
        // Your aesthetic moving is precisely when old rejections stop
        // being valid, so a stale ruling surfaces rather than applying.
        let s = suppress(f("dial", "left column", "too dense"), &[ruling("dial", "left column", "too dense", "OLD", Scope::Scenario)], "NEW");
        assert_eq!(s.kept.len(), 1, "the finding reappears");
        assert!(s.suppressed.is_empty());
        assert_eq!(s.stale.len(), 1);
    }

    #[test]
    fn a_non_matching_ruling_leaves_a_finding_alone() {
        let s = suppress(f("dial", "left column", "too dense"), &[ruling("dial", "right column", "too dense", "H", Scope::Scenario)], "H");
        assert_eq!(s.kept.len(), 1);
    }

    #[test]
    fn a_ruling_applies_across_lenses_because_the_fingerprint_excludes_the_lens() {
        let mut findings = f("dial", "left column", "too dense");
        findings[0].finding.lens = Lens::Motion;
        let s = suppress(findings, &[ruling("dial", "left column", "too dense", "H", Scope::Scenario)], "H");
        assert_eq!(s.suppressed.len(), 1);
    }

    #[test]
    fn an_overruled_cosmetic_finding_does_not_silence_a_blocker_in_the_same_region() {
        let mut findings = f("dial", "left column", "the label overlaps the frame");
        findings[0].finding.lens = Lens::Breakage;
        findings[0].finding.severity = Severity::Blocker;
        // The ruling was made against a `minor` design finding — same
        // scenario, region and claim, so the fingerprint matches.
        let r = ruling_with_severity("dial", "left column", "the label overlaps the frame", Severity::Minor, "H", Scope::Scenario);
        let s = suppress(findings, &[r], "H");
        assert_eq!(s.suppressed.len(), 0, "a waived cosmetic complaint must not suppress a blocker");
        assert_eq!(s.kept.len(), 1);
    }

    #[test]
    fn a_ruling_suppresses_at_or_below_its_own_severity() {
        let mut findings = f("dial", "left column", "too dense");
        findings[0].finding.severity = Severity::Nit;
        let r = ruling_with_severity("dial", "left column", "too dense", Severity::Minor, "H", Scope::Scenario);
        let s = suppress(findings, &[r], "H");
        assert_eq!(s.suppressed.len(), 1);
    }

    #[test]
    fn taste_hash_is_stable_and_distinguishes_an_edit() {
        assert_eq!(taste_hash(Some("abc")), taste_hash(Some("abc")));
        assert_ne!(taste_hash(Some("abc")), taste_hash(Some("abd")));
    }

    #[test]
    fn an_absent_taste_profile_hashes_to_a_stable_sentinel() {
        assert_eq!(taste_hash(None), taste_hash(None));
        assert_ne!(taste_hash(None), taste_hash(Some("")));
    }

    #[test]
    fn rulings_round_trip_through_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rulings.jsonl");
        append_ruling(&path, &ruling("a", "r", "c", "H", Scope::Scenario)).unwrap();
        append_ruling(&path, &ruling("b", "r", "c", "H", Scope::ProjectWide)).unwrap();
        let back = load_rulings(&path).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].scope, Scope::ProjectWide);
    }

    #[test]
    fn a_missing_rulings_file_loads_as_an_empty_list() {
        assert!(load_rulings(std::path::Path::new("/does/not/exist.jsonl")).unwrap().is_empty());
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test --manifest-path capture/Cargo.toml rulings::` → FAIL.

`taste_hash` is `sha256` hex-truncated to 16, over the file's contents,
with a fixed sentinel string for `None`. `suppress` partitions by
fingerprint match, scope, **and a severity ceiling**: a
`Scenario`-scoped ruling matches only its own scenario, `ProjectWide`
matches any, and in both cases a ruling suppresses only findings whose
severity is at or below the severity of the finding that was
originally overruled.

The severity ceiling closes a hole the lens-free fingerprint would
otherwise open. Because the fingerprint deliberately excludes the lens,
a ruling made against an *advisory* finding ("the mode label overlaps
the frame corner — looks sloppy", `minor`, from `design`) would
otherwise suppress a *blocker-capable* finding matching the same
scenario, region and claim ("the mode label overlaps the frame corner —
cells collide and the text is unreadable", `blocker`, from
`breakage`). Silencing a real defect because a cosmetic complaint about
the same spot was once waved off is exactly the failure a reviewer must
not have. Overruling a cosmetic finding waives the cosmetic finding and
nothing more; overruling a `blocker` still waives everything matching.

A ruling whose
`taste_hash` differs from the current one **does not suppress** — its
fingerprint is collected into `stale` and the finding is kept, so a
moved aesthetic re-raises what it silenced instead of applying an old
rejection forever.

Wire `plumb rule --run-dir <dir> --fingerprint <fp> --reason <text>
[--scope project-wide]` into `main.rs`: it reads the finding out of the
run's merged output, writes a `Ruling` to `.plumb/rulings.jsonl`, and
prints a confirmation. Wire `suppress` into `plumb merge` so
`VerdictInput`'s `suppressed` and `stale_rulings` are populated.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test --manifest-path capture/Cargo.toml rulings::`
Expected: PASS, 10 tests.

- [ ] **Step 4: Verify the ruling round-trip by hand, in TTUI**

Per the spec's Verification section: overrule a real finding, re-run
`/plumb:review`, confirm it is suppressed and appears in the
`previously overruled (1)` line; then edit `.plumb/taste.md` and
confirm the ruling is surfaced as stale and the finding returns.

- [ ] **Step 5: Commit**

```bash
git add capture/src/rulings.rs capture/src/main.rs capture/src/lib.rs
git commit -m "feat(rulings): suppress overruled findings after the fact, never before

Rulings never reach a prompt. Feeding 'the user likes X' into the
review would bias it and blind it to real regressions in that region;
applied post hoc, the eyes stay uncontaminated and only the report
learns to stop repeating itself. A ruling made under a different
taste.md hash surfaces as stale rather than applying forever."
```

---

## Arc 5: The `pty` adapter — extracting `visual-snapshot`

**Extraction, not invention.** The hard parts — ConPTY behavior, script
semantics, quiescence detection, frame-count-to-extension validation —
are already proven in `tools/visual-snapshot` and running in TTUI's CI
on every PR. This Arc moves them, generalizes one argument, and adds
one behavior.

**This Arc is deliberately *after* the first working version.** TTUI
adopts Plumb through the `command` adapter pointed at its existing
tool, so nothing in Arcs 1-4 depends on this. The `pty` adapter is what
makes Plumb useful to a project that does *not* already own a capture
tool — real value, but not on the critical path, and sequenced
accordingly.

### Slice 5.1: Port the pure modules

**Tags:** coding

#### Task 18: Port `script`, `keys`, `color`, `glyph`, `render`, `encode`

**Files:**
- Create: `capture/src/{script,keys,color,glyph,render,encode}.rs`
- Modify: `capture/src/lib.rs` (declare all six)
- Modify: `capture/Cargo.toml` (add `portable-pty = "0.9"`, `vt100 = "0.15"`, `font8x8 = "0.3"`)

**Interfaces:**
- Consumes: nothing new.
- Produces, unchanged from `tools/visual-snapshot`:
  - `script::{Step, ScriptError, parse_script}` — `Step` is
    `Wait { wait_ms: u64 } | Key { key: String } | Click { x: u16, y: u16 }`, `#[serde(untagged)]`.
  - `keys::{KeyEncodeError, encode_key, encode_click}`
  - `color::` the `vt100::Color -> image::Rgb<u8>` mapping plus its
    brighten/swap helpers.
  - `glyph::{GlyphError, glyph_for}` — including the algorithmic
    Braille Patterns renderer for U+2800-U+28FF, whose bit-to-dot
    layout mirrors TTUI's `src/canvas.rs` `blit_braille` exactly.
  - `render::{RenderError, render_screen}`
  - `encode::{EncodeError, write_png, png_bytes, write_gif}`

  Consumed by `adapter::pty` (Task 19).

**Port the tests with the code, unchanged.** All six modules ship with
inline `#[cfg(test)] mod tests` in the source repo; they are the
regression net for the port and must go across verbatim, including
`glyph.rs`'s `dingbat_star_is_unmapped` and the four Braille-layout
assertions.

**Do not port `judge.rs`.** The local Ollama vision judge stays in
`tools/visual-snapshot`, unmodified. It is a cheap offline inner-loop
sanity check that costs nothing; Plumb is the outer-loop reviewer that
carries authority. The two are not in conflict, and this plan neither
depends on nor removes it.

- [ ] **Step 1: Copy each module and its tests, one commit per module**

For each of the six: copy the file verbatim, adjust `use` paths to the
new crate, add `pub mod <name>;` to `lib.rs`, run
`cargo test --manifest-path capture/Cargo.toml <name>::`, confirm the
same test count passes as in TTUI, and commit
`chore(capture): port <name> from visual-snapshot`.

- [ ] **Step 2: Verify the whole suite**

Run: `cargo test --manifest-path capture/Cargo.toml`
Expected: every ported test passes with no modification to its
assertions. Any assertion that needed changing is a port bug — fix the
port, not the test.

---

### Slice 5.2: Generalize the spawn path

**Tags:** coding

#### Task 19: Port `pty.rs`, replacing `--example` with an arbitrary command

**Files:**
- Create: `capture/src/adapter/pty.rs` (replacing Task 5's placeholder)
- Create: `capture/examples/echo_key.rs`
- Test: `capture/tests/pty_roundtrip.rs`

**Interfaces:**
- Consumes: `script::Step`, `keys::*`, `render::render_screen`,
  `encode::{write_png, write_gif}` (Task 18).
- Produces:
  - `pub struct Session` with `spawn`, `send`, `capture_frame`, `capture_frame_after_key`, `kill`, and its `Drop` guard
  - `pub const POLL_INTERVAL: Duration` and `pub const MAX_SETTLE_WAIT: Duration`
  - `pub fn run_script(command: &[String], rows: u16, cols: u16, steps: &[Step], glyph_mode: GlyphMode) -> Result<CaptureFrames, PtyError>`
  - `pub struct CaptureFrames { pub frames: Vec<(image::RgbaImage, Duration)>, pub substitutions: Vec<(char, usize)> }`

  Consumed by `adapter::capture` (Task 21).

**The one generalization.** `visual-snapshot`'s `build_example(name)` —
which shells out to `cargo build --example <name> --manifest-path
<ttui root>` and resolves `target/debug/examples/<name>` — **does not
come across**. It is exactly the weld to TTUI this whole project exists
to remove. `run_script` takes an argv the scenario declares, spawned
directly. Everything else about the module ports unchanged: the
background reader thread draining into a shared buffer, the DSR
(`ESC[6n`) query detection with its 3-byte carry across `read()`
boundaries, the first-capture-vs-subsequent quiescence strategies, and
the `MAX_SETTLE_WAIT` safety valve.

**`GlyphMode` is introduced here, used fully in Task 20.** Add
`pub enum GlyphMode { Error, Substitute }` with
`impl Default for GlyphMode { fn default() -> Self { GlyphMode::Error } }`
to `glyph.rs` as part of this task, and thread it through
`run_script` — Task 19 passes it through unused beyond selecting the
existing hard-error path, and Task 20 gives `Substitute` its behavior.
Threading it now avoids changing `run_script`'s signature twice.

Port `MAX_SETTLE_WAIT`'s doc comment with the constant — including its
honest statement that 2000ms was calibrated against one dev
environment's measured ~1.9s worst-case first draw rather than derived
formally, and should be retuned if it proves insufficient or excessive.

- [ ] **Step 1: Port `examples/echo_key.rs` and `tests/pty_roundtrip.rs`**

The fixture binary and its integration tests come across as-is, with
the spawn call updated to pass an argv instead of an example name. These
tests are the proof the port did not break ConPTY behavior.

- [ ] **Step 2: Write the failing test for the generalized spawn**

```rust
// capture/tests/pty_roundtrip.rs
#[test]
fn run_script_spawns_an_arbitrary_command_not_a_cargo_example() {
    // The whole point of the extraction: no cargo, no --example, no
    // assumption about where a binary lives.
    let exe = env!("CARGO_BIN_EXE_echo_key");
    let out = parallax_plumb::adapter::pty::run_script(
        &[exe.to_string()],
        24,
        80,
        &[parallax_plumb::script::Step::Key { key: "Right".into() }],
        parallax_plumb::glyph::GlyphMode::Error,
    )
    .unwrap();
    assert_eq!(out.frames.len(), 2, "an initial frame plus one per step");
}
```

- [ ] **Step 3: Implement, run, commit**

Run: `cargo test --manifest-path capture/Cargo.toml --test pty_roundtrip`
Expected: PASS, the ported tests plus the new one.

```bash
git commit -m "feat(capture): port the pty session, spawning an arbitrary command

Dropping build_example is the whole extraction: cargo build --example
against a hardcoded TTUI manifest path was the weld to one repo that
made this tool untravelable."
```

---

### Slice 5.3: Unmapped glyphs — turning a hard stop into a disclosed caveat

**Tags:** coding

#### Task 20: `--on-unmapped-glyph {error,substitute}`

**Files:**
- Modify: `capture/src/glyph.rs`
- Modify: `capture/src/render.rs`
- Modify: `capture/src/adapter/pty.rs`
- Modify: `capture/src/config.rs` (per-scenario `on_unmapped_glyph`)

**Interfaces:**
- Consumes: `glyph::{GlyphError, glyph_for}` (Task 18),
  `manifest::Caveat` (Task 4).
- Produces:
  - `pub fn glyph_for_mode(ch: char, mode: GlyphMode) -> Result<[u8; 8], GlyphError>`
  - `pub const PLACEHOLDER_BOX: [u8; 8]`
  - `pub struct RenderedScreen { pub image: image::RgbaImage, pub substitutions: std::collections::HashMap<char, usize> }`
  - `render::render_screen(screen: &vt100::Screen, mode: GlyphMode) -> Result<RenderedScreen, RenderError>` — replacing Task 18's ported single-argument, image-only signature.

  (`GlyphMode` itself lands in Task 19, so `run_script`'s signature is
  written once rather than changed twice.)

  Consumed by `adapter::pty` (Task 19), which turns the counts into
  `Caveat::UnmappedGlyphSubstituted` entries on the manifest.

**Why this exists.** The known realistic failure is already documented
in TTUI: the rasterizer hard-errors on unmapped glyphs — `✦` (U+2726,
`EnergyCore`'s charged-state dingbat and `launcher`'s starfield),
`launcher`'s geometric/arrow decorations, `tardis`'s em dash, and
`smash_crabs`'s `💥` — which turns an entire scenario into a
non-result. `substitute` turns a hard stop into a reviewable frame with
a stated blind spot.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn error_mode_still_hard_errors_on_an_unmapped_codepoint() {
    // The existing default is preserved verbatim for anyone who wants it.
    assert_eq!(
        glyph_for_mode('\u{2726}', GlyphMode::Error).unwrap_err(),
        GlyphError::Unmapped('\u{2726}')
    );
}

#[test]
fn substitute_mode_returns_a_visible_placeholder_box() {
    let b = glyph_for_mode('\u{2726}', GlyphMode::Substitute).unwrap();
    assert_eq!(b, PLACEHOLDER_BOX);
    assert!(b.iter().any(|r| *r != 0), "a placeholder must be visible");
    assert_ne!(b, glyph_for('A').unwrap(), "and distinguishable from real content");
}

#[test]
fn substitute_mode_does_not_disturb_a_mapped_codepoint() {
    assert_eq!(glyph_for_mode('A', GlyphMode::Substitute).unwrap(), glyph_for('A').unwrap());
}

#[test]
fn error_is_the_default_mode() {
    assert_eq!(GlyphMode::default(), GlyphMode::Error);
}

#[test]
fn substitutions_are_counted_per_codepoint_for_the_manifest() {
    // Two cells of U+2726 and one of U+1F4A5 must disclose as such.
    let mut parser = vt100::Parser::new(4, 10, 0);
    parser.process("\u{2726}\u{2726}\u{1F4A5}".as_bytes());
    let counts = render_screen(parser.screen(), GlyphMode::Substitute)
        .unwrap()
        .substitutions;
    assert_eq!(counts.get(&'\u{2726}'), Some(&2));
    assert_eq!(counts.get(&'\u{1F4A5}'), Some(&1));
}
```

- [ ] **Step 2: Implement, then verify the caveat reaches the lenses**

`PLACEHOLDER_BOX` is a hollow 8x8 rectangle (`[0xFF, 0x81, 0x81, 0x81,
0x81, 0x81, 0x81, 0xFF]`) — visible, obviously not text, and not
confusable with any box-drawing glyph the rasterizer already renders.

`adapter::pty` converts the counts into
`Caveat::UnmappedGlyphSubstituted { codepoint: "U+2726", count: 2 }`
entries. Task 8's `a_disclosed_caveat_reaches_every_lens` test already
asserts those reach every lens prompt as *do not judge these cells* —
run it and confirm it still passes against a real substituted capture.

- [ ] **Step 3: Commit**

```bash
git add capture/src
git commit -m "feat(glyph): add substitute mode, disclosing placeholders as a caveat

An unmapped codepoint currently turns an entire scenario into a
non-result. In substitute mode it becomes a reviewable frame with a
stated blind spot the lens agents are told not to judge. error stays
the default, preserving visual-snapshot's behavior exactly."
```

---

#### Task 21: Wire the `pty` adapter into the contract

**Files:**
- Modify: `capture/src/adapter/mod.rs` (replace the `NotImplemented`
  arm for `AdapterKind::Pty`)
- Modify: `capture/src/config.rs` (`Scenario` gains three `pty`-only
  fields: `pub size: Option<String>`, `pub script: Option<PathBuf>`,
  and `pub on_unmapped_glyph: GlyphMode`, each `#[serde(default)]`.
  Because Task 2 derived `Default` on `Scenario` and every test helper
  uses `..Default::default()`, no earlier task's tests change.)

**Interfaces:**
- Consumes: `adapter::pty::run_script` (Task 19), `glyph::GlyphMode`
  (Task 20).
- Produces: `capture()` returning a real `RunManifest` for
  `AdapterKind::Pty`, with `size` populated (the `command` adapter
  leaves it `None`) and `caveats` filled from the substitution counts.

- [ ] **Step 1: Write the failing test**

A `pty` scenario's `args` is the command line to spawn; `size` and
`script` are their own fields rather than being parsed out of `args`,
because the adapter — not the spawned program — owns the pseudo-console
geometry and the driving script:

```yaml
  - name: fixture
    adapter: pty
    args: ./target/debug/examples/echo_key
    size: 80x24
    script: .plumb/scripts/fixture.json
    on_unmapped_glyph: error      # or: substitute
    touches: ['src/**']
```

```rust
/// Builds a `pty` scenario whose spawned command is `exe`, writing its
/// script to a temp path so the test owns both ends.
fn pty_scenario(exe: &str, size: &str, steps: &[Step]) -> Scenario {
    let script = std::env::temp_dir().join("plumb-fixture-script.json");
    std::fs::write(&script, serde_json::to_string(steps).unwrap()).unwrap();
    Scenario {
        name: "fixture".into(),
        adapter: AdapterKind::Pty,
        args: exe.into(),
        size: Some(size.into()),
        script: Some(script),
        on_unmapped_glyph: GlyphMode::Error,
        touches: vec!["src/**".into()],
        ..Default::default()
    }
}

#[test]
fn a_pty_scenario_captures_and_reports_its_terminal_size() {
    let dir = tempfile::tempdir().unwrap();
    let s = pty_scenario(env!("CARGO_BIN_EXE_echo_key"), "80x24", &[]);
    let m = capture(&s, dir.path(), "r").unwrap();
    assert_eq!(m.adapter, "pty");
    assert_eq!(m.size.as_deref(), Some("80x24"));
    assert_eq!(m.frame_count, 1, "a zero-step script yields exactly one frame");
    assert!(dir.path().join("fixture.png").exists());
}

#[test]
fn a_multi_step_pty_script_writes_a_gif() {
    let dir = tempfile::tempdir().unwrap();
    let s = pty_scenario(env!("CARGO_BIN_EXE_echo_key"), "80x24", &[Step::Key { key: "Right".into() }]);
    let m = capture(&s, dir.path(), "r").unwrap();
    assert_eq!(m.frame_count, 2);
    assert!(dir.path().join("fixture.gif").exists());
}
```

Extension selection is by frame count, exactly as `visual-snapshot`
does it: one frame is a `.png`, two or more is a `.gif`, and there is
no third case. The `pty` adapter chooses the extension itself rather
than validating a caller's, since it owns the output path.

- [ ] **Step 2: Implement, run the full suite, and commit**

Run: `cargo test --manifest-path capture/Cargo.toml`

```bash
git commit -m "feat(adapter): implement the pty adapter behind the capture contract"
```

---

## Arc 6: TTUI adoption and the scenario library

**Executes in the TTUI repo**, on a worktree branch, landing through
TTUI's normal Gated PR flow.

### Estimate confidence: low. Read this before scheduling it.

Every other Arc in this plan is sized against something that exists.
This one is not, and the honest statement is that **its size is
unknown**:

- **There is no prior art in this repository.** `tools/visual-snapshot`
  ships a script *parser* (`src/script.rs`) and has been used
  repeatedly, but **zero script JSON files are committed anywhere in
  the repo** — verified. Every visual-snapshot run to date used a
  throwaway script written for that moment and discarded. Nobody has
  ever authored a durable capture scenario here, so there is no
  measured cost to extrapolate from.
- **Authoring a good scenario is not mechanical.** It means driving an
  app to an interesting state through keys and real-time waits, with no
  feedback loop except capturing and looking, then writing an `intent`
  paragraph precise enough for a blinded reviewer to check against.
  Getting timing right against a `tick_rate()`-driven animation is
  iterative by nature.
- **Known blockers are unevenly distributed.** `smash_crabs`'s `💥` and
  `launcher`'s decorative glyphs will hard-error until Arc 5's
  `substitute` mode is available, so those two examples are gated on
  Arc 5 in a way the others are not.
- **The count is roughly eight and probably more.** Eight example
  binaries exist (`launcher`, `omnitrix`, `tardis`, `smash_crabs`,
  `falcon`, `mission_control`, `control_panel`, `demo`), but several
  are multi-screen — `omnitrix` alone has boot, faceplate, brainstorm,
  fasttrack, and upgrade screens. One scenario per example is a floor,
  not a ceiling.

**Therefore: no total estimate is given, and this Arc is deliberately
open-ended.** Slice 6.1 authors three scenarios and *measures* what
they cost. Slice 6.2 is a checkpoint that turns that measurement into a
rate. Slice 6.3 expands at that rate, one example per commit, and can
stop at any point with the library in a coherent state — a partial
library is genuinely useful, because a scenario that exists is reviewed
and one that does not simply is not selected.

**Do not attempt to author all eight in one task.** That is the failure
mode this Arc is structured to prevent.

### Slice 6.1: Three scenarios, and measure what they cost

**Tags:** coding, research

#### Task 22: Author two more scenarios and record the actual cost

**Files:**
- Modify: `.plumb/config.yaml`
- Create: `.plumb/scripts/tardis-console-idle.json`
- Create: `.plumb/scripts/falcon-glitch-burst.json`
- Create: `.plumb/SCENARIOS.md`

**Interfaces:**
- Consumes: `omnitrix-dial-rotate` (Task 14) as the worked example.
- Produces: three working scenarios and a recorded per-scenario cost
  Slice 6.2 consumes.

**These two specifically, and not two others.** `tardis-console-idle`
is a pure-animation capture with no input — the simplest possible
scenario, and the floor of the cost range. `falcon-glitch-burst` is the
**first scenario to declare `expects: [visual-corruption]`**, so it is
the real-world exercise of the intentional-distortion path that Task 2
and Task 8 only unit-test. Between them they bracket the range.

- [ ] **Step 1: Author `tardis-console-idle`**

```json
[
  { "wait_ms": 500 },
  { "wait_ms": 500 },
  { "wait_ms": 500 },
  { "wait_ms": 500 }
]
```

Four waits, no keys: 5 frames of the idle console's rotor animation.

```yaml
  - name: tardis-console-idle
    adapter: command
    args: >
      cargo run -p visual-snapshot --
      --example tardis --size 120x40
      --script .plumb/scripts/tardis-console-idle.json
      --out {out}.gif
    intent: >
      The TARDIS console sits idle. The time rotor animates continuously
      in the centre panel, the artron energy gauge holds a steady
      reading, and the surrounding instrument panels stay legible
      throughout.
    expects: []
    touches:
      - src/widgets/**
      - src/canvas.rs
      - examples/tardis/**
```

- [ ] **Step 2: Author `falcon-glitch-burst` with its distortion declared**

Drive Falcon to its percussive-maintenance state, then capture through
the corruption burst. The declaration is the point:

```yaml
  - name: falcon-glitch-burst
    adapter: command
    args: >
      cargo run -p visual-snapshot --
      --example falcon --size 120x40
      --script .plumb/scripts/falcon-glitch-burst.json
      --out {out}.gif
    intent: >
      Percussive maintenance triggers a corruption burst across the
      cockpit display; the instrument panel readings remain legible
      through it, and the display settles back to a clean state.
    expects:
      - visual-corruption
    touches:
      - src/glitch.rs
      - src/effects.rs
      - examples/falcon/**
```

- [ ] **Step 3: Verify the exemption works in both directions**

Run `/plumb:review --scenario falcon-glitch-burst`. Expected: the
breakage lens does **not** raise findings on the deliberate garbling.

Then temporarily remove `expects: [visual-corruption]` from the
scenario, re-run, and confirm the breakage lens **does** raise them.
Restore the declaration. This is the one place the declared/undeclared
pair is exercised against a real image rather than a unit test, and it
is the specific thing that would otherwise turn the project's most
distinctive effect into a NO-GO on every run.

- [ ] **Step 4: Record the actual cost in `.plumb/SCENARIOS.md`**

For each of the three scenarios so far: attempts needed to get the
timing right, whether the glyph rasterizer blocked it, how long the
`intent` paragraph took to write to a standard a blinded lens could
check, and total elapsed time. Plain honest numbers — this file exists
to be read by Slice 6.2 and by whoever schedules the rest.

- [ ] **Step 5: Commit**

```bash
git add .plumb
git commit -m "feat(design): add two more capture scenarios and record their cost

falcon-glitch-burst is the first scenario to declare visual-corruption,
exercising the intentional-distortion exemption against a real image
rather than a unit test."
```

---

### Slice 6.2: Calibration checkpoint

**Tags:** research, admin

#### Task 23: Turn three measurements into a rate, and decide the expansion

**Files:**
- Modify: `.plumb/SCENARIOS.md`

**Interfaces:**
- Consumes: the three recorded costs from Task 22.
- Produces: a stated per-scenario cost and an explicit decision about
  Slice 6.3's scope. **This is a decision point, not a formality** —
  it is allowed to conclude that the library stops at three.

- [ ] **Step 1: Write the calibration section**

Answer, in `.plumb/SCENARIOS.md`, with the numbers actually recorded:

1. **What did a scenario cost?** Give a range, not an average — the
   idle capture and the input-driven one will differ.
2. **What dominated?** Timing iteration, intent authoring, or glyph
   blockers. Each implies a different fix.
3. **Did any lens produce a useless verdict?** A scenario whose intent
   was too vague to check is a scenario-authoring lesson, and the
   lesson belongs in this file where the next author will read it.
4. **Is the remaining set worth its cost?** Multiply the range by the
   examples left. Then decide explicitly: expand to all, expand to a
   named subset, or stop.

- [ ] **Step 2: Write the house style for a scenario**

Whatever Task 22 taught, written down as a short checklist the next
author follows: how long a `wait_ms` needs to be to catch a
`tick_rate()` animation mid-cycle, how specific an `intent` must be
before the intent lens can do anything with it, and when to declare
`expects`.

- [ ] **Step 3: Commit**

```bash
git add .plumb/SCENARIOS.md
git commit -m "docs(design): calibrate scenario-authoring cost and set the expansion scope"
```

---

### Slice 6.3: Expand the library, one example per commit

**Tags:** coding

#### Task 24: Add a scenario per remaining example, incrementally

**Files:**
- Modify: `.plumb/config.yaml` (one scenario per commit)
- Create: `.plumb/scripts/<scenario>.json` (one per commit)

**Interfaces:**
- Consumes: the house style from Task 23.
- Produces: coverage across the example set.

**One example per commit, in this order, and stop whenever Task 23's
decision says to.** The order front-loads what is cheap and unblocked:

1. `mission_control` — a telemetry console; `BarChart`/`Sparkline`
   coverage, no known glyph blockers.
2. `control_panel` — the only example with real mouse support, so its
   script uses `{"x": N, "y": N}` click steps. It is the one scenario
   that covers the click path at all.
3. `demo` — the core-framework dashboard: `Text`/`List`/`Table`/`Block`
   and `Tab` focus switching. Cheapest scenario in the set and the one
   that guards the widest widget surface.
4. `omnitrix` second screen (boot or upgrade) — the first proof that
   one example warrants more than one scenario.
5. `launcher` — **gated on Arc 5.** Its portal/nexus decorative
   geometry and starfield `✦` hard-error until `substitute` mode
   exists. Declare `on_unmapped_glyph: substitute` when it lands.
6. `smash_crabs` — **gated on Arc 5**, same reason (`💥`).

For each: write the script, capture it, `Read` the result, iterate the
timing until the frame is worth reviewing, write the `intent`, declare
`expects` only if the app deliberately distorts, run
`/plumb:review --scenario <name>`, confirm the verdict is sensible, and
commit alone.

- [ ] **Step 1: Repeat the per-scenario cycle above for each entry**

- [ ] **Step 2: Update `.plumb/SCENARIOS.md`'s coverage table after each**

Name each example and whether it has a scenario. **An honest gap list
is the point** — a reader must be able to tell at a glance that
`smash_crabs` is uncovered, because an uncovered example is silently
never selected and never reviewed.

- [ ] **Step 3: Commit each scenario separately**

```bash
git commit -m "feat(design): add the <name> capture scenario"
```

---

### Slice 6.4: Make the convention official

**Tags:** admin

#### Task 25: Record Plumb in TTUI's development conventions

**Files:**
- Modify: `.claude/rules/development-conventions.md` ("Visual review")
- Modify: `docs/design/README.md` (if the `plumb/` Arc line needs the
  plan reference)

**Interfaces:**
- Consumes: the working `/plumb:review` and the scenario library.
- Produces: the convention that makes Plumb part of TTUI's process.

**Additive only.** The existing mandate — run `tools/visual-snapshot`
and `Read` the PNG/GIF before approving a rendering-affecting change —
**stays exactly as written**. Plumb does not replace it, and this plan
does not touch `tools/visual-snapshot`.

- [ ] **Step 1: Append to the "Visual review" section**

State: that `/plumb:review` is available and runs the blinded
multi-lens review over the scenarios a change touches; that its verdict
vocabulary is GO / NO-GO / HOLD; that a **NO-GO means the task may not
be claimed complete and no PR may be opened** until each blocker is
fixed, overruled (which writes a ruling), or deferred with a note; that
a **HOLD is not a GO** and names which lens could not report; that the
gate is convention-enforced inside the harness and human-overridable,
**not a required status check** — it maps onto the existing
Direct/Gated/Human autonomy tiers without inventing a fourth, with a
clean or advisory-only verdict leaving Gated work gated on its usual
four checks and an unresolved blocker holding the work until you fix or
overrule it; and that the reviewed run directory goes in the PR
template's existing freeform Verification section, the same pattern
already used for real-TTY test results and visual-snapshot captures.

- [ ] **Step 2: Commit**

```bash
git add .claude/rules/development-conventions.md docs/design/README.md
git commit -m "docs(design): make /plumb:review part of the visual-review convention

Additive: the existing run-visual-snapshot-and-Read-the-image mandate
is unchanged, and the Plumb gate is harness-level and human-overridable
rather than a required status check."
```

---

## Arc 7: The reviewer regression corpus

**Executes in the `plumb` repo.** This is what allows tuning agent
prompts against evidence rather than vibes, and it is what catches a
prompt regression when a lens definition is edited.

### Slice 7.1: Fixtures with known ground truth

**Tags:** coding

#### Task 26: Build the corpus

**Files:**
- Create: `capture/corpus/*.png`, `capture/corpus/*.gif`
- Create: `capture/corpus/ground-truth.json`
- Create: `capture/tests/corpus.rs`

**Interfaces:**
- Consumes: the four agent definitions and `prompt::build_prompt`.
- Produces: `cargo test --manifest-path capture/Cargo.toml --test
  corpus -- --ignored`, a **threshold suite** run on demand.

**Model output is non-deterministic, so this is an N-of-M threshold
suite, not a hard gate** — `#[ignore]`d and run on demand, the same
posture this project already takes toward real-TTY tests and
real-external-service calls. `cargo test`'s default exclusion of
`#[ignore]`d tests already makes CI do the right thing with no workflow
change.

- [ ] **Step 1: Build the fixture set**

Each fixture is a rasterized terminal frame generated by the `pty`
adapter against a small purpose-built fixture binary, so they are
reproducible rather than hand-drawn:

| Fixture | Ground truth |
|---|---|
| `garbled-glyphs.png` | breakage: **must** flag |
| `overlapping-panels.png` | breakage: **must** flag |
| `clipped-content.png` | breakage: **must** flag |
| `black-frame.png` | breakage: **must** flag |
| `one-cell-misalignment.png` | breakage: **must** flag |
| `unreadable-contrast.png` | breakage: **must** flag |
| `clean-dashboard.png` | breakage: **must** pass clean |
| `clean-dense-panel.png` | breakage: **must** pass clean (density is not damage) |
| `declared-corruption.png` | breakage with `expects: [visual-corruption]`: **must** pass clean |
| `declared-corruption.png` | breakage with **no** `expects`: **must** flag |
| `corruption-destroys-reading.png` | breakage with `expects: [visual-corruption]`: **must** flag anyway (legibility bound) |
| `intent-satisfied.png` + intent | intent: **must** pass clean |
| `intent-violated.png` + same intent | intent: **must** flag |

The three `declared-corruption` rows are the important ones: the same
image must produce **opposite** verdicts depending only on the
declaration, and a third image must be flagged *despite* the
declaration because it permanently destroys a reading. Those three are
what prove the exemption is a category exemption bounded by legibility,
and not a blanket silencer.

- [ ] **Step 2: Write `ground-truth.json`**

```json
[
  {
    "fixture": "declared-corruption.png",
    "lens": "breakage",
    "expects": ["visual-corruption"],
    "must_flag": false,
    "note": "declared distortion is the feature, not a defect"
  },
  {
    "fixture": "declared-corruption.png",
    "lens": "breakage",
    "expects": [],
    "must_flag": true,
    "note": "identical image, undeclared: garbling is a defect"
  }
]
```

- [ ] **Step 3: Write the threshold harness**

`capture/tests/corpus.rs`, `#[ignore]`d, iterating the ground truth,
dispatching each lens against each fixture through the real prompt
builder, and asserting the aggregate: **at least 5 of 6 must-flag
fixtures flagged, and at least 5 of 6 must-pass fixtures clean**, with
the three distortion rows required to pass **all** of their trials —
that pair is the behavior most likely to regress silently when a prompt
is edited, so it does not get the threshold's slack.

- [ ] **Step 4: Run it and record the baseline**

Run: `cargo test --manifest-path capture/Cargo.toml --test corpus -- --ignored`
Record the pass rate per fixture in `capture/corpus/README.md` as the
baseline any future prompt edit is compared against.

- [ ] **Step 5: Commit**

```bash
git add capture/corpus capture/tests/corpus.rs
git commit -m "test(corpus): add the reviewer regression threshold suite

Model output is non-deterministic, so this is an N-of-M suite run on
demand rather than a hard gate. The declared/undeclared corruption pair
is exempt from the threshold's slack: the same image must produce
opposite verdicts on the declaration alone, and that is the behavior
most likely to regress silently when a prompt is edited."
```

---

## Spec coverage

Every section of
`docs/design/specs/plumb/2026-08-14-plumb-design.md`, and where it
lands:

| Spec section | Tasks |
|---|---|
| Overview / plugin layout | 1 |
| Capture adapters — `command` | 5, 6 |
| Capture adapters — `pty` | 18, 19, 21 |
| Capture adapters — `window` | 5 (typed deferral; implementation out of scope) |
| Per-project state (`.plumb/`) | 2, 4, 6, 14, 17 |
| Scenario schema | 2 |
| Flow 1 — Trigger | 13 |
| Flow 2 — Select | 3, 6, 13 |
| Flow 3 — Capture | 5, 6, 21 |
| Flow 4 — Fan out | 8, 13 |
| Flow 5 — Merge | 11, 12, 17 |
| Flow 6 — Disposition | 13, 17 |
| Lenses / applicability | 8, 9, 10, 15, 16 |
| Concurrency cap and deferral reporting | 8, 12, 13 |
| Intentional distortion (`expects`) | 2, 8, 9, 22, 26 |
| Finding contract / mandatory `region` | 7 |
| Gate semantics (GO / NO-GO / HOLD) | 12, 25 |
| Blinding | 4, 8, 9, 13, 14 |
| Third-party framing / Sim Sup | 8, 9, 10, 15, 16 |
| No quota | 7, 8, 9 |
| Confidence governs voice | 8, 16 |
| Taste profile | 8, 16, 25 |
| Rulings + calcification guard | 17 |
| Failure handling — capture failure is never a GO | 5, 12 |
| Failure handling — `--on-unmapped-glyph` | 20 |
| Failure handling — retry once then HOLD | 12, 13 |
| Failure handling — scaffold, build-and-cache, no stack traces | 6, 13 |
| Testing — capture crate | 18-21 |
| Testing — orchestration logic | 2, 3, 5, 7, 8, 11, 12, 17 |
| Testing — reviewer regression corpus | 26 |
| Verification — all five items | 14, 16, 17, 21, 26 |

Non-goals are enforced by the Global Constraints section; no task
implements golden-image diffing, a browser adapter, macOS/Linux window
capture, CI integration, prebuilt binaries, or any change to
`tools/visual-snapshot`.

---

## Judgment calls made while planning

Places the spec was silent or ambiguous, what was decided, and what to
change if the decision is wrong.

1. **Prompt construction lives in Rust, not in skill prose.** The spec
   names `SKILL.md` "the orchestrator" but never says who *builds* the
   lens prompts. Putting it in `prompt::build_prompt` (Task 8) converts
   "verify the dispatched prompts contain no diff, no source, and no
   authorship framing" — a spec Verification item — from a manual
   inspection into a unit test, and leaves the skill owning only what
   the harness alone can do. This is the largest structural decision in
   the plan.

2. **The finding fingerprint deliberately excludes the lens** (Task 11),
   though the spec writes it as "lens + scenario + region + normalized
   claim". Reason: with the lens included, the same observation raised
   by a second lens produces a different fingerprint and slips past a
   ruling already made against the first — the report starts repeating
   exactly what a ruling exists to stop. The `Ruling` record still
   *stores* the lens that originally raised it. **If the literal spec
   reading is wanted, it is a one-line change** to `fingerprint`'s
   input.

3. **A stale ruling does not suppress.** The spec says rulings made
   under an old `taste.md` hash are "marked stale and surfaced for
   re-validation rather than silently applied forever", which admits two
   readings. The plan takes the stricter one (Task 17): the finding
   reappears and the ruling is listed as needing re-validation. The
   looser reading — suppress but warn — is a one-line change, and the
   tradeoff is real: editing `taste.md` will make a batch of previously
   silenced findings return at once.

4. **`parse_findings` overwrites the agent's `lens` and `scenario`**
   with what was actually dispatched (Task 7). An agent that mislabels
   its scenario would otherwise corrupt the merge and the ruling
   fingerprints. Not in the spec; a defensive choice.

5. **One recovery attempt before "unparseable".** The spec's retry rule
   counts unparseable output twice as a HOLD. The plan first extracts
   the outermost `[...]` from prose or a fenced block (Task 7) — models
   pad, and a cheap extraction is better than burning a retry. A report
   that survives neither extraction nor a retry is still a HOLD.

6. **An unknown `expects` value is a hard parse error** (Task 2). The
   spec does not say. Silent tolerance would let a typo
   (`visual-corrupton`) degrade to "expects nothing", which is the safe
   direction for a *lens* but the wrong direction for the *author* — it
   would look like the exemption was granted while the review NO-GO'd
   the app's signature effect.

7. **The manifest carries no `args` and no `touches`** (Task 4). The
   spec states that lens agents do not receive the diff or the source;
   it does not explicitly say the adapter's command line counts as
   source. It does — `cargo run -p visual-snapshot -- --example
   omnitrix` names the app, the tool, and the script path. The plan
   treats both fields as blinding leaks and tests for their absence.

8. **`taste_override` goes to the design lens only** (Task 8). The
   taste profile's own documentation of the mechanism describes it as
   additive to `taste.md`, and `taste.md` is a design-lens input, so it
   follows — but the spec does not state it, and a reading where
   breakage also sees it is defensible.

9. **Crate naming.** The Parallax naming table assigns Plumb the crate
   `parallax-plumb`; the Plumb spec's layout puts the Rust code in
   `capture/`. The plan uses both: package `parallax-plumb` in
   `capture/`, binary named `plumb`. The spec's file inventory also
   writes `capture/src/glyphs.rs` while the code being extracted is
   `glyph.rs`; the plan keeps the existing singular name to make the
   port a copy rather than a rename.

10. **CLI surface.** `init` / `select` / `capture` / `plan` / `merge` /
    `rule`, with exit codes 0=GO, 1=NO-GO, 2=HOLD, and 3=nothing
    selected. Entirely invented — the spec describes the flow, not a
    command surface. The exit codes matter beyond convenience: they are
    what lets "a pre-PR check can read `verdict.md`" actually be a
    check.

11. **`on_unmapped_glyph` is a per-scenario config field**, not only a
    CLI flag (Task 21). The spec presents it as a flag. Per-scenario is
    what the orchestrated path actually needs, since only some scenarios
    hit unmapped glyphs and the choice should be durable rather than
    re-typed.

12. **`serde_yaml = "0.9"`** is archived upstream. Chosen anyway for
    ubiquity, isolated behind `config.rs` so replacing it is a
    single-file change. Flagging it because it will eventually need
    revisiting.

13. **Arc 6's ordering gates `launcher` and `smash_crabs` on Arc 5.**
    Their known-unmapped glyphs (`✦`, `💥`) hard-error until
    `substitute` mode exists. Attempting them earlier wastes the
    authoring effort on a capture that cannot complete.

14. **The `window` adapter is planned only as far as a typed refusal**
    (Task 5), per the scope cut: it has no consumer. The contract admits
    it and the config schema parses it, so adding it later is one module
    and no change anywhere else — which is exactly what the spec's
    adapter boundary promises.

---

## Execution handoff

Plan complete and saved to
`docs/design/plans/plumb/2026-08-14-plumb-plan.md`. Two execution
options:

1. **Subagent-Driven (recommended)** — a fresh subagent per task, with
   review between tasks and fast iteration. Suits this plan well: Arcs
   1-2 are a long sequence of small, well-specified TDD tasks.
2. **Inline Execution** — execute tasks in-session via
   `superpowers:executing-plans`, batching with checkpoints.

Note two cross-repo handoffs whichever is chosen: Task 14 and all of
Arc 6 execute **in the TTUI repo**, on a TTUI worktree branch, through
TTUI's normal Gated PR flow; everything else executes in the new
`plumb` repository.

