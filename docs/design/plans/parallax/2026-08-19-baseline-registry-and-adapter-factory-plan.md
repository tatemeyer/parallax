# Baseline Registry and Adapter Factory — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> `superpowers:subagent-driven-development` or
> `superpowers:executing-plans`. Steps use checkbox (`- [ ]`) syntax.
>
> **Structure note:** organized as **Arcs → Slices → Tasks** per
> `docs/design/README.md`.

**Goal:** Close the two holes the cockpit's design found in
`parallax-baseline` — nothing enumerates registered projects, and
nothing maps a validated manifest onto the adapters it declares — plus
the third the refresh model found: nothing distinguishes a verification
check that *reads* from one that *runs*.

**Spec:**
`docs/design/specs/parallax/2026-08-18-baseline-registry-and-adapter-factory-design.md`,
approved and merged 2026-08-19. Every open question there is settled by
its stated default, since the spec merged unedited.

**Architecture:** Three arcs, each its own PR, ordered by what unblocks
what. Arc 1 is one method on an existing trait. Arc 2 is the factory,
which needs Arc 1 only so the thing it builds carries a cost. Arc 3 is
the registry, which is independent of both and could ship first — it
goes last because the factory is what Panopticon's Arc 1 actually
blocks on.

**Tech Stack:** Rust (stable, 2021 edition). **No new dependency.**
`serde`, `serde_yaml`, and `walkdir` are already in
`baseline/Cargo.toml` and are all this needs.

---

## Global Constraints

Inherited from the baseline plan and unchanged. Repeated because they
are the constraints most likely to be broken by a task that adds a
constructor.

- **The library never touches a terminal.** No UI, no TTY, no
  `print!`/`println!` outside `#[cfg(test)]`, no `crossterm`, no `ttui`.
- **No dependency on Plumb, in either direction.** The runs-directory
  convention in Task 4 is derived from a manifest path; it does not link
  `parallax-plumb` and must not.
- **Every `pub` item is documented.** `#![warn(missing_docs)]` plus CI's
  `-D warnings` means a missing doc comment fails the build.
- **No wall clock, no network, no `$HOME` in any test.** `SystemTime` is
  injected; HTTP goes through `HttpTransport`; the registry takes a path
  and never consults the environment.
- **TDD.** Write the failing test, run it, watch it fail, then
  implement. The one exception in this plan is Task 8's fixture trees,
  which are data.
- **Soft ceiling of 500 lines per file**, tests included. `registry.rs`
  is the only file at risk; Task 7 checks it.

---

## File Structure

```
baseline/
  src/
    lib.rs                     — gains `pub mod registry;`
    registry.rs                — Registry, RegisteredProject, RegistryError   (Arc 3)
    adapters/
      mod.rs                   — gains `pub mod factory;`
      factory.rs               — AdapterConfig, from_manifest, from_manifest_with  (Arc 2)
      verification.rs          — gains CheckCost and the trait method          (Arc 1)
  tests/
    factory_replay.rs          — the translation table, end to end             (Arc 2)
    registry.rs                — load, degrade, scan, order                    (Arc 3)
    fixtures/registry/
      registry.yaml            — two roots, one of them broken
      ttui/parallax.yaml       — a copy of manifests/ttui.yaml
      broken/parallax.yaml     — parses, fails validation
      unregistered/…           — a directory with no manifest, for `scan`
```

`manifests/` and every existing test file stay where they are.
`aggregate_replay.rs` is modified once, in Task 6, and its assertions
are not.

---

## Milestones

- **End of Arc 1** — a scheduler can tell a reading check from an
  executing one, so the cockpit's refresh cycle can be built without
  running `cargo test` in a loop. One method, three tests.
- **End of Arc 2** — a `Validated` manifest becomes its declared
  adapters, and `aggregate_replay.rs`'s hand-built helper is gone. **This
  is the milestone Panopticon's Arc 1 is waiting on.**
- **End of Arc 3** — a registry file or a directory scan yields every
  registered project's validated manifest, with a broken one degrading
  itself and nothing else. The library can now answer both halves of
  "which projects, and what serves them."

---

## Arc 1: Telling a read from a run

### Slice 1.1: The cost hint

**Tags:** coding

#### Task 1: `CheckCost` and `VerificationAdapter::cost`

**Files:**
- Modify: `baseline/src/adapters/verification.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum CheckCost { Read, Execute }`
  - `VerificationAdapter::cost(&self) -> CheckCost`, defaulting to
    `Execute`
  - `impl VerificationAdapter for PlumbVerificationAdapter` overrides to
    `Read`

  Consumed by Task 4 (the factory builds both kinds) and by Panopticon's
  refresh model.

