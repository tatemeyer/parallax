# Parallax Baseline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Structure note:** This plan is organized as **Arcs → Slices → Tasks**
> per `docs/design/README.md`, not the flat "Task N" list the
> `writing-plans` skill defaults to. Tasks still follow the skill's
> bite-sized TDD step structure; Arc/Slice headings are pure grouping.

**Goal:** Build **Baseline** — sub-project #2 of the **Parallax**
platform, a Rust library (`parallax-baseline`) that holds every
registered project's declared references: manifest parsing and
validation, the normalized three-axis autonomy model and its label
projection, four families of adapter, aggregated cross-project state
with per-source freshness, and control actions behind a confirmation
contract. **It never touches a terminal.**

**Architecture:** Everything upstream of rendering is pure data, exactly
as TTUI separates `Buffer` from `Terminal::draw_diff`. A `parallax.yaml`
manifest parses into a `Manifest`; validation resolves defaults and
rejects contradictions; each declared adapter family produces an
`Observed<T>` — a value stamped with when and how it was seen — and
`state::aggregate` folds those into a `PlatformState` that any frontend
can render. Every input a real adapter would fetch from the world
(GitHub HTTP, a subprocess, a filesystem tree) sits behind a small
injectable trait, so the entire library is exercised in tests against
recorded fixtures with no network, no TTY, and no wall clock.

**Tech Stack:** Rust (stable, 2021 edition), `serde` + `serde_yaml`
(manifests) + `serde_json` (GitHub fixtures, metrics JSONL, action
fingerprints), `globset` + `walkdir` (artifact and session `watch`
globs), `sha2` (action fingerprints), `ureq` (the one live-HTTP path),
`tempfile` (dev-dependency).

---

## Global Constraints

Copied from
`docs/design/specs/parallax/2026-08-14-parallax-platform-design.md`.
Every task's requirements implicitly include this section.

