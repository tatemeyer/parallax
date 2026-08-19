# parallax-baseline

The Parallax platform core. It holds every registered project's declared
references — a manifest, the normalized autonomy axes, adapters over
work/verification/artifacts/sessions, aggregated state with per-source
freshness, and control actions — and folds them into one cross-project
`PlatformState` that any frontend can render. It **never touches a
terminal**: no UI, no TTY, no rendering, no `print!` outside tests. The
cockpit is this library's first frontend, not its only possible one.

Sub-project #2 of [Parallax](../README.md). It shares **no dependency**
with sub-project #1, Plumb, in either direction: the platform consumes
Plumb's rendered `verdict.md` as a file on disk and re-parses the text,
rather than linking the crate that wrote it. A test asserts that
`Cargo.toml` never grows a `parallax-plumb` dependency.

## The manifest

A project joins the platform by dropping a `parallax.yaml` in its root —
which is where they live: `tatemeyer/ttui`, `tatemeyer/SESH`, and
`tatemeyer/Model-Experiments` each carry their own. The copies under
`baseline/tests/fixtures/manifests/` are recorded for testing and will
drift; the file in the project is the one that counts.

A worked example, close to what TTUI carries:

```yaml
apiVersion: parallax/v1
project:
  name: ttui
  root: <projects-root>/TTUI
  language: rust
  methodology: methodology-first     # informational only
work:
  adapter: github
  repo: tatemeyer/ttui
  autonomy_map:
    direct: { implement: agent, merge: direct-push }
    gated:  { implement: agent, merge: on-checks }
    human:  { implement: agent, merge: human-approval }
verification:
  - kind: lint
    adapter: command
    command: cargo clippy --all-targets -- -D warnings
  - kind: tests
    adapter: command
    command: cargo test
  - kind: perceptual
    adapter: plumb
    config: .plumb/config.yaml
artifacts:
  - kind: capture
    watch: .plumb/runs/**
sessions:
  watch: .claude/worktrees/*
```

Every section but `project:` is optional. **Partial support is normal,
not an error path**: a manifest declaring only `work:` parses,
validates, and aggregates into a valid, reduced view — an absent family
is empty, never a failure. Unknown keys, by contrast, are rejected, so a
typo'd `verifications:` cannot silently vanish.

`methodology:` is informational metadata only. Nothing in this crate
branches on it, and a test asserts that positively: two manifests
identical but for that field aggregate to identical state.

## Getting from a directory to state

Two calls, neither of which a frontend should have to write itself.

**Which projects are registered** — an explicit list of roots, a
registry file, or a scan of a directory holding sibling projects:

```rust
let registry = Registry::scan(Path::new("C:/Users/tatem/Dev"));
// or: Registry::from_file(Path::new("~/.parallax/registry.yaml"))  <- caller expands the path
```

The library takes a path and never consults the environment — where the
registry lives is frontend configuration. A project whose manifest is
missing or invalid becomes a `RegistryError` in `failures()` and every
other project still loads. The registry's root wins over the manifest's
declared `root:`, because a manifest is checked into a repository that
gets cloned to different paths while the registry is local
configuration that knows where the clone actually is.

**What serves each project** — the manifest's declared adapters:

```rust
let adapters = from_manifest(&project.manifest, &AdapterConfig::default());
let state = aggregate_project(&project.manifest, &mut adapters, SystemTime::now());
```

`from_manifest_with` is the same function taking transport and runner
factories, which is how a caller drives the identical translation
against recorded fixtures instead of the live world. The translation is
the manifest's meaning and lives in exactly one place: a frontend that
translated manifests itself would own part of the schema.

`VerificationAdapter::cost()` tells a scheduler which checks are safe to
poll. The `plumb` adapter reads a `verdict.md` and reports `Read`; the
`command` adapter runs what the manifest declared — for TTUI, `cargo
clippy` and `cargo test` — and reports `Execute`. An adapter that
overrides nothing is assumed to execute, because a reader misclassified
as an executor merely refreshes less often than it could, while an
executor misclassified as a reader spawns processes in a loop.