**The default is `Execute`, and that is the whole design.** The failure
modes are not symmetric: a reading adapter misclassified as executing
refreshes less often than it could, while an executing adapter
misclassified as reading spawns processes in a loop. An adapter this
crate has never seen gets the safe assumption.

- [ ] **Step 1: Write the failing tests**

Append to `verification.rs`'s `command_tests` module:

```rust
    #[test]
    fn a_command_check_costs_a_process() {
        let a = CommandVerificationAdapter::new("tests", "cargo test", ScriptedRunner::new());
        assert_eq!(a.cost(), CheckCost::Execute);
    }

    #[test]
    fn a_plumb_check_only_reads() {
        let a = PlumbVerificationAdapter::new("perceptual", "/tmp/runs");
        assert_eq!(a.cost(), CheckCost::Read);
    }

    /// An adapter defined outside this crate that overrides nothing is
    /// assumed to cost something. A scheduler that polls it on a cadence
    /// would be spawning whatever it spawns, forever.
    #[test]
    fn an_adapter_that_says_nothing_is_assumed_to_execute() {
        struct Unknown;
        impl VerificationAdapter for Unknown {
            fn source_name(&self) -> String {
                "verification:unknown".into()
            }
            fn check(
                &mut self,
                _ctx: &ProjectContext,
                now: SystemTime,
            ) -> Result<Observed<VerificationStatus>, AdapterError> {
                Ok(Observed::watched(
                    VerificationStatus {
                        kind: "unknown".into(),
                        outcome: VerificationOutcome::NotRun,
                        detail: None,
                    },
                    now,
                ))
            }
        }
        assert_eq!(Unknown.cost(), CheckCost::Execute);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p parallax-baseline --lib adapters::verification`
Expected: FAIL — `CheckCost` does not exist.

- [ ] **Step 3: Implement**

In `verification.rs`, above the trait:

```rust
/// What calling [`VerificationAdapter::check`] costs.
///
/// The two built-ins sit on opposite sides of this: the `plumb` adapter
/// reads a `verdict.md` off disk, and the `command` adapter runs
/// whatever the manifest declared — for TTUI, `cargo clippy` and `cargo
/// test`. A caller that polls on a cadence needs to tell them apart
/// before it schedules anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckCost {
    /// Reads state something else produced. Safe on any cadence.
    Read,
    /// Produces the state by running something. Operator-initiated.
    Execute,
}
```

Add to the trait, after `check`:

```rust
    /// What calling `check` costs.
    ///
    /// Defaults to [`CheckCost::Execute`]: the safe assumption about an
    /// adapter this crate has never seen is that calling it *does*
    /// something. A reader misclassified as an executor merely refreshes
    /// less often than it could; an executor misclassified as a reader
    /// spawns processes in a loop.
    fn cost(&self) -> CheckCost {
        CheckCost::Execute
    }
```

And on `PlumbVerificationAdapter`'s impl:

```rust
    fn cost(&self) -> CheckCost {
        CheckCost::Read
    }
```

`CommandVerificationAdapter` takes the default and gains nothing.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p parallax-baseline --lib adapters::verification`
Expected: PASS, 9 tests.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/adapters/verification.rs
git commit -m "feat(adapters): let a caller tell a reading check from a running one

check() means two different things depending on the implementation, and
a scheduler that cannot tell them apart runs every declared build
command on its poll interval. Defaults to Execute because the failure
modes are not symmetric."
```

---

## Arc 2: The adapter factory

The manifest's meaning, in one place. Everything here is a translation
of already-validated data into already-existing constructors; no adapter
changes and no new I/O path opens.

### Slice 2.1: Config and the two signatures

**Tags:** coding

#### Task 2: `AdapterConfig`

**Files:**
- Create: `baseline/src/adapters/factory.rs`
- Modify: `baseline/src/adapters/mod.rs` (add `pub mod factory;`)

**Interfaces:**
- Consumes: `freshness::DEFAULT_POLL_INTERVAL`.
- Produces:
  - `pub struct AdapterConfig { pub poll_interval: Duration, pub github_token: Option<String> }`
  - `impl Default for AdapterConfig`

**No environment reading, here or anywhere below.** `github_token` is
passed in. A library that reaches for `GITHUB_TOKEN` cannot be tested
twice on the same machine with different answers, and the frontend
already has to decide between an env var, `gh auth token`, and nothing.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn the_default_config_polls_at_the_crate_default_and_carries_no_token() {
        let c = AdapterConfig::default();
        assert_eq!(c.poll_interval, DEFAULT_POLL_INTERVAL);
        assert_eq!(c.github_token, None);
    }
}
```