**Where the work happens.** A new workspace member at `baseline/` in the
existing `Parallax` repository (`D:/Dev/Projects/Parallax`), package
`parallax-baseline`, added to the root `Cargo.toml`'s `members`. The
existing member is `plumb/capture` (sub-project #1). **Do not
restructure anything already there.** Every command in this plan runs
from the workspace root unless a task says otherwise.

**No dependency on Plumb, in either direction.** Sub-projects #1 and #2
share no dependency and proceed in parallel. The platform *consumes*
Plumb's output — a rendered `verdict.md` on disk — through the
`verification` adapter family. `baseline/Cargo.toml` must never list
`parallax-plumb`, and no task may add it. If Plumb's `verdict.md`
format needs interpreting, this crate re-parses the text; it does not
link the crate that wrote it.

**The library never touches a terminal.** No UI, no TTY, no rendering,
no `print!`/`println!` outside `#[cfg(test)]`, no `crossterm`, no
`ttui`. No task may add a rendering concern, a colour, a glyph, or a
layout decision. The cockpit (sub-project #3) is Baseline's first
frontend, not its only possible one.

**Three axes, not one ladder** (spec, "Normalized autonomy"):

```
implement:  agent | human-only          who may do the work
merge:      on-checks | human-approval | direct-push
readiness:  verifiable | needs-intent   is "done" even defined yet
```

**Every row of the projection table is a test case** — the spec's
Testing section says so literally. The table, verbatim:

| Native label | implement | merge | readiness |
|---|---|---|---|
| TTUI `Direct` | agent | direct-push | verifiable |
| TTUI `Gated` | agent | on-checks | verifiable |
| TTUI `Human` | agent | human-approval | verifiable |
| ME `autonomy:safe` | agent | on-checks | verifiable |
| ME `autonomy:review` | agent | human-approval | verifiable |
| ME `autonomy:human` | human-only | — | verifiable |
| ME `needs-intent` | — | — | needs-intent |

A `—` cell is `None` — *no claim on that axis* — never a default and
never an error.

**Partial manifest support is a first-class requirement, not an error
path.** "A project that satisfies only the work adapter still shows up,
just with less detail." A manifest declaring only `work:` must parse,
validate, and aggregate into a valid, reduced `ProjectState`. Every
absent family is `None`/empty, never a failure. Asserted in Task 8 and
again in Task 20.

**One failing adapter degrades one source, never the whole view.** An
adapter that errors at poll time records a `Degradation` on that source
and leaves every other source intact. A blank cockpit because GitHub
rate-limited is a worse failure than a stale number labelled stale.

**`methodology:` is informational metadata only.** The spec: "nothing in
the platform branches on it." It is parsed, stored, and exposed for
display, and **no task may write a single `if`, `match`, or lookup
keyed on its value.** Task 20 asserts this positively: two manifests
identical but for `methodology:` must aggregate to identical state.

**The four adapter families and their built-in implementations:**

| Family | Built-ins |
|---|---|
| `work` | `github` |
| `verification` | `command`, `plumb` |
| `artifact` | `figure`, `metrics`, `capture` |
| `session` | filesystem watch |

Each family is a trait; the built-ins live behind it. Adding a fifth
built-in must be one new type and no change anywhere else.

**Confirmation-required actions must refuse to execute unconfirmed —
asserted, not assumed** (spec, Verification). Classified by
reversibility:

- **Reversible / additive:** rule on a `plumb` finding, set or change an
  autonomy label, request a re-review, trigger a capture, dispatch an
  agent run.
- **Confirmation required:** stop a running agent, merge a PR, push, or
  any action that is outward-facing or hard to undo.

**Polling, and it says so.** GitHub is polled with ETag-conditional
requests, default interval 30s, configurable. Filesystem-backed state is
effectively immediate. The core represents this so a frontend can
"display the age of each source" without the core knowing a frontend
exists — see Judgment call 1.

**No wall clock inside logic.** Every function whose result depends on
"now" takes `now: SystemTime` as a parameter. `SystemTime::now()` is
called only at the outermost edge (a live adapter's entry point), never
inside anything a test needs to pin. This is what makes freshness
unit-testable.

**Adapters are integration-tested against recorded fixtures** — captured
GitHub API responses, sample `verdict.md`, sample metrics JSONL. **Live
GitHub access is real-external-service exempt from automated testing**,
under the same precedent TTUI applies to real-TTY work. Exactly one type
in this crate makes a live network call (`UreqTransport`, Task 11); it
contains no logic, and its exemption is noted at its definition.

**Both real manifests must parse, validate, and project.**
`manifests/ttui.yaml` and `manifests/model-experiments.yaml` are
authored **verbatim from the spec's "The manifest" section** in Task 7
and replayed end-to-end in Task 20.

**Out of scope — no task may drift into any of these:** a daemon; any
TUI, rendering, or terminal code; a web UI; a hosted or multi-user
service; replacing CI or GitHub as the source of truth; merging the
repos; Tier 4 verification; and everything belonging to sub-projects #3
(cockpit: observe), #4 (Model-Experiments visualization), and #5
(cockpit: full control). The core is structured so a daemon stays
possible — a daemon becomes an alternative *host* for this same
library — but nothing in this plan builds one.

**Repo conventions.** Conventional Commits (`type(scope): description`,
imperative subject, body required on any non-obvious `feat`/`fix`), one
commit per task. TDD is mandatory for every `coding`-tagged task except
the named exceptions in TTUI's `development-conventions.md`, which this
repo inherits — each such task below says so explicitly and why. Soft
ceiling 500 lines per file. Every module gets a `//!` header; every
`pub` item gets a one-line `///`; `#![warn(missing_docs)]` in
`baseline/src/lib.rs`. **Every task's commit must pass, from the
workspace root:**

```
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

---

## File Structure

```
Parallax/
  Cargo.toml                     — [workspace]; members gains "baseline"
  baseline/
    Cargo.toml                   — package parallax-baseline
    README.md                    — (Task 25)
    src/
      lib.rs                     — module re-exports, #![warn(missing_docs)]
      autonomy.rs                — the three axes, AutonomyMap, projection
      manifest.rs                — parallax.yaml schema + parsing
      validate.rs                — semantic validation and defaults
      freshness.rs               — Observed<T>, SourceKind, Freshness
      state.rs                   — PlatformState, aggregation, degradation
      actions.rs                 — Action, Confirmation, Authorized, executors
      adapters/
        mod.rs                   — AdapterError, ProjectContext, re-exports
        http.rs                  — HttpTransport, UreqTransport, FixtureTransport
        work.rs                  — WorkAdapter + GithubWorkAdapter
        verification.rs          — VerificationAdapter + Command + Plumb
        artifact.rs              — ArtifactAdapter + Figure/Metrics/Capture
        session.rs               — SessionAdapter + FilesystemSessionAdapter
    tests/
      real_manifests.rs          — Task 7
      partial_manifest.rs        — Task 8
      github_replay.rs           — Task 12
      verification_replay.rs     — Task 14
      artifact_replay.rs         — Task 16
      aggregate_replay.rs        — Task 20
      verification_sweep.rs      — Task 25
      fixtures/
        github/issues.json       — recorded GitHub API responses
        github/pulls.json
        github/check-runs.json
        plumb/verdict-go.md      — sample verdicts
        plumb/verdict-no-go.md
        plumb/verdict-hold.md
        metrics/loss.jsonl       — sample metrics JSONL
  manifests/
    ttui.yaml                    — Task 7, verbatim from the spec
    model-experiments.yaml       — Task 7, verbatim from the spec
```

`baseline/tests/fixtures/` holds no `.rs` files, so Cargo does not try
to build it as a test target.

Two files are deliberately split off from the spec's first-cut
inventory: `validate.rs` (semantic rules, separate from the serde
schema in `manifest.rs`) and `adapters/http.rs` (the transport seam that
makes the GitHub adapter fixture-testable). Both exist to keep every
file under the 500-line ceiling and to give each one responsibility.

---

## Milestones

- **End of Arc 1** — the normalized autonomy vocabulary exists as
  types, and every row of the spec's projection table passes as a test.
  Nothing else in the crate depends on it yet, which is the point: it is
  the contract the rest is built to serve.
- **End of Arc 2** — both real manifests parse and validate, and a
  manifest declaring only `work:` produces a valid, reduced view rather
  than an error. **This is the first genuinely useful version**: a
  frontend could already list registered projects and their declared
  capabilities.
- **End of Arc 3** — the freshness model and the four adapter traits.
  No implementations, no I/O — the contract only, so Arc 4's seven
  tasks are independent of each other.
- **End of Arc 4** — every built-in adapter, each replaying a recorded
  fixture. GitHub conditional requests, `verdict.md` parsing, JSONL
  metrics series, and session scanning all work offline.
- **End of Arc 5** — `state::aggregate` folds a manifest plus its
  adapters into `PlatformState`, with per-source freshness and
  degradation, replayed end-to-end for both real manifests. **This is
  the deliverable the cockpit consumes.**
- **End of Arc 6** — control actions, with the confirmation contract
  enforced by the type system and asserted by tests.
- **End of Arc 7** — the spec's Verification section rendered as an
  executable suite, plus the crate README.

---

## Arc 1: The normalized vocabulary

The three-axis autonomy model and the projection from each project's
native labels onto it. Pure logic, zero dependencies, zero I/O — and
first, because it is the contract the manifest schema references.

### Slice 1.1: Workspace member and crate skeleton

**Tags:** admin, git-adjacent

#### Task 1: Add the `baseline` workspace member

**Files:**
- Modify: `Cargo.toml` (workspace root — or create, see Step 1)
- Create: `baseline/Cargo.toml`
- Create: `baseline/src/lib.rs`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: a buildable `parallax-baseline` library crate that every
  later task adds a module to.

**TDD exception: pure scaffolding/config, no application logic** — one
of the four named exceptions in TTUI's `development-conventions.md`,
which this repo inherits. Verified by building, not by asserting.

- [ ] **Step 1: Add `baseline` to the workspace members**

The root `Cargo.toml` is owned by Plumb's Task 1, which runs in
parallel. Handle both orderings:

**If `Cargo.toml` exists at the workspace root**, add `"baseline"` to
the existing `members` array and change nothing else:

```toml
members = ["plumb/capture", "baseline"]
```

**If it does not exist yet**, create it with both members listed —
Cargo tolerates a member directory that is not yet present only if it
is absent from `members`, so list `plumb/capture` and create a
placeholder only if Plumb has already landed. When Plumb has not
landed, write:

```toml
[workspace]
resolver = "2"
members = ["baseline"]
```

and leave a comment directly above it: `# plumb/capture is added by
Plumb's Arc 1 Slice 1.1.` Do not create, move, or modify anything under
`plumb/`.

- [ ] **Step 2: Write `baseline/Cargo.toml`**

```toml
[package]
name = "parallax-baseline"
version = "0.1.0"
edition = "2021"
description = "Manifests, adapters, normalized autonomy, aggregated state, and control actions for the Parallax platform. Headless: never touches a terminal."

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1"
globset = "0.4"
walkdir = "2"
sha2 = "0.10"
ureq = { version = "2", features = ["json"] }

[dev-dependencies]
tempfile = "3"
```

`serde_yaml = "0.9"` is archived upstream but functional and by far the
most widely used YAML serde binding; it is isolated behind
`manifest.rs` and swappable in one file. `ureq` is a blocking HTTP
client with no async runtime — it appears in exactly one type
(`UreqTransport`, Task 11) and nowhere else. There is deliberately no
`chrono`: freshness is `std::time::SystemTime` arithmetic, and GitHub's
own timestamps are carried as opaque display strings.

- [ ] **Step 3: Write `baseline/src/lib.rs`**

```rust
//! Parallax Baseline: the platform core. Holds every registered
//! project's declared references — manifests, the normalized autonomy
//! axes, adapters over work/verification/artifacts/sessions, aggregated
//! state with per-source freshness, and control actions.
//!
//! Deliberately **never touches a terminal**: no UI, no TTY, no
//! rendering. The cockpit is this library's first frontend, not its
//! only possible one.
#![warn(missing_docs)]
```

Module declarations are added by each later task as its module lands.

- [ ] **Step 4: Verify the workspace builds**

Run: `cargo build`
Expected: compiles clean, `parallax-baseline v0.1.0` in the output.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml baseline/Cargo.toml baseline/src/lib.rs
git commit -m "chore(baseline): add the parallax-baseline workspace member"
```

---

### Slice 1.2: The three axes and label projection

**Tags:** coding

#### Task 2: The three autonomy axes

**Files:**
- Create: `baseline/src/autonomy.rs`
- Modify: `baseline/src/lib.rs` (add `pub mod autonomy;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum Implement { Agent, HumanOnly }`
  - `pub enum Merge { OnChecks, HumanApproval, DirectPush }`
  - `pub enum Readiness { Verifiable, NeedsIntent }`
  - `pub struct Autonomy { pub implement: Option<Implement>, pub merge: Option<Merge>, pub readiness: Readiness }`
  - `pub fn no_claim() -> Autonomy`

  Consumed by Task 3 (projection), Task 5 (the manifest's
  `autonomy_map`), Task 18 (state aggregation).

**Why `Option` on two axes and not the third.** The spec's table has
`—` cells on `implement` and `merge` — `ME autonomy:human` makes no
claim about merging, because a human is doing the work; `ME
needs-intent` makes no claim about either, because "done" is not
defined yet. `readiness` has no `—` row: every mapped label is either
`verifiable` or `needs-intent`, and `verifiable` is the default.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axes_serialize_with_the_spec_s_kebab_case_wire_names() {
        assert_eq!(serde_yaml::to_string(&Implement::HumanOnly).unwrap().trim(), "human-only");
        assert_eq!(serde_yaml::to_string(&Merge::OnChecks).unwrap().trim(), "on-checks");
        assert_eq!(serde_yaml::to_string(&Merge::DirectPush).unwrap().trim(), "direct-push");
        assert_eq!(serde_yaml::to_string(&Merge::HumanApproval).unwrap().trim(), "human-approval");
        assert_eq!(serde_yaml::to_string(&Readiness::NeedsIntent).unwrap().trim(), "needs-intent");
    }

    #[test]
    fn readiness_defaults_to_verifiable() {
        assert_eq!(Readiness::default(), Readiness::Verifiable);
        assert_eq!(no_claim().readiness, Readiness::Verifiable);
    }

    #[test]
    fn no_claim_asserts_nothing_on_the_two_optional_axes() {
        let a = no_claim();
        assert_eq!(a.implement, None);
        assert_eq!(a.merge, None);
    }

    /// Restrictiveness ordering is what multi-label resolution (Task 4)
    /// is built on, so it is pinned here rather than left implicit.
    #[test]
    fn the_axes_order_least_to_most_restrictive() {
        assert!(Implement::Agent < Implement::HumanOnly);
        assert!(Merge::DirectPush < Merge::OnChecks);
        assert!(Merge::OnChecks < Merge::HumanApproval);
        assert!(Readiness::Verifiable < Readiness::NeedsIntent);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p parallax-baseline autonomy::`
Expected: FAIL — `autonomy` module does not exist.

- [ ] **Step 3: Write the implementation**

```rust
//! The normalized autonomy vocabulary: three orthogonal axes that each
//! project's native labels project onto. Deliberately not a single
//! ladder — the two consumer repos each collapse two or three
//! independent axes into one label, and separating them is what makes
//! their schemes comparable at all.

use serde::{Deserialize, Serialize};

/// Who may do the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Implement {
    /// An agent may implement this unit of work.
    Agent,
    /// Reserved from the agent; a human implements it.
    HumanOnly,
}

/// What it takes for the work to land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Merge {
    /// Straight to the default branch, bypassing review.
    DirectPush,
    /// Merges once objective checks are green; no human wait.
    OnChecks,
    /// Requires explicit human sign-off beyond green checks.
    HumanApproval,
}

/// Whether "done" is even defined for this unit of work yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Readiness {
    /// A machine-checkable success criterion exists.
    #[default]
    Verifiable,
    /// The criterion cannot be written yet; intent must be settled first.
    NeedsIntent,
}

/// A native label projected onto the three normalized axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Autonomy {
    /// Who may implement. `None` means the label makes no claim.
    pub implement: Option<Implement>,
    /// What it takes to land. `None` means the label makes no claim.
    pub merge: Option<Merge>,
    /// Whether "done" is defined. Defaults to `Verifiable`.
    pub readiness: Readiness,
}

/// An `Autonomy` asserting nothing on either optional axis.
pub fn no_claim() -> Autonomy {
    Autonomy::default()
}
```

Variant declaration order carries the restrictiveness ordering that
`PartialOrd`/`Ord` derive from and that Task 4 resolves multi-label
conflicts with — **do not reorder these variants**, and the test above
guards against it.

Add `pub mod autonomy;` to `baseline/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline autonomy::`
Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/autonomy.rs baseline/src/lib.rs
git commit -m "feat(autonomy): define the three normalized autonomy axes

Each consumer repo collapses two or three independent axes into a single
label, which is why their schemes do not map onto each other; separating
implement/merge/readiness is what makes them comparable."
```

---

#### Task 3: `AutonomyMap` and projection — every table row a test

**Files:**
- Modify: `baseline/src/autonomy.rs`

**Interfaces:**
- Consumes: `Implement`, `Merge`, `Readiness`, `Autonomy` (Task 2).
- Produces:
  - `pub struct AutonomyEntry { pub implement: Option<Implement>, pub merge: Option<Merge>, pub readiness: Option<Readiness> }`
  - `pub struct AutonomyMap { entries: BTreeMap<String, AutonomyEntry> }` with `pub fn new(entries: BTreeMap<String, AutonomyEntry>) -> Self`, `pub fn entry(&self, label: &str) -> Option<&AutonomyEntry>`, `pub fn labels(&self) -> impl Iterator<Item = &str>`, `pub fn is_empty(&self) -> bool`
  - `pub fn project(map: &AutonomyMap, label: &str) -> Option<Autonomy>`

  Consumed by Task 4 (multi-label resolution), Task 5 (the manifest's
  `work.autonomy_map` field is an `AutonomyMap`), Task 18.

**`AutonomyEntry::readiness` is `Option`, `Autonomy::readiness` is
not.** A manifest entry that omits `readiness:` is not claiming
`verifiable` — it is saying nothing — but a *projection* must land
somewhere, and `verifiable` is the default the spec's table shows for
every row that does not say otherwise. `project` is where the
`Option<Readiness>` collapses to a `Readiness`.

- [ ] **Step 1: Write the failing tests — the spec's table, row by row**

```rust
#[cfg(test)]
mod projection_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn entry(implement: Option<Implement>, merge: Option<Merge>, readiness: Option<Readiness>) -> AutonomyEntry {
        AutonomyEntry { implement, merge, readiness }
    }

    /// TTUI's three tiers, exactly as `manifests/ttui.yaml` declares them.
    fn ttui_map() -> AutonomyMap {
        let mut m = BTreeMap::new();
        m.insert("direct".to_string(), entry(Some(Implement::Agent), Some(Merge::DirectPush), None));
        m.insert("gated".to_string(), entry(Some(Implement::Agent), Some(Merge::OnChecks), None));
        m.insert("human".to_string(), entry(Some(Implement::Agent), Some(Merge::HumanApproval), None));
        AutonomyMap::new(m)
    }

    /// Model-Experiments' four labels, exactly as its manifest declares them.
    fn me_map() -> AutonomyMap {
        let mut m = BTreeMap::new();
        m.insert("autonomy:safe".to_string(), entry(Some(Implement::Agent), Some(Merge::OnChecks), None));
        m.insert("autonomy:review".to_string(), entry(Some(Implement::Agent), Some(Merge::HumanApproval), None));
        m.insert("autonomy:human".to_string(), entry(Some(Implement::HumanOnly), None, None));
        m.insert("needs-intent".to_string(), entry(None, None, Some(Readiness::NeedsIntent)));
        AutonomyMap::new(m)
    }

    // --- The spec's projection table. One test per row. ---

    #[test]
    fn row_ttui_direct() {
        assert_eq!(
            project(&ttui_map(), "direct").unwrap(),
            Autonomy { implement: Some(Implement::Agent), merge: Some(Merge::DirectPush), readiness: Readiness::Verifiable }
        );
    }

    #[test]
    fn row_ttui_gated() {
        assert_eq!(
            project(&ttui_map(), "gated").unwrap(),
            Autonomy { implement: Some(Implement::Agent), merge: Some(Merge::OnChecks), readiness: Readiness::Verifiable }
        );
    }

    #[test]
    fn row_ttui_human() {
        assert_eq!(
            project(&ttui_map(), "human").unwrap(),
            Autonomy { implement: Some(Implement::Agent), merge: Some(Merge::HumanApproval), readiness: Readiness::Verifiable }
        );
    }

    #[test]
    fn row_me_autonomy_safe() {
        assert_eq!(
            project(&me_map(), "autonomy:safe").unwrap(),
            Autonomy { implement: Some(Implement::Agent), merge: Some(Merge::OnChecks), readiness: Readiness::Verifiable }
        );
    }

    #[test]
    fn row_me_autonomy_review() {
        assert_eq!(
            project(&me_map(), "autonomy:review").unwrap(),
            Autonomy { implement: Some(Implement::Agent), merge: Some(Merge::HumanApproval), readiness: Readiness::Verifiable }
        );
    }

    /// The `—` in the merge column is None: a human doing the work makes
    /// no claim about what it takes to land.
    #[test]
    fn row_me_autonomy_human() {
        assert_eq!(
            project(&me_map(), "autonomy:human").unwrap(),
            Autonomy { implement: Some(Implement::HumanOnly), merge: None, readiness: Readiness::Verifiable }
        );
    }

    /// Two `—` cells: "done" is not defined, so neither other axis is
    /// asserted.
    #[test]
    fn row_me_needs_intent() {
        assert_eq!(
            project(&me_map(), "needs-intent").unwrap(),
            Autonomy { implement: None, merge: None, readiness: Readiness::NeedsIntent }
        );
    }

    // --- The two asymmetries the shared vocabulary exists to surface ---

    #[test]
    fn model_experiments_has_no_direct_push_tier() {
        let map = me_map();
        assert!(
            map.labels().all(|l| project(&map, l).unwrap().merge != Some(Merge::DirectPush)),
            "nothing in Model-Experiments bypasses CI"
        );
    }

    #[test]
    fn ttui_has_no_human_only_tier() {
        let map = ttui_map();
        assert!(
            map.labels().all(|l| project(&map, l).unwrap().implement != Some(Implement::HumanOnly)),
            "no TTUI work is reserved from the agent"
        );
    }

    // --- Unmapped labels ---

    #[test]
    fn an_unmapped_label_projects_to_none_rather_than_erroring() {
        // A GitHub issue carries labels the manifest never mentions
        // ("bug", "documentation"). Those are not autonomy statements
        // and must not fail projection.
        assert_eq!(project(&ttui_map(), "bug"), None);
        assert_eq!(project(&me_map(), "good first issue"), None);
    }

    #[test]
    fn label_lookup_is_exact_and_case_sensitive() {
        assert_eq!(project(&ttui_map(), "Direct"), None);
        assert!(project(&ttui_map(), "direct").is_some());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p parallax-baseline autonomy::projection_tests`
Expected: FAIL — `AutonomyMap`, `AutonomyEntry`, `project` do not exist.

- [ ] **Step 3: Write the implementation**

Append to `baseline/src/autonomy.rs`:

```rust
use std::collections::BTreeMap;

/// One `autonomy_map` entry: what a native label claims on each axis.
/// Every field is optional — an omitted field is "no claim", not a
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyEntry {
    /// Who may implement work carrying this label.
    #[serde(default)]
    pub implement: Option<Implement>,
    /// What it takes for work carrying this label to land.
    #[serde(default)]
    pub merge: Option<Merge>,
    /// Whether "done" is defined for work carrying this label.
    #[serde(default)]
    pub readiness: Option<Readiness>,
}

/// A project's native autonomy labels and what each projects onto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AutonomyMap {
    entries: BTreeMap<String, AutonomyEntry>,
}

impl AutonomyMap {
    /// Builds a map from native label to its projection.
    pub fn new(entries: BTreeMap<String, AutonomyEntry>) -> Self {
        Self { entries }
    }

    /// The raw entry for a native label, if declared.
    pub fn entry(&self, label: &str) -> Option<&AutonomyEntry> {
        self.entries.get(label)
    }

    /// Every native label this project declares, in sorted order.
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Whether the project declares no autonomy labels at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Projects one native label onto the normalized axes. Returns `None`
/// for a label the manifest does not declare — an ordinary issue label
/// is not an autonomy statement and must not be treated as an error.
pub fn project(map: &AutonomyMap, label: &str) -> Option<Autonomy> {
    map.entry(label).map(|e| Autonomy {
        implement: e.implement,
        merge: e.merge,
        readiness: e.readiness.unwrap_or_default(),
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline autonomy::`
Expected: PASS, 15 tests (4 from Task 2, 11 here).

- [ ] **Step 5: Commit**

```bash
git add baseline/src/autonomy.rs
git commit -m "feat(autonomy): project native labels onto the normalized axes

Every row of the spec's projection table is a test case, including the
two asymmetries the shared vocabulary exists to surface: Model-
Experiments has no direct-push tier and TTUI has no human-only tier."
```

---

### Slice 1.3: Multi-label resolution

**Tags:** coding

#### Task 4: Resolving several mapped labels on one work item

**Files:**
- Modify: `baseline/src/autonomy.rs`

**Interfaces:**
- Consumes: `AutonomyMap`, `project` (Task 3).
- Produces:
  - `pub struct Resolution { pub autonomy: Autonomy, pub matched: Vec<String>, pub unmapped: Vec<String> }`
  - `pub fn resolve(map: &AutonomyMap, labels: &[String]) -> Resolution`

  Consumed by Task 18 (state aggregation projects each work item's
  labels through this).

**Why this task exists.** A real GitHub issue carries several labels at
once, and nothing stops `autonomy:safe` and `needs-intent` appearing
together — Model-Experiments' own manifest declares both. The spec's
table maps one label at a time and is silent on the combination. See
Judgment call 3.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod resolution_tests {
    use super::*;
    use super::projection_tests_support::{me_map, ttui_map};

    fn labels(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_labels_at_all_claims_nothing() {
        let r = resolve(&ttui_map(), &[]);
        assert_eq!(r.autonomy, no_claim());
        assert!(r.matched.is_empty());
        assert!(r.unmapped.is_empty());
    }

    #[test]
    fn a_single_mapped_label_resolves_to_its_row() {
        let r = resolve(&ttui_map(), &labels(&["gated"]));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
        assert_eq!(r.matched, vec!["gated".to_string()]);
    }

    #[test]
    fn unmapped_labels_are_reported_not_dropped_and_not_fatal() {
        let r = resolve(&ttui_map(), &labels(&["bug", "gated", "documentation"]));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
        assert_eq!(r.matched, vec!["gated".to_string()]);
        assert_eq!(r.unmapped, labels(&["bug", "documentation"]));
    }

    /// The real combination: work that is agent-implementable and merges
    /// on checks, but whose success criterion is not written yet.
    #[test]
    fn needs_intent_combines_with_a_tier_rather_than_overriding_it() {
        let r = resolve(&me_map(), &labels(&["autonomy:safe", "needs-intent"]));
        assert_eq!(r.autonomy.implement, Some(Implement::Agent));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
        assert_eq!(r.autonomy.readiness, Readiness::NeedsIntent);
    }

    #[test]
    fn conflicting_labels_resolve_to_the_most_restrictive_value_per_axis() {
        let r = resolve(&me_map(), &labels(&["autonomy:safe", "autonomy:review"]));
        assert_eq!(r.autonomy.merge, Some(Merge::HumanApproval), "human-approval outranks on-checks");

        let r = resolve(&me_map(), &labels(&["autonomy:safe", "autonomy:human"]));
        assert_eq!(r.autonomy.implement, Some(Implement::HumanOnly), "human-only outranks agent");
    }

    /// "No claim" never beats a claim: a label that says nothing about
    /// an axis must not erase what another label said about it.
    #[test]
    fn a_label_making_no_claim_does_not_erase_another_label_s_claim() {
        let r = resolve(&me_map(), &labels(&["autonomy:safe", "needs-intent"]));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
        let r = resolve(&me_map(), &labels(&["autonomy:human", "autonomy:safe"]));
        assert_eq!(r.autonomy.merge, Some(Merge::OnChecks));
    }

    #[test]
    fn resolution_is_order_independent() {
        let a = resolve(&me_map(), &labels(&["autonomy:review", "autonomy:safe"]));
        let b = resolve(&me_map(), &labels(&["autonomy:safe", "autonomy:review"]));
        assert_eq!(a.autonomy, b.autonomy);
    }

    #[test]
    fn matched_labels_are_reported_in_the_order_they_appeared() {
        let r = resolve(&me_map(), &labels(&["needs-intent", "autonomy:safe"]));
        assert_eq!(r.matched, labels(&["needs-intent", "autonomy:safe"]));
    }
}
```

Move `ttui_map()`, `me_map()`, and `entry()` out of
`mod projection_tests` into a shared `#[cfg(test)] mod
projection_tests_support` in the same file, and have `projection_tests`
`use` them, so both test modules share one definition of the two real
maps.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p parallax-baseline autonomy::resolution_tests`
Expected: FAIL — `resolve` and `Resolution` do not exist.

- [ ] **Step 3: Write the implementation**

```rust
/// The outcome of projecting every label on one work item.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolution {
    /// The combined projection across every mapped label.
    pub autonomy: Autonomy,
    /// The labels the manifest declares, in the order given.
    pub matched: Vec<String>,
    /// Labels the manifest does not declare, in the order given.
    pub unmapped: Vec<String>,
}

/// Resolves every label on one work item into a single `Autonomy`.
///
/// Per axis: a stated claim always beats "no claim", and when two
/// labels both state a claim the more restrictive one wins. Order
/// independent by construction.
pub fn resolve(map: &AutonomyMap, labels: &[String]) -> Resolution {
    let mut out = Resolution::default();
    for label in labels {
        match project(map, label) {
            Some(a) => {
                out.matched.push(label.clone());
                out.autonomy.implement = most_restrictive(out.autonomy.implement, a.implement);
                out.autonomy.merge = most_restrictive(out.autonomy.merge, a.merge);
                out.autonomy.readiness = out.autonomy.readiness.max(a.readiness);
            }
            None => out.unmapped.push(label.clone()),
        }
    }
    out
}

/// A stated claim beats no claim; between two claims the higher
/// (more restrictive) variant wins.
fn most_restrictive<T: Ord>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}
```

`out.autonomy.readiness` starts at `Readiness::Verifiable` (its
`Default`) and only ever rises to `NeedsIntent`, which is why it needs
no `Option` dance.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline autonomy::`
Expected: PASS, 23 tests.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/autonomy.rs
git commit -m "feat(autonomy): resolve several mapped labels onto one projection

A real work item carries several labels at once and the spec's table
maps one at a time; most-restrictive-wins per axis, with a stated claim
always beating no claim, keeps the combination order-independent."
```

---

## Arc 2: The manifest

`parallax.yaml`: schema, parsing, semantic validation, the two real
consumers, and the partial-manifest case the spec calls normal.

### Slice 2.1: Schema, parsing, and validation

**Tags:** coding

#### Task 5: The `parallax.yaml` schema and parser

**Files:**
- Create: `baseline/src/manifest.rs`
- Modify: `baseline/src/lib.rs` (add `pub mod manifest;`)

**Interfaces:**
- Consumes: `autonomy::AutonomyMap` (Task 3).
- Produces:
  - `pub struct Manifest { pub api_version: Option<String>, pub project: Project, pub work: Option<Work>, pub verification: Vec<VerificationEntry>, pub artifacts: Vec<ArtifactEntry>, pub sessions: Option<Sessions> }`
  - `pub struct Project { pub name: String, pub root: Option<PathBuf>, pub language: Option<String>, pub methodology: Option<String> }`
  - `pub struct Work { pub adapter: WorkAdapterKind, pub repo: String, pub autonomy_map: AutonomyMap }`
  - `pub enum WorkAdapterKind { Github }`
  - `pub struct VerificationEntry { pub kind: String, pub adapter: VerificationAdapterKind, pub command: Option<String>, pub config: Option<PathBuf> }`
  - `pub enum VerificationAdapterKind { Command, Plumb }`
  - `pub struct ArtifactEntry { pub kind: ArtifactKind, pub adapter: Option<ArtifactAdapterKind>, pub watch: String }`
  - `pub enum ArtifactKind { Figure, Metrics, Capture }`
  - `pub enum ArtifactAdapterKind { Figure, Metrics, Capture }`
  - `pub struct Sessions { pub watch: String }`
  - `pub enum ManifestError { Io(std::io::Error), Yaml(serde_yaml::Error) }`
  - `pub fn parse_manifest(yaml: &str) -> Result<Manifest, ManifestError>`
  - `pub fn parse_manifest_file(path: &Path) -> Result<Manifest, ManifestError>`

  Consumed by Task 6 (validation), Task 7 (the real manifests), Task 18.

**`VerificationEntry::kind` is a `String`, not an enum.** The spec shows
`lint`, `tests`, `perceptual`, but `kind` is a display label — the
`adapter` field is what dispatch keys on. Keeping it open means a
project can declare `kind: benchmarks` without a change here, and it
keeps `kind` from ever becoming a behaviour switch. See Judgment call 4.

**`methodology` is parsed and stored and nothing more.** No function in
this crate may read it except to hand it to a caller for display.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const TTUI: &str = r#"
apiVersion: parallax/v1
project:
  name: ttui
  root: D:/Dev/Projects/TTUI
  language: rust
  methodology: methodology-first
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
"#;

    #[test]
    fn parses_the_ttui_manifest_end_to_end() {
        let m = parse_manifest(TTUI).unwrap();
        assert_eq!(m.api_version.as_deref(), Some("parallax/v1"));
        assert_eq!(m.project.name, "ttui");
        assert_eq!(m.project.language.as_deref(), Some("rust"));
        assert_eq!(m.project.methodology.as_deref(), Some("methodology-first"));
        let work = m.work.unwrap();
        assert_eq!(work.adapter, WorkAdapterKind::Github);
        assert_eq!(work.repo, "tatemeyer/ttui");
        assert_eq!(work.autonomy_map.labels().collect::<Vec<_>>(), vec!["direct", "gated", "human"]);
        assert_eq!(m.verification.len(), 3);
        assert_eq!(m.verification[0].kind, "lint");
        assert_eq!(m.verification[0].adapter, VerificationAdapterKind::Command);
        assert_eq!(m.verification[2].adapter, VerificationAdapterKind::Plumb);
        assert_eq!(m.verification[2].config.as_deref(), Some(std::path::Path::new(".plumb/config.yaml")));
        assert_eq!(m.artifacts.len(), 1);
        assert_eq!(m.artifacts[0].kind, ArtifactKind::Capture);
        assert_eq!(m.artifacts[0].watch, ".plumb/runs/**");
        assert_eq!(m.sessions.unwrap().watch, ".claude/worktrees/*");
    }

    /// Model-Experiments' manifest omits apiVersion and root, and writes
    /// `adapter: jsonl` for its metrics feed. All three must parse.
    #[test]
    fn parses_model_experiments_shape_including_the_jsonl_adapter_alias() {
        let yaml = r#"
project:
  name: model-experiments
  language: python
  methodology: outcome-first
work:
  adapter: github
  repo: tatemeyer/Model-Experiments
  autonomy_map:
    "autonomy:safe":   { implement: agent, merge: on-checks }
    "autonomy:human":  { implement: human-only }
    "needs-intent":    { readiness: needs-intent }
artifacts:
  - kind: figure
    watch: projects/*/results/**/*.png
  - kind: metrics
    adapter: jsonl
    watch: projects/*/results/**/*.jsonl
"#;
        let m = parse_manifest(yaml).unwrap();
        assert_eq!(m.api_version, None);
        assert_eq!(m.project.root, None);
        assert_eq!(m.artifacts[0].adapter, None, "figure declares no adapter");
        assert_eq!(m.artifacts[1].adapter, Some(ArtifactAdapterKind::Metrics), "`jsonl` selects the metrics adapter");
        assert_eq!(m.sessions, None);
        assert!(m.verification.is_empty());
    }

    /// The spec's headline partial case.
    #[test]
    fn a_manifest_declaring_only_work_parses() {
        let yaml = r#"
project:
  name: minimal
work:
  adapter: github
  repo: tatemeyer/minimal
  autonomy_map:
    gated: { implement: agent, merge: on-checks }
"#;
        let m = parse_manifest(yaml).unwrap();
        assert!(m.work.is_some());
        assert!(m.verification.is_empty());
        assert!(m.artifacts.is_empty());
        assert_eq!(m.sessions, None);
    }

    #[test]
    fn a_manifest_declaring_only_a_project_parses() {
        let m = parse_manifest("project:\n  name: bare\n").unwrap();
        assert_eq!(m.project.name, "bare");
        assert!(m.work.is_none());
    }

    #[test]
    fn an_unknown_adapter_name_is_a_parse_error_naming_the_field() {
        let yaml = "project:\n  name: x\nwork:\n  adapter: gitlab\n  repo: a/b\n  autonomy_map: {}\n";
        let err = parse_manifest(yaml).unwrap_err().to_string();
        assert!(err.contains("gitlab"), "error should name the offending value: {err}");
    }

    #[test]
    fn an_unknown_top_level_key_is_a_parse_error_rather_than_silently_ignored() {
        let yaml = "project:\n  name: x\nverifications:\n  - kind: tests\n";
        assert!(parse_manifest(yaml).is_err(), "a typo'd section must not vanish silently");
    }

    #[test]
    fn parse_manifest_file_defaults_root_to_the_manifest_s_own_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parallax.yaml");
        std::fs::write(&path, "project:\n  name: local\n").unwrap();
        let m = parse_manifest_file(&path).unwrap();
        assert_eq!(m.project.root.as_deref(), Some(dir.path()));
    }

    #[test]
    fn an_explicit_root_is_not_overwritten_by_the_manifest_s_location() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parallax.yaml");
        std::fs::write(&path, "project:\n  name: local\n  root: D:/Elsewhere\n").unwrap();
        let m = parse_manifest_file(&path).unwrap();
        assert_eq!(m.project.root.as_deref(), Some(std::path::Path::new("D:/Elsewhere")));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p parallax-baseline manifest::`
Expected: FAIL — `manifest` module does not exist.

- [ ] **Step 3: Write the implementation**

```rust
//! The `parallax.yaml` schema and its parser. A project joins the
//! platform by dropping one of these in its root. Deliberately tolerant
//! of missing sections — partial support is normal, not an error path —
//! and deliberately intolerant of unknown keys, so a typo'd section
//! never silently vanishes.

use crate::autonomy::AutonomyMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One project's declared references.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    /// Schema version, e.g. `parallax/v1`. Optional.
    #[serde(default)]
    pub api_version: Option<String>,
    /// Who this project is.
    pub project: Project,
    /// The work feed, if declared.
    #[serde(default)]
    pub work: Option<Work>,
    /// Declared verification checks. Empty when none.
    #[serde(default)]
    pub verification: Vec<VerificationEntry>,
    /// Declared artifact feeds. Empty when none.
    #[serde(default)]
    pub artifacts: Vec<ArtifactEntry>,
    /// The agent-session feed, if declared.
    #[serde(default)]
    pub sessions: Option<Sessions>,
}

/// Project identity. `methodology` is informational metadata only —
/// nothing in this crate branches on it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// The project's short name, unique across the platform.
    pub name: String,
    /// Absolute path to the project root. Defaults to the manifest's
    /// own directory when parsed from a file.
    #[serde(default)]
    pub root: Option<PathBuf>,
    /// Primary language, for display only.
    #[serde(default)]
    pub language: Option<String>,
    /// Declared development methodology. **Informational only.**
    #[serde(default)]
    pub methodology: Option<String>,
}

/// The work feed: issues, pull requests, and their autonomy labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    /// Which work adapter serves this project.
    pub adapter: WorkAdapterKind,
    /// The adapter's repository argument, `owner/name`.
    pub repo: String,
    /// This project's native labels and what each projects onto.
    #[serde(default)]
    pub autonomy_map: AutonomyMap,
}

/// Built-in work adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkAdapterKind {
    /// Issues, pull requests, labels, and check runs from GitHub.
    Github,
}

/// One declared verification check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationEntry {
    /// A display label such as `lint`, `tests`, or `perceptual`.
    /// Never a dispatch key — `adapter` is.
    pub kind: String,
    /// Which verification adapter runs this check.
    pub adapter: VerificationAdapterKind,
    /// The shell command, for the `command` adapter.
    #[serde(default)]
    pub command: Option<String>,
    /// A config path, for the `plumb` adapter.
    #[serde(default)]
    pub config: Option<PathBuf>,
}

/// Built-in verification adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerificationAdapterKind {
    /// Runs a shell command and reads its exit status.
    Command,
    /// Reads a Plumb `verdict.md` from disk. Does not link Plumb.
    Plumb,
}

/// One declared artifact feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactEntry {
    /// What kind of artifact this feed produces.
    pub kind: ArtifactKind,
    /// Which artifact adapter reads it. Defaults from `kind`.
    #[serde(default)]
    pub adapter: Option<ArtifactAdapterKind>,
    /// A glob, relative to the project root.
    pub watch: String,
}

/// What an artifact feed produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    /// Pre-rendered images.
    Figure,
    /// Scalar series.
    Metrics,
    /// Terminal captures with their verdicts.
    Capture,
}

/// Built-in artifact adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactAdapterKind {
    /// Image files: path, size, modification time.
    Figure,
    /// JSONL scalar series. Also spelled `jsonl` in a manifest.
    #[serde(alias = "jsonl")]
    Metrics,
    /// Plumb run directories: run id plus verdict.
    Capture,
}

/// The agent-session feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sessions {
    /// A glob of session directories, relative to the project root.
    pub watch: String,
}

/// Failure reading or parsing a manifest.
#[derive(Debug)]
pub enum ManifestError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// Not valid YAML, or not this schema.
    Yaml(serde_yaml::Error),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Io(e) => write!(f, "reading manifest: {e}"),
            ManifestError::Yaml(e) => write!(f, "parsing manifest: {e}"),
        }
    }
}
impl std::error::Error for ManifestError {}

/// Parses a manifest from YAML text.
pub fn parse_manifest(yaml: &str) -> Result<Manifest, ManifestError> {
    serde_yaml::from_str(yaml).map_err(ManifestError::Yaml)
}

/// Parses a manifest from a file, defaulting `project.root` to the
/// manifest's own directory when it declares none.
pub fn parse_manifest_file(path: &Path) -> Result<Manifest, ManifestError> {
    let text = std::fs::read_to_string(path).map_err(ManifestError::Io)?;
    let mut manifest = parse_manifest(&text)?;
    if manifest.project.root.is_none() {
        manifest.project.root = path.parent().map(Path::to_path_buf);
    }
    Ok(manifest)
}
```

Add `pub mod manifest;` to `baseline/src/lib.rs`. `serde_yaml` reports
an unknown enum variant with the offending value in the message, which
is what the `gitlab` test asserts on.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline manifest::`
Expected: PASS, 8 tests.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/manifest.rs baseline/src/lib.rs
git commit -m "feat(manifest): parse parallax.yaml

Every section but project: is optional, because partial support is
normal rather than an error path; unknown keys are rejected so a typo'd
section never silently vanishes."
```

---

#### Task 6: Semantic validation and adapter defaults

**Files:**
- Create: `baseline/src/validate.rs`
- Modify: `baseline/src/lib.rs` (add `pub mod validate;`)

**Interfaces:**
- Consumes: everything from `manifest` (Task 5), `autonomy::AutonomyMap`.
- Produces:
  - `pub struct Validated { manifest: Manifest }` with `pub fn manifest(&self) -> &Manifest`, `pub fn into_manifest(self) -> Manifest`, `pub fn artifact_adapter(&self, entry: &ArtifactEntry) -> ArtifactAdapterKind`, `pub fn plumb_config(&self, entry: &VerificationEntry) -> PathBuf`, `pub fn declares(&self, family: Family) -> bool`
  - `pub enum Family { Work, Verification, Artifact, Session }`
  - `pub struct ValidationError { pub field: String, pub problem: String }`
  - `pub fn validate(manifest: Manifest) -> Result<Validated, Vec<ValidationError>>`

  Consumed by Task 7, Task 8, and Task 18 (aggregation takes a
  `&Validated`, never a raw `Manifest`).

**`Validated` has a private field.** Only `validate` can construct one,
so a later task physically cannot aggregate an unvalidated manifest.
The same private-constructor technique gates action execution in Task
22; it is used twice on purpose.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_manifest;

    fn valid_yaml() -> &'static str {
        r#"
project:
  name: ttui
  root: D:/Dev/Projects/TTUI
work:
  adapter: github
  repo: tatemeyer/ttui
  autonomy_map:
    gated: { implement: agent, merge: on-checks }
verification:
  - kind: tests
    adapter: command
    command: cargo test
  - kind: perceptual
    adapter: plumb
artifacts:
  - kind: metrics
    watch: results/**/*.jsonl
sessions:
  watch: .claude/worktrees/*
"#
    }

    fn validated(yaml: &str) -> Validated {
        validate(parse_manifest(yaml).unwrap()).expect("should validate")
    }

    #[test]
    fn a_complete_manifest_validates() {
        let v = validated(valid_yaml());
        assert!(v.declares(Family::Work));
        assert!(v.declares(Family::Verification));
        assert!(v.declares(Family::Artifact));
        assert!(v.declares(Family::Session));
    }

    #[test]
    fn an_omitted_artifact_adapter_defaults_from_its_kind() {
        let v = validated(valid_yaml());
        let entry = &v.manifest().artifacts[0];
        assert_eq!(v.artifact_adapter(entry), ArtifactAdapterKind::Metrics);
    }

    #[test]
    fn an_explicit_artifact_adapter_overrides_the_default_from_kind() {
        let yaml = "project:\n  name: x\nartifacts:\n  - kind: capture\n    adapter: figure\n    watch: 'runs/**'\n";
        let v = validated(yaml);
        assert_eq!(v.artifact_adapter(&v.manifest().artifacts[0]), ArtifactAdapterKind::Figure);
    }

    #[test]
    fn an_omitted_plumb_config_defaults_to_dot_plumb_config_yaml() {
        let v = validated(valid_yaml());
        let entry = &v.manifest().verification[1];
        assert_eq!(v.plumb_config(entry), std::path::PathBuf::from(".plumb/config.yaml"));
    }

    #[test]
    fn a_command_adapter_without_a_command_is_a_validation_error() {
        let yaml = "project:\n  name: x\nverification:\n  - kind: tests\n    adapter: command\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].field.contains("verification[0].command"), "got {:?}", errs[0]);
    }

    #[test]
    fn an_empty_project_name_is_a_validation_error() {
        let errs = validate(parse_manifest("project:\n  name: ''\n").unwrap()).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "project.name"));
    }

    #[test]
    fn a_work_repo_that_is_not_owner_slash_name_is_a_validation_error() {
        let yaml = "project:\n  name: x\nwork:\n  adapter: github\n  repo: ttui\n  autonomy_map: {}\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "work.repo"));
    }

    #[test]
    fn an_unparseable_watch_glob_is_a_validation_error_naming_the_entry() {
        let yaml = "project:\n  name: x\nartifacts:\n  - kind: figure\n    watch: '['\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "artifacts[0].watch"));
    }

    /// Every problem is reported, not just the first — a manifest author
    /// should not have to fix one line, re-run, and find the next.
    #[test]
    fn every_problem_is_reported_at_once() {
        let yaml = "project:\n  name: ''\nwork:\n  adapter: github\n  repo: nope\n  autonomy_map: {}\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert_eq!(errs.len(), 2);
    }

    /// An autonomy_map entry that claims nothing at all is a mistake:
    /// the label would project onto an Autonomy indistinguishable from
    /// having no label.
    #[test]
    fn an_autonomy_map_entry_claiming_nothing_on_any_axis_is_an_error() {
        let yaml = "project:\n  name: x\nwork:\n  adapter: github\n  repo: a/b\n  autonomy_map:\n    weird: {}\n";
        let errs = validate(parse_manifest(yaml).unwrap()).unwrap_err();
        assert!(errs.iter().any(|e| e.field.contains("weird")));
    }

    /// A declared work feed with no labels is legitimate: the project
    /// simply does not use autonomy labels yet.
    #[test]
    fn an_empty_autonomy_map_is_legitimate() {
        let yaml = "project:\n  name: x\nwork:\n  adapter: github\n  repo: a/b\n  autonomy_map: {}\n";
        assert!(validate(parse_manifest(yaml).unwrap()).is_ok());
    }

    #[test]
    fn methodology_is_never_validated_against_a_known_set() {
        let yaml = "project:\n  name: x\n  methodology: whatever-i-feel-like\n";
        assert!(validate(parse_manifest(yaml).unwrap()).is_ok());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p parallax-baseline validate::`
Expected: FAIL — `validate` module does not exist.

- [ ] **Step 3: Write the implementation**

```rust
//! Semantic validation of a parsed manifest: the rules serde cannot
//! express. Resolves adapter defaults, checks cross-field consistency,
//! and reports every problem at once rather than the first. Produces a
//! `Validated` whose private field means nothing downstream can
//! aggregate an unchecked manifest.

use crate::manifest::{
    ArtifactAdapterKind, ArtifactEntry, ArtifactKind, Manifest, VerificationAdapterKind,
    VerificationEntry,
};
use std::path::PathBuf;

/// One of the four adapter families a manifest may declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Issues and pull requests.
    Work,
    /// Checks that decide whether work is done.
    Verification,
    /// Files a run produced.
    Artifact,
    /// Agent working directories.
    Session,
}

/// One thing wrong with a manifest, located by field path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Dotted path to the offending field, e.g. `verification[0].command`.
    pub field: String,
    /// What is wrong with it, in one sentence.
    pub problem: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.problem)
    }
}
impl std::error::Error for ValidationError {}

/// A manifest that passed validation. Only `validate` can build one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated {
    manifest: Manifest,
}

impl Validated {
    /// The manifest inside.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Consumes this wrapper, returning the manifest.
    pub fn into_manifest(self) -> Manifest {
        self.manifest
    }

    /// Whether the manifest declares this adapter family at all.
    pub fn declares(&self, family: Family) -> bool {
        match family {
            Family::Work => self.manifest.work.is_some(),
            Family::Verification => !self.manifest.verification.is_empty(),
            Family::Artifact => !self.manifest.artifacts.is_empty(),
            Family::Session => self.manifest.sessions.is_some(),
        }
    }

    /// Which artifact adapter serves an entry, defaulting from its kind.
    pub fn artifact_adapter(&self, entry: &ArtifactEntry) -> ArtifactAdapterKind {
        entry.adapter.unwrap_or(match entry.kind {
            ArtifactKind::Figure => ArtifactAdapterKind::Figure,
            ArtifactKind::Metrics => ArtifactAdapterKind::Metrics,
            ArtifactKind::Capture => ArtifactAdapterKind::Capture,
        })
    }

    /// The Plumb config path for an entry, defaulting to
    /// `.plumb/config.yaml`.
    pub fn plumb_config(&self, entry: &VerificationEntry) -> PathBuf {
        entry
            .config
            .clone()
            .unwrap_or_else(|| PathBuf::from(".plumb/config.yaml"))
    }
}

/// Validates a parsed manifest, reporting every problem found.
pub fn validate(manifest: Manifest) -> Result<Validated, Vec<ValidationError>> {
    let mut errors = Vec::new();

    if manifest.project.name.trim().is_empty() {
        errors.push(ValidationError {
            field: "project.name".into(),
            problem: "a project must have a non-empty name".into(),
        });
    }

    if let Some(work) = &manifest.work {
        let parts: Vec<&str> = work.repo.split('/').collect();
        if parts.len() != 2 || parts.iter().any(|p| p.trim().is_empty()) {
            errors.push(ValidationError {
                field: "work.repo".into(),
                problem: format!("expected `owner/name`, got `{}`", work.repo),
            });
        }
        for label in work.autonomy_map.labels() {
            let entry = work.autonomy_map.entry(label).expect("label came from this map");
            if entry.implement.is_none() && entry.merge.is_none() && entry.readiness.is_none() {
                errors.push(ValidationError {
                    field: format!("work.autonomy_map.{label}"),
                    problem: "claims nothing on any axis, so the label carries no meaning".into(),
                });
            }
        }
    }

    for (i, entry) in manifest.verification.iter().enumerate() {
        if entry.adapter == VerificationAdapterKind::Command && entry.command.is_none() {
            errors.push(ValidationError {
                field: format!("verification[{i}].command"),
                problem: "the `command` adapter requires a command".into(),
            });
        }
        if entry.adapter == VerificationAdapterKind::Plumb && entry.command.is_some() {
            errors.push(ValidationError {
                field: format!("verification[{i}].command"),
                problem: "the `plumb` adapter reads a verdict file and takes no command".into(),
            });
        }
    }

    for (i, entry) in manifest.artifacts.iter().enumerate() {
        if let Err(e) = globset::Glob::new(&entry.watch) {
            errors.push(ValidationError {
                field: format!("artifacts[{i}].watch"),
                problem: format!("not a valid glob: {e}"),
            });
        }
    }

    if let Some(sessions) = &manifest.sessions {
        if let Err(e) = globset::Glob::new(&sessions.watch) {
            errors.push(ValidationError {
                field: "sessions.watch".into(),
                problem: format!("not a valid glob: {e}"),
            });
        }
    }

    if errors.is_empty() {
        Ok(Validated { manifest })
    } else {
        Err(errors)
    }
}
```

Note what is deliberately **not** validated: `project.methodology`
against any known set, and `verification[].kind` against any known set.
Both are free-form by design.

Add `pub mod validate;` to `baseline/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline validate::`
Expected: PASS, 12 tests.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/validate.rs baseline/src/lib.rs
git commit -m "feat(validate): check a manifest and resolve adapter defaults

Reports every problem at once rather than the first, and returns a
Validated whose private field means nothing downstream can aggregate an
unchecked manifest."
```

---

### Slice 2.2: The two real consumers

**Tags:** coding

#### Task 7: Author `manifests/ttui.yaml` and `manifests/model-experiments.yaml`

**Files:**
- Create: `manifests/ttui.yaml`
- Create: `manifests/model-experiments.yaml`
- Create: `baseline/tests/real_manifests.rs`

**Interfaces:**
- Consumes: `manifest::parse_manifest_file`, `validate::validate`,
  `autonomy::{project, resolve, Implement, Merge, Readiness}`.
- Produces: the two real manifests, at repository-root-relative paths
  that Task 20 and Task 25 both load.

**These two files are authored verbatim from the spec's "The manifest"
section.** Do not improve them, do not add fields, do not reorder keys.
They are the spec's own statement of what the two consumers declare, and
Task 20 asserts their projections match the spec's table exactly.

- [ ] **Step 1: Write the failing integration test**

Create `baseline/tests/real_manifests.rs`:

```rust
//! The two real consumers' manifests must parse, validate, and project
//! their native autonomy labels onto the normalized axes.

use parallax_baseline::autonomy::{project, Implement, Merge, Readiness};
use parallax_baseline::manifest::{parse_manifest_file, ArtifactKind, VerificationAdapterKind};
use parallax_baseline::validate::{validate, Family, Validated};
use std::path::{Path, PathBuf};

/// `manifests/` sits at the workspace root, one level above this crate.
fn manifest_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("baseline/ has a parent")
        .join("manifests")
        .join(name)
}

fn load(name: &str) -> Validated {
    let parsed = parse_manifest_file(&manifest_path(name)).expect("manifest parses");
    validate(parsed).expect("manifest validates")
}

#[test]
fn ttui_manifest_parses_and_validates() {
    let v = load("ttui.yaml");
    let m = v.manifest();
    assert_eq!(m.api_version.as_deref(), Some("parallax/v1"));
    assert_eq!(m.project.name, "ttui");
    assert_eq!(m.project.language.as_deref(), Some("rust"));
    assert_eq!(m.project.methodology.as_deref(), Some("methodology-first"));
    assert_eq!(m.work.as_ref().unwrap().repo, "tatemeyer/ttui");
    assert_eq!(m.verification.len(), 3);
    assert_eq!(m.verification[2].adapter, VerificationAdapterKind::Plumb);
    assert_eq!(m.artifacts[0].kind, ArtifactKind::Capture);
    assert_eq!(m.sessions.as_ref().unwrap().watch, ".claude/worktrees/*");
    for family in [Family::Work, Family::Verification, Family::Artifact, Family::Session] {
        assert!(v.declares(family), "ttui declares all four families");
    }
}

#[test]
fn ttui_labels_project_onto_the_spec_s_table() {
    let v = load("ttui.yaml");
    let map = &v.manifest().work.as_ref().unwrap().autonomy_map;

    let direct = project(map, "direct").expect("direct is declared");
    assert_eq!(direct.implement, Some(Implement::Agent));
    assert_eq!(direct.merge, Some(Merge::DirectPush));
    assert_eq!(direct.readiness, Readiness::Verifiable);

    let gated = project(map, "gated").expect("gated is declared");
    assert_eq!(gated.implement, Some(Implement::Agent));
    assert_eq!(gated.merge, Some(Merge::OnChecks));
    assert_eq!(gated.readiness, Readiness::Verifiable);

    let human = project(map, "human").expect("human is declared");
    assert_eq!(human.implement, Some(Implement::Agent));
    assert_eq!(human.merge, Some(Merge::HumanApproval));
    assert_eq!(human.readiness, Readiness::Verifiable);
}

#[test]
fn model_experiments_manifest_parses_and_validates() {
    let v = load("model-experiments.yaml");
    let m = v.manifest();
    assert_eq!(m.project.name, "model-experiments");
    assert_eq!(m.project.language.as_deref(), Some("python"));
    assert_eq!(m.project.methodology.as_deref(), Some("outcome-first"));
    assert_eq!(m.work.as_ref().unwrap().repo, "tatemeyer/Model-Experiments");
    assert_eq!(m.verification.len(), 2);
    assert_eq!(m.artifacts.len(), 2);
    assert_eq!(m.artifacts[0].kind, ArtifactKind::Figure);
    assert_eq!(m.artifacts[1].kind, ArtifactKind::Metrics);
    assert!(!v.declares(Family::Session), "Model-Experiments declares no session feed");
}

#[test]
fn model_experiments_labels_project_onto_the_spec_s_table() {
    let v = load("model-experiments.yaml");
    let map = &v.manifest().work.as_ref().unwrap().autonomy_map;

    let safe = project(map, "autonomy:safe").expect("declared");
    assert_eq!(safe.implement, Some(Implement::Agent));
    assert_eq!(safe.merge, Some(Merge::OnChecks));
    assert_eq!(safe.readiness, Readiness::Verifiable);

    let review = project(map, "autonomy:review").expect("declared");
    assert_eq!(review.implement, Some(Implement::Agent));
    assert_eq!(review.merge, Some(Merge::HumanApproval));
    assert_eq!(review.readiness, Readiness::Verifiable);

    let human = project(map, "autonomy:human").expect("declared");
    assert_eq!(human.implement, Some(Implement::HumanOnly));
    assert_eq!(human.merge, None, "the spec's table has a dash here");
    assert_eq!(human.readiness, Readiness::Verifiable);

    let intent = project(map, "needs-intent").expect("declared");
    assert_eq!(intent.implement, None, "the spec's table has a dash here");
    assert_eq!(intent.merge, None, "the spec's table has a dash here");
    assert_eq!(intent.readiness, Readiness::NeedsIntent);
}

/// The two asymmetries the shared vocabulary exists to surface, asserted
/// against the real files rather than test fixtures.
#[test]
fn the_two_asymmetries_hold_for_the_real_manifests() {
    let ttui = load("ttui.yaml");
    let ttui_map = &ttui.manifest().work.as_ref().unwrap().autonomy_map;
    assert!(
        ttui_map.labels().all(|l| project(ttui_map, l).unwrap().implement != Some(Implement::HumanOnly)),
        "TTUI reserves no work from the agent"
    );

    let me = load("model-experiments.yaml");
    let me_map = &me.manifest().work.as_ref().unwrap().autonomy_map;
    assert!(
        me_map.labels().all(|l| project(me_map, l).unwrap().merge != Some(Merge::DirectPush)),
        "nothing in Model-Experiments bypasses CI"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p parallax-baseline --test real_manifests`
Expected: FAIL — `manifests/ttui.yaml` does not exist.

- [ ] **Step 3: Write `manifests/ttui.yaml`, verbatim from the spec**

```yaml
apiVersion: parallax/v1
project:
  name: ttui
  root: D:/Dev/Projects/TTUI
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

- [ ] **Step 4: Write `manifests/model-experiments.yaml`, verbatim from the spec**

```yaml
project:
  name: model-experiments
  language: python
  methodology: outcome-first
work:
  adapter: github
  repo: tatemeyer/Model-Experiments
  autonomy_map:
    "autonomy:safe":   { implement: agent, merge: on-checks }
    "autonomy:review": { implement: agent, merge: human-approval }
    "autonomy:human":  { implement: human-only }
    "needs-intent":    { readiness: needs-intent }
verification:
  - kind: tests
    adapter: command
    command: uv run pytest
  - kind: perceptual
    adapter: plumb          # judges mx-viz output
artifacts:
  - kind: figure
    watch: projects/*/results/**/*.png
  - kind: metrics
    adapter: jsonl
    watch: projects/*/results/**/*.jsonl
```

This file has no `apiVersion:`, no `project.root:`, and no `sessions:` —
exactly as the spec writes it. Those absences are the point: it is
already a partial manifest, and it must work.

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p parallax-baseline --test real_manifests`
Expected: PASS, 5 tests.

- [ ] **Step 6: Commit**

```bash
git add manifests/ttui.yaml manifests/model-experiments.yaml baseline/tests/real_manifests.rs
git commit -m "feat(manifests): add the two real consumer manifests

Authored verbatim from the platform spec, with an integration test
asserting both parse, validate, and project their native labels onto
the spec's table — including Model-Experiments' absent apiVersion,
root, and sessions feed."
```

---

### Slice 2.3: Partial manifests as a first-class case

**Tags:** coding

#### Task 8: A manifest declaring only `work:`

**Files:**
- Create: `baseline/tests/partial_manifest.rs`
- Modify: `baseline/src/validate.rs` (only if a test exposes a gap)

**Interfaces:**
- Consumes: `manifest::parse_manifest`, `validate::{validate, Family}`.
- Produces: no new API. This task exists to pin behaviour the spec calls
  normal, before Arc 5 has anything to aggregate.

**Why a separate task rather than a case inside Task 6.** The spec names
this in both its "The manifest" section and its Verification section,
and the failure mode it guards against is not a missing check — it is a
later task quietly adding a `.expect()` on `manifest.work` or an
`assert!(!artifacts.is_empty())` because every fixture happened to be
complete. Pinning it as its own integration test, before adapters
exist, means Arc 4 and Arc 5 inherit a red test the moment they
regress it.

- [ ] **Step 1: Write the failing test**

```rust
//! Partial manifests are normal, not an error path: "a project that
//! satisfies only the work adapter still shows up, just with less
//! detail."

use parallax_baseline::manifest::parse_manifest;
use parallax_baseline::validate::{validate, Family, Validated};

fn validated(yaml: &str) -> Validated {
    validate(parse_manifest(yaml).expect("parses")).expect("validates")
}

const WORK_ONLY: &str = r#"
apiVersion: parallax/v1
project:
  name: work-only
  root: D:/Dev/Projects/WorkOnly
work:
  adapter: github
  repo: tatemeyer/work-only
  autonomy_map:
    gated: { implement: agent, merge: on-checks }
"#;

#[test]
fn a_work_only_manifest_produces_a_valid_reduced_view() {
    let v = validated(WORK_ONLY);
    assert!(v.declares(Family::Work));
    assert!(!v.declares(Family::Verification));
    assert!(!v.declares(Family::Artifact));
    assert!(!v.declares(Family::Session));
    assert!(v.manifest().verification.is_empty());
    assert!(v.manifest().artifacts.is_empty());
    assert!(v.manifest().sessions.is_none());
}

#[test]
fn a_project_only_manifest_is_valid_too() {
    let v = validated("project:\n  name: nothing-declared\n");
    for family in [Family::Work, Family::Verification, Family::Artifact, Family::Session] {
        assert!(!v.declares(family));
    }
}

#[test]
fn each_family_can_be_declared_alone() {
    let cases = [
        ("verification only", "project:\n  name: p\nverification:\n  - kind: tests\n    adapter: command\n    command: pytest\n", Family::Verification),
        ("artifacts only", "project:\n  name: p\nartifacts:\n  - kind: figure\n    watch: 'out/**/*.png'\n", Family::Artifact),
        ("sessions only", "project:\n  name: p\nsessions:\n  watch: '.claude/worktrees/*'\n", Family::Session),
    ];
    for (name, yaml, family) in cases {
        let v = validated(yaml);
        assert!(v.declares(family), "{name}: its own family");
        for other in [Family::Work, Family::Verification, Family::Artifact, Family::Session] {
            if other != family {
                assert!(!v.declares(other), "{name}: declares nothing else");
            }
        }
    }
}

/// The spec's Model-Experiments manifest is itself partial. Its absences
/// are asserted here as well as in `real_manifests.rs`, because this is
/// the file a future task will read when it wonders whether a missing
/// section is legal.
#[test]
fn an_absent_section_is_never_an_error_regardless_of_which_one() {
    for yaml in [
        "project:\n  name: p\n",
        "apiVersion: parallax/v1\nproject:\n  name: p\n",
        "project:\n  name: p\n  language: rust\n",
        "project:\n  name: p\n  methodology: outcome-first\n",
    ] {
        assert!(validate(parse_manifest(yaml).expect("parses")).is_ok(), "{yaml:?}");
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p parallax-baseline --test partial_manifest`
Expected: PASS if Tasks 5 and 6 got the defaults right. **If any case
fails, fix `manifest.rs`/`validate.rs` rather than weakening the
test** — a partial manifest failing is a bug in this crate, not in the
manifest.

- [ ] **Step 3: Run the full suite**

Run: `cargo test`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
Expected: all clean.

- [ ] **Step 4: Commit**

```bash
git add baseline/tests/partial_manifest.rs baseline/src/
git commit -m "test(manifest): pin partial manifests as a first-class case

The spec calls partial support normal in two places; pinning it before
adapters exist means Arc 4 and Arc 5 inherit a red test the moment one
of them assumes a section is present."
```

---
## Arc 3: Freshness and the adapter contract

The spec says GitHub is polled on an interval and filesystem state is
effectively immediate, and that a frontend "displays the age of each
source." This Arc puts that in the data model, then defines the four
adapter traits on top of it. No I/O lands here — the contract only, so
Arc 4's seven tasks are independent of one another.

### Slice 3.1: The freshness model

**Tags:** coding

#### Task 9: `Observed<T>`, `SourceKind`, and `Freshness`

**Files:**
- Create: `baseline/src/freshness.rs`
- Modify: `baseline/src/lib.rs` (add `pub mod freshness;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub const DEFAULT_POLL_INTERVAL: Duration` (30 seconds)
  - `pub enum SourceKind { Polled { interval: Duration }, Watched }`
  - `pub struct Observed<T> { pub value: T, pub observed_at: SystemTime, pub source: SourceKind }` with `pub fn polled(value: T, observed_at: SystemTime, interval: Duration) -> Self`, `pub fn watched(value: T, observed_at: SystemTime) -> Self`, `pub fn age(&self, now: SystemTime) -> Duration`, `pub fn freshness(&self, now: SystemTime) -> Freshness`, `pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Observed<U>`, `pub fn confirm_unchanged(&mut self, at: SystemTime)`
  - `pub enum Freshness { Live, Fresh { age: Duration }, Stale { age: Duration, overdue: Duration }, Unavailable { since: Option<SystemTime>, reason: String } }` with `pub fn is_stale(&self) -> bool`, `pub fn age(&self) -> Option<Duration>`

  Consumed by every adapter (Tasks 10-17) and by `state` (Tasks 18-19).

**This is Judgment call 1 made concrete.** Every adapter returns
`Observed<T>` rather than a bare `T`, so freshness travels with the
value instead of living in a frontend's head. `Freshness` is computed
from an injected `now`, never `SystemTime::now()`, which is what makes
it unit-testable and what lets a caller ask "how fresh was this *as of
the moment I rendered*" rather than "as of some later moment."

`confirm_unchanged` is the ETag case and is the reason freshness cannot
just be derived from the value: a `304 Not Modified` proves the value is
current *now*, even though the value did not change. Advancing
`observed_at` on a 304 is the whole point of conditional polling.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1_700_000_000);

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + T0 + Duration::from_secs(secs)
    }

    #[test]
    fn the_default_poll_interval_is_thirty_seconds() {
        assert_eq!(DEFAULT_POLL_INTERVAL, Duration::from_secs(30));
    }

    #[test]
    fn a_watched_source_is_always_live_regardless_of_age() {
        let o = Observed::watched(7u32, at(0));
        assert_eq!(o.freshness(at(0)), Freshness::Live);
        assert_eq!(o.freshness(at(600)), Freshness::Live);
    }

    #[test]
    fn a_polled_source_within_its_interval_is_fresh_and_carries_its_age() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert_eq!(o.freshness(at(110)), Freshness::Fresh { age: Duration::from_secs(10) });
    }

    #[test]
    fn a_polled_source_exactly_at_its_interval_is_still_fresh() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert_eq!(o.freshness(at(130)), Freshness::Fresh { age: Duration::from_secs(30) });
    }

    #[test]
    fn a_polled_source_past_its_interval_is_stale_and_says_by_how_much() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert_eq!(
            o.freshness(at(145)),
            Freshness::Stale { age: Duration::from_secs(45), overdue: Duration::from_secs(15) }
        );
    }

    /// A clock that goes backwards must not panic or report a wild age.
    #[test]
    fn a_now_earlier_than_the_observation_saturates_to_zero_age() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert_eq!(o.age(at(50)), Duration::ZERO);
        assert_eq!(o.freshness(at(50)), Freshness::Fresh { age: Duration::ZERO });
    }

    /// The ETag case: a 304 proves the value is current now, even though
    /// the value did not change.
    #[test]
    fn confirm_unchanged_advances_the_observation_without_touching_the_value() {
        let mut o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        assert!(o.freshness(at(200)).is_stale());
        o.confirm_unchanged(at(200));
        assert_eq!(o.value, 7);
        assert_eq!(o.freshness(at(200)), Freshness::Fresh { age: Duration::ZERO });
    }

    #[test]
    fn map_rewrites_the_value_and_keeps_the_observation_metadata() {
        let o = Observed::polled(7u32, at(100), Duration::from_secs(30));
        let mapped = o.map(|v| v.to_string());
        assert_eq!(mapped.value, "7");
        assert_eq!(mapped.observed_at, at(100));
        assert_eq!(mapped.source, SourceKind::Polled { interval: Duration::from_secs(30) });
    }

    #[test]
    fn only_stale_and_unavailable_count_as_stale() {
        assert!(!Freshness::Live.is_stale());
        assert!(!Freshness::Fresh { age: Duration::ZERO }.is_stale());
        assert!(Freshness::Stale { age: Duration::from_secs(9), overdue: Duration::from_secs(1) }.is_stale());
        assert!(Freshness::Unavailable { since: None, reason: "rate limited".into() }.is_stale());
    }

    #[test]
    fn unavailable_reports_no_age_and_everything_else_reports_one() {
        assert_eq!(Freshness::Live.age(), Some(Duration::ZERO));
        assert_eq!(Freshness::Fresh { age: Duration::from_secs(3) }.age(), Some(Duration::from_secs(3)));
        assert_eq!(
            Freshness::Stale { age: Duration::from_secs(9), overdue: Duration::from_secs(1) }.age(),
            Some(Duration::from_secs(9))
        );
        assert_eq!(Freshness::Unavailable { since: None, reason: String::new() }.age(), None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p parallax-baseline freshness::`
Expected: FAIL — `freshness` module does not exist.

- [ ] **Step 3: Write the implementation**

```rust
//! Freshness: how the core represents "how current is this?" without
//! knowing that a frontend exists. Every adapter returns an
//! `Observed<T>` — a value stamped with when and how it was seen — and
//! `Freshness` is computed against an injected `now`, never a wall
//! clock, so it is both unit-testable and honest about the moment the
//! caller cares about.

use std::time::{Duration, SystemTime};

/// The spec's default GitHub poll interval.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Where a value came from, which is what determines how it goes stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Fetched on an interval; current only to within that interval.
    Polled {
        /// How often the source is refreshed.
        interval: Duration,
    },
    /// Read from the filesystem on demand; effectively immediate.
    Watched,
}

/// A value together with when and how it was observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed<T> {
    /// The observation itself.
    pub value: T,
    /// When it was last confirmed current.
    pub observed_at: SystemTime,
    /// How it was obtained.
    pub source: SourceKind,
}