## The three autonomy axes

Not one ladder. Each consumer repo collapses two or three independent
axes into a single label, and separating them is what makes their
schemes comparable at all:

```
implement:  agent | human-only          who may do the work
merge:      on-checks | human-approval | direct-push
readiness:  verifiable | needs-intent   is "done" even defined yet
```

Each project's native labels project onto them:

| Native label | implement | merge | readiness |
|---|---|---|---|
| TTUI `direct` | agent | direct-push | verifiable |
| TTUI `gated` | agent | on-checks | verifiable |
| TTUI `human` | agent | human-approval | verifiable |
| ME `autonomy:safe` | agent | on-checks | verifiable |
| ME `autonomy:review` | agent | human-approval | verifiable |
| ME `autonomy:human` | human-only | — | verifiable |
| ME `needs-intent` | — | — | needs-intent |

A `—` is *no claim on that axis* — never a default and never an error.
Every row is a test case. The two asymmetries the shared vocabulary
exists to surface fall straight out of the table: Model-Experiments has
no direct-push tier, and TTUI reserves no work from the agent.

A real work item carries several labels at once, so `autonomy::resolve`
combines them per axis: a stated claim always beats "no claim", and
between two claims the more restrictive one wins. Labels the manifest
never declared are reported as unmapped rather than dropped — an
ordinary issue label is not an autonomy statement.

## The four adapter families

| Family | Built-ins |
|---|---|
| `work` | `github` |
| `verification` | `command`, `plumb` |
| `artifact` | `figure`, `metrics`, `capture` |
| `session` | filesystem watch |

Each family is one object-safe trait, so aggregation holds heterogeneous
adapters as `Box<dyn _>` and a frontend can register an adapter this
crate has never heard of. Every method takes an injected `now`, which
keeps `SystemTime::now()` confined to the outermost caller and makes
freshness testable without a sleep.

Every adapter returns an `Observed<T>` — a value stamped with when and
how it was seen — so freshness travels with the value instead of living
in a frontend's head. **One failing adapter degrades one source, never
the whole view**: a source that cannot be read records a `Degradation`
and stays in `ProjectState::sources` as `Unavailable`, because a blank
cockpit is a worse failure than a number labelled stale, and a source
that vanished from the list would read as one that was never declared.

## Control actions

Control is plain data plus a plain API, so every action is available
headless. Actions are classified by reversibility, exactly as the
platform spec classifies them — reversible: rule on a finding, set an
autonomy label, request a re-review, trigger a capture, dispatch an
agent run; confirmation required: stop a running agent, merge a pull
request, push.

A `Confirmation` fingerprints the exact action it approves, so
confirming "merge #12" cannot execute "merge #99". `ActionExecutor::
execute` takes an `Authorized`, whose field is private — the only way to
obtain one is `authorize`, so "did you check confirmation?" is not a
question an executor implementer can get wrong. A `compile_fail`
doctest proves there is no other way to build one.

## Testing

Everything is exercised with no TTY, no network, and no wall clock. Each
adapter replays a recorded fixture from `baseline/tests/fixtures/`:
trimmed real GitHub responses, sample Plumb verdicts, a JSONL metrics
feed. Live GitHub access is real-external-service exempt and confined to
`UreqTransport`, the one type in the crate that touches the network; it
holds no logic beyond mapping a status code onto an `AdapterError`.

`baseline/tests/verification_sweep.rs` is the platform spec's
Verification section rendered as an executable checklist, one test per
bullet in the spec's own order.

```
cargo test -p parallax-baseline
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Design of record

`docs/design/specs/parallax/2026-08-14-parallax-platform-design.md`, with
the implementation plan at
`docs/design/plans/parallax/2026-08-14-parallax-baseline-plan.md`.