- [ ] **Step 2: Run to verify it fails, then implement**

```rust
//! Building a project's adapters from its validated manifest.
//!
//! The manifest's meaning lives here and nowhere else: `adapter: github`
//! means "poll GitHub with conditional requests at the configured
//! interval," and that sentence has to have exactly one implementation
//! or the manifest stops being a specification. A frontend that
//! translates manifests owns part of the schema.

use super::artifact::{CaptureArtifactAdapter, FigureArtifactAdapter, MetricsArtifactAdapter};
use super::http::{HttpTransport, UreqTransport};
use super::session::FilesystemSessionAdapter;
use super::verification::{
    CommandRunner, CommandVerificationAdapter, PlumbVerificationAdapter, ProcessRunner,
};
use super::work::GithubWorkAdapter;
use crate::freshness::DEFAULT_POLL_INTERVAL;
use crate::manifest::{
    ArtifactAdapterKind, VerificationAdapterKind, VerificationEntry, WorkAdapterKind,
};
use crate::state::ProjectAdapters;
use crate::validate::Validated;
use std::path::PathBuf;
use std::time::Duration;

/// What the built-in adapters need that the manifest does not say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterConfig {
    /// How often polled sources are refreshed.
    pub poll_interval: Duration,
    /// The token the GitHub adapter authenticates with, when there is
    /// one. **Passed in, never read from the environment.**
    pub github_token: Option<String>,
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            poll_interval: DEFAULT_POLL_INTERVAL,
            github_token: None,
        }
    }
}
```

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(factory): add the adapter config the manifest does not carry"
```

---

#### Task 3: `from_manifest_with`, work and sessions

**Files:**
- Modify: `baseline/src/adapters/factory.rs`

**Interfaces:**
- Produces:
  - `pub fn from_manifest_with<T, R>(validated: &Validated, config: &AdapterConfig, transport: impl Fn() -> T, runner: impl Fn() -> R) -> ProjectAdapters where T: HttpTransport + 'static, R: CommandRunner + 'static`

**Factories, not values.** A manifest can declare several `command`
checks and each adapter owns its runner, so a single `R` cannot be
shared. The same signature is what lets Panopticon's fixture mode wire
adapters through this exact translation rather than a parallel one.

**No error path.** `validate` already rejects a `command` entry with no
command, a `work.repo` that is not `owner/name`, and an unparseable
watch glob, so the factory takes `&Validated` and cannot fail. Third use
of the private-constructor discipline `Validated` and `Authorized`
already carry.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod work_tests {
    use super::*;
    use crate::adapters::http::FixtureTransport;
    use crate::adapters::verification::ScriptedRunner;
    use crate::manifest::parse_manifest;
    use crate::validate::validate;

    pub(super) fn built(yaml: &str) -> ProjectAdapters {
        let validated = validate(parse_manifest(yaml).expect("parses")).expect("validates");
        from_manifest_with(
            &validated,
            &AdapterConfig::default(),
            FixtureTransport::new,
            ScriptedRunner::new,
        )
    }

    #[test]
    fn a_declared_github_work_feed_becomes_a_github_adapter() {
        let a = built(
            "project:\n  name: p\n  root: /tmp/p\nwork:\n  adapter: github\n  repo: a/b\n  autonomy_map: {}\n",
        );
        assert_eq!(
            a.work.as_ref().map(|w| w.source_name()),
            Some("work:github".to_string())
        );
    }

    #[test]
    fn a_manifest_with_no_work_feed_builds_no_work_adapter() {
        let a = built("project:\n  name: p\n  root: /tmp/p\n");
        assert!(a.work.is_none());
        assert!(a.verification.is_empty());
        assert!(a.artifacts.is_empty());
        assert!(a.sessions.is_none());
    }

    #[test]
    fn a_declared_session_feed_becomes_a_filesystem_session_adapter() {
        let a = built("project:\n  name: p\n  root: /tmp/p\nsessions:\n  watch: '.claude/worktrees/*'\n");
        assert_eq!(
            a.sessions.as_ref().map(|s| s.source_name()),
            Some("session:filesystem".to_string())
        );
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

```rust
/// Builds a project's adapters from its validated manifest, taking the
/// transport and runner from factories so each adapter owns its own.
///
/// Cannot fail: `validate` has already rejected everything that would
/// make an adapter unconstructible.
pub fn from_manifest_with<T, R>(
    validated: &Validated,
    config: &AdapterConfig,
    transport: impl Fn() -> T,
    runner: impl Fn() -> R,
) -> ProjectAdapters
where
    T: HttpTransport + 'static,
    R: CommandRunner + 'static,
{
    let manifest = validated.manifest();
    let mut adapters = ProjectAdapters::new();

    if let Some(work) = &manifest.work {
        adapters.work = Some(match work.adapter {
            WorkAdapterKind::Github => Box::new(
                GithubWorkAdapter::new(transport()).with_interval(config.poll_interval),
            ),
        });
    }

    // Verification and artifacts land in Task 4.

    if let Some(sessions) = &manifest.sessions {
        adapters.sessions = Some(Box::new(FilesystemSessionAdapter::new(
            sessions.watch.clone(),
        )));
    }

    adapters
}
```

The `match` on `WorkAdapterKind` is deliberately exhaustive with one
arm: adding a second work adapter must not compile until this line
chooses between them.

- [ ] **Step 3: Run to verify they pass; commit**

```bash
git commit -m "feat(factory): build the work and session adapters a manifest declares"
```

---

### Slice 2.2: The rest of the table

**Tags:** coding

#### Task 4: Verification and artifacts

**Files:**
- Modify: `baseline/src/adapters/factory.rs`

**Interfaces:**
- Produces: `fn plumb_runs_dir(validated: &Validated, entry: &VerificationEntry) -> PathBuf`
  (private), plus the verification and artifact arms of
  `from_manifest_with`.

**The runs-directory convention, stated once.** `PlumbVerificationAdapter`
needs a runs directory; the manifest declares a *config* path. TTUI says
`config: .plumb/config.yaml` and its runs live at `.plumb/runs/`, so the
convention is **`<config parent>/runs`, resolved against the project
root**. The spec settles this as a convention rather than a schema field,
so it is pinned by a test here — conventions rot silently.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod translation_tests {
    use super::work_tests::built;
    use super::*;

    fn source_names(a: &ProjectAdapters) -> Vec<String> {
        a.verification.iter().map(|v| v.source_name()).collect()
    }

    #[test]
    fn each_verification_entry_becomes_its_declared_adapter_in_order() {
        let a = built(
            "project:\n  name: p\n  root: /tmp/p\nverification:\n  - kind: lint\n    adapter: command\n    command: cargo clippy\n  - kind: perceptual\n    adapter: plumb\n",
        );
        assert_eq!(
            source_names(&a),
            vec![
                "verification:command:lint".to_string(),
                "verification:plumb:perceptual".to_string()
            ]
        );
    }

    /// The cost hint from Arc 1, carried through the factory: a
    /// scheduler must be able to partition what it just built.
    #[test]
    fn the_built_adapters_carry_their_cost() {
        let a = built(
            "project:\n  name: p\n  root: /tmp/p\nverification:\n  - kind: lint\n    adapter: command\n    command: cargo clippy\n  - kind: perceptual\n    adapter: plumb\n",
        );
        assert_eq!(a.verification[0].cost(), CheckCost::Execute);
        assert_eq!(a.verification[1].cost(), CheckCost::Read);
    }

    #[test]
    fn the_plumb_runs_directory_is_the_config_s_parent_plus_runs() {
        let validated = validate(
            parse_manifest(
                "project:\n  name: p\n  root: /tmp/p\nverification:\n  - kind: perceptual\n    adapter: plumb\n    config: .plumb/config.yaml\n",
            )
            .unwrap(),
        )
        .unwrap();
        let entry = &validated.manifest().verification[0];
        assert_eq!(
            plumb_runs_dir(&validated, entry),
            std::path::PathBuf::from("/tmp/p/.plumb/runs")
        );
    }

    /// An entry that declares no config still gets the default
    /// `.plumb/config.yaml`, and therefore the same runs directory.
    #[test]
    fn an_entry_with_no_declared_config_still_resolves_to_dot_plumb_runs() {
        let validated = validate(
            parse_manifest(
                "project:\n  name: p\n  root: /tmp/p\nverification:\n  - kind: perceptual\n    adapter: plumb\n",
            )
            .unwrap(),
        )
        .unwrap();
        let entry = &validated.manifest().verification[0];
        assert_eq!(
            plumb_runs_dir(&validated, entry),
            std::path::PathBuf::from("/tmp/p/.plumb/runs")
        );
    }

    #[test]
    fn each_artifact_entry_becomes_the_adapter_its_kind_resolves_to() {
        let a = built(
            "project:\n  name: p\n  root: /tmp/p\nartifacts:\n  - kind: figure\n    watch: 'out/**/*.png'\n  - kind: metrics\n    adapter: jsonl\n    watch: 'r/**/*.jsonl'\n  - kind: capture\n    watch: '.plumb/runs/**'\n",
        );
        let names: Vec<String> = a.artifacts.iter().map(|x| x.source_name()).collect();
        assert_eq!(
            names,
            vec![
                "artifact:figure".to_string(),
                "artifact:metrics".to_string(),
                "artifact:capture".to_string()
            ]
        );
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

```rust
/// Where a `plumb` entry's runs live: `<config parent>/runs`, resolved
/// against the project root.
///
/// The manifest declares a config path, not a runs path — TTUI writes
/// `config: .plumb/config.yaml` and Plumb writes its runs to
/// `.plumb/runs/`. This is a convention rather than a declaration, and
/// the day a project disagrees the escape hatch is a `runs:` key on the
/// entry, deliberately not added while nothing needs it.
fn plumb_runs_dir(validated: &Validated, entry: &VerificationEntry) -> PathBuf {
    let config = validated.plumb_config(entry);
    let parent = config.parent().unwrap_or_else(|| std::path::Path::new(""));
    let root = validated
        .manifest()
        .project
        .root
        .clone()
        .unwrap_or_default();
    root.join(parent).join("runs")
}
```

Inside `from_manifest_with`, replacing the Task 3 placeholder comment:

```rust
    for entry in &manifest.verification {
        adapters.verification.push(match entry.adapter {
            VerificationAdapterKind::Command => Box::new(CommandVerificationAdapter::new(
                entry.kind.clone(),
                entry
                    .command
                    .clone()
                    .expect("validation rejects a command adapter with no command"),
                runner(),
            )),
            VerificationAdapterKind::Plumb => Box::new(PlumbVerificationAdapter::new(
                entry.kind.clone(),
                plumb_runs_dir(validated, entry),
            )),
        });
    }

    for entry in &manifest.artifacts {
        let watch = entry.watch.clone();
        adapters.artifacts.push(match validated.artifact_adapter(entry) {
            ArtifactAdapterKind::Figure => Box::new(FigureArtifactAdapter::new(watch)),
            ArtifactAdapterKind::Metrics => Box::new(MetricsArtifactAdapter::new(watch)),
            ArtifactAdapterKind::Capture => Box::new(CaptureArtifactAdapter::new(watch)),
        });
    }