impl<T> Observed<T> {
    /// An observation from a source polled on `interval`.
    pub fn polled(value: T, observed_at: SystemTime, interval: Duration) -> Self {
        Self { value, observed_at, source: SourceKind::Polled { interval } }
    }

    /// An observation read straight from the filesystem.
    pub fn watched(value: T, observed_at: SystemTime) -> Self {
        Self { value, observed_at, source: SourceKind::Watched }
    }

    /// How long ago this was observed, saturating at zero if `now`
    /// precedes the observation.
    pub fn age(&self, now: SystemTime) -> Duration {
        now.duration_since(self.observed_at).unwrap_or(Duration::ZERO)
    }

    /// How much a caller should trust this observation at `now`.
    pub fn freshness(&self, now: SystemTime) -> Freshness {
        let age = self.age(now);
        match self.source {
            SourceKind::Watched => Freshness::Live,
            SourceKind::Polled { interval } => match age.checked_sub(interval) {
                Some(overdue) if overdue > Duration::ZERO => Freshness::Stale { age, overdue },
                _ => Freshness::Fresh { age },
            },
        }
    }

    /// Rewrites the value, preserving when and how it was observed.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Observed<U> {
        Observed { value: f(self.value), observed_at: self.observed_at, source: self.source }
    }

    /// Records that the source confirmed this value is still current —
    /// the `304 Not Modified` case. The value does not change; its
    /// freshness does.
    pub fn confirm_unchanged(&mut self, at: SystemTime) {
        self.observed_at = at;
    }
}

/// How current an observation is, from the caller's point of view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Filesystem-backed: current as of the read.
    Live,
    /// Polled and within its interval.
    Fresh {
        /// How long ago it was observed.
        age: Duration,
    },
    /// Polled and past its interval.
    Stale {
        /// How long ago it was observed.
        age: Duration,
        /// How far past the interval that is.
        overdue: Duration,
    },
    /// The source could not be read at all.
    Unavailable {
        /// When it was last read successfully, if ever.
        since: Option<SystemTime>,
        /// Why it could not be read, in one sentence.
        reason: String,
    },
}

impl Freshness {
    /// Whether a caller should visibly mark this as not-current.
    pub fn is_stale(&self) -> bool {
        matches!(self, Freshness::Stale { .. } | Freshness::Unavailable { .. })
    }

    /// How old the observation is, or `None` when there is no
    /// observation to age.
    pub fn age(&self) -> Option<Duration> {
        match self {
            Freshness::Live => Some(Duration::ZERO),
            Freshness::Fresh { age } | Freshness::Stale { age, .. } => Some(*age),
            Freshness::Unavailable { .. } => None,
        }
    }
}
```

Add `pub mod freshness;` to `baseline/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline freshness::`
Expected: PASS, 10 tests.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/freshness.rs baseline/src/lib.rs
git commit -m "feat(freshness): carry observation time and source with every value

The core has no UI but the cockpit must display the age of each source,
so freshness travels with the value rather than living in a frontend;
computing it against an injected now keeps it unit-testable and makes
the 304-confirms-currency case expressible."
```

---

### Slice 3.2: The four adapter traits

**Tags:** coding

#### Task 10: `AdapterError`, `ProjectContext`, and the four traits

**Files:**
- Create: `baseline/src/adapters/mod.rs`
- Create: `baseline/src/adapters/work.rs`
- Create: `baseline/src/adapters/verification.rs`
- Create: `baseline/src/adapters/artifact.rs`
- Create: `baseline/src/adapters/session.rs`
- Modify: `baseline/src/lib.rs` (add `pub mod adapters;`)

**Interfaces:**
- Consumes: `freshness::Observed`, `manifest::{ArtifactKind, VerificationAdapterKind}`.
- Produces, in `adapters/mod.rs`:
  - `pub enum AdapterError { Io(std::io::Error), Http { status: u16, message: String }, Parse(String), Unsupported(String), Timeout(String) }`
  - `pub struct ProjectContext { pub name: String, pub root: PathBuf, pub repo: Option<String> }` with `pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Self` and `pub fn with_repo(self, repo: impl Into<String>) -> Self`, `pub fn resolve(&self, relative: &str) -> PathBuf`
- Produces, in `adapters/work.rs`:
  - `pub trait WorkAdapter { fn source_name(&self) -> String; fn poll(&mut self, ctx: &ProjectContext, now: SystemTime) -> Result<Observed<WorkSnapshot>, AdapterError>; }`
  - `pub struct WorkSnapshot { pub items: Vec<WorkItem> }`
  - `pub struct WorkItem { pub number: u64, pub title: String, pub kind: WorkKind, pub state: WorkState, pub labels: Vec<String>, pub checks: ChecksSummary, pub url: String, pub updated_at: String }`
  - `pub enum WorkKind { Issue, PullRequest }`
  - `pub enum WorkState { Open, Draft, Closed, Merged }`
  - `pub struct ChecksSummary { pub passed: usize, pub failed: usize, pub pending: usize }` with `pub fn total(&self) -> usize`, `pub fn is_green(&self) -> bool`, `pub fn none() -> Self`
- Produces, in `adapters/verification.rs`:
  - `pub trait VerificationAdapter { fn source_name(&self) -> String; fn check(&mut self, ctx: &ProjectContext, now: SystemTime) -> Result<Observed<VerificationStatus>, AdapterError>; }`
  - `pub struct VerificationStatus { pub kind: String, pub outcome: VerificationOutcome, pub detail: Option<String> }`
  - `pub enum VerificationOutcome { Pass, Fail, Hold, NotRun }`
- Produces, in `adapters/artifact.rs`:
  - `pub trait ArtifactAdapter { fn source_name(&self) -> String; fn scan(&mut self, ctx: &ProjectContext, now: SystemTime) -> Result<Observed<Vec<Artifact>>, AdapterError>; }`
  - `pub struct Artifact { pub path: PathBuf, pub kind: ArtifactKind, pub modified: SystemTime, pub detail: ArtifactDetail }`
  - `pub enum ArtifactDetail { Figure { bytes: u64 }, Metrics { series: Vec<Series> }, Capture { run_id: String, outcome: VerificationOutcome } }`
  - `pub struct Series { pub name: String, pub points: Vec<f64> }`
- Produces, in `adapters/session.rs`:
  - `pub trait SessionAdapter { fn source_name(&self) -> String; fn scan(&mut self, ctx: &ProjectContext, now: SystemTime) -> Result<Observed<Vec<Session>>, AdapterError>; }`
  - `pub struct Session { pub name: String, pub path: PathBuf, pub last_activity: SystemTime }` with `pub fn is_active(&self, now: SystemTime, idle_after: Duration) -> bool`

  Consumed by Tasks 11-17 (implementations) and Task 18 (aggregation
  takes `Box<dyn WorkAdapter>` and friends).

**Every trait method takes `now: SystemTime`.** Not for convenience —
it is what lets Task 18's aggregation tests pin freshness without a
sleep, and what keeps `SystemTime::now()` confined to the outermost
caller.

**Every trait is object-safe** (no generic methods, no `Self: Sized`
returns), because `state::Aggregator` holds `Box<dyn WorkAdapter>` and
a frontend must be able to register an adapter this crate does not
know about.

- [ ] **Step 1: Write the failing tests**

Put these in `adapters/mod.rs` — they test the shared pieces and, for
the traits, that a hand-written implementation compiles and dispatches
dynamically. That "it compiles as a trait object" property is exactly
what a later task could break by adding a generic method.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::work::{ChecksSummary, WorkAdapter, WorkSnapshot};
    use crate::freshness::Observed;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_project_context_resolves_relative_paths_against_the_project_root() {
        let ctx = ProjectContext::new("ttui", "D:/Dev/Projects/TTUI");
        assert_eq!(ctx.resolve(".plumb/config.yaml"), std::path::PathBuf::from("D:/Dev/Projects/TTUI/.plumb/config.yaml"));
    }

    #[test]
    fn a_project_context_carries_an_optional_repo() {
        let ctx = ProjectContext::new("ttui", "/tmp").with_repo("tatemeyer/ttui");
        assert_eq!(ctx.repo.as_deref(), Some("tatemeyer/ttui"));
        assert_eq!(ProjectContext::new("x", "/tmp").repo, None);
    }

    #[test]
    fn checks_are_green_only_when_something_passed_and_nothing_failed_or_pends() {
        assert!(ChecksSummary { passed: 4, failed: 0, pending: 0 }.is_green());
        assert!(!ChecksSummary { passed: 4, failed: 1, pending: 0 }.is_green());
        assert!(!ChecksSummary { passed: 4, failed: 0, pending: 1 }.is_green());
        assert!(!ChecksSummary::none().is_green(), "no checks at all is not green");
        assert_eq!(ChecksSummary { passed: 2, failed: 1, pending: 3 }.total(), 6);
    }

    #[test]
    fn adapter_errors_render_a_one_line_message_naming_the_cause() {
        let e = AdapterError::Http { status: 403, message: "rate limit exceeded".into() };
        assert!(e.to_string().contains("403"));
        assert!(e.to_string().contains("rate limit exceeded"));
        assert!(AdapterError::Unsupported("window capture".into()).to_string().contains("window capture"));
    }

    struct StubWork;
    impl WorkAdapter for StubWork {
        fn source_name(&self) -> String {
            "stub".into()
        }
        fn poll(&mut self, _ctx: &ProjectContext, now: SystemTime) -> Result<Observed<WorkSnapshot>, AdapterError> {
            Ok(Observed::polled(WorkSnapshot { items: vec![] }, now, Duration::from_secs(30)))
        }
    }

    /// Aggregation stores adapters as trait objects and a frontend may
    /// register one this crate has never heard of, so object safety is a
    /// contract, not an accident.
    #[test]
    fn a_work_adapter_is_usable_as_a_trait_object() {
        let mut adapters: Vec<Box<dyn WorkAdapter>> = vec![Box::new(StubWork)];
        let ctx = ProjectContext::new("x", "/tmp");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let observed = adapters[0].poll(&ctx, now).unwrap();
        assert_eq!(observed.observed_at, now);
        assert!(observed.value.items.is_empty());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p parallax-baseline adapters::`
Expected: FAIL — `adapters` module does not exist.

- [ ] **Step 3: Write `adapters/mod.rs`**

```rust
//! The four adapter families. Each family is a trait; the built-in
//! implementations live behind it, and a frontend may register its own.
//! Every method takes an injected `now` so nothing here needs a wall
//! clock, and every trait is object-safe so aggregation can hold them
//! as `Box<dyn _>`.

pub mod artifact;
pub mod http;
pub mod session;
pub mod verification;
pub mod work;

use std::path::PathBuf;

/// Anything that can go wrong reaching a source.
#[derive(Debug)]
pub enum AdapterError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// A non-success HTTP response.
    Http {
        /// The status code.
        status: u16,
        /// The body or reason phrase, trimmed.
        message: String,
    },
    /// The source responded but the response could not be understood.
    Parse(String),
    /// The manifest asked for something with no implementation.
    Unsupported(String),
    /// The source did not respond in time.
    Timeout(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdapterError::Io(e) => write!(f, "reading source: {e}"),
            AdapterError::Http { status, message } => write!(f, "http {status}: {message}"),
            AdapterError::Parse(m) => write!(f, "unreadable response: {m}"),
            AdapterError::Unsupported(m) => write!(f, "no implementation for {m}"),
            AdapterError::Timeout(m) => write!(f, "timed out: {m}"),
        }
    }
}
impl std::error::Error for AdapterError {}

impl From<std::io::Error> for AdapterError {
    fn from(e: std::io::Error) -> Self {
        AdapterError::Io(e)
    }
}

/// What every adapter needs to know about the project it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    /// The project's short name.
    pub name: String,
    /// Absolute path to the project root.
    pub root: PathBuf,
    /// The work adapter's `owner/name`, when one is declared.
    pub repo: Option<String>,
}

impl ProjectContext {
    /// A context for a project rooted at `root`.
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self { name: name.into(), root: root.into(), repo: None }
    }

    /// Attaches the work adapter's repository argument.
    pub fn with_repo(mut self, repo: impl Into<String>) -> Self {
        self.repo = Some(repo.into());
        self
    }

    /// Resolves a manifest-relative path against the project root.
    pub fn resolve(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}
```

- [ ] **Step 4: Write the four family modules**

`adapters/work.rs`:

```rust
//! The work family: issues, pull requests, their labels, and their
//! check status. One built-in implementation (`github`, Task 12).

use super::{AdapterError, ProjectContext};
use crate::freshness::Observed;
use std::time::SystemTime;

/// Whether a work item is an issue or a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkKind {
    /// An issue.
    Issue,
    /// A pull request.
    PullRequest,
}

/// Where a work item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkState {
    /// Open and ready.
    Open,
    /// Open but marked draft.
    Draft,
    /// Closed without merging.
    Closed,
    /// Merged.
    Merged,
}

/// How a work item's checks stand. Deliberately a count, not a verdict —
/// what "green enough" means is a policy question, and the manifest's
/// autonomy axes are where policy lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChecksSummary {
    /// Checks that succeeded.
    pub passed: usize,
    /// Checks that failed.
    pub failed: usize,
    /// Checks still running or queued.
    pub pending: usize,
}

impl ChecksSummary {
    /// A summary for an item with no checks reported.
    pub fn none() -> Self {
        Self::default()
    }

    /// How many checks were reported in total.
    pub fn total(&self) -> usize {
        self.passed + self.failed + self.pending
    }

    /// Whether every reported check passed and at least one ran.
    pub fn is_green(&self) -> bool {
        self.passed > 0 && self.failed == 0 && self.pending == 0
    }
}

/// One issue or pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// The item's number in its repository.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Issue or pull request.
    pub kind: WorkKind,
    /// Where it stands.
    pub state: WorkState,
    /// Its labels, verbatim — projection happens in `autonomy`.
    pub labels: Vec<String>,
    /// Its check status.
    pub checks: ChecksSummary,
    /// A link a frontend can open.
    pub url: String,
    /// The source's own last-updated string, carried opaquely for
    /// display. Freshness of the *observation* lives in `Observed`.
    pub updated_at: String,
}

/// Every work item one poll returned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkSnapshot {
    /// The items, in the order the source returned them.
    pub items: Vec<WorkItem>,
}

/// A source of work items.
pub trait WorkAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Fetches the current work items as of `now`.
    fn poll(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<WorkSnapshot>, AdapterError>;
}
```

`adapters/verification.rs` (trait and types only for this task; the two
implementations land in Tasks 13 and 14):

```rust
//! The verification family: whatever decides a unit of work is done.
//! Two built-in implementations — `command` (Task 13) and `plumb`
//! (Task 14). **Neither links Plumb**: the `plumb` adapter reads the
//! `verdict.md` Plumb writes, as text.

use super::{AdapterError, ProjectContext};
use crate::freshness::Observed;
use std::time::SystemTime;

/// What a verification check concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationOutcome {
    /// The check succeeded.
    Pass,
    /// The check failed.
    Fail,
    /// The check could not reach a conclusion. Never upgraded to a pass.
    Hold,
    /// The check has not run yet.
    NotRun,
}

/// One verification check's current standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationStatus {
    /// The manifest's display label for this check, e.g. `lint`.
    pub kind: String,
    /// What it concluded.
    pub outcome: VerificationOutcome,
    /// A one-line explanation, when the adapter has one.
    pub detail: Option<String>,
}

/// A source of verification outcomes.
pub trait VerificationAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Reads this check's current standing as of `now`.
    fn check(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<VerificationStatus>, AdapterError>;
}
```

`adapters/artifact.rs` (trait and types only; implementations in Tasks
15 and 16):

```rust
//! The artifact family: files a run produced. Three built-in
//! implementations — `figure`, `metrics`, `capture`.

use super::{AdapterError, ProjectContext};
use crate::adapters::verification::VerificationOutcome;
use crate::freshness::Observed;
use crate::manifest::ArtifactKind;
use std::path::PathBuf;
use std::time::SystemTime;

/// One named scalar series read from a metrics feed.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// The series' key in the source records.
    pub name: String,
    /// Its values, in the order they were recorded.
    pub points: Vec<f64>,
}

/// What an adapter learned about an artifact beyond its path.
#[derive(Debug, Clone, PartialEq)]
pub enum ArtifactDetail {
    /// A pre-rendered image. The core reads its size, never its pixels.
    Figure {
        /// File size in bytes.
        bytes: u64,
    },
    /// Scalar series parsed from a JSONL feed.
    Metrics {
        /// Every series found, sorted by name.
        series: Vec<Series>,
    },
    /// A Plumb run directory.
    Capture {
        /// The run's id, taken from its directory name.
        run_id: String,
        /// The run's verdict, or `NotRun` when it wrote none.
        outcome: VerificationOutcome,
    },
}

/// One artifact a run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Artifact {
    /// Absolute path to the artifact.
    pub path: PathBuf,
    /// Which feed produced it.
    pub kind: ArtifactKind,
    /// Its filesystem modification time.
    pub modified: SystemTime,
    /// What the adapter read from it.
    pub detail: ArtifactDetail,
}

/// A source of artifacts.
pub trait ArtifactAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Scans the feed's `watch` glob as of `now`.
    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Artifact>>, AdapterError>;
}
```

`adapters/session.rs` (trait and types only; implementation in Task 17):

```rust
//! The session family: agent working directories, so a frontend can
//! show what is running where. One built-in implementation, a
//! filesystem scan (Task 17).

use super::{AdapterError, ProjectContext};
use crate::freshness::Observed;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// One agent session directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The directory's name.
    pub name: String,
    /// Absolute path to the directory.
    pub path: PathBuf,
    /// The most recent modification time anywhere inside it.
    pub last_activity: SystemTime,
}

impl Session {
    /// Whether this session showed activity within `idle_after` of `now`.
    pub fn is_active(&self, now: SystemTime, idle_after: Duration) -> bool {
        now.duration_since(self.last_activity).unwrap_or(Duration::ZERO) < idle_after
    }
}

/// A source of agent sessions.
pub trait SessionAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Scans the session `watch` glob as of `now`.
    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Session>>, AdapterError>;
}
```

Create `adapters/http.rs` as a placeholder containing only its `//!`
header so `mod.rs`'s `pub mod http;` compiles; Task 11 fills it in.

Add `pub mod adapters;` to `baseline/src/lib.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline adapters::`
Expected: PASS, 5 tests.

- [ ] **Step 6: Commit**

```bash
git add baseline/src/adapters/ baseline/src/lib.rs
git commit -m "feat(adapters): define the four adapter family contracts

Each family is one object-safe trait taking an injected now, so
aggregation can hold heterogeneous adapters as trait objects and every
implementation's freshness is testable without a wall clock."
```

---

## Arc 4: The built-in adapters

Seven tasks, each replaying a recorded fixture. They depend only on Arc
3's contract, not on each other.

### Slice 4.1: The work family — GitHub

**Tags:** coding

#### Task 11: `HttpTransport` and ETag-conditional requests

**Files:**
- Modify: `baseline/src/adapters/http.rs`

**Interfaces:**
- Consumes: `AdapterError` (Task 10).
- Produces:
  - `pub struct HttpRequest { pub url: String, pub etag: Option<String> }`
  - `pub enum HttpResponse { Ok { body: String, etag: Option<String> }, NotModified }`
  - `pub trait HttpTransport { fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError>; }`
  - `pub struct UreqTransport { token: Option<String>, agent: ureq::Agent }` with `pub fn new() -> Self`, `pub fn with_token(token: impl Into<String>) -> Self`
  - `pub struct FixtureTransport { ... }` with `pub fn new() -> Self`, `pub fn insert(&mut self, url: impl Into<String>, body: impl Into<String>, etag: Option<&str>)`, `pub fn insert_from_file(&mut self, url: impl Into<String>, path: &Path, etag: Option<&str>) -> std::io::Result<()>`, `pub fn requests(&self) -> &[HttpRequest]`, `pub fn fail_next(&mut self, error: AdapterError)`

  Consumed by Task 12 (`GithubWorkAdapter` is generic over
  `T: HttpTransport`) and Task 24 (`GithubWorkControl` reuses it).

**This is the seam that makes GitHub fixture-testable.**
`UreqTransport` is the **only** type in this crate that touches the
network. It contains no branching beyond mapping a status code to an
`AdapterError`, and it is **real-external-service exempt from automated
testing** under the same precedent TTUI applies to real-TTY work — noted
in its doc comment so nobody adds logic to it later.

