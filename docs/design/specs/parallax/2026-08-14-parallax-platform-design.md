# Parallax — Master Design

**Status:** draft, pending your review before we move to planning.
**Date:** 2026-08-14
**Scope:** the master design binding TTUI, Model-Experiments, and
`plumb` into one system. Specs sub-project #2
(`parallax-baseline`) properly; sketches #3–#5 as scoped follow-ons, each
of which gets its own spec → plan cycle.

**Relationship to prior work:** depends on
`docs/design/specs/plumb/2026-08-14-plumb-design.md`
(sub-project #1, already spec-complete) as the perceptual verification
provider. Normalizes the autonomy vocabulary that
`.claude/rules/git-github-standards.md` and `.claude/rules/code-forge.md`
currently cross-reference against Model-Experiments by hand.

**Home:** its own repository. Filed here because this is where it was
designed and TTUI is both a consumer and the cockpit's UI dependency;
it moves with the code once that repo exists.

## Thesis

**Verification is the binding constraint on agent autonomy.** An agent
can work unattended exactly as far as "done" is machine-checkable. Every
gap in the checking is a place a human must stand.

The engineering discipline that follows is not "supervise the agent
better" — it is **build checkers for progressively fuzzier things**:

| Tier | What it checks | Cost | Who has it today |
|---|---|---|---|
| 0 | Compiles, types, lints | free, instant | both repos |
| 1 | Tests pass | cheap, deterministic | both repos |
| 2 | Numeric thresholds met | cheap, deterministic | Model-Experiments (CI) |
| 3 | **Looks right; matches stated intent** | model-as-checker | `plumb` (new) |
| 4 | Is the result *interesting* / novel | unsolved | nobody — open frontier |

Tiers 0–2 are settled practice. **Tier 3 is the frontier this system
actually advances**, and `plumb` is its first instance. Tier 4
is named honestly as unsolved and is not in scope.

### Why these two repos

TTUI and Model-Experiments are the same experiment run with opposite
bets, by the same operator, against the same agent:

- **Model-Experiments is outcome-first.** *"Instead of handing Claude a
  methodology, we specify outcomes and verification, and let the model
  figure out how."* CI is the source of truth for done. No prescribed
  process.
- **TTUI is methodology-first.** *"No implementation without an approved
  design doc and plan first."* Spec → Plan → Arc → Slice → Task, with
  mandatory TDD and mandatory visual review.

Both converge on the same bottleneck from opposite directions.
Model-Experiments states the rule — *if a machine can check it, it
belongs in CI, not in a human's head* — and TTUI's entire visual-review
convention exists precisely because a machine could not check "does it
look right."

That reframes `plumb`. It is not a TTUI tool that happens to be
portable. **It is the mechanism that lets Model-Experiments' philosophy
reach TTUI's domain**: Bitter Lesson Engineering requires a verifiable
success criterion, and for perceptual work none could be written. A
taste profile plus an intent lens is that criterion, made
machine-checkable.

The traffic runs both ways. Model-Experiments' `mx-viz` emits plots and
PyVista 3D field renders that nothing currently has eyes on — a
wrong-looking field visualization passes CI silently today. It is
`plumb`'s second consumer with no modification to either.

## Naming

The platform is **Parallax**: determining a true position by observing
the same object from two separated points. The name states the
*mechanism* rather than the goal — Plumb's four blinded lenses, and the
wider principle that a verdict from a single viewpoint is not a
measurement.

| Concept | Name | Crate |
|---|---|---|
| Platform | **Parallax** | — |
| Core library; holds every project's references | **Baseline** | `parallax-baseline` |
| Cockpit | **Panopticon** | `parallax-panopticon` |
| Perceptual verification | **Plumb** | `parallax-plumb` |
| Per-project manifest | — | `parallax.yaml` |
| Reviewer persona | **Sim Sup** | — |
| Verdict states | **GO / NO-GO / HOLD** | — |
| Blocker alert in the cockpit | **Cloister Bell** | — |

Each name does work. A *baseline* is the known separation between two
viewpoints — the quantity that makes a parallax measurement possible at
all — which is exactly what the core holds: every project's declared
references. A *panopticon* is a structure built so one observer can see
everything (and, in the register TTUI's examples already use, the Time
Lord Capitol's seat of judgment). *Plumb* tests trueness against a
reference; "out of plumb" is the verdict it renders. *Sim Sup* is
NASA's Simulation Supervisor, whose job was inventing failures to see
whether anyone caught them — the reviewer's stance in two words. The
*Cloister Bell* rings only for impending catastrophe, which is the
correct frequency for a blocker alert.

`parallax-core` is already taken on crates.io; `parallax-baseline` is
both free and the better name.

## What the platform normalizes — and what it deliberately does not

**Normalizes:** the vocabulary of *done* and *who may act*. Verification
tiers, autonomy semantics, work state, artifacts.

**Leaves alone:** how work is planned. TTUI stays methodology-first,
Model-Experiments stays outcome-first, future initiatives pick either or
neither. `methodology:` appears in the manifest as *informational
metadata only* — nothing in the platform branches on it.

This is deliberate. Forcing one process onto both would destroy the most
valuable property of the pair: they are a live controlled comparison of
two opposing theories of agentic development. The platform's job is to
make both observable through one lens, not to pick a winner
prematurely.

## Architecture

Three layers, and a strict dependency direction:

```
  cockpit (Panopticon)          TUI frontend — depends on `ttui` crate
        │
  parallax-baseline             manifests, adapters, state, control actions
        │                   no UI, no TTY, fully headless-testable
        ├── work adapters          (github)
        ├── verification adapters  (command, plumb)
        ├── artifact adapters      (figure, metrics, capture)
        └── session adapters       (filesystem watch)
```

`parallax-baseline` is a Rust library that never touches a terminal. The
cockpit is its first frontend, not its only possible one. This mirrors
TTUI's own `Buffer` / `Terminal::draw_diff` separation: everything
upstream of rendering is pure data, testable without a TTY.

A daemon is explicitly **not** built now. If "act while the cockpit is
closed" becomes a real need, a daemon becomes an alternative *host* for
the same core — not a rewrite.

**The cockpit lives in the platform repo and depends on `ttui` as a
published crate.** This makes it TTUI's first genuine external
consumer, which is exactly the API pressure a v1.0.0 needs and which
in-repo examples structurally cannot supply.

### The manifest

A project joins the platform by dropping a `parallax.yaml` in its root.
Partial support is
normal — a project that satisfies only the work adapter still shows up,
just with less detail.

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

Model-Experiments' manifest differs only in its adapter arguments:

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

### Normalized autonomy — two axes, not one ladder

The two repos' autonomy schemes do not map cleanly onto each other, and
the reason is diagnostic: **each collapses two or three independent
axes into a single label.** TTUI has a `Direct` tier (push straight to
`main`) that Model-Experiments has no equivalent for. Model-Experiments'
`autonomy:human` means *the agent must not implement this*, while TTUI's
`Human` tier means *the agent may implement, but a human must sign off* —
the same word, materially different rules.

The platform therefore normalizes onto three orthogonal fields, and each
project's native labels project onto them via `autonomy_map`:

```
implement:  agent | human-only          who may do the work
merge:      on-checks | human-approval | direct-push
readiness:  verifiable | needs-intent   is "done" even defined yet
```

Projected:

| Native label | implement | merge | readiness |
|---|---|---|---|
| TTUI `Direct` | agent | direct-push | verifiable |
| TTUI `Gated` | agent | on-checks | verifiable |
| TTUI `Human` | agent | human-approval | verifiable |
| ME `autonomy:safe` | agent | on-checks | verifiable |
| ME `autonomy:review` | agent | human-approval | verifiable |
| ME `autonomy:human` | human-only | — | verifiable |
| ME `needs-intent` | — | — | needs-intent |

Two asymmetries become visible immediately, which is the point: **ME has
no direct-push tier** (nothing bypasses CI), and **TTUI has no
human-only tier** (no work is reserved from the agent). Neither repo
could see that about itself; the shared vocabulary is what surfaces it.

`readiness: needs-intent` also has a TTUI counterpart already — the
brainstorming skill's mandatory clarifying-questions gate, which
`git-github-standards.md` explicitly identifies as its structural
equivalent.

### Watching development live

Adapters poll GitHub with ETag-conditional requests (cheap, rate-limit
friendly) and watch the filesystem for run artifacts and agent session
directories. No producer needs instrumenting, which is what keeps
"future initiatives" cheap to onboard.

This is polling, and the design says so rather than implying push:
GitHub state is fresh to within the poll interval (default 30s,
configurable); filesystem-backed state — captures, verdicts, session
activity — is effectively immediate. The cockpit displays the age of
each source so stale data is never mistaken for current.

### Control actions

Control lives in `parallax-baseline` as a plain API, so the same actions are
available headless. Each is classified by reversibility, and the cockpit
requires explicit confirmation for anything in the second group:

- **Reversible / additive:** rule on a `plumb` finding, set or
  change an autonomy label, request a re-review, trigger a capture,
  dispatch an agent run.
- **Confirmation required:** stop a running agent, merge a PR, push, or
  any action that is outward-facing or hard to undo.

Ruling on findings is the action with the highest leverage: it is the
one input `plumb`'s learned-rejection store depends on, and it
currently has no home.

### Visualizing Model-Experiments

The artifact adapters feed two distinct kinds of thing, and they need
different treatment:

- **Metrics** (JSONL scalar series — loss curves, probe accuracy,
  spectral error) render natively. TTUI already has `Sparkline` and
  `BarChart` from the mission-control Arc; this is a direct fit.
- **Fields and 3D surfaces** are the interesting case. TTUI's graduated
  perspective-projection work plus Braille-cell rendering gives a
  genuine terminal analogue to `mx-viz`'s PyVista surfaces — lower
  fidelity, but live and inline.
- **Pre-rendered PNG figures** cannot be shown at full fidelity in a
  terminal. Two honest options, both in scope for sub-project #4: show
  metadata plus the `plumb` verdict and offer to open the file
  externally, or render a downsampled preview using half-block cells
  (`▀` with independent fg/bg = two pixels per cell) at 24-bit color.
  The preview is a preview and will be labelled as one.

## Roadmap

Five sub-projects. Each gets its own spec → plan cycle; this document is
the master only.

| # | Sub-project | Depends on | Status |
|---|---|---|---|
| 1 | `plumb` | — | **spec complete**, ready to plan |
| 2 | `parallax-baseline` | — | specced by this document |
| 3 | Cockpit: observe | 2, `ttui` crate | sketched below |
| 4 | Model-Experiments visualization | 3 | sketched below |
| 5 | Cockpit: full control | 3 | sketched below |

#1 and #2 share no dependency and can proceed in parallel.

**#3 — Cockpit: observe.** A TUI over `parallax-baseline`, read-only.
Work in flight across all registered projects, CI and verification
status, `plumb` verdicts, autonomy distribution, session
activity. Lands before any control surface because control without
observation is not useful, and because this is where the "watch
development" value actually sits.

**#4 — Model-Experiments visualization.** Artifact-feed adapters plus
the rendering described above. Pulled ahead of full control because it
is a stated priority and because it exercises `parallax-baseline`'s artifact
path against a real, messy producer.

**#5 — Cockpit: full control.** The control actions above, wired to the
UI with the confirmation contract enforced.

## Governing this repo

Parallax normalizes two opposed philosophies, which raises the obvious
question of which one governs Parallax itself. The answer is neither
wholesale: the repo declares a **rule** for when each applies, because
deciding per unit of work is what Parallax's own three-axis autonomy
model already does everywhere else.

**The rule:**

> Does this unit of work create or change a contract that other units
> depend on?
>
> - **Yes → methodology-first.** Brainstorm → spec → plan before any
>   code, per TTUI's convention. Contracts are expensive to get wrong
>   and cheap to argue about on paper.
> - **No → outcome-first.** File an intent issue stating the desired
>   end state and how to verify it, let the implementation choose its
>   own approach, and let CI decide done, per Model-Experiments'
>   convention.

"Contract" means: adapter traits, the manifest schema, the finding
schema, the autonomy axes, verdict semantics, plugin and agent
interfaces, and anything crossing a repo boundary. Everything behind an
already-settled contract — a specific adapter implementation, a parser
for a declared schema, mechanical build-out, fixes — is outcome-first
work.

**The tiebreaker, when the rule is unclear:** try to write the
machine-checkable success criterion. If you can, it is outcome-first
work by definition — you just produced the artifact that method
requires. If you cannot, that inability *is* the signal that the
contract is not settled yet, and it needs a design pass first.

That tiebreaker is worth stating plainly because it collapses the two
philosophies into one test. Model-Experiments' `needs-intent` label and
TTUI's mandatory clarifying-questions gate are not two different
process rules — they are the same mechanism, triggered by the same
condition, wearing different names. The repos already agreed; nobody
had written it down.

**Interaction with the autonomy axes:** outcome-first work defaults to
`implement: agent, merge: on-checks`. Contract work requires human
sign-off on the *spec*; once the spec is approved, implementing it is
ordinary `merge: on-checks` work like anything else. The sign-off is on
the design, not on the diff.

## Non-goals

- **Replacing CI or GitHub.** The platform reads and acts through them;
  it never becomes the source of truth. Model-Experiments' "GitHub is
  the harness" stance is preserved, not superseded.
- **Imposing one methodology.** Explicitly rejected above.
- **A hosted or multi-user service.** Single-operator, local-first.
- **A web UI.** The cockpit is a TUI; that is the point.
- **Merging the repos.** They stay independent, keep their own CI,
  histories, and release paths. TTUI in particular must remain
  publishable as a standalone crate.
- **Tier 4 verification** (is a result novel or interesting). Named as
  unsolved, not attempted.
- **A daemon.** Deferred, with the core structured so it stays possible.

## Testing

- **`parallax-baseline`** — manifest parsing and validation, autonomy
  projection (every row of the table above is a test case), state
  aggregation, artifact classification, control-action authorization
  (confirmation-required actions must refuse to execute unconfirmed).
  All pure logic, all unit-tested, no TTY and no network.
- **Adapters** — integration-tested against recorded fixtures: captured
  GitHub API responses, sample `verdict.md` files, sample metrics JSONL.
  Live GitHub access is real-external-service exempt under the same
  precedent `plumb` establishes and TTUI already applies to
  real-TTY work.
- **Cockpit** — verified through `plumb`. The cockpit is a TUI
  built with TTUI, so it is exactly the kind of artifact sub-project #1
  exists to judge. The system verifies its own interface with its own
  perceptual tier, which is both the cleanest available test and the
  strongest possible dogfooding of the thesis.

## Critical files

New repository. First-cut inventory for sub-project #2:

- `core/src/manifest.rs` — schema, parsing, validation.
- `core/src/autonomy.rs` — the three-axis model and label projection.
- `core/src/adapters/{work,verification,artifact,session}.rs` — the four
  adapter traits and their built-in implementations.
- `core/src/state.rs` — aggregation across projects, source freshness.
- `core/src/actions.rs` — control actions and the confirmation contract.
- `manifests/{ttui,model-experiments}.yaml` — the two initial consumers.

## Verification

- `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --check` clean on `parallax-baseline`.
- Both real manifests parse, validate, and project their native
  autonomy labels onto the normalized axes matching the table above.
- Adapter fixtures replay to correct aggregated state, including the
  partial-support case (a manifest declaring only `work:` produces a
  valid, reduced view rather than an error).
- Confirmation-required actions refuse to execute without explicit
  confirmation — asserted, not assumed.