```

The `expect` is the only panic in this crate outside tests, and it is
load-bearing rather than lazy: reaching it means `validate` let through
a manifest it documents as rejected, which is a bug in this crate and
not a condition a caller can handle. Task 5 asserts the validator
actually holds that line.

- [ ] **Step 3: Run to verify they pass; commit**

```bash
git commit -m "feat(factory): build the verification and artifact adapters

The plumb runs directory is <config parent>/runs resolved against the
project root — a convention rather than a declaration, so it is pinned
by a test."
```

---

#### Task 5: `from_manifest`, the live convenience wrapper

**Files:**
- Modify: `baseline/src/adapters/factory.rs`

**Interfaces:**
- Produces: `pub fn from_manifest(validated: &Validated, config: &AdapterConfig) -> ProjectAdapters`

**Real-external-service exempt, and therefore thin.** This function's
entire body is the two closures the generic form takes. It is the only
place `UreqTransport` and `ProcessRunner` are named, which keeps the
network and process seams countable.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn the_live_wrapper_builds_the_same_shape_as_the_generic_form() {
        let validated = validate(parse_manifest(TTUI_LIKE).unwrap()).unwrap();
        let live = from_manifest(&validated, &AdapterConfig::default());
        assert!(live.work.is_some());
        assert_eq!(live.verification.len(), 3);
        assert_eq!(live.artifacts.len(), 1);
        assert!(live.sessions.is_some());
    }

    /// The guard behind Task 4's `expect`: if this ever fails, the
    /// factory panics on a manifest a caller was told was valid.
    #[test]
    fn validation_still_rejects_a_command_entry_with_no_command() {
        let m = parse_manifest(
            "project:\n  name: p\nverification:\n  - kind: tests\n    adapter: command\n",
        )
        .unwrap();
        assert!(validate(m).is_err(), "the factory's expect depends on this");
    }
```