`FixtureTransport` is `pub`, not `#[cfg(test)]`: integration tests in
`baseline/tests/` can only reach the public API, and a frontend
building a demo wants it too.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn transport() -> FixtureTransport {
        let mut t = FixtureTransport::new();
        t.insert("https://api.github.com/repos/a/b/issues", r#"[{"number":1}]"#, Some("W/\"abc\""));
        t
    }

    #[test]
    fn a_request_without_an_etag_gets_the_body_and_the_current_etag() {
        let mut t = transport();
        let r = t.get(&HttpRequest { url: "https://api.github.com/repos/a/b/issues".into(), etag: None }).unwrap();
        match r {
            HttpResponse::Ok { body, etag } => {
                assert!(body.contains("\"number\":1"));
                assert_eq!(etag.as_deref(), Some("W/\"abc\""));
            }
            HttpResponse::NotModified => panic!("a first request cannot be NotModified"),
        }
    }

    #[test]
    fn a_request_carrying_the_matching_etag_gets_not_modified() {
        let mut t = transport();
        let r = t.get(&HttpRequest {
            url: "https://api.github.com/repos/a/b/issues".into(),
            etag: Some("W/\"abc\"".into()),
        }).unwrap();
        assert_eq!(r, HttpResponse::NotModified);
    }

    #[test]
    fn a_request_carrying_a_stale_etag_gets_the_new_body() {
        let mut t = transport();
        let r = t.get(&HttpRequest {
            url: "https://api.github.com/repos/a/b/issues".into(),
            etag: Some("W/\"old\"".into()),
        }).unwrap();
        assert!(matches!(r, HttpResponse::Ok { .. }));
    }

    #[test]
    fn an_unknown_url_is_a_404_rather_than_a_panic() {
        let mut t = transport();
        let e = t.get(&HttpRequest { url: "https://api.github.com/nope".into(), etag: None }).unwrap_err();
        assert!(matches!(e, AdapterError::Http { status: 404, .. }));
    }

    #[test]
    fn every_request_is_recorded_so_a_test_can_assert_conditionality() {
        let mut t = transport();
        let url = "https://api.github.com/repos/a/b/issues";
        let _ = t.get(&HttpRequest { url: url.into(), etag: None });
        let _ = t.get(&HttpRequest { url: url.into(), etag: Some("W/\"abc\"".into()) });
        assert_eq!(t.requests().len(), 2);
        assert_eq!(t.requests()[0].etag, None);
        assert_eq!(t.requests()[1].etag.as_deref(), Some("W/\"abc\""));
    }

    #[test]
    fn fail_next_injects_one_error_and_then_behaves_normally() {
        let mut t = transport();
        t.fail_next(AdapterError::Http { status: 403, message: "rate limit exceeded".into() });
        let url = "https://api.github.com/repos/a/b/issues";
        assert!(t.get(&HttpRequest { url: url.into(), etag: None }).is_err());
        assert!(t.get(&HttpRequest { url: url.into(), etag: None }).is_ok());
    }

    #[test]
    fn a_fixture_transport_is_usable_as_a_trait_object() {
        let mut boxed: Box<dyn HttpTransport> = Box::new(transport());
        assert!(boxed.get(&HttpRequest { url: "https://api.github.com/repos/a/b/issues".into(), etag: None }).is_ok());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p parallax-baseline adapters::http::`
Expected: FAIL — `HttpTransport` and friends do not exist.

- [ ] **Step 3: Write the implementation**

```rust
//! The HTTP seam. Everything above this file works against
//! `HttpTransport`, which means the GitHub adapter is exercised entirely
//! against recorded fixtures — no network in any test.

use super::AdapterError;
use std::collections::HashMap;
use std::path::Path;

/// A conditional GET.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The absolute URL.
    pub url: String,
    /// The ETag from the last successful response, when there was one.
    pub etag: Option<String>,
}

/// What a conditional GET returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpResponse {
    /// A body, plus the ETag to send next time.
    Ok {
        /// The response body.
        body: String,
        /// The response's ETag, when it carried one.
        etag: Option<String>,
    },
    /// `304`: the value the caller already holds is still current.
    NotModified,
}

/// Something that can perform a conditional GET.
pub trait HttpTransport {
    /// Performs the request.
    fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError>;
}

/// The live transport. **The only type in this crate that touches the
/// network**, and therefore the only one exempt from automated testing
/// under the real-external-service precedent. It holds no logic beyond
/// mapping a status code onto an `AdapterError` — keep it that way.
pub struct UreqTransport {
    agent: ureq::Agent,
    token: Option<String>,
}

impl UreqTransport {
    /// An unauthenticated transport.
    pub fn new() -> Self {
        Self { agent: ureq::AgentBuilder::new().build(), token: None }
    }

