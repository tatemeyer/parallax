# Baseline — Registry and Adapter Factory (Design)

**Status:** proposed — awaiting sign-off. Amends an approved spec.
**Date:** 2026-08-18

**Amends:**
`docs/design/specs/parallax/2026-08-14-parallax-platform-design.md`,
whose sub-project #2 is implemented through Arc 7 and on `main`. Nothing
below contradicts that document; this fills two holes it named and did
not detail.

**Found by:**
`docs/design/specs/panopticon/2026-08-18-panopticon-observe-design.md`,
open question 3. Writing the cockpit's design against baseline's
*implemented* API rather than its plan surfaced both gaps. They are
specified here rather than there because every frontend needs them, and
the second frontend must not have to reimplement the first one's
guesses.

## Context / Motivation

The master design describes sub-project #2 as "**registry**, manifest,
transport." Two of those three shipped.

**There is no registry.** `manifest::parse_manifest_file` reads *a*
manifest from *a* path. Nothing in the crate answers "which projects are
registered," so `aggregate(&mut [(Validated, ProjectAdapters)], now)` —
the entry point the whole library exists to serve — takes a list its
caller must have assembled from somewhere the library does not define.

**There is no adapter factory.** `ProjectAdapters` is a struct with four
public fields, and every caller fills them in by hand. `aggregate_replay.rs`
does it across 40 lines that read the manifest with their eyes: the
manifest says `adapter: github`, so the test writes
`GithubWorkAdapter::new(...)`; the manifest says
`watch: .plumb/runs/**`, so the test writes
`CaptureArtifactAdapter::new(".plumb/runs/**")`. That translation is the
manifest's *meaning*, and it currently lives nowhere.

Both gaps have the same failure mode if left alone: the first frontend
invents an answer, the second invents a different one, and a manifest
means two things.

## Design

### The registry

```rust
pub struct RegisteredProject {
    pub name: String,
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: Validated,
}

pub struct RegistryError { pub source: PathBuf, pub problem: String }

pub struct Registry {
    projects: Vec<RegisteredProject>,
    failures: Vec<RegistryError>,
}

impl Registry {
    pub fn from_roots(roots: &[PathBuf]) -> Self;
    pub fn from_file(path: &Path) -> Result<Self, RegistryError>;
    pub fn scan(projects_root: &Path) -> Self;
    pub fn projects(&self) -> &[RegisteredProject];
    pub fn failures(&self) -> &[RegistryError];
}
```

Three ways in, one type out:

- **`from_roots`** — an explicit list of project directories, each
  expected to contain a `parallax.yaml`.
- **`from_file`** — a registry file listing those roots. Format below.
- **`scan`** — every immediate child of a directory that contains a
  `parallax.yaml`. This is the zero-configuration path for the common
  case where projects are siblings.

**Load failures degrade one project, never the registry.** A registered
project whose manifest is missing, unparseable, or invalid becomes a
`RegistryError` in `failures()` and every other project still loads.
This is the same rule aggregation already follows for adapters — a
blank view is a worse failure than one row labelled broken — and it is
the reason `from_roots` and `scan` return `Self` rather than `Result`.
`from_file` is the one fallible constructor, because a registry file
that cannot be read is not a partial answer; it is no answer.

**Order is registration order**, preserved from the file or from the
sorted directory listing, because `PlatformState::projects` is
documented as "one entry per registered project, in registration order"
and a frontend's project rail must not reshuffle between refreshes.

**The registry file format**, deliberately smaller than the manifest:

```yaml
apiVersion: parallax/v1
projects:
  - root: C:/Users/tatem/Dev/TTUI
  - root: C:/Users/tatem/Dev/Model-Experiments
```

`root` is the only required key. A project's *name* comes from its own
manifest, never from the registry — one source of truth for identity,
and a rename in `parallax.yaml` cannot desynchronize from a list
somewhere else. `deny_unknown_fields`, as with the manifest, so a typo
is an error rather than a silent omission.

**Where the file lives is the caller's decision, not the library's.**
`Registry::from_file` takes a path. `parallax-baseline` never consults
an environment variable, never expands `~`, and never picks a default
location — that is frontend configuration, and a headless library that
reaches for `$HOME` is one that cannot be tested twice on the same
machine.

### The adapter factory

```rust
pub struct AdapterConfig {
    pub poll_interval: Duration,
    pub github_token: Option<String>,
}

pub fn from_manifest(validated: &Validated, config: &AdapterConfig) -> ProjectAdapters;

pub fn from_manifest_with<T, R>(
    validated: &Validated,
    config: &AdapterConfig,
    transport: impl Fn() -> T,
    runner: impl Fn() -> R,
) -> ProjectAdapters
where
    T: HttpTransport + 'static,
    R: CommandRunner + 'static;
```

`from_manifest` is the convenience wrapper: `UreqTransport` for work,
`ProcessRunner` for `command` verification. `from_manifest_with` is the
real function, and it takes **factories rather than values** because a
manifest can declare several `command` checks and each adapter owns its
runner.

That second signature is what makes fixture mode possible without a
parallel implementation: the cockpit's `--fixtures` path calls
`from_manifest_with(&validated, &config, || fixture_transport(), || scripted_runner())`
and gets adapters wired exactly as the manifest declares, differing only
in where their bytes come from. Fixture mode testing the same
translation as production is the entire point.

**The translation, stated so it is reviewable:**