`TTUI_LIKE` is `manifests/ttui.yaml`'s text inlined as a `const`, with
`root: /tmp/p` so nothing touches a real tree.

- [ ] **Step 2: Implement**

```rust
/// Builds a project's adapters against the live world: `UreqTransport`
/// for work, `ProcessRunner` for `command` verification.
pub fn from_manifest(validated: &Validated, config: &AdapterConfig) -> ProjectAdapters {
    let token = config.github_token.clone();
    from_manifest_with(
        validated,
        config,
        move || match &token {
            Some(t) => UreqTransport::with_token(t.clone()),
            None => UreqTransport::new(),
        },
        || ProcessRunner,
    )
}
```

- [ ] **Step 3: Run; commit**

```bash
git commit -m "feat(factory): add the live wrapper over the generic form"
```

---

### Slice 2.3: The proof

**Tags:** coding

#### Task 6: Rewire `aggregate_replay` onto the factory

**Files:**
- Modify: `baseline/tests/aggregate_replay.rs`
- Create: `baseline/tests/factory_replay.rs`

**Interfaces:** no new API. This task is the spec's stated verification:
*"`aggregate_replay.rs`'s hand-built `ttui_adapters(...)` helper is
replaced by a `from_manifest_with` call and its assertions still pass
unchanged — which is the real proof that the factory encodes what the
manifest was already understood to mean."*