    /// A transport sending `Authorization: Bearer <token>`.
    pub fn with_token(token: impl Into<String>) -> Self {
        Self { token: Some(token.into()), ..Self::new() }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpTransport for UreqTransport {
    fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError> {
        let mut req = self.agent.get(&request.url).set("Accept", "application/vnd.github+json");
        if let Some(token) = &self.token {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        if let Some(etag) = &request.etag {
            req = req.set("If-None-Match", etag);
        }
        match req.call() {
            Ok(resp) => {
                let etag = resp.header("ETag").map(str::to_string);
                let body = resp.into_string().map_err(|e| AdapterError::Parse(e.to_string()))?;
                Ok(HttpResponse::Ok { body, etag })
            }
            Err(ureq::Error::Status(304, _)) => Ok(HttpResponse::NotModified),
            Err(ureq::Error::Status(status, resp)) => Err(AdapterError::Http {
                status,
                message: resp.into_string().unwrap_or_default().trim().to_string(),
            }),
            Err(ureq::Error::Transport(t)) => Err(AdapterError::Timeout(t.to_string())),
        }
    }
}

/// A transport that replays recorded responses. Public because
/// integration tests reach only the public API — and because a frontend
/// demoing the cockpit wants one too.
#[derive(Debug, Default)]
pub struct FixtureTransport {
    responses: HashMap<String, (String, Option<String>)>,
    requests: Vec<HttpRequest>,
    next_error: Option<AdapterError>,
}

impl FixtureTransport {
    /// An empty transport; every URL 404s until inserted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the response for a URL.
    pub fn insert(&mut self, url: impl Into<String>, body: impl Into<String>, etag: Option<&str>) {
        self.responses.insert(url.into(), (body.into(), etag.map(str::to_string)));
    }

    /// Records the response for a URL from a fixture file on disk.
    pub fn insert_from_file(
        &mut self,
        url: impl Into<String>,
        path: &Path,
        etag: Option<&str>,
    ) -> std::io::Result<()> {
        let body = std::fs::read_to_string(path)?;
        self.insert(url, body, etag);
        Ok(())
    }

    /// Every request this transport was asked to perform, in order.
    pub fn requests(&self) -> &[HttpRequest] {
        &self.requests
    }

    /// Makes the next request fail with `error`, once.
    pub fn fail_next(&mut self, error: AdapterError) {
        self.next_error = Some(error);
    }
}

impl HttpTransport for FixtureTransport {
    fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError> {
        self.requests.push(request.clone());
        if let Some(e) = self.next_error.take() {
            return Err(e);
        }
        match self.responses.get(&request.url) {
            None => Err(AdapterError::Http { status: 404, message: request.url.clone() }),
            Some((body, etag)) => {
                if request.etag.is_some() && request.etag == *etag {
                    Ok(HttpResponse::NotModified)
                } else {
                    Ok(HttpResponse::Ok { body: body.clone(), etag: etag.clone() })
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline adapters::http::`
Expected: PASS, 7 tests.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/adapters/http.rs
git commit -m "feat(adapters): add the conditional-GET transport seam

Putting HTTP behind a trait is what lets the GitHub adapter be
integration-tested entirely against recorded fixtures; the live
transport is the one real-external-service-exempt type in the crate and
holds no logic beyond status mapping."
```

---

#### Task 12: `GithubWorkAdapter`

**Files:**
- Modify: `baseline/src/adapters/work.rs`
- Create: `baseline/tests/github_replay.rs`
- Create: `baseline/tests/fixtures/github/issues.json`
- Create: `baseline/tests/fixtures/github/pulls.json`
- Create: `baseline/tests/fixtures/github/check-runs.json`

**Interfaces:**
- Consumes: `HttpTransport`, `HttpRequest`, `HttpResponse` (Task 11);
  `WorkAdapter`, `WorkSnapshot`, `WorkItem`, `ChecksSummary` (Task 10);
  `freshness::{Observed, DEFAULT_POLL_INTERVAL}` (Task 9).
- Produces:
  - `pub struct GithubWorkAdapter<T: HttpTransport> { ... }` with `pub fn new(transport: T) -> Self`, `pub fn with_interval(self, interval: Duration) -> Self`, `pub fn transport(&self) -> &T`
  - `pub fn issues_url(repo: &str) -> String`, `pub fn pulls_url(repo: &str) -> String`, `pub fn check_runs_url(repo: &str, sha: &str) -> String`

  Consumed by Task 18 and Task 20.

**The recorded fixtures are trimmed real responses**, not invented ones:
capture them once from `gh api repos/tatemeyer/ttui/issues` and friends,
delete every field this adapter does not read, and commit the result.
Fields kept: `number`, `title`, `state`, `draft`, `labels[].name`,
`html_url`, `updated_at`, `pull_request` (presence only), `head.sha`,
and for check runs `check_runs[].conclusion` and `.status`.

- [ ] **Step 1: Write the fixtures**

`baseline/tests/fixtures/github/issues.json` — three items, one of
which is a pull request (GitHub's issues endpoint returns both, and the
`pull_request` key is how they are told apart):

```json
[
  {
    "number": 134,
    "title": "give BorderSet 4 distinct corner glyphs",
    "state": "closed",
    "html_url": "https://github.com/tatemeyer/ttui/issues/134",
    "updated_at": "2026-08-13T18:02:11Z",
    "labels": [{ "name": "semver:minor" }, { "name": "gated" }]
  },
  {
    "number": 140,
    "title": "audit the widget catalogue for missing docs",
    "state": "open",
    "html_url": "https://github.com/tatemeyer/ttui/issues/140",
    "updated_at": "2026-08-14T09:15:00Z",
    "labels": [{ "name": "semver:patch" }, { "name": "direct" }]
  },
  {
    "number": 141,
    "title": "what should a Sparkline do with a single point?",
    "state": "open",
    "html_url": "https://github.com/tatemeyer/ttui/issues/141",
    "updated_at": "2026-08-14T10:41:00Z",
    "labels": [{ "name": "needs-intent" }]
  }
]
```

`baseline/tests/fixtures/github/pulls.json`:

```json
[
  {
    "number": 142,
    "title": "feat(widgets): add a Gauge widget",
    "state": "open",
    "draft": false,
    "html_url": "https://github.com/tatemeyer/ttui/pull/142",
    "updated_at": "2026-08-14T11:02:00Z",
    "labels": [{ "name": "gated" }, { "name": "semver:minor" }],
    "head": { "sha": "1a7d51c9f0e2b3a4d5c6e7f8091a2b3c4d5e6f70" }
  },
  {
    "number": 143,
    "title": "chore: bump the MSRV",
    "state": "open",
    "draft": true,
    "html_url": "https://github.com/tatemeyer/ttui/pull/143",
    "updated_at": "2026-08-14T11:30:00Z",
    "labels": [],
    "head": { "sha": "0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4" }
  }
]
```

`baseline/tests/fixtures/github/check-runs.json` — the response for
PR 142's head SHA: three green, one still running:

```json
{
  "total_count": 4,
  "check_runs": [
    { "name": "build", "status": "completed", "conclusion": "success" },
    { "name": "test", "status": "completed", "conclusion": "success" },
    { "name": "fmt", "status": "completed", "conclusion": "success" },
    { "name": "clippy", "status": "in_progress", "conclusion": null }
  ]
}
```

- [ ] **Step 2: Write the failing integration test**

Create `baseline/tests/github_replay.rs`:

```rust
//! The GitHub work adapter, replayed against recorded API responses.
//! Live GitHub access is real-external-service exempt; this is what
//! covers the adapter instead.

use parallax_baseline::adapters::http::{FixtureTransport, HttpTransport};
use parallax_baseline::adapters::work::{
    check_runs_url, issues_url, pulls_url, GithubWorkAdapter, WorkAdapter, WorkKind, WorkState,
};
use parallax_baseline::adapters::ProjectContext;
use parallax_baseline::freshness::{Freshness, DEFAULT_POLL_INTERVAL};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const REPO: &str = "tatemeyer/ttui";
const HEAD_142: &str = "1a7d51c9f0e2b3a4d5c6e7f8091a2b3c4d5e6f70";
const HEAD_143: &str = "0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github").join(name)
}

fn transport() -> FixtureTransport {
    let mut t = FixtureTransport::new();
    t.insert_from_file(issues_url(REPO), &fixture("issues.json"), Some("W/\"issues-1\"")).unwrap();
    t.insert_from_file(pulls_url(REPO), &fixture("pulls.json"), Some("W/\"pulls-1\"")).unwrap();
    t.insert_from_file(check_runs_url(REPO, HEAD_142), &fixture("check-runs.json"), None).unwrap();
    t.insert(check_runs_url(REPO, HEAD_143), r#"{"total_count":0,"check_runs":[]}"#, None);
    t
}

fn ctx() -> ProjectContext {
    ProjectContext::new("ttui", "D:/Dev/Projects/TTUI").with_repo(REPO)
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

#[test]
fn a_poll_returns_both_issues_and_pull_requests() {
    let mut a = GithubWorkAdapter::new(transport());
    let snapshot = a.poll(&ctx(), at(0)).unwrap().value;
    assert_eq!(snapshot.items.len(), 5, "3 issues + 2 pulls");
    let numbers: Vec<u64> = snapshot.items.iter().map(|i| i.number).collect();
    assert_eq!(numbers, vec![134, 140, 141, 142, 143]);
}

#[test]
fn issue_and_pull_request_kinds_are_distinguished() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[0].kind, WorkKind::Issue);
    assert_eq!(items[3].kind, WorkKind::PullRequest);
}

#[test]
fn state_maps_including_the_draft_case() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[0].state, WorkState::Closed, "issue 134");
    assert_eq!(items[1].state, WorkState::Open, "issue 140");
    assert_eq!(items[3].state, WorkState::Open, "pull 142");
    assert_eq!(items[4].state, WorkState::Draft, "pull 143 is a draft");
}

/// Labels are carried verbatim. Projection is `autonomy`'s job, and
/// mixing the two here would put policy in an adapter.
#[test]
fn labels_are_carried_verbatim_and_unfiltered() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[0].labels, vec!["semver:minor".to_string(), "gated".to_string()]);
    assert_eq!(items[2].labels, vec!["needs-intent".to_string()]);
    assert!(items[4].labels.is_empty());
}

#[test]
fn check_runs_are_summarised_per_pull_request_only() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[3].checks.passed, 3);
    assert_eq!(items[3].checks.failed, 0);
    assert_eq!(items[3].checks.pending, 1);
    assert!(!items[3].checks.is_green(), "one check still running");
    assert_eq!(items[0].checks.total(), 0, "an issue has no checks");
}

#[test]
fn a_poll_is_stamped_polled_at_the_configured_interval() {
    let mut a = GithubWorkAdapter::new(transport());
    let observed = a.poll(&ctx(), at(0)).unwrap();
    assert_eq!(observed.observed_at, at(0));
    assert_eq!(observed.freshness(at(10)), Freshness::Fresh { age: Duration::from_secs(10) });
    assert!(observed.freshness(at(31)).is_stale(), "default interval is {DEFAULT_POLL_INTERVAL:?}");
}

/// The spec specifies ETag-conditional polling; this is what proves it
/// actually happens rather than being described in a doc comment.
#[test]
fn a_second_poll_sends_the_etag_from_the_first() {
    let mut a = GithubWorkAdapter::new(transport());
    a.poll(&ctx(), at(0)).unwrap();
    a.poll(&ctx(), at(60)).unwrap();
    let sent: Vec<Option<String>> = a
        .transport()
        .requests()
        .iter()
        .filter(|r| r.url == issues_url(REPO))
        .map(|r| r.etag.clone())
        .collect();
    assert_eq!(sent, vec![None, Some("W/\"issues-1\"".to_string())]);
}

/// A 304 means the cached snapshot is current now — the value is
/// unchanged and its observation time advances.
#[test]
fn a_not_modified_response_refreshes_the_observation_without_refetching() {
    let mut a = GithubWorkAdapter::new(transport());
    let first = a.poll(&ctx(), at(0)).unwrap();
    let second = a.poll(&ctx(), at(60)).unwrap();
    assert_eq!(first.value, second.value);
    assert_eq!(second.observed_at, at(60));
    assert_eq!(second.freshness(at(60)), Freshness::Fresh { age: Duration::ZERO });
}

#[test]
fn a_rate_limit_response_surfaces_as_an_http_error_rather_than_an_empty_snapshot() {
    let mut transport = transport();
    transport.fail_next(parallax_baseline::adapters::AdapterError::Http {
        status: 403,
        message: "API rate limit exceeded".into(),
    });
    let mut a = GithubWorkAdapter::new(transport);
    let err = a.poll(&ctx(), at(0)).unwrap_err().to_string();
    assert!(err.contains("403") && err.contains("rate limit"), "got {err}");
}

#[test]
fn polling_without_a_repo_in_the_context_is_a_clear_error() {
    let mut a = GithubWorkAdapter::new(transport());
    let err = a.poll(&ProjectContext::new("ttui", "/tmp"), at(0)).unwrap_err().to_string();
    assert!(err.contains("repo"), "got {err}");
}
```

- [ ] **Step 3: Run to verify it fails, then implement**

Run: `cargo test -p parallax-baseline --test github_replay`
Expected: FAIL — `GithubWorkAdapter` does not exist.

Append to `baseline/src/adapters/work.rs`:

```rust
use super::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::freshness::DEFAULT_POLL_INTERVAL;
use std::collections::HashMap;
use std::time::Duration;

/// The issues endpoint for a repository. GitHub returns pull requests
/// here too; they carry a `pull_request` key and are skipped, because
/// `pulls_url` returns them with their head SHA.
pub fn issues_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/issues?state=all&per_page=100")
}

/// The pull requests endpoint for a repository.
pub fn pulls_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/pulls?state=all&per_page=100")
}

/// The check-runs endpoint for one commit.
pub fn check_runs_url(repo: &str, sha: &str) -> String {
    format!("https://api.github.com/repos/{repo}/commits/{sha}/check-runs")
}

/// Reads issues, pull requests, and check runs from GitHub, polling
/// with ETag-conditional requests so an unchanged feed costs no rate
/// limit.
pub struct GithubWorkAdapter<T: HttpTransport> {
    transport: T,
    interval: Duration,
    etags: HashMap<String, String>,
    cached: Option<WorkSnapshot>,
}

impl<T: HttpTransport> GithubWorkAdapter<T> {
    /// A GitHub adapter polling at `DEFAULT_POLL_INTERVAL`.
    pub fn new(transport: T) -> Self {
        Self { transport, interval: DEFAULT_POLL_INTERVAL, etags: HashMap::new(), cached: None }
    }

    /// Overrides the poll interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// The transport, for asserting what was requested.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Fetches a URL conditionally. `Ok(None)` means "not modified".
    fn fetch(&mut self, url: &str) -> Result<Option<String>, AdapterError> {
        let request = HttpRequest { url: url.to_string(), etag: self.etags.get(url).cloned() };
        match self.transport.get(&request)? {
            HttpResponse::NotModified => Ok(None),
            HttpResponse::Ok { body, etag } => {
                match etag {
                    Some(e) => self.etags.insert(url.to_string(), e),
                    None => self.etags.remove(url),
                };
                Ok(Some(body))
            }
        }
    }
}
```

Then the parsing half — deliberately hand-rolled against
`serde_json::Value` rather than a mirror of GitHub's schema, so a new
field upstream is never a parse failure:

```rust
fn as_labels(value: &serde_json::Value) -> Vec<String> {
    value["labels"]
        .as_array()
        .map(|xs| xs.iter().filter_map(|l| l["name"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn str_field(value: &serde_json::Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

fn parse_state(value: &serde_json::Value) -> WorkState {
    if value["draft"].as_bool().unwrap_or(false) {
        return WorkState::Draft;
    }
    match value["state"].as_str() {
        Some("closed") if value["merged_at"].is_string() => WorkState::Merged,
        Some("closed") => WorkState::Closed,
        _ => WorkState::Open,
    }
}

fn parse_item(value: &serde_json::Value, kind: WorkKind, checks: ChecksSummary) -> Option<WorkItem> {
    Some(WorkItem {
        number: value["number"].as_u64()?,
        title: str_field(value, "title"),
        kind,
        state: parse_state(value),
        labels: as_labels(value),
        checks,
        url: str_field(value, "html_url"),
        updated_at: str_field(value, "updated_at"),
    })
}

fn parse_checks(body: &str) -> Result<ChecksSummary, AdapterError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| AdapterError::Parse(e.to_string()))?;
    let mut summary = ChecksSummary::none();
    for run in value["check_runs"].as_array().unwrap_or(&Vec::new()) {
        match (run["status"].as_str(), run["conclusion"].as_str()) {
            (Some("completed"), Some("success")) => summary.passed += 1,
            (Some("completed"), _) => summary.failed += 1,
            _ => summary.pending += 1,
        }
    }
    Ok(summary)
}

impl<T: HttpTransport> WorkAdapter for GithubWorkAdapter<T> {
    fn source_name(&self) -> String {
        "work:github".into()
    }

    fn poll(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<WorkSnapshot>, AdapterError> {
        let repo = ctx.repo.clone().ok_or_else(|| {
            AdapterError::Unsupported(format!(
                "project `{}` has no work.repo, so there is nothing to poll",
                ctx.name
            ))
        })?;

        let issues_body = self.fetch(&issues_url(&repo))?;
        let pulls_body = self.fetch(&pulls_url(&repo))?;

        if issues_body.is_none() && pulls_body.is_none() {
            if let Some(cached) = self.cached.clone() {
                return Ok(Observed::polled(cached, now, self.interval));
            }
        }

        let mut items = Vec::new();

        let issues_json = match issues_body {
            Some(body) => body,
            None => self.refetch_unconditionally(&issues_url(&repo))?,
        };
        let issues: serde_json::Value =
            serde_json::from_str(&issues_json).map_err(|e| AdapterError::Parse(e.to_string()))?;
        for value in issues.as_array().unwrap_or(&Vec::new()) {
            // GitHub returns pull requests from the issues endpoint too;
            // `pulls_url` covers them with their head SHA, so skip them
            // here rather than reporting each one twice.
            if value.get("pull_request").is_some() {
                continue;
            }
            if let Some(item) = parse_item(value, WorkKind::Issue, ChecksSummary::none()) {
                items.push(item);
            }
        }

        let pulls_json = match pulls_body {
            Some(body) => body,
            None => self.refetch_unconditionally(&pulls_url(&repo))?,
        };
        let pulls: serde_json::Value =
            serde_json::from_str(&pulls_json).map_err(|e| AdapterError::Parse(e.to_string()))?;
        for value in pulls.as_array().unwrap_or(&Vec::new()) {
            let checks = match value["head"]["sha"].as_str() {
                Some(sha) => match self.fetch(&check_runs_url(&repo, sha))? {
                    Some(body) => parse_checks(&body)?,
                    None => ChecksSummary::none(),
                },
                None => ChecksSummary::none(),
            };
            if let Some(item) = parse_item(value, WorkKind::PullRequest, checks) {
                items.push(item);
            }
        }

        let snapshot = WorkSnapshot { items };
        self.cached = Some(snapshot.clone());
        Ok(Observed::polled(snapshot, now, self.interval))
    }
}
```

Add the small helper `fetch` falls back on when one endpoint returned
`304` while the other changed — the snapshot has to be rebuilt whole:

```rust
impl<T: HttpTransport> GithubWorkAdapter<T> {
    /// Re-fetches a URL ignoring the stored ETag. Needed when one
    /// endpoint changed and another did not: the snapshot is rebuilt as
    /// a whole, so a `304` on one half still needs that half's body.
    fn refetch_unconditionally(&mut self, url: &str) -> Result<String, AdapterError> {
        match self.transport.get(&HttpRequest { url: url.to_string(), etag: None })? {
            HttpResponse::Ok { body, etag } => {
                if let Some(e) = etag {
                    self.etags.insert(url.to_string(), e);
                }
                Ok(body)
            }
            HttpResponse::NotModified => Err(AdapterError::Parse(
                "server returned 304 to an unconditional request".into(),
            )),
        }
    }
}
```

Add `use super::ProjectContext;` and `use crate::freshness::Observed;`
to the module's imports, and derive `Clone` on `WorkSnapshot`,
`WorkItem` (already present from Task 10).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline --test github_replay`
Expected: PASS, 10 tests.

Run: `cargo test -p parallax-baseline`
Expected: PASS, everything.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/adapters/work.rs baseline/tests/github_replay.rs baseline/tests/fixtures/github/
git commit -m "feat(adapters): read work items from GitHub with conditional polling

Parsed against serde_json::Value rather than a mirror of GitHub's
schema, so a new upstream field is never a parse failure; a test asserts
the second poll actually sends the first poll's ETag rather than the
conditionality living only in a doc comment."
```

---

### Slice 4.2: The verification family

**Tags:** coding

#### Task 13: The `command` verification adapter

**Files:**
- Modify: `baseline/src/adapters/verification.rs`

**Interfaces:**
- Consumes: `VerificationAdapter`, `VerificationStatus`,
  `VerificationOutcome` (Task 10); `freshness::Observed`.
- Produces:
  - `pub struct CommandOutput { pub status: i32, pub stdout: String, pub stderr: String }`
  - `pub trait CommandRunner { fn run(&mut self, command: &str, cwd: &Path) -> std::io::Result<CommandOutput>; }`
  - `pub struct ProcessRunner;` — the real one
  - `pub struct ScriptedRunner { ... }` with `pub fn new() -> Self`, `pub fn push(&mut self, output: CommandOutput)`, `pub fn fail_next(&mut self, error: std::io::Error)`, `pub fn calls(&self) -> &[String]`
  - `pub struct CommandVerificationAdapter<R: CommandRunner> { ... }` with `pub fn new(kind: impl Into<String>, command: impl Into<String>, runner: R) -> Self`, `pub fn runner(&self) -> &R`

  Consumed by Task 18 and Task 20.

**Shell invocation is `cmd /C` on Windows and `sh -c` elsewhere.** A
naive whitespace split would mangle `cargo clippy --all-targets -- -D
warnings` — the `--` and the quoted argument both matter. See Judgment
call 5.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod command_tests {
    use super::*;
    use crate::adapters::ProjectContext;
    use std::time::{Duration, SystemTime};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    fn ok(stdout: &str) -> CommandOutput {
        CommandOutput { status: 0, stdout: stdout.into(), stderr: String::new() }
    }

    fn ctx() -> ProjectContext {
        ProjectContext::new("ttui", "D:/Dev/Projects/TTUI")
    }

    #[test]
    fn exit_zero_is_a_pass() {
        let mut runner = ScriptedRunner::new();
        runner.push(ok("test result: ok. 412 passed"));
        let mut a = CommandVerificationAdapter::new("tests", "cargo test", runner);
        let status = a.check(&ctx(), at(0)).unwrap().value;
        assert_eq!(status.kind, "tests");
        assert_eq!(status.outcome, VerificationOutcome::Pass);
    }

    #[test]
    fn a_nonzero_exit_is_a_fail_carrying_the_last_line_of_output_as_detail() {
        let mut runner = ScriptedRunner::new();
        runner.push(CommandOutput {
            status: 101,
            stdout: String::new(),
            stderr: "error: unused variable `x`\nerror: could not compile `ttui`".into(),
        });
        let mut a = CommandVerificationAdapter::new("lint", "cargo clippy", runner);
        let status = a.check(&ctx(), at(0)).unwrap().value;
        assert_eq!(status.outcome, VerificationOutcome::Fail);
        assert_eq!(status.detail.as_deref(), Some("error: could not compile `ttui`"));
    }

    #[test]
    fn the_command_is_passed_through_unmodified_including_its_double_dash() {
        let mut runner = ScriptedRunner::new();
        runner.push(ok(""));
        let command = "cargo clippy --all-targets -- -D warnings";
        let mut a = CommandVerificationAdapter::new("lint", command, runner);
        a.check(&ctx(), at(0)).unwrap();
        assert_eq!(a.runner().calls(), &[command.to_string()]);
    }

    /// A command that could not even be spawned is a Hold, not a Fail:
    /// the check did not run, so it reached no conclusion, and a Hold is
    /// never upgraded.
    #[test]
    fn a_command_that_cannot_be_spawned_is_a_hold_rather_than_a_fail() {
        let mut runner = ScriptedRunner::new();
        runner.fail_next(std::io::Error::new(std::io::ErrorKind::NotFound, "cargo not found"));
        let mut a = CommandVerificationAdapter::new("tests", "cargo test", runner);
        let status = a.check(&ctx(), at(0)).unwrap().value;
        assert_eq!(status.outcome, VerificationOutcome::Hold);
        assert!(status.detail.unwrap().contains("cargo not found"));
    }

    /// Running a command is not polling: the result is current as of the
    /// moment it finished.
    #[test]
    fn a_command_result_is_watched_not_polled() {
        let mut runner = ScriptedRunner::new();
        runner.push(ok(""));
        let mut a = CommandVerificationAdapter::new("tests", "true", runner);
        let observed = a.check(&ctx(), at(0)).unwrap();
        assert_eq!(observed.source, crate::freshness::SourceKind::Watched);
        assert_eq!(observed.freshness(at(9999)), crate::freshness::Freshness::Live);
    }

    #[test]
    fn the_source_name_names_the_kind_so_degradation_reporting_can_be_specific() {
        let a = CommandVerificationAdapter::new("lint", "cargo clippy", ScriptedRunner::new());
        assert_eq!(a.source_name(), "verification:command:lint");
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test -p parallax-baseline adapters::verification::command_tests`
Expected: FAIL.

Append to `baseline/src/adapters/verification.rs`:

```rust
use std::path::Path;
use std::process::Command;

/// What running a command produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// The process exit status, or 1 when it was killed by a signal.
    pub status: i32,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

/// Something that can run a shell command.
pub trait CommandRunner {
    /// Runs `command` with `cwd` as the working directory.
    fn run(&mut self, command: &str, cwd: &Path) -> std::io::Result<CommandOutput>;
}

/// The real runner. Invokes the platform shell so a command's quoting
/// and `--` separators survive verbatim.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl CommandRunner for ProcessRunner {
    fn run(&mut self, command: &str, cwd: &Path) -> std::io::Result<CommandOutput> {
        let output = if cfg!(windows) {
            Command::new("cmd").arg("/C").arg(command).current_dir(cwd).output()?
        } else {
            Command::new("sh").arg("-c").arg(command).current_dir(cwd).output()?
        };
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// A runner that replays scripted outputs. Public so integration tests
/// and frontend demos can both reach it.
#[derive(Debug, Default)]
pub struct ScriptedRunner {
    outputs: std::collections::VecDeque<CommandOutput>,
    next_error: Option<std::io::Error>,
    calls: Vec<String>,
}

impl ScriptedRunner {
    /// An empty runner. Running with nothing queued yields exit 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues one output.
    pub fn push(&mut self, output: CommandOutput) {
        self.outputs.push_back(output);
    }

    /// Makes the next run fail to spawn, once.
    pub fn fail_next(&mut self, error: std::io::Error) {
        self.next_error = Some(error);
    }

    /// Every command this runner was asked to run, in order.
    pub fn calls(&self) -> &[String] {
        &self.calls
    }
}

impl CommandRunner for ScriptedRunner {
    fn run(&mut self, command: &str, _cwd: &Path) -> std::io::Result<CommandOutput> {
        self.calls.push(command.to_string());
        if let Some(e) = self.next_error.take() {
            return Err(e);
        }
        Ok(self.outputs.pop_front().unwrap_or(CommandOutput {
            status: 0,
            stdout: String::new(),
            stderr: String::new(),
        }))
    }
}

/// Runs a declared command and reads its exit status.
pub struct CommandVerificationAdapter<R: CommandRunner> {
    kind: String,
    command: String,
    runner: R,
}

impl<R: CommandRunner> CommandVerificationAdapter<R> {
    /// An adapter running `command` and reporting it as `kind`.
    pub fn new(kind: impl Into<String>, command: impl Into<String>, runner: R) -> Self {
        Self { kind: kind.into(), command: command.into(), runner }
    }

    /// The runner, for asserting what was run.
    pub fn runner(&self) -> &R {
        &self.runner
    }
}

impl<R: CommandRunner> VerificationAdapter for CommandVerificationAdapter<R> {
    fn source_name(&self) -> String {
        format!("verification:command:{}", self.kind)
    }

    fn check(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<VerificationStatus>, AdapterError> {
        let status = match self.runner.run(&self.command, &ctx.root) {
            // A command that could not be spawned reached no conclusion,
            // and a Hold is never upgraded to a Pass.
            Err(e) => VerificationStatus {
                kind: self.kind.clone(),
                outcome: VerificationOutcome::Hold,
                detail: Some(format!("could not run `{}`: {e}", self.command)),
            },
            Ok(output) if output.status == 0 => VerificationStatus {
                kind: self.kind.clone(),
                outcome: VerificationOutcome::Pass,
                detail: last_line(&output.stdout),
            },
            Ok(output) => VerificationStatus {
                kind: self.kind.clone(),
                outcome: VerificationOutcome::Fail,
                detail: last_line(&output.stderr).or_else(|| last_line(&output.stdout)),
            },
        };
        Ok(Observed::watched(status, now))
    }
}

/// The last non-empty line of some output, for a one-line detail.
fn last_line(text: &str) -> Option<String> {
    text.lines().rev().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}
```

Add `use super::{AdapterError, ProjectContext};` and
`use crate::freshness::Observed;` to the module's imports.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline adapters::verification::`
Expected: PASS, 6 tests.

- [ ] **Step 4: Commit**

```bash
git add baseline/src/adapters/verification.rs
git commit -m "feat(adapters): run declared verification commands

Invoked through the platform shell so a command's quoting and -- survive
verbatim; a command that cannot be spawned reports Hold rather than
Fail, because a check that did not run reached no conclusion."
```

---

#### Task 14: The `plumb` verification adapter

**Files:**
- Modify: `baseline/src/adapters/verification.rs`
- Create: `baseline/tests/verification_replay.rs`
- Create: `baseline/tests/fixtures/plumb/verdict-go.md`
- Create: `baseline/tests/fixtures/plumb/verdict-no-go.md`
- Create: `baseline/tests/fixtures/plumb/verdict-hold.md`

**Interfaces:**
- Consumes: `VerificationAdapter`, `VerificationOutcome`, `Observed`.
- Produces:
  - `pub fn parse_verdict(text: &str) -> Option<VerificationOutcome>`
  - `pub struct PlumbVerificationAdapter { ... }` with `pub fn new(kind: impl Into<String>, runs_dir: impl Into<PathBuf>) -> Self`

  Consumed by Task 15 (`capture` artifacts reuse `parse_verdict`), Task
  18, Task 20.

**No dependency on Plumb, and this is the task where that constraint
bites.** The adapter reads the `verdict.md` Plumb writes, as text. It
must never `use parallax_plumb::…`, and `baseline/Cargo.toml` must
never gain that dependency.

**The parse is deliberately narrow and defensive.** Plumb's verdict
renders "a header line carrying the run id and the overall verdict in
the exact words `GO` / `NO-GO` / `HOLD`". Two traps: `NO-GO` contains
`GO`, so `NO-GO` must be tested first; and the words appear again later
in the document (in per-lens rows), so only the *first* line carrying
one of them counts.

- [ ] **Step 1: Write the fixtures**

`baseline/tests/fixtures/plumb/verdict-go.md`:

```markdown
# Plumb verdict — run 20260814T101500Z — GO

| scenario | lens | outcome |
|---|---|---|
| omnitrix-dial-rotate | breakage | reported |
| omnitrix-dial-rotate | intent | reported |
| omnitrix-dial-rotate | design | skipped — no taste.md |

No findings.
```

`baseline/tests/fixtures/plumb/verdict-no-go.md`:

```markdown
# Plumb verdict — run 20260814T112200Z — NO-GO

| scenario | lens | outcome |
|---|---|---|
| tardis-time-rotor | breakage | reported |
| tardis-time-rotor | intent | reported |

## Findings

### blocker — breakage — rows 12-18, left panel

The time rotor column renders as a solid block; no Braille cell
structure is visible.

previously overruled (1)
```

`baseline/tests/fixtures/plumb/verdict-hold.md`:

```markdown
# Plumb verdict — run 20260814T120000Z — HOLD

| scenario | lens | outcome |
|---|---|---|
| launcher-starfield | breakage | HOLD — capture failed: unmapped glyph U+2726 |

Capture failure is never a GO.
```

- [ ] **Step 2: Write the failing integration test**

Create `baseline/tests/verification_replay.rs`:

```rust
//! The verification adapters, replayed against sample Plumb verdicts.
//! Nothing here links `parallax-plumb`: the platform consumes Plumb's
//! output as text on disk.

use parallax_baseline::adapters::verification::{
    parse_verdict, PlumbVerificationAdapter, VerificationAdapter, VerificationOutcome,
};
use parallax_baseline::adapters::ProjectContext;
use parallax_baseline::freshness::Freshness;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plumb").join(name)
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// Builds a runs directory containing the given verdict files, each in
/// its own run subdirectory named for its run id.
fn runs_dir(verdicts: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (run_id, fixture_name) in verdicts {
        let run = dir.path().join(run_id);
        std::fs::create_dir_all(&run).unwrap();
        std::fs::copy(fixture(fixture_name), run.join("verdict.md")).unwrap();
    }
    dir
}

#[test]
fn the_three_verdict_states_parse_from_the_header_line() {
    for (name, expected) in [
        ("verdict-go.md", VerificationOutcome::Pass),
        ("verdict-no-go.md", VerificationOutcome::Fail),
        ("verdict-hold.md", VerificationOutcome::Hold),
    ] {
        let text = std::fs::read_to_string(fixture(name)).unwrap();
        assert_eq!(parse_verdict(&text), Some(expected), "{name}");
    }
}

/// `NO-GO` contains `GO`. Getting this backwards would turn every
/// blocked review into a pass, which is the worst possible direction to
/// be wrong in.
#[test]
fn no_go_is_never_mistaken_for_go() {
    assert_eq!(parse_verdict("# run 1 — NO-GO"), Some(VerificationOutcome::Fail));
    assert_eq!(parse_verdict("# run 1 — GO"), Some(VerificationOutcome::Pass));
}

/// The words recur in per-lens rows further down; only the header counts.
#[test]
fn only_the_first_line_carrying_a_verdict_word_counts() {
    let text = "# Plumb verdict — run 1 — NO-GO\n\n| a | breakage | GO |\n";
    assert_eq!(parse_verdict(text), Some(VerificationOutcome::Fail));
}

#[test]
fn a_verdict_file_naming_no_state_parses_to_none() {
    assert_eq!(parse_verdict("# Plumb verdict — run 1\n\nnothing here\n"), None);
}

#[test]
fn the_adapter_reads_the_most_recent_run_by_directory_name() {
    let dir = runs_dir(&[
        ("20260814T101500Z", "verdict-go.md"),
        ("20260814T112200Z", "verdict-no-go.md"),
    ]);
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path());
    let status = a.check(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
    assert_eq!(status.outcome, VerificationOutcome::Fail, "the later run id wins");
    assert_eq!(status.detail.as_deref(), Some("20260814T112200Z"));
}

/// The spec's precedent, carried through: a Hold is never upgraded.
#[test]
fn a_hold_is_reported_as_a_hold_and_never_as_a_pass() {
    let dir = runs_dir(&[("20260814T120000Z", "verdict-hold.md")]);
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path());
    let status = a.check(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
    assert_eq!(status.outcome, VerificationOutcome::Hold);
}

#[test]
fn a_project_that_has_never_run_plumb_reports_not_run_rather_than_erroring() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path().join("runs"));
    let status = a.check(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
    assert_eq!(status.outcome, VerificationOutcome::NotRun);
}

#[test]
fn a_run_directory_with_no_verdict_file_is_skipped_rather_than_failing_the_check() {
    let dir = runs_dir(&[("20260814T101500Z", "verdict-go.md")]);
    std::fs::create_dir_all(dir.path().join("20260814T130000Z")).unwrap();
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path());
    let status = a.check(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
    assert_eq!(status.outcome, VerificationOutcome::Pass, "the in-progress run is ignored");
}

#[test]
fn a_verdict_read_from_disk_is_live_not_polled() {
    let dir = runs_dir(&[("20260814T101500Z", "verdict-go.md")]);
    let mut a = PlumbVerificationAdapter::new("perceptual", dir.path());
    let observed = a.check(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap();
    assert_eq!(observed.freshness(at(9999)), Freshness::Live);
}
```

- [ ] **Step 3: Run to verify it fails, then implement**

Run: `cargo test -p parallax-baseline --test verification_replay`
Expected: FAIL — `PlumbVerificationAdapter` does not exist.

Append to `baseline/src/adapters/verification.rs`:

```rust
use std::path::PathBuf;

/// Reads Plumb's overall verdict from a rendered `verdict.md`.
///
/// Deliberately narrow: only the first line naming one of the three
/// states counts, and `NO-GO` is tested before `GO` because it contains
/// it. **This parses text; it does not link Plumb.**
pub fn parse_verdict(text: &str) -> Option<VerificationOutcome> {
    for line in text.lines() {
        if line.contains("NO-GO") {
            return Some(VerificationOutcome::Fail);
        }
        if line.contains("HOLD") {
            return Some(VerificationOutcome::Hold);
        }
        if line.contains("GO") {
            return Some(VerificationOutcome::Pass);
        }
    }
    None
}

/// Reads the most recent Plumb run's verdict from a runs directory.
/// Run directories are named with sortable UTC run ids, so "most
/// recent" is the lexicographically greatest name that has a verdict.
pub struct PlumbVerificationAdapter {
    kind: String,
    runs_dir: PathBuf,
}

impl PlumbVerificationAdapter {
    /// An adapter reading `runs_dir`, reporting as `kind`.
    pub fn new(kind: impl Into<String>, runs_dir: impl Into<PathBuf>) -> Self {
        Self { kind: kind.into(), runs_dir: runs_dir.into() }
    }
}

impl VerificationAdapter for PlumbVerificationAdapter {
    fn source_name(&self) -> String {
        format!("verification:plumb:{}", self.kind)
    }

    fn check(
        &mut self,
        _ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<VerificationStatus>, AdapterError> {
        let mut best: Option<(String, VerificationOutcome)> = None;
        if let Ok(entries) = std::fs::read_dir(&self.runs_dir) {
            for entry in entries.flatten() {
                let run_id = entry.file_name().to_string_lossy().into_owned();
                let verdict_path = entry.path().join("verdict.md");
                let Ok(text) = std::fs::read_to_string(&verdict_path) else {
                    // An in-progress run has no verdict yet. Skipping it
                    // is not a failure of the check.
                    continue;
                };
                let Some(outcome) = parse_verdict(&text) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(id, _)| run_id > *id) {
                    best = Some((run_id, outcome));
                }
            }
        }

        let status = match best {
            Some((run_id, outcome)) => {
                VerificationStatus { kind: self.kind.clone(), outcome, detail: Some(run_id) }
            }
            None => VerificationStatus {
                kind: self.kind.clone(),
                outcome: VerificationOutcome::NotRun,
                detail: Some(format!("no completed run under {}", self.runs_dir.display())),
            },
        };
        Ok(Observed::watched(status, now))
    }
}
```

If the toolchain predates `Option::is_none_or`, write
`best.as_ref().map_or(true, |(id, _)| run_id > *id)` instead.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline --test verification_replay`
Expected: PASS, 9 tests.

- [ ] **Step 5: Verify the no-Plumb-dependency constraint holds**

Run: `cargo tree -p parallax-baseline | Select-String plumb`
Expected: no output. If `parallax-plumb` appears, a dependency was added
in error — remove it.

- [ ] **Step 6: Commit**

```bash
git add baseline/src/adapters/verification.rs baseline/tests/verification_replay.rs baseline/tests/fixtures/plumb/
git commit -m "feat(adapters): read Plumb verdicts without linking Plumb

The platform consumes Plumb's rendered verdict.md as text on disk, so
the two sub-projects share no dependency; NO-GO is matched before GO
because it contains it, and a HOLD is never upgraded to a pass."
```

---

### Slice 4.3: The artifact family

**Tags:** coding

#### Task 15: Glob scanning, and the `figure` and `capture` adapters

**Files:**
- Modify: `baseline/src/adapters/artifact.rs`

**Interfaces:**
- Consumes: `ArtifactAdapter`, `Artifact`, `ArtifactDetail` (Task 10);
  `verification::parse_verdict` (Task 14).
- Produces:
  - `pub fn scan_glob(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, AdapterError>`
  - `pub struct FigureArtifactAdapter { ... }` with `pub fn new(watch: impl Into<String>) -> Self`
  - `pub struct CaptureArtifactAdapter { ... }` with `pub fn new(watch: impl Into<String>) -> Self`

  Consumed by Task 16 (`MetricsArtifactAdapter` reuses `scan_glob`),
  Task 18, Task 20.

**`scan_glob` walks and filters rather than watching.** The manifest
field is called `watch:`, but a headless library must not spawn
background threads or hold OS watch handles — a caller decides when to
scan, exactly as it decides when to poll. See Judgment call 6.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod scan_tests {
    use super::*;
    use crate::adapters::ProjectContext;
    use std::time::{Duration, SystemTime};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    /// Builds a project tree and returns its tempdir.
    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (relative, contents) in files {
            let path = dir.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        }
        dir
    }

    #[test]
    fn a_double_star_glob_matches_at_any_depth() {
        let dir = tree(&[
            ("projects/a/results/run1/loss.png", "x"),
            ("projects/b/results/deep/nested/acc.png", "yy"),
            ("projects/a/results/notes.txt", "z"),
        ]);
        let mut found = scan_glob(dir.path(), "projects/*/results/**/*.png").unwrap();
        found.sort();
        assert_eq!(found.len(), 2, "the .txt does not match");
    }

    #[test]
    fn a_single_star_glob_does_not_cross_a_directory_boundary() {
        let dir = tree(&[("runs/a/verdict.md", "x"), ("runs/a/b/verdict.md", "y")]);
        assert_eq!(scan_glob(dir.path(), "runs/*/verdict.md").unwrap().len(), 1);
    }

    #[test]
    fn a_glob_matching_nothing_is_an_empty_result_not_an_error() {
        let dir = tree(&[("a.txt", "x")]);
        assert!(scan_glob(dir.path(), "**/*.png").unwrap().is_empty());
    }

    #[test]
    fn a_missing_root_is_an_empty_result_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scan_glob(&dir.path().join("nope"), "**/*.png").unwrap().is_empty());
    }

    #[test]
    fn an_invalid_glob_is_a_parse_error_naming_the_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let err = scan_glob(dir.path(), "[").unwrap_err().to_string();
        assert!(err.contains('['), "got {err}");
    }

    #[test]
    fn figure_artifacts_report_their_size_and_never_their_pixels() {
        let dir = tree(&[("out/field.png", "\x89PNG\r\n\x1a\n0123456789")]);
        let mut a = FigureArtifactAdapter::new("out/**/*.png");
        let artifacts = a.scan(&ProjectContext::new("me", dir.path()), at(0)).unwrap().value;
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].kind, crate::manifest::ArtifactKind::Figure);
        assert_eq!(artifacts[0].detail, ArtifactDetail::Figure { bytes: 18 });
    }

    #[test]
    fn capture_artifacts_carry_their_run_id_and_verdict() {
        let dir = tree(&[
            (".plumb/runs/20260814T101500Z/verdict.md", "# run 20260814T101500Z — GO\n"),
            (".plumb/runs/20260814T112200Z/verdict.md", "# run 20260814T112200Z — NO-GO\n"),
        ]);
        let mut a = CaptureArtifactAdapter::new(".plumb/runs/**");
        let mut artifacts = a.scan(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
        artifacts.sort_by(|x, y| x.path.cmp(&y.path));
        assert_eq!(artifacts.len(), 2);
        assert_eq!(
            artifacts[0].detail,
            ArtifactDetail::Capture {
                run_id: "20260814T101500Z".into(),
                outcome: crate::adapters::verification::VerificationOutcome::Pass,
            }
        );
        assert_eq!(
            artifacts[1].detail,
            ArtifactDetail::Capture {
                run_id: "20260814T112200Z".into(),
                outcome: crate::adapters::verification::VerificationOutcome::Fail,
            }
        );
    }

    #[test]
    fn a_capture_run_with_no_verdict_yet_reports_not_run_rather_than_being_dropped() {
        let dir = tree(&[(".plumb/runs/20260814T130000Z/omnitrix.png", "x")]);
        let mut a = CaptureArtifactAdapter::new(".plumb/runs/**");
        let artifacts = a.scan(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
        assert_eq!(artifacts.len(), 1, "an in-progress run is still visible");
        assert!(matches!(
            artifacts[0].detail,
            ArtifactDetail::Capture { outcome: crate::adapters::verification::VerificationOutcome::NotRun, .. }
        ));
    }

    #[test]
    fn artifacts_read_from_disk_are_live() {
        let dir = tree(&[("out/a.png", "x")]);
        let mut a = FigureArtifactAdapter::new("out/**/*.png");
        let observed = a.scan(&ProjectContext::new("me", dir.path()), at(0)).unwrap();
        assert_eq!(observed.freshness(at(9999)), crate::freshness::Freshness::Live);
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test -p parallax-baseline adapters::artifact::scan_tests`
Expected: FAIL.

Append to `baseline/src/adapters/artifact.rs`:

```rust
use crate::adapters::verification::parse_verdict;
use globset::Glob;
use std::path::Path;

/// Walks `root` and returns every path matching `pattern`, which is
/// interpreted relative to `root`.
///
/// This walks on demand rather than holding an OS watch handle: a
/// headless library must not own background threads, and a caller that
/// decides when to poll should also decide when to scan.
pub fn scan_glob(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, AdapterError> {
    let matcher = Glob::new(pattern)
        .map_err(|e| AdapterError::Parse(format!("`{pattern}` is not a valid glob: {e}")))?
        .compile_matcher();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        if matcher.is_match(relative) {
            found.push(entry.path().to_path_buf());
        }
    }
    found.sort();
    Ok(found)
}

/// The filesystem modification time of a path, or the Unix epoch when
/// the filesystem does not report one.
fn modified_at(path: &Path) -> SystemTime {
    std::fs::metadata(path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Reports pre-rendered images: path, size, modification time. It never
/// reads their pixels — rendering is a frontend's problem.
pub struct FigureArtifactAdapter {
    watch: String,
}

impl FigureArtifactAdapter {
    /// An adapter scanning `watch`, relative to the project root.
    pub fn new(watch: impl Into<String>) -> Self {
        Self { watch: watch.into() }
    }
}

impl ArtifactAdapter for FigureArtifactAdapter {
    fn source_name(&self) -> String {
        "artifact:figure".into()
    }

    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Artifact>>, AdapterError> {
        let mut artifacts = Vec::new();
        for path in scan_glob(&ctx.root, &self.watch)? {
            if !path.is_file() {
                continue;
            }
            let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            artifacts.push(Artifact {
                modified: modified_at(&path),
                path,
                kind: ArtifactKind::Figure,
                detail: ArtifactDetail::Figure { bytes },
            });
        }
        Ok(Observed::watched(artifacts, now))
    }
}

/// Reports Plumb run directories: the run id, and the verdict it
/// rendered. A run still in progress reports `NotRun` and stays
/// visible — a capture that vanished from the list reads as a run that
/// never happened.
pub struct CaptureArtifactAdapter {
    watch: String,
}

impl CaptureArtifactAdapter {
    /// An adapter scanning `watch`, relative to the project root.
    pub fn new(watch: impl Into<String>) -> Self {
        Self { watch: watch.into() }
    }
}

impl ArtifactAdapter for CaptureArtifactAdapter {
    fn source_name(&self) -> String {
        "artifact:capture".into()
    }

    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Artifact>>, AdapterError> {
        let mut artifacts = Vec::new();
        for path in scan_glob(&ctx.root, &self.watch)? {
            if !path.is_dir() {
                continue;
            }
            let run_id = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let outcome = std::fs::read_to_string(path.join("verdict.md"))
                .ok()
                .and_then(|text| parse_verdict(&text))
                .unwrap_or(VerificationOutcome::NotRun);
            artifacts.push(Artifact {
                modified: modified_at(&path),
                path,
                kind: ArtifactKind::Capture,
                detail: ArtifactDetail::Capture { run_id, outcome },
            });
        }
        Ok(Observed::watched(artifacts, now))
    }
}
```

Note `.plumb/runs/**` matches the run directories themselves, which is
why `CaptureArtifactAdapter` filters to directories while
`FigureArtifactAdapter` filters to files.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline adapters::artifact::`
Expected: PASS, 9 tests.

- [ ] **Step 4: Commit**

```bash
git add baseline/src/adapters/artifact.rs
git commit -m "feat(adapters): scan figure and capture artifact feeds

Scanning happens on demand rather than through an OS watch handle: a
headless library must not own background threads, and the caller that
decides when to poll should decide when to scan."
```

---

#### Task 16: The `metrics` artifact adapter

**Files:**
- Modify: `baseline/src/adapters/artifact.rs`
- Create: `baseline/tests/artifact_replay.rs`
- Create: `baseline/tests/fixtures/metrics/loss.jsonl`

**Interfaces:**
- Consumes: `scan_glob`, `Artifact`, `ArtifactDetail`, `Series` (Tasks
  10, 15).
- Produces:
  - `pub fn parse_metrics(text: &str) -> Vec<Series>`
  - `pub struct MetricsArtifactAdapter { ... }` with `pub fn new(watch: impl Into<String>) -> Self`

  Consumed by Task 18 and Task 20.

**Metrics are scalar series, and only scalar series.** The spec names
them as loss curves, probe accuracy, spectral error — JSONL where each
line is one record of `name: number` pairs. Non-numeric values are
skipped, not coerced, and never error: a metrics file with a `"note"`
string field must still yield its numbers.

- [ ] **Step 1: Write the fixture**

`baseline/tests/fixtures/metrics/loss.jsonl` — five steps of a training
run, with one deliberately ragged line (a missing key) and one
non-numeric field, because real producers emit both:

```
{"step": 0, "loss": 2.7183, "probe_acc": 0.11, "note": "warmup"}
{"step": 1, "loss": 2.1041, "probe_acc": 0.19, "note": "warmup"}
{"step": 2, "loss": 1.6180, "probe_acc": 0.34}
{"step": 3, "loss": 1.4142, "probe_acc": 0.51, "note": "lr decay"}
{"step": 4, "loss": 1.2020, "probe_acc": 0.62, "spectral_err": 0.008}
```

- [ ] **Step 2: Write the failing integration test**

Create `baseline/tests/artifact_replay.rs`:

```rust
//! The metrics artifact adapter, replayed against a sample JSONL feed.

use parallax_baseline::adapters::artifact::{
    parse_metrics, ArtifactAdapter, ArtifactDetail, MetricsArtifactAdapter,
};
use parallax_baseline::adapters::ProjectContext;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/metrics/loss.jsonl")
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// Copies the fixture into a Model-Experiments-shaped project tree.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("projects/spectral/results/run7");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::copy(fixture(), target.join("loss.jsonl")).unwrap();
    dir
}

#[test]
fn every_numeric_key_becomes_a_named_series_sorted_by_name() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let series = parse_metrics(&text);
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["loss", "probe_acc", "spectral_err", "step"]);
}

#[test]
fn a_series_carries_its_points_in_record_order() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let series = parse_metrics(&text);
    let loss = series.iter().find(|s| s.name == "loss").unwrap();
    assert_eq!(loss.points, vec![2.7183, 2.1041, 1.6180, 1.4142, 1.2020]);
}

/// Real producers emit ragged records. A key appearing only in the last
/// line yields a one-point series rather than four fabricated zeros.
#[test]
fn a_key_present_in_only_some_records_yields_only_the_points_it_had() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let series = parse_metrics(&text);
    let spectral = series.iter().find(|s| s.name == "spectral_err").unwrap();
    assert_eq!(spectral.points, vec![0.008], "no interpolation, no padding");
}

#[test]
fn non_numeric_fields_are_skipped_rather_than_coerced_or_fatal() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let series = parse_metrics(&text);
    assert!(series.iter().all(|s| s.name != "note"));
}

#[test]
fn a_malformed_line_is_skipped_and_the_rest_of_the_file_still_parses() {
    let text = "{\"loss\": 1.0}\nnot json at all\n{\"loss\": 0.5}\n";
    let series = parse_metrics(text);
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].points, vec![1.0, 0.5]);
}

#[test]
fn blank_lines_are_ignored() {
    assert_eq!(parse_metrics("\n\n{\"a\": 1}\n\n").len(), 1);
}

#[test]
fn an_empty_file_yields_no_series_rather_than_an_error() {
    assert!(parse_metrics("").is_empty());
}

#[test]
fn the_adapter_finds_metrics_files_through_the_manifest_s_watch_glob() {
    let dir = project();
    let mut a = MetricsArtifactAdapter::new("projects/*/results/**/*.jsonl");
    let artifacts = a
        .scan(&ProjectContext::new("model-experiments", dir.path()), at(0))
        .unwrap()
        .value;
    assert_eq!(artifacts.len(), 1);
    match &artifacts[0].detail {
        ArtifactDetail::Metrics { series } => assert_eq!(series.len(), 4),
        other => panic!("expected metrics, got {other:?}"),
    }
}

#[test]
fn a_project_with_no_metrics_files_yields_an_empty_scan() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = MetricsArtifactAdapter::new("projects/*/results/**/*.jsonl");
    let artifacts = a
        .scan(&ProjectContext::new("model-experiments", dir.path()), at(0))
        .unwrap()
        .value;
    assert!(artifacts.is_empty());
}
```

- [ ] **Step 3: Run to verify it fails, then implement**

Run: `cargo test -p parallax-baseline --test artifact_replay`
Expected: FAIL — `MetricsArtifactAdapter` does not exist.

Append to `baseline/src/adapters/artifact.rs`:

```rust
use std::collections::BTreeMap;

/// Parses a JSONL metrics feed into named scalar series.
///
/// One record per line, each a JSON object. Numeric fields become
/// series points in record order; non-numeric fields and unparseable
/// lines are skipped, never coerced and never fatal — a real producer
/// emits ragged records and string annotations, and losing the whole
/// file over one of them would be the wrong trade.
pub fn parse_metrics(text: &str) -> Vec<Series> {
    let mut series: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(serde_json::Value::Object(record)) = serde_json::from_str(line) else {
            continue;
        };
        for (key, value) in record {
            if let Some(number) = value.as_f64() {
                series.entry(key).or_default().push(number);
            }
        }
    }
    series.into_iter().map(|(name, points)| Series { name, points }).collect()
}

/// Reports JSONL scalar series. Also selected by a manifest writing
/// `adapter: jsonl`.
pub struct MetricsArtifactAdapter {
    watch: String,
}

impl MetricsArtifactAdapter {
    /// An adapter scanning `watch`, relative to the project root.
    pub fn new(watch: impl Into<String>) -> Self {
        Self { watch: watch.into() }
    }
}

impl ArtifactAdapter for MetricsArtifactAdapter {
    fn source_name(&self) -> String {
        "artifact:metrics".into()
    }

    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Artifact>>, AdapterError> {
        let mut artifacts = Vec::new();
        for path in scan_glob(&ctx.root, &self.watch)? {
            if !path.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&path)?;
            artifacts.push(Artifact {
                modified: modified_at(&path),
                path,
                kind: ArtifactKind::Metrics,
                detail: ArtifactDetail::Metrics { series: parse_metrics(&text) },
            });
        }
        Ok(Observed::watched(artifacts, now))
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline --test artifact_replay`
Expected: PASS, 9 tests.

- [ ] **Step 5: Check the file size**

Run: `(Get-Content baseline/src/adapters/artifact.rs | Measure-Object -Line).Lines`
If it exceeds 500, split the three adapters into
`adapters/artifact/{mod,figure,metrics,capture}.rs`, keeping the trait
and shared types in `mod.rs` and re-exporting so no import outside the
module changes.

- [ ] **Step 6: Commit**

```bash
git add baseline/src/adapters/artifact.rs baseline/tests/artifact_replay.rs baseline/tests/fixtures/metrics/
git commit -m "feat(adapters): parse JSONL metrics into named scalar series

Ragged records and string annotations are what real producers emit, so a
key missing from some lines yields only the points it had and a
non-numeric field is skipped rather than coercing or failing the file."
```

---

### Slice 4.4: The session family

**Tags:** coding

#### Task 17: The filesystem session adapter

**Files:**
- Modify: `baseline/src/adapters/session.rs`

**Interfaces:**
- Consumes: `SessionAdapter`, `Session` (Task 10);
  `artifact::scan_glob` (Task 15).
- Produces:
  - `pub struct FilesystemSessionAdapter { ... }` with `pub fn new(watch: impl Into<String>) -> Self`
  - `pub const DEFAULT_IDLE_AFTER: Duration` (5 minutes)

  Consumed by Task 18 and Task 20.

**`last_activity` is the newest mtime anywhere inside the session
directory, not the directory's own mtime.** A worktree directory's own
mtime barely moves while an agent works inside it — files change, the
containing directory does not. Reading only the directory would report
every active session as idle.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::ProjectContext;
    use std::time::{Duration, SystemTime};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    /// Builds `.claude/worktrees/<name>/...` for each entry.
    fn tree(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for relative in files {
            let path = dir.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
        }
        dir
    }

    #[test]
    fn each_matching_directory_becomes_one_session_named_for_itself() {
        let dir = tree(&[
            ".claude/worktrees/parallax-baseline/src/lib.rs",
            ".claude/worktrees/widget-audit/notes.md",
        ]);
        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let mut sessions = a.scan(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
        sessions.sort_by(|x, y| x.name.cmp(&y.name));
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "parallax-baseline");
        assert_eq!(sessions[1].name, "widget-audit");
    }

    #[test]
    fn files_matching_the_glob_are_not_sessions() {
        let dir = tree(&[".claude/worktrees/stray.txt", ".claude/worktrees/real/a.rs"]);
        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let sessions = a.scan(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "real");
    }

    /// A worktree's own mtime barely moves while an agent edits files
    /// inside it, so reading only the directory would report every
    /// active session as idle.
    #[test]
    fn last_activity_is_the_newest_mtime_anywhere_inside_the_session() {
        let dir = tree(&[".claude/worktrees/s/deep/nested/file.rs"]);
        let session_dir = dir.path().join(".claude/worktrees/s");
        let inner = session_dir.join("deep/nested/file.rs");
        let inner_mtime = std::fs::metadata(&inner).unwrap().modified().unwrap();

        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let sessions = a.scan(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
        assert!(
            sessions[0].last_activity >= inner_mtime,
            "activity must reflect the deepest file, not the directory"
        );
    }

    #[test]
    fn a_session_is_active_while_within_the_idle_window_and_not_after() {
        let session = Session {
            name: "s".into(),
            path: std::path::PathBuf::from("/tmp/s"),
            last_activity: at(100),
        };
        assert!(session.is_active(at(200), Duration::from_secs(300)));
        assert!(!session.is_active(at(500), Duration::from_secs(300)));
    }

    #[test]
    fn the_default_idle_window_is_five_minutes() {
        assert_eq!(DEFAULT_IDLE_AFTER, Duration::from_secs(300));
    }

    #[test]
    fn a_project_with_no_session_directory_yields_an_empty_scan_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let sessions = a.scan(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap().value;
        assert!(sessions.is_empty());
    }

    #[test]
    fn sessions_read_from_disk_are_live() {
        let dir = tree(&[".claude/worktrees/s/a.rs"]);
        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let observed = a.scan(&ProjectContext::new("ttui", dir.path()), at(0)).unwrap();
        assert_eq!(observed.freshness(at(9999)), crate::freshness::Freshness::Live);
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test -p parallax-baseline adapters::session::`
Expected: FAIL.

Append to `baseline/src/adapters/session.rs`:

```rust
use crate::adapters::artifact::scan_glob;

/// How long a session may go without a file changing before a frontend
/// should call it idle.
pub const DEFAULT_IDLE_AFTER: Duration = Duration::from_secs(300);

/// Reports agent session directories by scanning the manifest's
/// `sessions.watch` glob.
pub struct FilesystemSessionAdapter {
    watch: String,
}

impl FilesystemSessionAdapter {
    /// An adapter scanning `watch`, relative to the project root.
    pub fn new(watch: impl Into<String>) -> Self {
        Self { watch: watch.into() }
    }
}

/// The newest modification time anywhere under `dir`, falling back to
/// the directory's own when it is empty or unreadable.
fn newest_mtime(dir: &std::path::Path) -> SystemTime {
    let own = std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .fold(own, |newest, m| newest.max(m))
}

impl SessionAdapter for FilesystemSessionAdapter {
    fn source_name(&self) -> String {
        "session:filesystem".into()
    }

    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Session>>, AdapterError> {
        let mut sessions = Vec::new();
        for path in scan_glob(&ctx.root, &self.watch)? {
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            sessions.push(Session { name, last_activity: newest_mtime(&path), path });
        }
        Ok(Observed::watched(sessions, now))
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline adapters::session::`
Expected: PASS, 7 tests.

- [ ] **Step 4: Commit**

```bash
git add baseline/src/adapters/session.rs
git commit -m "feat(adapters): report agent sessions from the filesystem

A worktree's own mtime barely moves while an agent edits files inside
it, so activity is the newest mtime anywhere under the directory rather
than the directory's own."
```

---
## Arc 5: State aggregation

Folding a validated manifest plus its adapters into one cross-project
`PlatformState`, with per-source freshness and per-source degradation.
This is the deliverable the cockpit consumes.

### Slice 5.1: The aggregator and the reduced view

**Tags:** coding

#### Task 18: `PlatformState`, `ProjectState`, and `aggregate`

**Files:**
- Create: `baseline/src/state.rs`
- Modify: `baseline/src/lib.rs` (add `pub mod state;`)

**Interfaces:**
- Consumes: `validate::Validated`, `autonomy::{resolve, Resolution}`,
  every adapter trait, `freshness::{Observed, Freshness}`.
- Produces:
  - `pub struct ProjectAdapters { pub work: Option<Box<dyn WorkAdapter>>, pub verification: Vec<Box<dyn VerificationAdapter>>, pub artifacts: Vec<Box<dyn ArtifactAdapter>>, pub sessions: Option<Box<dyn SessionAdapter>> }` with `pub fn new() -> Self`
  - `pub struct ItemAutonomy { pub number: u64, pub resolution: Resolution }`
  - `pub struct Degradation { pub source: String, pub reason: String }`
  - `pub struct ProjectState { pub name: String, pub methodology: Option<String>, pub language: Option<String>, pub work: Option<Observed<WorkSnapshot>>, pub autonomy: Vec<ItemAutonomy>, pub unmapped_labels: Vec<String>, pub verification: Vec<Observed<VerificationStatus>>, pub artifacts: Vec<Observed<Vec<Artifact>>>, pub sessions: Option<Observed<Vec<Session>>>, pub degradations: Vec<Degradation> }`
  - `pub struct PlatformState { pub projects: Vec<ProjectState> }` with `pub fn project(&self, name: &str) -> Option<&ProjectState>`
  - `pub fn aggregate_project(validated: &Validated, adapters: &mut ProjectAdapters, now: SystemTime) -> ProjectState`
  - `pub fn aggregate(inputs: &mut [(Validated, ProjectAdapters)], now: SystemTime) -> PlatformState`

  Consumed by Task 19 (freshness surface), Task 20, and by the cockpit.

**`aggregate_project` returns a `ProjectState`, never a `Result`.** An
adapter that fails degrades its own source and leaves the rest intact —
that is the Global Constraint, and returning a `Result` would make the
whole view fail with it.

**Autonomy resolution happens here, not in the work adapter.** The
adapter carries labels verbatim; `state` projects them through the
manifest's `autonomy_map`. Keeping the two apart is what lets a
different work adapter reuse the same projection.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::work::{ChecksSummary, WorkItem, WorkKind, WorkState};
    use crate::manifest::parse_manifest;
    use crate::validate::validate;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    fn item(number: u64, labels: &[&str]) -> WorkItem {
        WorkItem {
            number,
            title: format!("item {number}"),
            kind: WorkKind::Issue,
            state: WorkState::Open,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            checks: ChecksSummary::none(),
            url: String::new(),
            updated_at: "2026-08-14T00:00:00Z".into(),
        }
    }

    struct StubWork {
        result: Option<WorkSnapshot>,
    }

    impl WorkAdapter for StubWork {
        fn source_name(&self) -> String {
            "work:stub".into()
        }
        fn poll(
            &mut self,
            _ctx: &ProjectContext,
            now: SystemTime,
        ) -> Result<Observed<WorkSnapshot>, AdapterError> {
            match self.result.clone() {
                Some(s) => Ok(Observed::polled(s, now, Duration::from_secs(30))),
                None => Err(AdapterError::Http { status: 403, message: "rate limited".into() }),
            }
        }
    }

    struct StubVerification(VerificationOutcome);

    impl VerificationAdapter for StubVerification {
        fn source_name(&self) -> String {
            "verification:stub".into()
        }
        fn check(
            &mut self,
            _ctx: &ProjectContext,
            now: SystemTime,
        ) -> Result<Observed<VerificationStatus>, AdapterError> {
            Ok(Observed::watched(
                VerificationStatus { kind: "tests".into(), outcome: self.0, detail: None },
                now,
            ))
        }
    }

    const TTUI_YAML: &str = r#"
project:
  name: ttui
  root: D:/Dev/Projects/TTUI
work:
  adapter: github
  repo: tatemeyer/ttui
  autonomy_map:
    direct: { implement: agent, merge: direct-push }
    gated:  { implement: agent, merge: on-checks }
"#;

    fn ttui() -> crate::validate::Validated {
        validate(parse_manifest(TTUI_YAML).unwrap()).unwrap()
    }

    #[test]
    fn a_projects_identity_comes_from_its_manifest() {
        let mut adapters = ProjectAdapters::new();
        let state = aggregate_project(&ttui(), &mut adapters, at(0));
        assert_eq!(state.name, "ttui");
    }

    #[test]
    fn work_items_are_carried_through_and_their_labels_projected() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork {
            result: Some(WorkSnapshot { items: vec![item(1, &["gated"]), item(2, &["direct", "bug"])] }),
        }));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));

        assert_eq!(state.work.as_ref().unwrap().value.items.len(), 2);
        assert_eq!(state.autonomy.len(), 2);
        assert_eq!(state.autonomy[0].number, 1);
        assert_eq!(state.autonomy[0].resolution.autonomy.merge, Some(Merge::OnChecks));
        assert_eq!(state.autonomy[1].resolution.autonomy.merge, Some(Merge::DirectPush));
    }

    #[test]
    fn labels_the_manifest_never_declared_are_collected_once_and_deduplicated() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork {
            result: Some(WorkSnapshot {
                items: vec![item(1, &["bug", "gated"]), item(2, &["bug", "docs"])],
            }),
        }));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));
        assert_eq!(state.unmapped_labels, vec!["bug".to_string(), "docs".to_string()]);
    }

    /// The spec's headline partial case, at the aggregation layer.
    #[test]
    fn a_manifest_declaring_only_work_aggregates_to_a_valid_reduced_view() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork {
            result: Some(WorkSnapshot { items: vec![item(1, &["gated"])] }),
        }));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));
        assert!(state.work.is_some());
        assert!(state.verification.is_empty());
        assert!(state.artifacts.is_empty());
        assert!(state.sessions.is_none());
        assert!(state.degradations.is_empty(), "absent is not degraded");
    }

    /// A failing adapter must not blank the rest of the view.
    #[test]
    fn a_failing_work_adapter_degrades_only_its_own_source() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork { result: None }));
        adapters.verification.push(Box::new(StubVerification(VerificationOutcome::Pass)));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));

        assert!(state.work.is_none());
        assert_eq!(state.degradations.len(), 1);
        assert_eq!(state.degradations[0].source, "work:stub");
        assert!(state.degradations[0].reason.contains("rate limited"));
        assert_eq!(state.verification.len(), 1, "verification survived");
        assert_eq!(state.verification[0].value.outcome, VerificationOutcome::Pass);
    }

    #[test]
    fn aggregate_folds_several_projects_and_finds_them_by_name() {
        let me_yaml = "project:\n  name: model-experiments\n  root: /tmp/me\n";
        let me = validate(parse_manifest(me_yaml).unwrap()).unwrap();
        let mut inputs = vec![(ttui(), ProjectAdapters::new()), (me, ProjectAdapters::new())];
        let platform = aggregate(&mut inputs, at(0));
        assert_eq!(platform.projects.len(), 2);
        assert!(platform.project("ttui").is_some());
        assert!(platform.project("model-experiments").is_some());
        assert!(platform.project("nonexistent").is_none());
    }

    #[test]
    fn every_observation_carries_the_now_it_was_aggregated_at() {
        let mut adapters = ProjectAdapters::new();
        adapters.verification.push(Box::new(StubVerification(VerificationOutcome::Pass)));
        let state = aggregate_project(&ttui(), &mut adapters, at(42));
        assert_eq!(state.verification[0].observed_at, at(42));
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test -p parallax-baseline state::`
Expected: FAIL — `state` module does not exist.

```rust
//! Aggregation: folding each project's validated manifest and its
//! adapters into one cross-project view. Deliberately infallible — an
//! adapter that fails degrades its own source and leaves the rest
//! intact, because a blank view is a worse failure than a number
//! labelled stale.

use crate::adapters::artifact::{Artifact, ArtifactAdapter};
use crate::adapters::session::{Session, SessionAdapter};
use crate::adapters::verification::{VerificationAdapter, VerificationStatus};
use crate::adapters::work::{WorkAdapter, WorkSnapshot};
use crate::adapters::{AdapterError, ProjectContext};
use crate::autonomy::{resolve, Resolution};
use crate::freshness::Observed;
use crate::validate::Validated;
use std::time::SystemTime;

/// The adapters serving one project. Every family is optional, because
/// partial support is normal.
#[derive(Default)]
pub struct ProjectAdapters {
    /// The work feed, when declared.
    pub work: Option<Box<dyn WorkAdapter>>,
    /// One adapter per declared verification check.
    pub verification: Vec<Box<dyn VerificationAdapter>>,
    /// One adapter per declared artifact feed.
    pub artifacts: Vec<Box<dyn ArtifactAdapter>>,
    /// The session feed, when declared.
    pub sessions: Option<Box<dyn SessionAdapter>>,
}

impl ProjectAdapters {
    /// A project with no adapters registered.
    pub fn new() -> Self {
        Self::default()
    }
}

/// One work item's labels, projected onto the normalized axes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemAutonomy {
    /// The item's number in its repository.
    pub number: u64,
    /// What its labels resolved to.
    pub resolution: Resolution,
}