| Manifest | Adapter |
|---|---|
| `work.adapter: github` | `GithubWorkAdapter::new(transport()).with_interval(config.poll_interval)` |
| `verification[].adapter: command` | `CommandVerificationAdapter::new(kind, command, runner())` |
| `verification[].adapter: plumb` | `PlumbVerificationAdapter::new(kind, runs_dir(entry))` |
| `artifacts[]` where `Validated::artifact_adapter` resolves `figure` | `FigureArtifactAdapter::new(watch)` |
| … resolves `metrics` | `MetricsArtifactAdapter::new(watch)` |
| … resolves `capture` | `CaptureArtifactAdapter::new(watch)` |
| `sessions.watch` | `FilesystemSessionAdapter::new(watch)` |

Two details the table hides, both real decisions:

1. **`PlumbVerificationAdapter` needs a runs directory and the manifest
   declares a config path.** TTUI's manifest says
   `config: .plumb/config.yaml`; its runs live at `.plumb/runs/`. The
   convention is therefore **`<config parent>/runs`, resolved against
   the project root** — `.plumb/config.yaml` → `<root>/.plumb/runs`. It
   is a convention rather than a declaration, so it is written down
   here and given a manifest escape hatch the day a project disagrees:
   an optional `verification[].runs` key. Not added now, because no
   project needs it and a schema field with no user is a field that
   ossifies wrong.
2. **A validated manifest cannot produce an adapter that fails to
   construct.** `validate` already rejects a `command` check with no
   command, a `work.repo` that is not `owner/name`, and an unparseable
   watch glob. The factory therefore takes `&Validated` — not
   `&Manifest` — and has no error path. That is the same
   private-constructor discipline `Validated` and `Authorized` already
   use twice, applied a third time.

### Why both go in baseline rather than in the cockpit

The manifest's meaning is a platform contract. `adapter: github` means
"poll GitHub with conditional requests at the configured interval," and
that sentence has to have exactly one implementation or the manifest
stops being a specification. A frontend that translates manifests is a
frontend that owns part of the schema.

The registry is the same argument one level up: "which projects are
registered" is a platform question, and a daemon — explicitly deferred,
explicitly kept possible — would need the identical answer.

## Non-goals

- **Watching the registry for changes.** `Registry::load` is a read.
  Re-reading is the caller's decision, exactly as re-polling is.
- **Writing the registry.** Nothing in baseline registers a project;
  editing the file is a text-editor operation. A `parallax register`
  CLI is a fine idea and is not this.
- **Defaulting the registry location.** Stated above: the library takes
  a path.
- **Environment or credential discovery.** `AdapterConfig.github_token`
  is passed in. Reading `GITHUB_TOKEN` or shelling out to `gh auth` is
  frontend work, kept out of a library that must run identically in a
  test.
- **A fifth adapter family, or a plugin registry for third-party
  adapters.** The traits are already public and a frontend can build
  `ProjectAdapters` by hand; the factory covers the built-ins.

## Testing

- **The translation table above is a test per row**, asserted through
  each adapter's `source_name()` on the constructed `ProjectAdapters`,
  so a mis-wired family fails loudly rather than silently producing an
  empty pane.
- **Both real manifests build their full adapter sets** —
  `manifests/ttui.yaml` yields one work adapter, three verification
  adapters, one artifact adapter, one session adapter;
  `manifests/model-experiments.yaml` yields one work, two verification,
  two artifact, and **no** session adapter.
- **A partial manifest yields a partial adapter set**, not an error: a
  manifest declaring only `work:` produces exactly one adapter.
- **The Plumb runs-directory convention is pinned by a test**, because
  it is a convention and conventions rot silently.
- **A registry with one broken project loads the rest**, and the broken
  one appears in `failures()` naming the file and the problem.
- **`scan` ignores a directory with no `parallax.yaml`**, and a registry
  file with an unknown key is an error.
- **End to end**: a registry over both real manifests, through the
  factory with a fixture transport and scripted runner, into
  `aggregate` — the same assertions `aggregate_replay.rs` makes today,
  reached without any hand-built adapter.

No test reads `$HOME`, touches the network, or needs a TTY.

## Critical files

```
baseline/src/
  registry.rs            - Registry, RegisteredProject, RegistryError
  adapters/factory.rs    - AdapterConfig, from_manifest, from_manifest_with
baseline/tests/
  registry.rs            - load, degrade, scan, order
  factory_replay.rs      - the translation table, end to end into aggregate
baseline/tests/fixtures/
  registry/…             - a registry file and two project trees
```

`adapters/mod.rs` gains `pub mod factory;` and `lib.rs` gains
`pub mod registry;`. Nothing existing changes shape: the factory
constructs the adapters that already exist, and the registry wraps the
parser that already exists.

## Verification

- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D
  warnings`, `cargo fmt --check` clean.
- `aggregate_replay.rs`'s hand-built `ttui_adapters(...)` helper is
  replaced by a `from_manifest_with` call and its assertions still pass
  unchanged — which is the real proof that the factory encodes what the
  manifest was already understood to mean.
- A registry containing a project with a deliberately corrupt manifest
  produces a `PlatformState` for every other project.
- No new dependency is added to `baseline/Cargo.toml`.

## Open questions for sign-off

1. **Is the registry file the right shape?** It carries only roots
   today. The alternative — letting the registry override a project's
   display name or poll interval — is deliberately rejected above
   (identity has one source), but a per-project poll interval is the
   one override with a plausible case: a repo nobody is working on does
   not need a 30-second poll.
2. **`<config parent>/runs`, or an explicit `runs:` key now?** The
   convention costs nothing today and one manifest change later; the
   key costs a schema field with no current user.
3. **Should `from_manifest_with` take factories or a single builder
   trait?** Factories are simpler and match how the adapters are
   already constructed; a trait would be more extensible and is
   probably premature.
4. **Does this land as one plan or two?** They are independent — the
   factory is useful without the registry and vice versa — but they
   share the same consumer and would review naturally together.