**If an assertion has to change, stop.** A changed assertion means the
factory and the hand-built helper disagree about what the manifest says,
and the interesting question is which one is wrong — not how to make the
test green.

- [ ] **Step 1: Replace the helper**

`ttui_adapters(root)` becomes:

```rust
/// TTUI's adapters, built the way the library builds them.
fn ttui_adapters(_root: &Path, validated: &Validated) -> ProjectAdapters {
    // One scripted runner per `command` entry, in declaration order:
    // `lint` passes, `tests` fails. A factory hands out a fresh runner
    // per adapter, so the closure scripts by call index.
    let call = std::cell::Cell::new(0usize);
    from_manifest_with(
        validated,
        &AdapterConfig::default(),
        github_transport,
        || {
            let mut r = ScriptedRunner::new();
            match call.replace(call.get() + 1) {
                0 => r.push(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() }),
                _ => r.push(CommandOutput {
                    status: 101,
                    stdout: String::new(),
                    stderr: "test result: FAILED. 1 failed".into(),
                }),
            }
            r
        },
    )
}
```

The call sites pass the already-loaded `Validated`. Nothing else in the
file changes.

- [ ] **Step 2: Run the suite**

Run: `cargo test -p parallax-baseline --test aggregate_replay`
Expected: PASS, 9 tests, **no assertion edited**.

Note the one thing this proves that a unit test cannot: the runs
directory the factory derives (`<root>/.plumb/runs`) is the same
directory the old helper was given by hand, against a temp tree that
actually contains a verdict.

- [ ] **Step 3: Add `factory_replay.rs`**

An integration test that reaches only the public API, covering what the
unit tests cannot: both **real** manifests from `manifests/`, built by
the factory, aggregated, with the counts the spec names —

```
ttui.yaml               → 1 work, 3 verification, 1 artifact, 1 session
model-experiments.yaml  → 1 work, 2 verification, 2 artifact, 0 session
```

plus a partial manifest declaring only `work:` yielding exactly one
adapter, and an assertion that `model-experiments.yaml` builds **no**
session adapter because it declares none.

- [ ] **Step 4: Run every gate; commit**

```bash
git add baseline/tests/
git commit -m "test(factory): replay both real manifests through the factory

aggregate_replay's hand-built adapter helper is gone and its assertions
are untouched, which is what proves the factory encodes the manifest's
existing meaning rather than a second opinion about it."
```

---

## Arc 3: The registry

### Slice 3.1: Loading

**Tags:** coding

#### Task 7: `Registry`, `RegisteredProject`, `RegistryError`

**Files:**
- Create: `baseline/src/registry.rs`
- Modify: `baseline/src/lib.rs` (add `pub mod registry;`)

**Interfaces:**
- Produces:
  - `pub struct RegisteredProject { pub name: String, pub root: PathBuf, pub manifest_path: PathBuf, pub manifest: Validated }`
  - `pub struct RegistryError { pub source: PathBuf, pub problem: String }`
  - `pub struct Registry` with `from_roots`, `from_file`, `scan`,
    `projects`, `failures`
  - `pub const MANIFEST_FILENAME: &str = "parallax.yaml"`