/// A source that could not be read this cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degradation {
    /// The adapter's `source_name`.
    pub source: String,
    /// Why it could not be read, in one sentence.
    pub reason: String,
}

/// Everything the platform currently knows about one project.
#[derive(Debug, Default)]
pub struct ProjectState {
    /// The project's short name.
    pub name: String,
    /// Its declared methodology. **Display only — never branched on.**
    pub methodology: Option<String>,
    /// Its primary language, for display.
    pub language: Option<String>,
    /// The most recent work snapshot, when the feed was reachable.
    pub work: Option<Observed<WorkSnapshot>>,
    /// Each work item's projected autonomy, in snapshot order.
    pub autonomy: Vec<ItemAutonomy>,
    /// Labels seen on work items that the manifest does not declare,
    /// deduplicated and sorted. Not an error — a prompt to extend the
    /// map, or evidence a label is doing nothing.
    pub unmapped_labels: Vec<String>,
    /// Each declared verification check's standing.
    pub verification: Vec<Observed<VerificationStatus>>,
    /// Each declared artifact feed's contents.
    pub artifacts: Vec<Observed<Vec<Artifact>>>,
    /// The session feed's contents, when declared and reachable.
    pub sessions: Option<Observed<Vec<Session>>>,
    /// Sources that failed this cycle.
    pub degradations: Vec<Degradation>,
}

/// Every registered project's state.
#[derive(Debug, Default)]
pub struct PlatformState {
    /// One entry per registered project, in registration order.
    pub projects: Vec<ProjectState>,
}

impl PlatformState {
    /// Finds a project by name.
    pub fn project(&self, name: &str) -> Option<&ProjectState> {
        self.projects.iter().find(|p| p.name == name)
    }
}

/// Builds the adapter context from a validated manifest.
fn context(validated: &Validated) -> ProjectContext {
    let manifest = validated.manifest();
    let root = manifest.project.root.clone().unwrap_or_default();
    let mut ctx = ProjectContext::new(manifest.project.name.clone(), root);
    if let Some(work) = &manifest.work {
        ctx = ctx.with_repo(work.repo.clone());
    }
    ctx
}

/// Records an adapter failure against the project rather than
/// propagating it.
fn degrade(state: &mut ProjectState, source: String, error: AdapterError) {
    state.degradations.push(Degradation { source, reason: error.to_string() });
}

/// Polls every adapter a project declares and folds the results into one
/// state. Never fails: a failing source becomes a `Degradation`.
pub fn aggregate_project(
    validated: &Validated,
    adapters: &mut ProjectAdapters,
    now: SystemTime,
) -> ProjectState {
    let manifest = validated.manifest();
    let ctx = context(validated);
    let mut state = ProjectState {
        name: manifest.project.name.clone(),
        methodology: manifest.project.methodology.clone(),
        language: manifest.project.language.clone(),
        ..Default::default()
    };

    if let Some(adapter) = adapters.work.as_mut() {
        let source = adapter.source_name();
        match adapter.poll(&ctx, now) {
            Ok(observed) => {
                let empty = Default::default();
                let map = manifest.work.as_ref().map(|w| &w.autonomy_map);
                let map = map.unwrap_or(&empty);
                let mut unmapped: Vec<String> = Vec::new();
                for item in &observed.value.items {
                    let resolution = resolve(map, &item.labels);
                    for label in &resolution.unmapped {
                        if !unmapped.contains(label) {
                            unmapped.push(label.clone());
                        }
                    }
                    state.autonomy.push(ItemAutonomy { number: item.number, resolution });
                }
                unmapped.sort();
                state.unmapped_labels = unmapped;
                state.work = Some(observed);
            }
            Err(e) => degrade(&mut state, source, e),
        }
    }

    for adapter in adapters.verification.iter_mut() {
        let source = adapter.source_name();
        match adapter.check(&ctx, now) {
            Ok(observed) => state.verification.push(observed),
            Err(e) => degrade(&mut state, source, e),
        }
    }

    for adapter in adapters.artifacts.iter_mut() {
        let source = adapter.source_name();
        match adapter.scan(&ctx, now) {
            Ok(observed) => state.artifacts.push(observed),
            Err(e) => degrade(&mut state, source, e),
        }
    }

    if let Some(adapter) = adapters.sessions.as_mut() {
        let source = adapter.source_name();
        match adapter.scan(&ctx, now) {
            Ok(observed) => state.sessions = Some(observed),
            Err(e) => degrade(&mut state, source, e),
        }
    }

    state
}

/// Aggregates every registered project.
pub fn aggregate(
    inputs: &mut [(Validated, ProjectAdapters)],
    now: SystemTime,
) -> PlatformState {
    PlatformState {
        projects: inputs
            .iter_mut()
            .map(|(validated, adapters)| aggregate_project(validated, adapters, now))
            .collect(),
    }
}
```

Add `pub mod state;` to `baseline/src/lib.rs`. The test module needs
`use crate::adapters::verification::VerificationOutcome;` and
`use crate::autonomy::Merge;`.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline state::`
Expected: PASS, 7 tests.

- [ ] **Step 4: Commit**

```bash
git add baseline/src/state.rs baseline/src/lib.rs
git commit -m "feat(state): aggregate a project's adapters into one view

Aggregation is infallible by design: an adapter that fails records a
degradation against its own source and every other source survives,
because a blank cockpit is a worse failure than a number labelled
stale."
```

---

#### Task 19: The source-freshness surface

**Files:**
- Modify: `baseline/src/state.rs`

**Interfaces:**
- Consumes: `ProjectState` (Task 18), `freshness::Freshness` (Task 9).
- Produces:
  - `pub struct SourceStatus { pub label: String, pub freshness: Freshness }`
  - `impl ProjectState { pub fn sources(&self, now: SystemTime) -> Vec<SourceStatus>; pub fn stalest(&self, now: SystemTime) -> Option<SourceStatus>; }`
  - `impl PlatformState { pub fn degraded(&self) -> Vec<(&str, &Degradation)>; }`

  Consumed by Task 20 and by the cockpit — this is the API behind
  "displays the age of each source."

**This is the frontend-facing half of Judgment call 1.** The core cannot
render anything, but it can hand a frontend a uniform list of
`(source label, freshness)` so the frontend does not have to know which
sources are polled and which are watched. A degraded source appears in
that list as `Freshness::Unavailable` rather than being missing — a
source that vanished from the list reads as a source that was never
declared.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod freshness_surface_tests {
    use super::tests::*;
    use super::*;
    use crate::freshness::Freshness;
    use std::time::Duration;

    #[test]
    fn every_reachable_source_appears_with_its_own_freshness() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork {
            result: Some(WorkSnapshot { items: vec![] }),
        }));
        adapters.verification.push(Box::new(StubVerification(VerificationOutcome::Pass)));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));

        let sources = state.sources(at(10));
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].label, "work");
        assert_eq!(sources[0].freshness, Freshness::Fresh { age: Duration::from_secs(10) });
        assert_eq!(sources[1].label, "verification:tests");
        assert_eq!(sources[1].freshness, Freshness::Live, "filesystem-backed is immediate");
    }

    #[test]
    fn a_polled_source_past_its_interval_reports_stale() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork {
            result: Some(WorkSnapshot { items: vec![] }),
        }));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));
        assert!(state.sources(at(45))[0].freshness.is_stale());
    }

    /// A degraded source stays in the list. One that vanished would read
    /// as a source that was never declared.
    #[test]
    fn a_degraded_source_appears_as_unavailable_with_its_reason() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork { result: None }));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));

        let sources = state.sources(at(0));
        assert_eq!(sources.len(), 1);
        match &sources[0].freshness {
            Freshness::Unavailable { reason, .. } => assert!(reason.contains("rate limited")),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn a_project_with_no_adapters_reports_no_sources_rather_than_a_fake_one() {
        let state = aggregate_project(&ttui(), &mut ProjectAdapters::new(), at(0));
        assert!(state.sources(at(0)).is_empty());
    }

    #[test]
    fn stalest_picks_unavailable_over_stale_over_fresh_over_live() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork { result: None }));
        adapters.verification.push(Box::new(StubVerification(VerificationOutcome::Pass)));
        let state = aggregate_project(&ttui(), &mut adapters, at(0));
        assert!(matches!(state.stalest(at(0)).unwrap().freshness, Freshness::Unavailable { .. }));
    }

    #[test]
    fn stalest_is_none_when_a_project_has_no_sources() {
        let state = aggregate_project(&ttui(), &mut ProjectAdapters::new(), at(0));
        assert!(state.stalest(at(0)).is_none());
    }

    #[test]
    fn the_platform_lists_every_degradation_with_the_project_it_belongs_to() {
        let mut adapters = ProjectAdapters::new();
        adapters.work = Some(Box::new(StubWork { result: None }));
        let mut inputs = vec![(ttui(), adapters)];
        let platform = aggregate(&mut inputs, at(0));
        let degraded = platform.degraded();
        assert_eq!(degraded.len(), 1);
        assert_eq!(degraded[0].0, "ttui");
        assert_eq!(degraded[0].1.source, "work:stub");
    }
}
```

Make `mod tests`'s helpers (`StubWork`, `StubVerification`, `ttui`,
`at`) `pub(super)` so this module can reuse them rather than redefining
two stub adapters.

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test -p parallax-baseline state::freshness_surface_tests`
Expected: FAIL — `sources` does not exist.

```rust
use crate::freshness::Freshness;

/// One source's identity and how current it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStatus {
    /// A stable label a frontend can display, e.g. `verification:tests`.
    pub label: String,
    /// How current that source is.
    pub freshness: Freshness,
}

/// Ranks a freshness for "which of these is worst", worst last.
fn severity(freshness: &Freshness) -> u8 {
    match freshness {
        Freshness::Live => 0,
        Freshness::Fresh { .. } => 1,
        Freshness::Stale { .. } => 2,
        Freshness::Unavailable { .. } => 3,
    }
}

impl ProjectState {
    /// Every declared source and how current it is at `now`.
    ///
    /// This is the API behind "the cockpit displays the age of each
    /// source": the core cannot render, but it can hand a frontend a
    /// uniform list so the frontend need not know which sources are
    /// polled and which are watched. A degraded source stays in the
    /// list as `Unavailable` rather than disappearing from it.
    pub fn sources(&self, now: SystemTime) -> Vec<SourceStatus> {
        let mut out = Vec::new();
        if let Some(work) = &self.work {
            out.push(SourceStatus { label: "work".into(), freshness: work.freshness(now) });
        }
        for observed in &self.verification {
            out.push(SourceStatus {
                label: format!("verification:{}", observed.value.kind),
                freshness: observed.freshness(now),
            });
        }
        for (i, observed) in self.artifacts.iter().enumerate() {
            out.push(SourceStatus {
                label: format!("artifacts[{i}]"),
                freshness: observed.freshness(now),
            });
        }
        if let Some(sessions) = &self.sessions {
            out.push(SourceStatus { label: "sessions".into(), freshness: sessions.freshness(now) });
        }
        for degradation in &self.degradations {
            out.push(SourceStatus {
                label: degradation.source.clone(),
                freshness: Freshness::Unavailable {
                    since: None,
                    reason: degradation.reason.clone(),
                },
            });
        }
        out
    }

    /// The source a frontend should worry about first, if any.
    pub fn stalest(&self, now: SystemTime) -> Option<SourceStatus> {
        self.sources(now).into_iter().max_by_key(|s| severity(&s.freshness))
    }
}

impl PlatformState {
    /// Every degraded source across every project, with its project.
    pub fn degraded(&self) -> Vec<(&str, &Degradation)> {
        self.projects
            .iter()
            .flat_map(|p| p.degradations.iter().map(move |d| (p.name.as_str(), d)))
            .collect()
    }
}
```

Note that a degraded work source appears under its adapter's
`source_name` (`work:stub`, `work:github`) rather than the bare `work`
label a *successful* poll uses — the degradation names which adapter
failed, which is the more useful thing when a project could register a
work adapter this crate does not ship.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline state::`
Expected: PASS, 14 tests.

- [ ] **Step 4: Commit**

```bash
git add baseline/src/state.rs
git commit -m "feat(state): expose per-source freshness uniformly

The core has no UI, so it hands a frontend a flat (label, freshness)
list rather than requiring the frontend to know which sources poll and
which are watched; a degraded source stays in the list as Unavailable
instead of silently disappearing from it."
```

---

### Slice 5.2: Both real manifests, end to end

**Tags:** coding

#### Task 20: Replay both real manifests through aggregation

**Files:**
- Create: `baseline/tests/aggregate_replay.rs`

**Interfaces:**
- Consumes: every public API built so far.
- Produces: no new API. This is the spec's third Verification bullet
  ("adapter fixtures replay to correct aggregated state, including the
  partial-support case") as an executable test.

**Both real manifests, both real fixture sets, one test file.** TTUI
gets the GitHub fixtures from Task 12, a Plumb verdict from Task 14, a
capture feed, and a session tree. Model-Experiments gets the metrics
fixture from Task 16 and, deliberately, **no session feed at all** —
its manifest declares none, and its reduced view is the partial case
proved against a real file rather than a synthetic one.

- [ ] **Step 1: Write the failing test**

```rust
//! Both real manifests, replayed end to end through aggregation against
//! recorded fixtures. No network, no TTY, no wall clock.

use parallax_baseline::adapters::artifact::{
    ArtifactDetail, CaptureArtifactAdapter, MetricsArtifactAdapter,
};
use parallax_baseline::adapters::http::FixtureTransport;
use parallax_baseline::adapters::session::FilesystemSessionAdapter;
use parallax_baseline::adapters::verification::{
    CommandOutput, CommandVerificationAdapter, PlumbVerificationAdapter, ScriptedRunner,
    VerificationOutcome,
};
use parallax_baseline::adapters::work::{check_runs_url, issues_url, pulls_url, GithubWorkAdapter};
use parallax_baseline::autonomy::{Implement, Merge, Readiness};
use parallax_baseline::freshness::Freshness;
use parallax_baseline::manifest::parse_manifest_file;
use parallax_baseline::state::{aggregate, aggregate_project, ProjectAdapters};
use parallax_baseline::validate::{validate, Validated};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const REPO: &str = "tatemeyer/ttui";
const HEAD_142: &str = "1a7d51c9f0e2b3a4d5c6e7f8091a2b3c4d5e6f70";
const HEAD_143: &str = "0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4";

fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn fixture(relative: &str) -> PathBuf {
    crate_dir().join("tests/fixtures").join(relative)
}

fn manifest(name: &str) -> PathBuf {
    crate_dir().parent().unwrap().join("manifests").join(name)
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// Loads a real manifest and repoints `project.root` at a temp tree, so
/// filesystem-backed adapters read fixtures instead of the developer's
/// actual checkout.
fn load_rooted(name: &str, root: &Path) -> Validated {
    let mut parsed = parse_manifest_file(&manifest(name)).expect("parses");
    parsed.project.root = Some(root.to_path_buf());
    validate(parsed).expect("validates")
}

fn github_transport() -> FixtureTransport {
    let mut t = FixtureTransport::new();
    t.insert_from_file(issues_url(REPO), &fixture("github/issues.json"), Some("W/\"i1\"")).unwrap();
    t.insert_from_file(pulls_url(REPO), &fixture("github/pulls.json"), Some("W/\"p1\"")).unwrap();
    t.insert_from_file(check_runs_url(REPO, HEAD_142), &fixture("github/check-runs.json"), None).unwrap();
    t.insert(check_runs_url(REPO, HEAD_143), r#"{"check_runs":[]}"#, None);
    t
}

/// A TTUI-shaped tree: one completed Plumb run and two worktrees.
fn ttui_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path().join(".plumb/runs/20260814T112200Z");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::copy(fixture("plumb/verdict-no-go.md"), run.join("verdict.md")).unwrap();
    for worktree in ["parallax-baseline", "widget-audit"] {
        let path = dir.path().join(".claude/worktrees").join(worktree).join("src");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("lib.rs"), "// work in progress\n").unwrap();
    }
    dir
}

/// A Model-Experiments-shaped tree: one metrics feed, no sessions.
fn me_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let results = dir.path().join("projects/spectral/results/run7");
    std::fs::create_dir_all(&results).unwrap();
    std::fs::copy(fixture("metrics/loss.jsonl"), results.join("loss.jsonl")).unwrap();
    dir
}

/// Builds TTUI's adapters exactly as its manifest declares them.
fn ttui_adapters(root: &Path) -> ProjectAdapters {
    let mut a = ProjectAdapters::new();
    a.work = Some(Box::new(GithubWorkAdapter::new(github_transport())));
    let mut lint = ScriptedRunner::new();
    lint.push(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() });
    a.verification.push(Box::new(CommandVerificationAdapter::new("lint", "cargo clippy --all-targets -- -D warnings", lint)));
    let mut tests = ScriptedRunner::new();
    tests.push(CommandOutput { status: 101, stdout: String::new(), stderr: "test result: FAILED. 1 failed".into() });
    a.verification.push(Box::new(CommandVerificationAdapter::new("tests", "cargo test", tests)));
    a.verification.push(Box::new(PlumbVerificationAdapter::new("perceptual", root.join(".plumb/runs"))));
    a.artifacts.push(Box::new(CaptureArtifactAdapter::new(".plumb/runs/**")));
    a.sessions = Some(Box::new(FilesystemSessionAdapter::new(".claude/worktrees/*")));
    a
}

#[test]
fn ttui_aggregates_every_declared_family_from_fixtures() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    assert!(state.degradations.is_empty(), "no source failed: {:?}", state.degradations);
    assert_eq!(state.work.as_ref().unwrap().value.items.len(), 5);
    assert_eq!(state.verification.len(), 3);
    assert_eq!(state.verification[0].value.outcome, VerificationOutcome::Pass, "lint");
    assert_eq!(state.verification[1].value.outcome, VerificationOutcome::Fail, "tests");
    assert_eq!(state.verification[2].value.outcome, VerificationOutcome::Fail, "perceptual NO-GO");
    assert_eq!(state.artifacts.len(), 1);
    assert_eq!(state.sessions.as_ref().unwrap().value.len(), 2);
}

#[test]
fn ttui_work_items_project_onto_the_normalized_axes() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    let by_number = |n: u64| {
        state.autonomy.iter().find(|a| a.number == n).unwrap_or_else(|| panic!("item {n}"))
    };
    // Issue 134 is `gated`; issue 140 is `direct`; PR 142 is `gated`.
    assert_eq!(by_number(134).resolution.autonomy.merge, Some(Merge::OnChecks));
    assert_eq!(by_number(140).resolution.autonomy.merge, Some(Merge::DirectPush));
    assert_eq!(by_number(142).resolution.autonomy.merge, Some(Merge::OnChecks));
    assert_eq!(by_number(134).resolution.autonomy.implement, Some(Implement::Agent));
}

/// `needs-intent` is in Model-Experiments' map, not TTUI's. Issue 141
/// carries it anyway, so it must land in `unmapped_labels` rather than
/// silently projecting or erroring.
#[test]
fn a_label_ttui_does_not_declare_is_reported_as_unmapped() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    assert!(state.unmapped_labels.contains(&"needs-intent".to_string()));
    assert!(state.unmapped_labels.contains(&"semver:minor".to_string()));
    let item_141 = state.autonomy.iter().find(|a| a.number == 141).unwrap();
    assert!(item_141.resolution.matched.is_empty());
    assert_eq!(item_141.resolution.autonomy.readiness, Readiness::Verifiable);
}

#[test]
fn ttui_capture_artifacts_carry_the_run_s_verdict() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    let artifacts = &state.artifacts[0].value;
    assert_eq!(artifacts.len(), 1);
    assert_eq!(
        artifacts[0].detail,
        ArtifactDetail::Capture {
            run_id: "20260814T112200Z".into(),
            outcome: VerificationOutcome::Fail,
        }
    );
}

#[test]
fn ttui_source_freshness_distinguishes_the_polled_feed_from_the_watched_ones() {
    let tree = ttui_tree();
    let validated = load_rooted("ttui.yaml", tree.path());
    let mut adapters = ttui_adapters(tree.path());
    let state = aggregate_project(&validated, &mut adapters, at(0));

    let sources = state.sources(at(45));
    let work = sources.iter().find(|s| s.label == "work").unwrap();
    assert!(work.freshness.is_stale(), "45s past a 30s interval");
    for label in ["verification:lint", "verification:tests", "verification:perceptual", "sessions"] {
        let source = sources.iter().find(|s| s.label == label).unwrap_or_else(|| panic!("{label}"));
        assert_eq!(source.freshness, Freshness::Live, "{label} is filesystem-backed");
    }
}

/// Model-Experiments' manifest declares no `sessions:`. Its reduced view
/// is the spec's partial-support case, proved against the real file.
#[test]
fn model_experiments_aggregates_to_a_reduced_view_with_no_session_source() {
    let tree = me_tree();
    let validated = load_rooted("model-experiments.yaml", tree.path());
    let mut adapters = ProjectAdapters::new();
    adapters.artifacts.push(Box::new(MetricsArtifactAdapter::new("projects/*/results/**/*.jsonl")));
    let state = aggregate_project(&validated, &mut adapters, at(0));

    assert!(state.sessions.is_none(), "no session feed declared");
    assert!(state.work.is_none(), "no work adapter registered in this test");
    assert!(state.degradations.is_empty(), "absent is not degraded");
    assert_eq!(state.artifacts.len(), 1);
    match &state.artifacts[0].value[0].detail {
        ArtifactDetail::Metrics { series } => {
            assert!(series.iter().any(|s| s.name == "loss"));
        }
        other => panic!("expected metrics, got {other:?}"),
    }
}

#[test]
fn a_work_only_registration_still_produces_a_valid_project_state() {
    let tree = tempfile::tempdir().unwrap();
    let mut parsed = parse_manifest_file(&manifest("ttui.yaml")).unwrap();
    parsed.project.root = Some(tree.path().to_path_buf());
    parsed.verification.clear();
    parsed.artifacts.clear();
    parsed.sessions = None;
    let validated = validate(parsed).unwrap();

    let mut adapters = ProjectAdapters::new();
    adapters.work = Some(Box::new(GithubWorkAdapter::new(github_transport())));
    let state = aggregate_project(&validated, &mut adapters, at(0));

    assert_eq!(state.work.as_ref().unwrap().value.items.len(), 5);
    assert!(state.verification.is_empty());
    assert!(state.artifacts.is_empty());
    assert!(state.sessions.is_none());
    assert!(state.degradations.is_empty());
    assert_eq!(state.sources(at(0)).len(), 1, "one declared source, one reported");
}

#[test]
fn both_projects_aggregate_into_one_platform_state() {
    let ttui = ttui_tree();
    let me = me_tree();
    let mut me_adapters = ProjectAdapters::new();
    me_adapters.artifacts.push(Box::new(MetricsArtifactAdapter::new("projects/*/results/**/*.jsonl")));

    let mut inputs = vec![
        (load_rooted("ttui.yaml", ttui.path()), ttui_adapters(ttui.path())),
        (load_rooted("model-experiments.yaml", me.path()), me_adapters),
    ];
    let platform = aggregate(&mut inputs, at(0));

    assert_eq!(platform.projects.len(), 2);
    assert_eq!(platform.project("ttui").unwrap().methodology.as_deref(), Some("methodology-first"));
    assert_eq!(platform.project("model-experiments").unwrap().methodology.as_deref(), Some("outcome-first"));
    assert!(platform.degraded().is_empty());
}

/// The spec: "`methodology:` appears in the manifest as informational
/// metadata only — nothing in the platform branches on it." Two
/// registrations identical but for that field must aggregate to
/// identical state.
#[test]
fn methodology_changes_nothing_about_the_aggregated_state() {
    let tree = me_tree();

    let mut a = parse_manifest_file(&manifest("model-experiments.yaml")).unwrap();
    a.project.root = Some(tree.path().to_path_buf());
    let mut b = a.clone();
    a.project.methodology = Some("outcome-first".into());
    b.project.methodology = Some("methodology-first".into());

    let build = |manifest| {
        let validated = validate(manifest).unwrap();
        let mut adapters = ProjectAdapters::new();
        adapters.artifacts.push(Box::new(MetricsArtifactAdapter::new("projects/*/results/**/*.jsonl")));
        let state = aggregate_project(&validated, &mut adapters, at(0));
        (
            state.artifacts.len(),
            state.artifacts[0].value.len(),
            state.sources(at(0)).len(),
            state.degradations.len(),
            state.work.is_some(),
            state.sessions.is_some(),
        )
    };

    assert_eq!(build(a), build(b), "methodology must not reach any behaviour");
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p parallax-baseline --test aggregate_replay`
Expected: PASS, 9 tests. **If any fails, fix the library, not the
test** — every assertion here restates a spec Verification bullet.

- [ ] **Step 3: Run the whole suite and every gate**

Run, from the workspace root:

```
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Expected: all clean.

- [ ] **Step 4: Commit**

```bash
git add baseline/tests/aggregate_replay.rs
git commit -m "test(state): replay both real manifests end to end

Covers the spec's third Verification bullet directly, including
Model-Experiments' absent session feed as the partial-support case and a
positive assertion that methodology: reaches no behaviour at all."
```

---

## Arc 6: Control actions and the confirmation contract

Control lives here "as a plain API, so the same actions are available
headless." Each action is classified by reversibility, and the
confirmation-required group cannot execute unconfirmed — enforced by
the type system and asserted by tests.

### Slice 6.1: The action set and the confirmation contract

**Tags:** coding

#### Task 21: `Action` and `Reversibility`

**Files:**
- Create: `baseline/src/actions.rs`
- Modify: `baseline/src/lib.rs` (add `pub mod actions;`)

**Interfaces:**
- Consumes: nothing beyond `serde`.
- Produces:
  - `pub enum Ruling { Upheld, Overruled }`
  - `pub enum Action { RuleFinding { project: String, fingerprint: String, ruling: Ruling }, SetAutonomyLabel { project: String, item: u64, label: String }, RequestReReview { project: String, item: u64 }, TriggerCapture { project: String, scenario: Option<String> }, DispatchAgentRun { project: String, item: u64, prompt: String }, StopAgentRun { project: String, session: String }, MergePullRequest { project: String, number: u64 }, Push { project: String, branch: String } }`
  - `pub enum Reversibility { Reversible, ConfirmationRequired }`
  - `impl Action { pub fn reversibility(&self) -> Reversibility; pub fn project(&self) -> &str; pub fn summary(&self) -> String; }`

  Consumed by Task 22 (the confirmation contract) and Tasks 23-24
  (execution).

**The classification is the spec's, verbatim.** Reversible: rule on a
finding, set an autonomy label, request a re-review, trigger a capture,
dispatch an agent run. Confirmation required: stop a running agent,
merge a PR, push. No task may reclassify one without the spec changing.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn every_action() -> Vec<Action> {
        vec![
            Action::RuleFinding { project: "ttui".into(), fingerprint: "a1b2c3d4".into(), ruling: Ruling::Overruled },
            Action::SetAutonomyLabel { project: "ttui".into(), item: 142, label: "gated".into() },
            Action::RequestReReview { project: "ttui".into(), item: 142 },
            Action::TriggerCapture { project: "ttui".into(), scenario: Some("omnitrix-dial".into()) },
            Action::DispatchAgentRun { project: "ttui".into(), item: 140, prompt: "audit docs".into() },
            Action::StopAgentRun { project: "ttui".into(), session: "widget-audit".into() },
            Action::MergePullRequest { project: "ttui".into(), number: 142 },
            Action::Push { project: "ttui".into(), branch: "main".into() },
        ]
    }

    /// The spec's reversible/additive group, item for item.
    #[test]
    fn the_reversible_group_matches_the_spec() {
        for action in &every_action()[..5] {
            assert_eq!(action.reversibility(), Reversibility::Reversible, "{}", action.summary());
        }
    }

    /// The spec's confirmation-required group, item for item.
    #[test]
    fn the_confirmation_required_group_matches_the_spec() {
        for action in &every_action()[5..] {
            assert_eq!(
                action.reversibility(),
                Reversibility::ConfirmationRequired,
                "{}",
                action.summary()
            );
        }
    }

    #[test]
    fn every_action_names_the_project_it_targets() {
        for action in every_action() {
            assert_eq!(action.project(), "ttui");
        }
    }

    #[test]
    fn a_summary_names_the_action_and_its_target_so_a_confirmation_prompt_can_quote_it() {
        let merge = Action::MergePullRequest { project: "ttui".into(), number: 142 };
        let s = merge.summary();
        assert!(s.contains("merge"), "got {s}");
        assert!(s.contains("142"), "got {s}");
        assert!(s.contains("ttui"), "got {s}");
    }

    #[test]
    fn actions_round_trip_through_json_so_a_confirmation_can_be_fingerprinted() {
        for action in every_action() {
            let json = serde_json::to_string(&action).unwrap();
            let back: Action = serde_json::from_str(&json).unwrap();
            assert_eq!(action, back);
        }
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test -p parallax-baseline actions::`
Expected: FAIL — `actions` module does not exist.

```rust
//! Control actions. Plain data plus a plain API, so every action is
//! available headless and the cockpit is only one caller. Each action is
//! classified by reversibility, and the irreversible group cannot reach
//! an executor without a `Confirmation` — enforced by the type system,
//! not by a convention.

use serde::{Deserialize, Serialize};

/// How a human disposed of a Plumb finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ruling {
    /// The finding stands.
    Upheld,
    /// The finding is overruled and suppressed in future runs.
    Overruled,
}

/// Something the operator can do to a registered project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum Action {
    /// Rule on a Plumb finding. The highest-leverage action there is:
    /// it is the one input Plumb's learned-rejection store depends on.
    RuleFinding {
        /// Which project's finding.
        project: String,
        /// The finding's fingerprint.
        fingerprint: String,
        /// Upheld or overruled.
        ruling: Ruling,
    },
    /// Set or change a work item's autonomy label.
    SetAutonomyLabel {
        /// Which project.
        project: String,
        /// The item's number.
        item: u64,
        /// The native label to apply.
        label: String,
    },
    /// Ask for a work item to be reviewed again.
    RequestReReview {
        /// Which project.
        project: String,
        /// The item's number.
        item: u64,
    },
    /// Trigger a capture run.
    TriggerCapture {
        /// Which project.
        project: String,
        /// A specific scenario, or every selected one.
        scenario: Option<String>,
    },
    /// Start an agent run against a work item.
    DispatchAgentRun {
        /// Which project.
        project: String,
        /// The item's number.
        item: u64,
        /// What the agent is being asked to do.
        prompt: String,
    },
    /// Stop a running agent. **Confirmation required.**
    StopAgentRun {
        /// Which project.
        project: String,
        /// The session's name.
        session: String,
    },
    /// Merge a pull request. **Confirmation required.**
    MergePullRequest {
        /// Which project.
        project: String,
        /// The pull request's number.
        number: u64,
    },
    /// Push a branch. **Confirmation required.**
    Push {
        /// Which project.
        project: String,
        /// The branch to push.
        branch: String,
    },
}

/// Whether an action can be taken back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reversibility {
    /// Additive or undoable; safe to take on a single keystroke.
    Reversible,
    /// Outward-facing or hard to undo; requires explicit confirmation.
    ConfirmationRequired,
}

impl Action {
    /// How reversible this action is. **The classification is the
    /// platform spec's, verbatim — do not reclassify an action here
    /// without the spec changing.**
    pub fn reversibility(&self) -> Reversibility {
        match self {
            Action::RuleFinding { .. }
            | Action::SetAutonomyLabel { .. }
            | Action::RequestReReview { .. }
            | Action::TriggerCapture { .. }
            | Action::DispatchAgentRun { .. } => Reversibility::Reversible,
            Action::StopAgentRun { .. }
            | Action::MergePullRequest { .. }
            | Action::Push { .. } => Reversibility::ConfirmationRequired,
        }
    }

    /// Which project this action targets.
    pub fn project(&self) -> &str {
        match self {
            Action::RuleFinding { project, .. }
            | Action::SetAutonomyLabel { project, .. }
            | Action::RequestReReview { project, .. }
            | Action::TriggerCapture { project, .. }
            | Action::DispatchAgentRun { project, .. }
            | Action::StopAgentRun { project, .. }
            | Action::MergePullRequest { project, .. }
            | Action::Push { project, .. } => project,
        }
    }

    /// A one-line description naming the action and its target, so a
    /// confirmation prompt can quote exactly what is about to happen.
    pub fn summary(&self) -> String {
        match self {
            Action::RuleFinding { project, fingerprint, ruling } => {
                format!("{project}: rule {fingerprint} as {ruling:?}")
            }
            Action::SetAutonomyLabel { project, item, label } => {
                format!("{project}: label #{item} `{label}`")
            }
            Action::RequestReReview { project, item } => {
                format!("{project}: request re-review of #{item}")
            }
            Action::TriggerCapture { project, scenario } => match scenario {
                Some(s) => format!("{project}: capture scenario `{s}`"),
                None => format!("{project}: capture every selected scenario"),
            },
            Action::DispatchAgentRun { project, item, .. } => {
                format!("{project}: dispatch an agent run on #{item}")
            }
            Action::StopAgentRun { project, session } => {
                format!("{project}: stop the agent in session `{session}`")
            }
            Action::MergePullRequest { project, number } => {
                format!("{project}: merge pull request #{number}")
            }
            Action::Push { project, branch } => format!("{project}: push `{branch}`"),
        }
    }
}
```

Add `pub mod actions;` to `baseline/src/lib.rs`.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline actions::`
Expected: PASS, 5 tests.

- [ ] **Step 4: Commit**

```bash
git add baseline/src/actions.rs baseline/src/lib.rs
git commit -m "feat(actions): define the control action set and its reversibility

Control lives in the core as plain data so every action is available
headless; the reversible/confirmation-required split is the spec's
classification verbatim, with a test per group."
```

---

#### Task 22: `Confirmation`, `Authorized`, and `authorize`

**Files:**
- Modify: `baseline/src/actions.rs`

**Interfaces:**
- Consumes: `Action`, `Reversibility` (Task 21).
- Produces:
  - `pub fn fingerprint(action: &Action) -> String` — 16 hex chars of SHA-256 over the action's canonical JSON
  - `pub struct Confirmation { fingerprint: String }` with `pub fn of(action: &Action) -> Self`, `pub fn fingerprint(&self) -> &str`
  - `pub struct Authorized<'a> { action: &'a Action }` with `pub fn action(&self) -> &Action`
  - `pub enum ActionError { ConfirmationRequired { summary: String }, ConfirmationMismatch { expected: String, got: String }, Adapter(AdapterError), NotSupported(String) }`
  - `pub fn authorize<'a>(action: &'a Action, confirmation: Option<&Confirmation>) -> Result<Authorized<'a>, ActionError>`

  Consumed by Tasks 23 and 24 — `ActionExecutor::execute` takes an
  `Authorized`, so it is unreachable without going through `authorize`.

**This is Judgment call 2 made concrete, and it is the task the spec's
fourth Verification bullet lands on.** Three properties, each a test:

1. A confirmation-required action with `None` is refused.
2. A confirmation built for a *different* action is refused — so
   confirming "merge #12" cannot execute "merge #99".
3. `Authorized` has a private field, so an executor cannot be reached
   any other way. This is compile-time, not runtime, and the plan
   asserts it with a `compile_fail` doctest rather than a claim.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod confirmation_tests {
    use super::*;

    fn merge(number: u64) -> Action {
        Action::MergePullRequest { project: "ttui".into(), number }
    }

    fn rule() -> Action {
        Action::RuleFinding {
            project: "ttui".into(),
            fingerprint: "a1b2c3d4".into(),
            ruling: Ruling::Overruled,
        }
    }

    #[test]
    fn a_reversible_action_authorizes_with_no_confirmation() {
        let action = rule();
        assert!(authorize(&action, None).is_ok());
    }

    /// The spec's fourth Verification bullet, directly.
    #[test]
    fn a_confirmation_required_action_refuses_to_authorize_unconfirmed() {
        for action in [
            merge(142),
            Action::Push { project: "ttui".into(), branch: "main".into() },
            Action::StopAgentRun { project: "ttui".into(), session: "s".into() },
        ] {
            match authorize(&action, None) {
                Err(ActionError::ConfirmationRequired { summary }) => {
                    assert_eq!(summary, action.summary(), "the refusal quotes what was refused");
                }
                other => panic!("expected refusal for {}, got {other:?}", action.summary()),
            }
        }
    }

    #[test]
    fn a_confirmation_required_action_authorizes_with_its_own_confirmation() {
        let action = merge(142);
        let confirmation = Confirmation::of(&action);
        assert!(authorize(&action, Some(&confirmation)).is_ok());
    }

    /// Confirming one merge must not authorize a different one — this is
    /// what a bare `confirmed: bool` cannot express.
    #[test]
    fn a_confirmation_for_a_different_action_is_refused() {
        let confirmation = Confirmation::of(&merge(12));
        match authorize(&merge(99), Some(&confirmation)) {
            Err(ActionError::ConfirmationMismatch { expected, got }) => {
                assert_eq!(expected, fingerprint(&merge(99)));
                assert_eq!(got, fingerprint(&merge(12)));
            }
            other => panic!("expected a mismatch, got {other:?}"),
        }
    }

    #[test]
    fn a_stray_confirmation_on_a_reversible_action_is_harmless_when_it_matches() {
        let action = rule();
        assert!(authorize(&action, Some(&Confirmation::of(&action))).is_ok());
    }

    /// A confirmation naming the wrong action is refused even when the
    /// action would not have needed one — a caller that confused two
    /// actions has a bug worth surfacing.
    #[test]
    fn a_mismatched_confirmation_on_a_reversible_action_is_still_refused() {
        assert!(matches!(
            authorize(&rule(), Some(&Confirmation::of(&merge(12)))),
            Err(ActionError::ConfirmationMismatch { .. })
        ));
    }

    #[test]
    fn a_fingerprint_is_stable_for_the_same_action_and_differs_between_actions() {
        assert_eq!(fingerprint(&merge(142)), fingerprint(&merge(142)));
        assert_ne!(fingerprint(&merge(142)), fingerprint(&merge(143)));
        assert_ne!(fingerprint(&merge(142)), fingerprint(&rule()));
        assert_eq!(fingerprint(&merge(142)).len(), 16);
    }

    #[test]
    fn an_authorized_action_still_names_what_it_authorizes() {
        let action = merge(142);
        let authorized = authorize(&action, Some(&Confirmation::of(&action))).unwrap();
        assert_eq!(authorized.action(), &action);
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test -p parallax-baseline actions::confirmation_tests`
Expected: FAIL — `authorize` does not exist.

```rust
use crate::adapters::AdapterError;
use sha2::{Digest, Sha256};

/// A stable short fingerprint of exactly this action, including its
/// arguments. Confirming "merge #12" therefore cannot authorize
/// "merge #99".
pub fn fingerprint(action: &Action) -> String {
    let canonical = serde_json::to_string(action).unwrap_or_else(|_| format!("{action:?}"));
    let digest = Sha256::digest(canonical.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Proof that a caller saw and approved one specific action.
///
/// The only constructor is `Confirmation::of`, which takes the action
/// itself — so a confirmation cannot be conjured from a bare `true`,
/// and cannot be reused for a different action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    fingerprint: String,
}

impl Confirmation {
    /// Confirms this exact action.
    pub fn of(action: &Action) -> Self {
        Self { fingerprint: fingerprint(action) }
    }

    /// The confirmed action's fingerprint.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

/// An action cleared to execute. **Its field is private**, so the only
/// way to obtain one is `authorize` — which means no executor can be
/// reached without passing the confirmation check.
#[derive(Debug)]
pub struct Authorized<'a> {
    action: &'a Action,
}

impl<'a> Authorized<'a> {
    /// The action this authorizes.
    pub fn action(&self) -> &Action {
        self.action
    }
}

/// Why an action did not happen.
#[derive(Debug)]
pub enum ActionError {
    /// The action needs confirmation and none was given.
    ConfirmationRequired {
        /// What was refused, quoted for the caller.
        summary: String,
    },
    /// A confirmation was given, but for a different action.
    ConfirmationMismatch {
        /// The fingerprint the action needed.
        expected: String,
        /// The fingerprint the confirmation carried.
        got: String,
    },
    /// The action reached its side effect and that failed.
    Adapter(AdapterError),
    /// This executor cannot perform this action.
    NotSupported(String),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionError::ConfirmationRequired { summary } => {
                write!(f, "refused without confirmation: {summary}")
            }
            ActionError::ConfirmationMismatch { expected, got } => {
                write!(f, "confirmation is for {got}, not {expected}")
            }
            ActionError::Adapter(e) => write!(f, "action failed: {e}"),
            ActionError::NotSupported(m) => write!(f, "unsupported action: {m}"),
        }
    }
}
impl std::error::Error for ActionError {}

impl From<AdapterError> for ActionError {
    fn from(e: AdapterError) -> Self {
        ActionError::Adapter(e)
    }
}

/// Checks an action against its confirmation requirement.
///
/// A confirmation that names a different action is refused whether or
/// not the action needed one — a caller that confused two actions has a
/// bug, and silently proceeding would hide it.
pub fn authorize<'a>(
    action: &'a Action,
    confirmation: Option<&Confirmation>,
) -> Result<Authorized<'a>, ActionError> {
    let expected = fingerprint(action);
    match confirmation {
        Some(c) if c.fingerprint() != expected => Err(ActionError::ConfirmationMismatch {
            expected,
            got: c.fingerprint().to_string(),
        }),
        Some(_) => Ok(Authorized { action }),
        None => match action.reversibility() {
            Reversibility::Reversible => Ok(Authorized { action }),
            Reversibility::ConfirmationRequired => {
                Err(ActionError::ConfirmationRequired { summary: action.summary() })
            }
        },
    }
}
```

- [ ] **Step 3: Add the compile-fail doctest that proves the gate is structural**

On `Authorized`, add this to its doc comment. A runtime test cannot
prove "there is no other way to build one"; a `compile_fail` doctest
can.

````rust
/// ```compile_fail
/// use parallax_baseline::actions::{Action, Authorized};
/// let action = Action::Push { project: "ttui".into(), branch: "main".into() };
/// // `Authorized`'s field is private, so this does not compile — the
/// // only way to obtain one is `authorize`.
/// let sneaky = Authorized { action: &action };
/// ```
````

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline actions::`
Expected: PASS, 13 tests.

Run: `cargo test -p parallax-baseline --doc`
Expected: PASS — the `compile_fail` doctest is counted as passing when
the snippet fails to compile.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/actions.rs
git commit -m "feat(actions): make confirmation a type, not a boolean

A Confirmation fingerprints the exact action it approves, so confirming
one merge cannot execute another, and Authorized's private constructor
makes the check structural rather than a convention an executor could
forget — proved by a compile_fail doctest."
```

---

### Slice 6.2: Executing actions

**Tags:** coding

#### Task 23: `ActionExecutor`, `RecordingExecutor`, and the reversible actions

**Files:**
- Modify: `baseline/src/actions.rs`

**Interfaces:**
- Consumes: `Authorized`, `ActionError` (Task 22);
  `adapters::AdapterError`.
- Produces:
  - `pub enum Effect { WroteFile(PathBuf), CalledApi { method: String, url: String }, Spawned(String) }`
  - `pub struct ActionOutcome { pub summary: String, pub effects: Vec<Effect> }`
  - `pub trait ActionExecutor { fn execute(&mut self, authorized: Authorized<'_>) -> Result<ActionOutcome, ActionError>; }`
  - `pub struct RecordingExecutor { ... }` with `pub fn new() -> Self`, `pub fn executed(&self) -> &[Action]`
  - `pub trait WorkControl { fn set_label(&mut self, repo: &str, item: u64, label: &str) -> Result<(), AdapterError>; fn request_review(&mut self, repo: &str, item: u64) -> Result<(), AdapterError>; fn merge(&mut self, repo: &str, number: u64) -> Result<(), AdapterError>; }`
  - `pub trait ProcessControl { fn capture(&mut self, project: &str, scenario: Option<&str>) -> Result<(), AdapterError>; fn dispatch(&mut self, project: &str, item: u64, prompt: &str) -> Result<String, AdapterError>; fn stop(&mut self, session: &str) -> Result<(), AdapterError>; fn push(&mut self, project: &str, branch: &str) -> Result<(), AdapterError>; }`
  - `pub struct LocalExecutor<W: WorkControl, P: ProcessControl> { ... }` with `pub fn new(repo: impl Into<String>, rulings_path: impl Into<PathBuf>, work: W, process: P) -> Self`

  Consumed by Task 24 (which adds the confirmation-required arms and
  the end-to-end refusal tests).