**The registry's root wins over the manifest's.** This is the judgment
call the spec did not make; see Judgment call 1. `parse_manifest_file`
defaults `project.root` to the manifest's own directory *only when the
manifest declares none*, and `manifests/ttui.yaml` declares
`root: <projects-root>/TTUI` — a placeholder that exists on no machine.
So the registry **overwrites** `project.root` with the directory it
found the manifest in, before validating.

**Two constructors return `Self`, one returns `Result`.** A registered
project that fails to load is a `RegistryError` in `failures()` and
every other project still loads — the same rule aggregation follows for
adapters. A registry *file* that cannot be read is not a partial answer;
it is no answer, so `from_file` is fallible.

- [ ] **Step 1: Write the failing tests**

Unit tests in `registry.rs` covering: a root whose manifest parses and
validates yields a `RegisteredProject` whose `name` comes from the
manifest and whose `root` is the directory scanned, **not** the
manifest's declared placeholder; a root with no `parallax.yaml` becomes
one failure naming the expected path and loads nothing; a root whose
manifest fails validation becomes one failure carrying the validator's
own message; order follows the input; and `MANIFEST_FILENAME` is
`parallax.yaml`.

- [ ] **Step 2: Implement**

```rust
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
//! labelled broken.
```

Types, then `from_roots`:

```rust
impl Registry {
    /// Loads every root's `parallax.yaml`.
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
}

/// Loads one project, or says why it could not be loaded.
fn load_one(root: &Path) -> Result<RegisteredProject, RegistryError> {
    let manifest_path = root.join(MANIFEST_FILENAME);
    let mut parsed = parse_manifest_file(&manifest_path).map_err(|e| RegistryError {
        source: manifest_path.clone(),
        problem: e.to_string(),
    })?;
    // The registry knows where the project actually is; the manifest's
    // `root:` is checked in and cannot be machine-specific.
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
```

- [ ] **Step 3: Run; check the file is under 500 lines; commit**

```bash
git commit -m "feat(registry): load a project from its root

The registry's root wins over the manifest's declared one: a manifest is
checked in and cannot be machine-specific, while the registry is local
configuration and knows where the project actually is."
```

---

#### Task 8: The registry file and the scan

**Files:**
- Modify: `baseline/src/registry.rs`
- Create: `baseline/tests/registry.rs`
- Create: `baseline/tests/fixtures/registry/…`

**Interfaces:**
- Produces: `Registry::from_file`, `Registry::scan`, and the file schema.

**The file carries roots and nothing else.** A project's name comes from
its own manifest — one source of truth for identity, so a rename in
`parallax.yaml` cannot desynchronize from a list somewhere else.
`deny_unknown_fields`, as with the manifest, so a typo is an error
rather than a silent omission.

- [ ] **Step 1: Write the fixtures** (TDD exception: data)

```
fixtures/registry/registry.yaml     — apiVersion + two roots: ttui/, broken/
fixtures/registry/ttui/parallax.yaml
fixtures/registry/broken/parallax.yaml   — parses, empty project name
fixtures/registry/unregistered/README    — a directory with no manifest
```

- [ ] **Step 2: Write the failing integration tests**

`baseline/tests/registry.rs`: a registry file with one good and one
broken root loads exactly one project and one failure, and the failure
names the broken manifest; a registry file with an unknown key is an
error; a missing registry file is an error naming the path; `scan` over
the fixtures directory finds the two projects and ignores
`unregistered/`; `scan` of a nonexistent directory is empty rather than
an error; and both loaded projects' `root` fields point inside the
fixture tree rather than at the manifests' declared placeholders.

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RegistryFile {
    #[serde(default)]
    api_version: Option<String>,
    projects: Vec<RegistryEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryEntry {
    root: PathBuf,
}
```

`from_file` reads, parses, and delegates to `from_roots`, so the
degradation rule has exactly one implementation. `scan` sorts
`read_dir`'s entries by file name — `read_dir` order is
filesystem-defined and `PlatformState::projects` is documented as being
in registration order — keeps every child that contains a manifest, and
returns an empty registry for a directory that does not exist.

- [ ] **Step 4: Run; commit**

```bash
git commit -m "feat(registry): load from a file or a directory scan

Roots only: a project's name comes from its own manifest, so a rename
cannot desynchronize from a list somewhere else."
```

---

### Slice 3.2: End to end

**Tags:** coding

#### Task 9: A registry, through the factory, into `aggregate`

**Files:**
- Modify: `baseline/tests/registry.rs`

**Interfaces:** no new API. The spec's last verification item: *"a
registry over both real manifests, through the factory with a fixture
transport and scripted runner, into `aggregate` — the same assertions
`aggregate_replay.rs` makes today, reached without any hand-built
adapter."*

- [ ] **Step 1: Write the test**

Build a `Registry` over a temp tree holding copies of both real
manifests, map each `RegisteredProject` through `from_manifest_with`
into `(Validated, ProjectAdapters)`, call `aggregate`, and assert: two
projects in registration order, `methodology` carried through for each,
no degradations, and the broken third project present in `failures()`
rather than in `projects()`.

This is the first test in the crate where nothing is hand-wired — the
path from "a directory on disk" to "a `PlatformState`" is exercised
whole, which is exactly the path a frontend takes.

- [ ] **Step 2: Run every gate from the workspace root**

```
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo doc -p parallax-baseline --no-deps
```

- [ ] **Step 3: Update `baseline/README.md`**

Two paragraphs: the registry, and building adapters from a manifest.
The README currently documents a library you cannot start using without
writing the translation yourself.

- [ ] **Step 4: Commit**

```bash
git commit -m "test(registry): a directory on disk, aggregated, with nothing hand-wired"
```

---

## Spec coverage

| Spec section | Tasks |
|---|---|
| `Registry` and its three constructors | 7, 8 |
| Failures degrade one project, never the registry | 7, 8, 9 |
| Registration order preserved | 7, 8, 9 |
| Registry file format, roots only, `deny_unknown_fields` | 8 |
| The library never picks the registry's location | 7, 8 (asserted by taking a path everywhere) |
| `AdapterConfig`, no environment reading | 2 |
| `from_manifest_with` takes factories | 3, 4 |
| `from_manifest` live wrapper | 5 |
| The translation table, a test per row | 3, 4 |
| Plumb runs directory convention | 4 |
| Factory takes `&Validated`, has no error path | 4, 5 |
| `CheckCost` and the `Execute` default | 1 |
| `aggregate_project` unchanged | — (asserted by Task 6 editing no assertion) |
| Verification: no new dependency | Global Constraints; checked in Task 9 |

Non-goals stay non-goals: nothing here watches the registry for changes,
writes it, defaults its location, discovers credentials, or adds a fifth
adapter family.

---

## Judgment calls made while planning

1. **The registry's root overrides the manifest's `root:`.** The spec
   settles that a project's *name* comes from its manifest, and is
   silent on its root. `manifests/ttui.yaml` declares
   `root: <projects-root>/TTUI`, which exists on no machine — the
   placeholder is deliberate, because the manifest is checked into a
   repository that gets cloned to different paths. A registry entry, by
   contrast, is local configuration that knows where the clone actually
   is. Decided (Task 7): the registry overwrites `project.root` with the
   directory it found the manifest in. **If this is wrong**, the
   alternative is to treat a declared `root:` as authoritative and let
   the placeholder fail loudly at scan time, which turns every checked-in
   manifest into a per-machine file.

2. **One runner per `command` entry, handed out by a factory closure in
   declaration order.** The spec says factories rather than values and
   gives the reason; it does not say how a test scripts several
   different runners. Decided (Task 6): the closure counts its calls.
   The alternative — a runner that inspects the command string and
   answers accordingly — reads better but couples the test to the
   manifest's exact command text.

3. **`from_manifest_with` panics on a `command` entry with no command,
   rather than returning a `Result`.** The spec's "no error path" is
   what makes the factory pleasant to call; the cost is one `expect` in
   otherwise panic-free library code. Decided (Task 4): keep it, and
   pin the validator's guarantee with its own test (Task 5) so the
   `expect` has a named guard rather than a hope. **If this is wrong**,
   the change is mechanical: return `Result<ProjectAdapters,
   ValidationError>` and let every caller `?` it.

4. **Arc order puts the registry last.** It is independent and could
   ship first. Decided: Panopticon's Arc 1 blocks on the factory, not on
   the registry — a cockpit can take `--projects-root` on the command
   line and hand-roll a list for one release, but it cannot build a
   single adapter without Task 4.

---

## Execution handoff

Everything executes in `<projects-root>/Parallax` on a worktree branch
per `git-github-standards.md`, one Gated PR per Arc with all four checks
green, squash-merged.

Two notes:

- **Arc 2 is the one that matters.** If time runs short, Arcs 1 and 2
  shipped without Arc 3 still unblock Panopticon; Arc 3 without Arc 2
  unblocks nothing.
- **Task 6 is a tripwire, not a chore.** Its whole value is that the
  assertions do not change. An edited assertion there is a finding about
  the factory, and it should be raised rather than accommodated.