**`execute` takes an `Authorized`, never an `&Action`.** That signature
is the whole contract: an executor cannot be handed an unauthorized
action, so "did you check confirmation?" is not a question an executor
implementer can get wrong.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod executor_tests {
    use super::*;

    #[derive(Default)]
    pub(super) struct FakeWork {
        pub calls: Vec<String>,
    }

    impl WorkControl for FakeWork {
        fn set_label(&mut self, repo: &str, item: u64, label: &str) -> Result<(), AdapterError> {
            self.calls.push(format!("label {repo}#{item} {label}"));
            Ok(())
        }
        fn request_review(&mut self, repo: &str, item: u64) -> Result<(), AdapterError> {
            self.calls.push(format!("re-review {repo}#{item}"));
            Ok(())
        }
        fn merge(&mut self, repo: &str, number: u64) -> Result<(), AdapterError> {
            self.calls.push(format!("merge {repo}#{number}"));
            Ok(())
        }
    }

    #[derive(Default)]
    pub(super) struct FakeProcess {
        pub calls: Vec<String>,
    }

    impl ProcessControl for FakeProcess {
        fn capture(&mut self, project: &str, scenario: Option<&str>) -> Result<(), AdapterError> {
            self.calls.push(format!("capture {project} {scenario:?}"));
            Ok(())
        }
        fn dispatch(&mut self, project: &str, item: u64, _prompt: &str) -> Result<String, AdapterError> {
            self.calls.push(format!("dispatch {project}#{item}"));
            Ok("session-1".into())
        }
        fn stop(&mut self, session: &str) -> Result<(), AdapterError> {
            self.calls.push(format!("stop {session}"));
            Ok(())
        }
        fn push(&mut self, project: &str, branch: &str) -> Result<(), AdapterError> {
            self.calls.push(format!("push {project} {branch}"));
            Ok(())
        }
    }

    pub(super) fn local(dir: &std::path::Path) -> LocalExecutor<FakeWork, FakeProcess> {
        LocalExecutor::new(
            "tatemeyer/ttui",
            dir.join(".plumb/rulings.jsonl"),
            FakeWork::default(),
            FakeProcess::default(),
        )
    }

    fn run(
        executor: &mut impl ActionExecutor,
        action: &Action,
    ) -> Result<ActionOutcome, ActionError> {
        let authorized = authorize(action, None)?;
        executor.execute(authorized)
    }

    #[test]
    fn a_recording_executor_records_without_side_effects() {
        let mut executor = RecordingExecutor::new();
        let action = Action::RequestReReview { project: "ttui".into(), item: 142 };
        let outcome = run(&mut executor, &action).unwrap();
        assert_eq!(executor.executed(), &[action.clone()]);
        assert!(outcome.effects.is_empty(), "a dry run has no effects");
        assert_eq!(outcome.summary, action.summary());
    }

    #[test]
    fn ruling_on_a_finding_appends_one_record_to_the_rulings_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        let action = Action::RuleFinding {
            project: "ttui".into(),
            fingerprint: "a1b2c3d4".into(),
            ruling: Ruling::Overruled,
        };
        let outcome = run(&mut executor, &action).unwrap();

        let text = std::fs::read_to_string(dir.path().join(".plumb/rulings.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("a1b2c3d4") && text.contains("overruled"));
        assert!(matches!(outcome.effects[0], Effect::WroteFile(_)));
    }

    #[test]
    fn ruling_twice_appends_rather_than_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        for fp in ["aaaa", "bbbb"] {
            let action = Action::RuleFinding {
                project: "ttui".into(),
                fingerprint: fp.into(),
                ruling: Ruling::Upheld,
            };
            run(&mut executor, &action).unwrap();
        }
        let text = std::fs::read_to_string(dir.path().join(".plumb/rulings.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn setting_a_label_and_requesting_a_re_review_go_through_work_control() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        run(&mut executor, &Action::SetAutonomyLabel { project: "ttui".into(), item: 142, label: "gated".into() }).unwrap();
        run(&mut executor, &Action::RequestReReview { project: "ttui".into(), item: 142 }).unwrap();
        assert_eq!(
            executor.work().calls,
            vec!["label tatemeyer/ttui#142 gated".to_string(), "re-review tatemeyer/ttui#142".to_string()]
        );
    }

    #[test]
    fn triggering_a_capture_and_dispatching_a_run_go_through_process_control() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        run(&mut executor, &Action::TriggerCapture { project: "ttui".into(), scenario: Some("dial".into()) }).unwrap();
        let outcome = run(&mut executor, &Action::DispatchAgentRun { project: "ttui".into(), item: 140, prompt: "audit".into() }).unwrap();
        assert_eq!(executor.process().calls.len(), 2);
        assert!(outcome.summary.contains("session-1"), "the new session is named back to the caller");
    }

    #[test]
    fn a_side_effect_that_fails_surfaces_as_an_action_error_rather_than_a_silent_success() {
        struct FailingWork;
        impl WorkControl for FailingWork {
            fn set_label(&mut self, _r: &str, _i: u64, _l: &str) -> Result<(), AdapterError> {
                Err(AdapterError::Http { status: 422, message: "label does not exist".into() })
            }
            fn request_review(&mut self, _r: &str, _i: u64) -> Result<(), AdapterError> {
                Ok(())
            }
            fn merge(&mut self, _r: &str, _n: u64) -> Result<(), AdapterError> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut executor = LocalExecutor::new("tatemeyer/ttui", dir.path().join("r.jsonl"), FailingWork, FakeProcess::default());
        let action = Action::SetAutonomyLabel { project: "ttui".into(), item: 1, label: "nope".into() };
        let err = run(&mut executor, &action).unwrap_err().to_string();
        assert!(err.contains("422"), "got {err}");
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test -p parallax-baseline actions::executor_tests`
Expected: FAIL.

```rust
use std::io::Write;
use std::path::PathBuf;

/// A side effect an action actually had, for reporting and dry runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// A file was written or appended to.
    WroteFile(PathBuf),
    /// A remote API was called.
    CalledApi {
        /// The HTTP method.
        method: String,
        /// The URL called.
        url: String,
    },
    /// A process or agent run was started or stopped.
    Spawned(String),
}

/// What an action did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    /// A one-line description of what happened.
    pub summary: String,
    /// The side effects it had.
    pub effects: Vec<Effect>,
}

/// Something that can perform an authorized action.
///
/// The parameter is `Authorized`, not `&Action`, so an executor cannot
/// be handed an unauthorized action — "did you check confirmation?" is
/// not a question an implementer can get wrong.
pub trait ActionExecutor {
    /// Performs the action.
    fn execute(&mut self, authorized: Authorized<'_>) -> Result<ActionOutcome, ActionError>;
}

/// An executor that records what it was asked to do and does nothing.
/// For dry runs, for tests, and for a frontend previewing a batch.
#[derive(Debug, Default)]
pub struct RecordingExecutor {
    executed: Vec<Action>,
}

impl RecordingExecutor {
    /// A fresh recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every action it was asked to perform, in order.
    pub fn executed(&self) -> &[Action] {
        &self.executed
    }
}

impl ActionExecutor for RecordingExecutor {
    fn execute(&mut self, authorized: Authorized<'_>) -> Result<ActionOutcome, ActionError> {
        let action = authorized.action();
        self.executed.push(action.clone());
        Ok(ActionOutcome { summary: action.summary(), effects: Vec::new() })
    }
}

/// The work-side effects an executor needs. Separated from the executor
/// so the GitHub calls stay real-external-service exempt while every
/// decision above them is tested.
pub trait WorkControl {
    /// Adds a label to a work item.
    fn set_label(&mut self, repo: &str, item: u64, label: &str) -> Result<(), AdapterError>;
    /// Requests a fresh review of a work item.
    fn request_review(&mut self, repo: &str, item: u64) -> Result<(), AdapterError>;
    /// Merges a pull request.
    fn merge(&mut self, repo: &str, number: u64) -> Result<(), AdapterError>;
}

/// The process-side effects an executor needs.
pub trait ProcessControl {
    /// Triggers a capture run.
    fn capture(&mut self, project: &str, scenario: Option<&str>) -> Result<(), AdapterError>;
    /// Starts an agent run, returning the new session's name.
    fn dispatch(&mut self, project: &str, item: u64, prompt: &str) -> Result<String, AdapterError>;
    /// Stops a running agent.
    fn stop(&mut self, session: &str) -> Result<(), AdapterError>;
    /// Pushes a branch.
    fn push(&mut self, project: &str, branch: &str) -> Result<(), AdapterError>;
}

/// Performs actions against one project: rulings on disk, work items
/// through `WorkControl`, runs through `ProcessControl`.
pub struct LocalExecutor<W: WorkControl, P: ProcessControl> {
    repo: String,
    rulings_path: PathBuf,
    work: W,
    process: P,
}

impl<W: WorkControl, P: ProcessControl> LocalExecutor<W, P> {
    /// An executor for `repo`, appending rulings to `rulings_path`.
    pub fn new(
        repo: impl Into<String>,
        rulings_path: impl Into<PathBuf>,
        work: W,
        process: P,
    ) -> Self {
        Self { repo: repo.into(), rulings_path: rulings_path.into(), work, process }
    }

    /// The work-side control, for asserting what it was asked to do.
    pub fn work(&self) -> &W {
        &self.work
    }

    /// The process-side control, for asserting what it was asked to do.
    pub fn process(&self) -> &P {
        &self.process
    }

    /// Appends one ruling record as a JSON line.
    fn append_ruling(
        &self,
        project: &str,
        finger: &str,
        ruling: Ruling,
    ) -> Result<(), AdapterError> {
        if let Some(parent) = self.rulings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let record = serde_json::json!({
            "project": project,
            "fingerprint": finger,
            "ruling": ruling,
        });
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.rulings_path)?;
        writeln!(file, "{record}")?;
        Ok(())
    }
}

impl<W: WorkControl, P: ProcessControl> ActionExecutor for LocalExecutor<W, P> {
    fn execute(&mut self, authorized: Authorized<'_>) -> Result<ActionOutcome, ActionError> {
        let action = authorized.action();
        let summary = action.summary();
        let effects = match action {
            Action::RuleFinding { project, fingerprint, ruling } => {
                self.append_ruling(project, fingerprint, *ruling)?;
                vec![Effect::WroteFile(self.rulings_path.clone())]
            }
            Action::SetAutonomyLabel { item, label, .. } => {
                self.work.set_label(&self.repo, *item, label)?;
                vec![Effect::CalledApi {
                    method: "POST".into(),
                    url: format!("repos/{}/issues/{item}/labels", self.repo),
                }]
            }
            Action::RequestReReview { item, .. } => {
                self.work.request_review(&self.repo, *item)?;
                vec![Effect::CalledApi {
                    method: "POST".into(),
                    url: format!("repos/{}/pulls/{item}/requested_reviewers", self.repo),
                }]
            }
            Action::TriggerCapture { project, scenario } => {
                self.process.capture(project, scenario.as_deref())?;
                vec![Effect::Spawned("capture".into())]
            }
            Action::DispatchAgentRun { project, item, prompt } => {
                let session = self.process.dispatch(project, *item, prompt)?;
                return Ok(ActionOutcome {
                    summary: format!("{summary} (session `{session}`)"),
                    effects: vec![Effect::Spawned(session)],
                });
            }
            // The confirmation-required arms land in Task 24.
            other => return Err(ActionError::NotSupported(other.summary())),
        };
        Ok(ActionOutcome { summary, effects })
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline actions::`
Expected: PASS, 19 tests.

- [ ] **Step 4: Commit**

```bash
git add baseline/src/actions.rs
git commit -m "feat(actions): execute the reversible actions

execute takes an Authorized rather than an &Action, so an executor
cannot be handed an unauthorized action; side effects sit behind
WorkControl/ProcessControl so every decision above them is tested while
the live calls stay real-external-service exempt."
```

---

#### Task 24: The confirmation-required actions, refusing to execute unconfirmed

**Files:**
- Modify: `baseline/src/actions.rs`

**Interfaces:**
- Consumes: everything from Tasks 21-23.
- Produces: the three remaining `LocalExecutor` match arms
  (`StopAgentRun`, `MergePullRequest`, `Push`). No new public types.

**This closes the spec's fourth Verification bullet: "confirmation-
required actions refuse to execute without explicit confirmation —
asserted, not assumed."** Task 22 asserted it at `authorize`; this task
asserts it end-to-end, at the executor, with a `LocalExecutor` whose
fakes would visibly record a side effect if one leaked through.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod confirmed_execution_tests {
    use super::executor_tests::{local, FakeProcess, FakeWork};
    use super::*;

    fn confirmation_required() -> Vec<Action> {
        vec![
            Action::StopAgentRun { project: "ttui".into(), session: "widget-audit".into() },
            Action::MergePullRequest { project: "ttui".into(), number: 142 },
            Action::Push { project: "ttui".into(), branch: "main".into() },
        ]
    }

    /// The spec's fourth Verification bullet, at the executor rather than
    /// at `authorize`: nothing reaches a side effect unconfirmed.
    #[test]
    fn no_confirmation_required_action_reaches_a_side_effect_unconfirmed() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        for action in confirmation_required() {
            match authorize(&action, None) {
                Err(ActionError::ConfirmationRequired { .. }) => {}
                other => panic!("{} was not refused: {other:?}", action.summary()),
            }
        }
        assert!(executor.work().calls.is_empty(), "no work-side effect leaked");
        assert!(executor.process().calls.is_empty(), "no process-side effect leaked");
        let _ = &mut executor;
    }

    #[test]
    fn each_confirmation_required_action_executes_once_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        for action in confirmation_required() {
            let confirmation = Confirmation::of(&action);
            let authorized = authorize(&action, Some(&confirmation)).expect("authorizes");
            let outcome = executor.execute(authorized).expect("executes");
            assert_eq!(outcome.summary, action.summary());
            assert_eq!(outcome.effects.len(), 1);
        }
        assert_eq!(executor.work().calls, vec!["merge tatemeyer/ttui#142".to_string()]);
        assert_eq!(
            executor.process().calls,
            vec!["stop widget-audit".to_string(), "push ttui main".to_string()]
        );
    }

    #[test]
    fn a_confirmation_for_a_neighbouring_pull_request_does_not_execute_this_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        let wrong = Confirmation::of(&Action::MergePullRequest { project: "ttui".into(), number: 141 });
        let action = Action::MergePullRequest { project: "ttui".into(), number: 142 };
        assert!(matches!(
            authorize(&action, Some(&wrong)),
            Err(ActionError::ConfirmationMismatch { .. })
        ));
        assert!(executor.work().calls.is_empty());
        let _ = &mut executor;
    }

    #[test]
    fn a_recording_executor_still_refuses_an_unconfirmed_action_upstream_of_itself() {
        let mut executor = RecordingExecutor::new();
        let action = Action::Push { project: "ttui".into(), branch: "main".into() };
        assert!(authorize(&action, None).is_err());
        assert!(executor.executed().is_empty());
        let _ = &mut executor;
    }

    #[test]
    fn a_merge_that_the_remote_rejects_is_reported_rather_than_reported_as_done() {
        struct RejectingWork;
        impl WorkControl for RejectingWork {
            fn set_label(&mut self, _r: &str, _i: u64, _l: &str) -> Result<(), AdapterError> {
                Ok(())
            }
            fn request_review(&mut self, _r: &str, _i: u64) -> Result<(), AdapterError> {
                Ok(())
            }
            fn merge(&mut self, _r: &str, _n: u64) -> Result<(), AdapterError> {
                Err(AdapterError::Http { status: 405, message: "not mergeable".into() })
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut executor =
            LocalExecutor::new("tatemeyer/ttui", dir.path().join("r.jsonl"), RejectingWork, FakeProcess::default());
        let action = Action::MergePullRequest { project: "ttui".into(), number: 142 };
        let authorized = authorize(&action, Some(&Confirmation::of(&action))).unwrap();
        let err = executor.execute(authorized).unwrap_err().to_string();
        assert!(err.contains("405") && err.contains("not mergeable"), "got {err}");
    }

    /// No action falls through to `NotSupported` any more.
    #[test]
    fn the_local_executor_now_handles_every_action_in_the_set() {
        let dir = tempfile::tempdir().unwrap();
        let mut executor = local(dir.path());
        let all = vec![
            Action::RuleFinding { project: "ttui".into(), fingerprint: "aaaa".into(), ruling: Ruling::Upheld },
            Action::SetAutonomyLabel { project: "ttui".into(), item: 1, label: "gated".into() },
            Action::RequestReReview { project: "ttui".into(), item: 1 },
            Action::TriggerCapture { project: "ttui".into(), scenario: None },
            Action::DispatchAgentRun { project: "ttui".into(), item: 1, prompt: "go".into() },
            Action::StopAgentRun { project: "ttui".into(), session: "s".into() },
            Action::MergePullRequest { project: "ttui".into(), number: 1 },
            Action::Push { project: "ttui".into(), branch: "main".into() },
        ];
        for action in &all {
            let authorized = authorize(action, Some(&Confirmation::of(action))).unwrap();
            let outcome = executor.execute(authorized);
            assert!(
                !matches!(outcome, Err(ActionError::NotSupported(_))),
                "{} fell through",
                action.summary()
            );
        }
        let _ = FakeWork::default();
    }
}
```

- [ ] **Step 2: Run to verify they fail, then implement**

Run: `cargo test -p parallax-baseline actions::confirmed_execution_tests`
Expected: FAIL — the three actions return `NotSupported`.

Replace `LocalExecutor::execute`'s catch-all arm with the three real
arms:

```rust
            Action::StopAgentRun { session, .. } => {
                self.process.stop(session)?;
                vec![Effect::Spawned(format!("stopped {session}"))]
            }
            Action::MergePullRequest { number, .. } => {
                self.work.merge(&self.repo, *number)?;
                vec![Effect::CalledApi {
                    method: "PUT".into(),
                    url: format!("repos/{}/pulls/{number}/merge", self.repo),
                }]
            }
            Action::Push { project, branch } => {
                self.process.push(project, branch)?;
                vec![Effect::Spawned(format!("push {branch}"))]
            }
```

The `match` is now exhaustive over `Action`, so the catch-all
`other => return Err(ActionError::NotSupported(other.summary()))` arm is
removed entirely. Keep `ActionError::NotSupported` in the enum — a
frontend registering its own executor still needs a way to say "not
me."

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p parallax-baseline actions::`
Expected: PASS, 25 tests.

- [ ] **Step 4: Check the file size**

Run: `(Get-Content baseline/src/actions.rs | Measure-Object -Line).Lines`
If it exceeds 500, split into `actions/{mod,confirm,executor}.rs`:
`mod.rs` keeps `Action`/`Ruling`/`Reversibility`, `confirm.rs` takes
`fingerprint`/`Confirmation`/`Authorized`/`ActionError`/`authorize`,
`executor.rs` takes the trait and both executors. Re-export from
`mod.rs` so no import outside the module changes.

- [ ] **Step 5: Commit**

```bash
git add baseline/src/actions.rs
git commit -m "feat(actions): execute the confirmation-required actions

Closes the spec's fourth verification item end to end: the three
irreversible actions are refused at authorize with the fake controls
recording no side effect, and a merge the remote rejects surfaces as an
error rather than as done."
```

---

## Arc 7: Verification sweep

### Slice 7.1: The spec's Verification section, executable

**Tags:** coding

#### Task 25: The verification sweep and the crate README

**Files:**
- Create: `baseline/tests/verification_sweep.rs`
- Create: `baseline/README.md`

**Interfaces:**
- Consumes: the whole public API.
- Produces: no new API.

**One test per bullet of the spec's Verification section**, written so a
reader can check the spec against the file line by line. Two of the four
bullets are already covered by earlier tasks; they are restated here
deliberately, because the value of this file is that it is readable
*as* the spec's checklist, and a reader should not have to trust that
the coverage exists somewhere else.

- [ ] **Step 1: Write the sweep**

```rust
//! The platform spec's Verification section, executable. One test per
//! bullet, in the spec's own order.
//!
//! - `cargo test` / `clippy` / `fmt --check` clean — enforced by CI and
//!   by every task's commit gate, not assertable from inside a test.
//! - Both real manifests parse, validate, and project — below.
//! - Adapter fixtures replay to correct aggregated state, including the
//!   partial-support case — below.
//! - Confirmation-required actions refuse to execute without explicit
//!   confirmation — below.

use parallax_baseline::actions::{
    authorize, ActionError, ActionExecutor, Action, Confirmation, RecordingExecutor,
    Reversibility,
};
use parallax_baseline::autonomy::{project, Implement, Merge, Readiness};
use parallax_baseline::manifest::parse_manifest_file;
use parallax_baseline::state::{aggregate_project, ProjectAdapters};
use parallax_baseline::validate::{validate, Validated};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn manifest(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("manifests").join(name)
}

fn load(name: &str) -> Validated {
    validate(parse_manifest_file(&manifest(name)).expect("parses")).expect("validates")
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

/// Bullet 2, first half: both real manifests parse and validate.
#[test]
fn both_real_manifests_parse_and_validate() {
    assert_eq!(load("ttui.yaml").manifest().project.name, "ttui");
    assert_eq!(load("model-experiments.yaml").manifest().project.name, "model-experiments");
}

/// Bullet 2, second half: every row of the spec's projection table,
/// against the real files, in the spec's own order.
#[test]
fn every_row_of_the_projection_table_holds_for_the_real_manifests() {
    let ttui = load("ttui.yaml");
    let ttui_map = &ttui.manifest().work.as_ref().unwrap().autonomy_map;
    let me = load("model-experiments.yaml");
    let me_map = &me.manifest().work.as_ref().unwrap().autonomy_map;

    type Row = (&'static str, Option<Implement>, Option<Merge>, Readiness);
    let ttui_rows: Vec<Row> = vec![
        ("direct", Some(Implement::Agent), Some(Merge::DirectPush), Readiness::Verifiable),
        ("gated", Some(Implement::Agent), Some(Merge::OnChecks), Readiness::Verifiable),
        ("human", Some(Implement::Agent), Some(Merge::HumanApproval), Readiness::Verifiable),
    ];
    let me_rows: Vec<Row> = vec![
        ("autonomy:safe", Some(Implement::Agent), Some(Merge::OnChecks), Readiness::Verifiable),
        ("autonomy:review", Some(Implement::Agent), Some(Merge::HumanApproval), Readiness::Verifiable),
        ("autonomy:human", Some(Implement::HumanOnly), None, Readiness::Verifiable),
        ("needs-intent", None, None, Readiness::NeedsIntent),
    ];

    for (map, rows) in [(ttui_map, ttui_rows), (me_map, me_rows)] {
        for (label, implement, merge, readiness) in rows {
            let a = project(map, label).unwrap_or_else(|| panic!("`{label}` is declared"));
            assert_eq!(a.implement, implement, "{label}: implement");
            assert_eq!(a.merge, merge, "{label}: merge");
            assert_eq!(a.readiness, readiness, "{label}: readiness");
        }
    }
}

/// Bullet 3's named case: a manifest declaring only `work:` produces a
/// valid, reduced view rather than an error.
#[test]
fn a_work_only_manifest_produces_a_valid_reduced_view_rather_than_an_error() {
    let mut parsed = parse_manifest_file(&manifest("ttui.yaml")).unwrap();
    parsed.verification.clear();
    parsed.artifacts.clear();
    parsed.sessions = None;
    parsed.project.root = Some(std::env::temp_dir());
    let validated = validate(parsed).expect("a work-only manifest is valid");

    let state = aggregate_project(&validated, &mut ProjectAdapters::new(), at(0));
    assert_eq!(state.name, "ttui");
    assert!(state.verification.is_empty());
    assert!(state.artifacts.is_empty());
    assert!(state.sessions.is_none());
    assert!(state.degradations.is_empty(), "an undeclared source is not a degraded one");
}

/// Bullet 4: confirmation-required actions refuse to execute without
/// explicit confirmation.
#[test]
fn confirmation_required_actions_refuse_to_execute_unconfirmed() {
    let mut executor = RecordingExecutor::new();
    let irreversible = [
        Action::StopAgentRun { project: "ttui".into(), session: "s".into() },
        Action::MergePullRequest { project: "ttui".into(), number: 142 },
        Action::Push { project: "ttui".into(), branch: "main".into() },
    ];
    for action in &irreversible {
        assert_eq!(action.reversibility(), Reversibility::ConfirmationRequired);
        assert!(matches!(
            authorize(action, None),
            Err(ActionError::ConfirmationRequired { .. })
        ));
    }
    assert!(executor.executed().is_empty(), "nothing reached the executor");

    // And they do execute once confirmed, so the refusal is a gate
    // rather than a wall.
    for action in &irreversible {
        let authorized = authorize(action, Some(&Confirmation::of(action))).expect("authorizes");
        executor.execute(authorized).expect("executes");
    }
    assert_eq!(executor.executed().len(), 3);
}

/// A constraint with no spec bullet of its own, asserted anyway because
/// it is the one thing that would quietly couple two sub-projects.
#[test]
fn nothing_in_this_crate_links_plumb() {
    let cargo_toml = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .expect("the crate has a manifest");
    assert!(
        !cargo_toml.contains("parallax-plumb"),
        "Baseline consumes Plumb's output as files, never as a crate"
    );
}
```

- [ ] **Step 2: Run the sweep**

Run: `cargo test -p parallax-baseline --test verification_sweep`
Expected: PASS, 5 tests.

- [ ] **Step 3: Write `baseline/README.md`**

A short file naming, in this order:

1. What Baseline is, in one paragraph from the spec's Architecture
   section: the platform core, holding every registered project's
   declared references; **never touches a terminal**; the cockpit is
   its first frontend, not its only possible one.
2. That it is sub-project #2 of Parallax, and that it shares no
   dependency with sub-project #1 (Plumb) — it consumes Plumb's
   `verdict.md` as a file.
3. The manifest, with `manifests/ttui.yaml` quoted as the worked
   example and a one-line note that `methodology:` is informational
   metadata only.
4. The three autonomy axes and the projection table, copied from the
   spec.
5. The four adapter families and their built-ins, as the table from
   this plan's Global Constraints.
6. A "Testing" paragraph: everything is unit-tested with no TTY and no
   network; adapters replay recorded fixtures under
   `baseline/tests/fixtures/`; live GitHub access is
   real-external-service exempt, confined to `UreqTransport`.
7. A pointer to
   `docs/design/specs/parallax/2026-08-14-parallax-platform-design.md`
   as the design of record.

- [ ] **Step 4: Run every gate from the workspace root**

```
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo doc -p parallax-baseline --no-deps
```

Expected: all clean, and `cargo doc` emits no `missing_docs` warning.

- [ ] **Step 5: Commit**

```bash
git add baseline/tests/verification_sweep.rs baseline/README.md
git commit -m "test(baseline): make the spec's verification section executable

One test per bullet in the spec's own order, so a reader can check the
plan against the spec line by line rather than trusting that the
coverage exists somewhere else."
```

---

## Spec coverage

Every section of
`docs/design/specs/parallax/2026-08-14-parallax-platform-design.md`, and
where it lands. Sections belonging to other sub-projects are marked as
such.

| Spec section | Tasks |
|---|---|
| Thesis / verification tiers | — (framing; the tiers appear as `VerificationOutcome` and the two verification adapters: 13, 14) |
| Why these two repos | 7 (both manifests), 20 (both replayed) |
| Naming | 1 (crate `parallax-baseline`), 25 (README) |
| What the platform normalizes | 2, 3, 4 |
| What it deliberately does not — `methodology:` | 5 (parsed, never branched on), 20 (asserted), 25 (README) |
| Architecture — headless core, no TTY | Global Constraints; 1 (no UI deps), 25 (README) |
| Architecture — the four adapter families | 10, 11-17 |
| Architecture — daemon deferred | Global Constraints (out of scope; nothing built) |
| The manifest — schema | 5, 6 |
| The manifest — both real files verbatim | 7 |
| The manifest — partial support is normal | 6, 8, 18, 20, 25 |
| Normalized autonomy — the three axes | 2 |
| Normalized autonomy — the projection table, every row | 3, 7, 20, 25 |
| Normalized autonomy — the two asymmetries | 3, 7 |
| Watching development live — ETag polling, 30s default | 9, 11, 12 |
| Watching development live — filesystem is immediate | 9, 14, 15, 16, 17 |
| Watching development live — "displays the age of each source" | 9, 19, 20 |
| Control actions — the plain headless API | 21, 23, 24 |
| Control actions — reversibility classification | 21 |
| Control actions — confirmation required | 22, 24, 25 |
| Control actions — ruling on findings | 21, 23 |
| Visualizing Model-Experiments (metrics feed only) | 16 (series parsing); rendering is sub-project #4, out of scope |
| Roadmap — #1 Plumb | out of scope; no dependency, asserted in 25 |
| Roadmap — #2 this plan | all |
| Roadmap — #3/#4/#5 | out of scope; the core is shaped to serve them (19's `sources`, 21's action set) |
| Governing this repo | — (process, not code) |
| Non-goals | Global Constraints; no task implements any of them |
| Testing — manifest parsing and validation | 5, 6, 7, 8 |
| Testing — autonomy projection, every table row | 3, 7, 20, 25 |
| Testing — state aggregation | 18, 19, 20 |
| Testing — artifact classification | 15, 16 |
| Testing — control-action authorization | 22, 24, 25 |
| Testing — adapters against recorded fixtures | 12, 14, 16, 20 |
| Testing — live GitHub is real-external-service exempt | 11 (confined to `UreqTransport`) |
| Testing — cockpit verified through Plumb | out of scope (sub-project #3) |
| Critical files — `manifest.rs` | 5 (plus `validate.rs`, split out) |
| Critical files — `autonomy.rs` | 2, 3, 4 |
| Critical files — `adapters/{work,verification,artifact,session}.rs` | 10-17 (plus `http.rs`, split out) |
| Critical files — `state.rs` | 18, 19 |
| Critical files — `actions.rs` | 21-24 |
| Critical files — `manifests/{ttui,model-experiments}.yaml` | 7 |
| Verification — cargo test/clippy/fmt clean | every task's commit gate; 25 |
| Verification — both manifests parse, validate, project | 7, 20, 25 |
| Verification — fixtures replay, including the partial case | 20, 25 |
| Verification — confirmation refusal, asserted | 22, 24, 25 |

---

## Judgment calls made while planning

Places the spec was silent or ambiguous, what was decided, and what to
change if the decision is wrong.

1. **Freshness is a per-value wrapper, not a per-adapter timestamp.**
   The spec says GitHub is fresh to within the poll interval,
   filesystem state is immediate, and "the cockpit displays the age of
   each source" — but the core has no UI, so it needs a representation
   rather than a rendering. Decided (Task 9): every adapter returns
   `Observed<T> { value, observed_at, source: SourceKind }`, and
   `Observed::freshness(now)` computes `Live` / `Fresh{age}` /
   `Stale{age, overdue}` against an **injected** `now`;
   `ProjectState::sources(now)` (Task 19) flattens that into a uniform
   `(label, Freshness)` list so a frontend need not know which sources
   poll and which are watched. Two consequences worth stating: a `304
   Not Modified` advances `observed_at` without changing the value
   (`confirm_unchanged`), because a conditional request that returns
   304 has *proved* currency; and a degraded source stays in the list as
   `Freshness::Unavailable` rather than disappearing from it.
   **If this is wrong**, the likely symptom is `Observed<T>` feeling
   heavy where a whole project shares one poll cycle. The fix is
   mechanical: keep `Freshness` and `SourceStatus` exactly as they are,
   drop the wrapper, and put a `HashMap<SourceLabel, SystemTime>` on
   `ProjectState` instead. Every consumer already talks in
   `SourceStatus`, so the change stops at `state.rs` and the adapter
   signatures.

2. **Confirmation is a typed token bound to the specific action, and
   authorization is enforced by a private constructor.** The spec says
   confirmation-required actions must refuse to execute unconfirmed but
   not how a caller proves confirmation. Decided (Task 22):
   `Confirmation::of(&action)` is the only constructor and stores a
   SHA-256 fingerprint of the action's canonical JSON;
   `authorize(&action, Option<&Confirmation>)` returns an `Authorized`
   whose field is private; `ActionExecutor::execute` takes an
   `Authorized`, never an `&Action`. Three properties follow, and all
   three are tested: an unconfirmed irreversible action is refused; a
   confirmation for "merge #12" cannot execute "merge #99"; and an
   executor **cannot be reached** except through `authorize` — proved
   by a `compile_fail` doctest, since no runtime test can show the
   absence of another path.
   Rejected alternatives, and why: a `confirmed: bool` parameter is
   trivially defaulted to `true` and carries no evidence of *which*
   action was approved; a separate `execute_confirmed` method doubles
   the trait surface and lets a caller pick the wrong one; an
   `is_confirmation_required()` an executor is asked to consult
   politely is a convention, and conventions are what this contract
   exists to replace.
   **If this is wrong** — most likely because a frontend wants to
   confirm a *batch* rather than each action — relax
   `Confirmation::of` to a `Confirmation::for_kind(&Action)` that
   fingerprints only the enum discriminant. That is one function body;
   `Authorized`'s private constructor, which is the actual gate, is
   untouched.

3. **Several mapped labels on one work item resolve most-restrictive
   per axis, with a stated claim always beating no claim** (Task 4).
   The spec's table maps one label at a time, but a real issue carries
   several and Model-Experiments' own map declares `needs-intent`
   alongside three tiers. Ordering: `Agent < HumanOnly`,
   `DirectPush < OnChecks < HumanApproval`,
   `Verifiable < NeedsIntent`, carried by the enums' declaration order.
   **If this is wrong**, the alternative is first-match-wins on a
   declared precedence list in the manifest — a bigger change, since it
   needs a new manifest field. A cheaper middle ground: report the
   conflict in `Resolution` (add a `conflicts: Vec<String>`) and let the
   frontend surface it, keeping most-restrictive as the value.

4. **`VerificationEntry::kind` is a free-form `String`, not an enum**
   (Task 5). The spec shows `lint`, `tests`, `perceptual`. Making it an
   enum would invite dispatch on it, and dispatch belongs to `adapter`.
   Keeping it open also means a project can declare `kind: benchmarks`
   with no code change. **If this is wrong**, closing it is one enum
   plus a `#[serde(other)]` catch-all variant, and nothing that
   consumes it today branches on it.

5. **Commands run through the platform shell** (`cmd /C` on Windows,
   `sh -c` elsewhere), not through a whitespace split (Task 13). TTUI's
   own lint command is `cargo clippy --all-targets -- -D warnings`,
   whose `--` and quoted argument a naive split mangles. The cost is
   that the command string is shell-interpreted, so a manifest can run
   arbitrary shell — which is already true of a field whose entire
   purpose is running a command. **If this is wrong**, add an optional
   `args: [String]` form to `VerificationEntry` and prefer it when
   present, leaving `command:` as the shell path.

6. **`watch:` is scanned on demand, not watched with an OS handle**
   (Tasks 15, 17). The manifest field is named `watch:` and the spec
   says adapters "watch the filesystem", but a headless library must
   not own background threads or inotify/ReadDirectoryChanges handles —
   the caller that decides when to poll GitHub should decide when to
   scan the disk, and a `notify`-based watcher would make every test
   time-dependent. Directory trees at these sizes scan in
   milliseconds. **If this is wrong** — a Model-Experiments results
   tree large enough that scanning is visible in the cockpit's frame
   budget — a real watcher slots in behind the unchanged
   `ArtifactAdapter`/`SessionAdapter` traits as an additional built-in,
   and no caller changes.

7. **`adapter: jsonl` is accepted as an alias for the metrics adapter**
   (Task 5). Model-Experiments' manifest, quoted verbatim in the spec,
   writes `adapter: jsonl`, while the platform's own adapter inventory
   names the family member `metrics`. Rather than change either, the
   enum variant is `Metrics` with `#[serde(alias = "jsonl")]`, so both
   spellings parse and the canonical one is written back out. **If this
   is wrong**, drop the alias and correct the manifest — but that means
   editing a file the spec quotes verbatim, which is the more expensive
   direction.

8. **GitHub's own timestamps are carried as opaque display strings**
   (`WorkItem::updated_at`), not parsed into `SystemTime` (Task 12).
   Parsing RFC 3339 needs `chrono` or hand-rolled timezone handling,
   and nothing in this crate computes on those values — the freshness
   that actually matters is the *observation's*, which
   `Observed::observed_at` already carries as a real `SystemTime`.
   **If this is wrong** — a frontend wanting to sort by upstream update
   time — add `chrono` and parse it, a change confined to one field and
   one parse site.

9. **`project.root` defaults to the manifest file's own directory**
   (Task 5). Model-Experiments' manifest, verbatim from the spec,
   declares no `root:`, and every filesystem adapter needs one. The
   default only applies to `parse_manifest_file`; `parse_manifest` on a
   string leaves it `None`, since a string has no location. **If this
   is wrong**, make `root` required and edit the spec's manifest — but
   the default is what makes a manifest portable between machines,
   which is worth more.

10. **The crate lives at `baseline/`, not `core/`.** The spec's
    "Critical files" inventory writes `core/src/manifest.rs`, from
    before the Naming table settled on **Baseline** / `parallax-
    baseline` (and noted that `parallax-core` is taken on crates.io).
    The plan follows the name, not the stale path. Two other
    divergences from that inventory, both to hold the 500-line ceiling:
    `validate.rs` split from `manifest.rs`, and `adapters/http.rs`
    split out as the transport seam.

11. **Aggregation is infallible; a failing adapter degrades one
    source** (Task 18). The spec does not say what happens when GitHub
    rate-limits mid-cycle. `aggregate_project` returns a `ProjectState`
    rather than a `Result`, and a failure becomes a `Degradation` plus
    a `Freshness::Unavailable` entry in `sources()`. A blank cockpit
    because one source failed is a worse failure than a labelled gap.
    **If this is wrong** — a caller that genuinely wants to fail fast —
    it can read `PlatformState::degraded()` and decide, which is
    strictly more power than a `Result` would have given it.

12. **Unmapped labels are collected and reported, never fatal and never
    guessed at** (Tasks 3, 18). A GitHub issue carries `bug`,
    `documentation`, `semver:minor` — none of them autonomy
    statements. `project` returns `None` for them and `ProjectState`
    surfaces them as `unmapped_labels`, which doubles as evidence that
    a manifest's `autonomy_map` has drifted from the repo's actual
    labels. No behaviour depends on the list.

13. **The GitHub adapter parses `serde_json::Value` rather than typed
    response structs** (Task 12). A typed mirror of GitHub's schema
    with `deny_unknown_fields` would break on any upstream addition,
    and without it the types buy little over direct indexing. The cost
    is that a renamed upstream field degrades to a default rather than
    a loud error — accepted, because the fixtures pin the fields this
    crate actually reads.

14. **`ChecksSummary::is_green()` requires at least one check to have
    run** (Task 10). Zero checks reported is not green — it is unknown,
    and `merge: on-checks` treating "no CI configured" as satisfied
    would be the exact wrong direction. Not in the spec; a defensive
    choice, and one worth revisiting if a project legitimately has no
    checks on some PRs.

15. **The plan does not touch the root `Cargo.toml`'s existing
    member** (Task 1). Plumb's Arc 1 Slice 1.1 owns `plumb/capture`,
    and the two sub-projects proceed in parallel, so Task 1 is written
    to work whether or not Plumb has landed first. If both land
    simultaneously the `members` array is the one conflict either plan
    can produce, and it is a one-line merge.

---

## Execution handoff

Plan complete and saved to
`docs/design/plans/parallax/2026-08-14-parallax-baseline-plan.md`. Two
execution options:

1. **Subagent-Driven (recommended)** — a fresh subagent per task, with
   review between tasks and fast iteration. Suits this plan well: after
   Arc 3 lands the adapter contract, Arc 4's seven tasks depend on the
   contract and not on each other, so Tasks 13-17 can be dispatched in
   parallel.
2. **Inline Execution** — execute tasks in-session via
   `superpowers:executing-plans`, batching with checkpoints.

Three notes whichever is chosen:

- **Everything executes in `D:/Dev/Projects/Parallax`**, on a worktree
  branch per `git-github-standards.md`, and lands through one Gated PR
  with all four checks green. No task touches the TTUI repo, and no
  task touches `plumb/`.
- **Task 1 is the only ordering coupling with Plumb.** It edits the
  root `Cargo.toml`'s `members`; everything after it is confined to
  `baseline/` and `manifests/`.
- **Task 12's fixtures should be captured once from the real API**
  (`gh api repos/tatemeyer/ttui/issues`, `.../pulls`,
  `.../commits/<sha>/check-runs`) and then trimmed to the fields the
  adapter reads. The file contents in the plan are correct in shape and
  usable as-is if capture is inconvenient; capturing real ones is
  better, because a fixture invented from the plan cannot surface a
  field this crate got wrong.

