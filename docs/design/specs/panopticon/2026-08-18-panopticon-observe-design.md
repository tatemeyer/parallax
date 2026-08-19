# Panopticon — Observe (Design)

**Status:** proposed — awaiting sign-off. Nothing here is implemented.
**Date:** 2026-08-18

**Place in the roadmap:** sub-project #3 of the Parallax platform
(`docs/design/specs/parallax/2026-08-14-parallax-platform-design.md`),
where it is sketched in one paragraph — "a TUI over
`parallax-baseline`, read-only" — and named **Panopticon**, crate
`parallax-panopticon`. This document is that paragraph turned into a
design.

**Dependencies, both now satisfied:**

- **Sub-project #2, `parallax-baseline`** — implemented through Arc 7
  and on `main`. `state::aggregate` produces the `PlatformState` this
  frontend renders, and `ProjectState::sources` is the API behind the
  master design's "the cockpit displays the age of each source."
- **`ttui` 1.0.0**, published to crates.io on 2026-08-14. Panopticon
  depends on it as a published crate, which is the point: it makes
  TTUI's first genuine external consumer real, and applies the API
  pressure that in-repo examples structurally cannot.

**Why this needs a spec at all.** Per "Governing this repo," work that
creates or changes a contract other units depend on is
methodology-first. Sub-projects #4 (Model-Experiments visualization)
and #5 (Cockpit: full control) both build on this one: #4 adds panes to
its layout and #5 wires actions to its key map. The pane model, the
refresh model, and the key map are contracts before they are code. The
tiebreaker agrees — the machine-checkable success criterion for "the
cockpit shows what is happening" cannot be written without first
deciding what "what is happening" means.

## Context / Motivation

`parallax-baseline` can already answer, for every registered project,
what work is in flight and how it is labelled, whether the checks are
green, what Plumb concluded, what artifacts a run produced, which agent
sessions are active, how current each of those answers is, and which
sources failed. It answers all of it as data, into a void: **nothing
renders it.** 533 passing tests describe a view nobody can see.

Meanwhile the operating situation the platform exists for is now real —
agent sessions running concurrently on separate machines against
separate repositories, each with its own harness, each landing PRs. The
state is genuinely distributed and the only aggregation point is a
human refreshing browser tabs. That is the gap: not "we need a
dashboard" in the abstract, but that the data is already assembled and
strictly unobservable.

Two secondary motivations, both stated in the master design and neither
sufficient on its own:

- **Dogfooding the thesis.** The cockpit is a TUI built with TTUI, so it
  is exactly the artifact Plumb exists to judge. The system verifying
  its own interface with its own perceptual tier is the strongest
  available demonstration that the tier is real.
- **External-consumer pressure on TTUI.** An example app inside a
  framework repo cannot discover that an API is awkward to depend on;
  it has the whole crate in scope and the author in the next file.

## Design

### The four questions

The cockpit exists to answer four questions about every registered
project at a glance, plus two about the cockpit's own honesty. Every
layout decision below is downstream of this list, and anything that
serves none of them is out of scope.

1. **What is in flight?** Open issues and pull requests, each with its
   projected autonomy — who may implement it, what it takes to land,
   whether "done" is even defined — and its check counts.
2. **Is it green?** Each declared verification check's standing: `lint`,
   `tests`, `perceptual`, with Plumb's GO / NO-GO / HOLD carried
   through as Pass / Fail / Hold and never flattened.
3. **What did it produce?** Capture runs with their verdicts, metrics
   series as sparklines, figures as counts and paths.
4. **Who is working where?** Session directories, their last activity,
   and whether that is inside the idle window.

And two the operator must never have to take on trust:

5. **How current is any of this?** Per source, not per screen. A polled
   GitHub feed 45 seconds into a 30-second interval is stale; a
   filesystem read taken this frame is live; the two must not look
   alike.
6. **What is broken?** A source that could not be read appears as
   unavailable *with its reason*, never as an empty pane.
   `PlatformState` already models this; the UI's job is not to launder
   it.

### Layout

Three regions. A project rail on the left, a detail region in the
centre, a source-freshness footer across the bottom.

```
+- PARALLAX ----------------------------------------- 00:41:12 -+
| PROJECTS     | ttui - rust - methodology-first                 |
| > ttui    OK | +- WORK - VERIFY - ARTIFACTS - SESSIONS ------+ |
|   model-x .. | |  #154  fix(depth_spike): LOD cutoff         | |
|   sesh    !! | |        agent - on-checks - verifiable  3/1  | |
|              | |  #131  visual-snapshot: capture a moment    | |
|              | |        -     - -         - needs-intent     | |
|              | |  #113  block.rs diverges from its spec      | |
|              | |        agent - direct-push - verifiable     | |
|              | +--------------------------------------------+ |
|              | unmapped: semver:patch, documentation          |
+--------------+------------------------------------------------+
| work 12s - verification:lint live - perceptual live - sessions |
+---------------------------------------------------------------+
```

- **The rail** is one row per registered project with a single status
  glyph: green when every declared source is fresh and every check
  passes, amber when something is stale or pending, red when a check
  fails or a source is unavailable. It is a `List`; the glyph is the
  worst of `ProjectState::stalest` and the verification outcomes.
- **The detail region** is four tabs over the selected project, one per
  question above. `Table` for work, a small `List` for verification,
  `Sparkline`/`BarChart` for metrics, `Table` for sessions.
- **The footer** is `ProjectState::sources(now)` rendered verbatim: one
  entry per source, its label, and its freshness. This is the pane that
  makes the whole screen trustworthy, so it is always visible and never
  scrolls off.

Keys, resolved through TTUI's `InputBinder` so chords are available
later without restructuring: `j`/`k` move within a pane, `Tab` cycles
panes, `1`-`4` select a detail tab, `r` forces a refresh of the reading
sources, `c` runs the selected project's command checks and `C` runs
every project's, `?` toggles help, `q` quits. Nothing else is bound, because every remaining verb
belongs to sub-project #5 and binding it now would mean binding it
twice.

### The refresh model

This is the load-bearing decision and the only one with a real
constraint behind it.

TTUI's `run()` is a **single-source blocking loop**: it polls terminal
input with a timeout equal to `App::tick_rate()`, dispatches input to
`update`, calls `on_tick` when the poll times out, and redraws. There is
no channel into it and no way to inject an external event. That is a
known TTUI limitation, currently open there, and it is the one thing
about this cockpit that could have forced a cross-repo dependency.

It does not, and this spec picks the option that avoids it:

- **Rejected — poll adapters inside `on_tick`.** Simplest, and wrong. A
  GitHub fetch is hundreds of milliseconds on a good day and unbounded
  on a bad one; a filesystem scan of a large worktree is not free
  either. Both would run on the UI thread, so every refresh would freeze
  input for its duration and a hung socket would hang the cockpit. The
  master design's own framing — a cockpit that goes blank because GitHub
  rate-limited is a worse failure than a number labelled stale — applies
  to responsiveness identically.
- **Chosen — a refresh thread, drained on tick.** The adapters live on a
  worker thread that owns them. It aggregates one project at a time and
  sends each finished `ProjectState` down an `mpsc::channel`. `on_tick`
  drains the receiver with `try_recv` until empty, replacing entries in
  the `PlatformState` the UI owns, and returns. Nothing blocks, nothing
  is shared mutably, and no lock is ever held across a render.

Consequences, stated rather than discovered later:

- **No TTUI change is required.** Panopticon ships against `ttui = "1"`
  unmodified. If external-event injection lands in TTUI later, this
  design gets simpler; it does not get unblocked, because it was never
  blocked.
- **Refresh latency is bounded by one tick.** At a 100 ms `tick_rate`
  that is imperceptible for data whose own freshness budget is 30
  seconds.
- **The cockpit wakes ~10 times a second while idle.** Accepted. TTUI's
  own examples tick at ~33 ms; this is a third of that rate.
- **Per-project sends, not one batch.** One slow project must not
  withhold the others. The rail shows each project updating as its own
  data lands, which is also more honest than a screen that changes all
  at once.
- **`r` does not fetch.** It signals the refresh thread. A key that
  performs I/O on the UI thread is the rejected option wearing a
  different hat.

### What the refresh cycle must never do

One adapter in the set is not a reader. `CommandVerificationAdapter::
check()` does not look up a check's standing — **it runs the check**.
TTUI's manifest declares `cargo clippy --all-targets -- -D warnings` and
`cargo test`, and `aggregate_project` calls every verification adapter
unconditionally, so a cockpit that aggregates on a 30-second cadence
would run both suites, for every registered project, forever — on the
machine whose entire purpose is running agent sessions in those same
repositories, from a background thread, with no visible cause.

That is not a tuning problem to be solved with a longer interval. It is
a category error: a cadence is the right shape for observing state and
the wrong shape for producing it.

So the rule is **the refresh cycle polls only sources that read**:

| Source | On cadence | Why |
|---|---|---|
| work (`github`) | yes | one conditional GET, ETag-cheap |
| verification (`plumb`) | yes | reads a `verdict.md` off disk |
| artifacts | yes | stat and read |
| sessions | yes | stat |
| verification (`command`) | **no** | spawns a process and burns the machine |

Command checks run when the operator asks: `c` runs the selected
project's, `C` runs every registered project's. Until one has run, the
pane says **not run this session** rather than showing a stale green —
a check whose last result predates the code on disk is worse than no
result, because it looks like an answer. Once one has run, its result
carries the age of the run that produced it, which is what `Observed`
already models and what the footer already displays.

Running a build is not a repository mutation, so this stays inside the
read-only non-goal below; it is called out because "read-only" would
otherwise imply "harmless to run in a loop," and one of these adapters
is not.

This needs one thing baseline does not expose: a way to tell an
executing check from a reading one without re-reading the manifest and
duplicating its interpretation. That is folded into the amendment named
in the next section, as `VerificationAdapter::cost()`.

### Two gaps in `parallax-baseline`, found by writing this document

Writing the spec against the implemented API rather than against the
plan surfaced two things baseline does not do, both of which every
frontend would need, and neither of which belongs here (the cost hint
above is a third, and rides along with them):

1. **There is no adapter factory.** `ProjectAdapters` is built by hand —
   the aggregation tests and `aggregate_replay.rs` each construct one
   field by field. Nothing maps a `Validated` manifest onto the adapters
   it declares. The cockpit cannot be the first place that logic lives,
   because a second frontend would have to reimplement it and the
   manifest's meaning would fork. Proposed:
   `parallax_baseline::adapters::from_manifest(&Validated, &AdapterConfig)
   -> ProjectAdapters`, where `AdapterConfig` carries the GitHub token
   and the poll interval.
2. **There is no registry.** `parse_manifest_file` reads *a* manifest
   from *a* path. Nothing answers "which projects are registered." The
   master design names baseline "registry, manifest, transport" and the
   registry half is absent. Proposed: a small `registry.rs` — a
   `~/.parallax/registry.yaml` listing project roots, plus a
   `--projects-root` scan that treats every `*/parallax.yaml` under a
   directory as registered.

Both are contract additions to a sub-project whose spec is approved, so
the honest route is an **amendment to the baseline design** and a short
plan against it, landing before Panopticon's implementation begins —
not a quiet expansion of this one's scope. They are called out here
because this document is where they were discovered.

### Determinism, and how the cockpit gets judged

The master design says the cockpit is verified through Plumb. That
requires captures that do not differ run to run, and a live cockpit
differs constantly: ages tick, sessions age out, GitHub moves.

So Panopticon ships a **fixture mode** as a first-class feature, not a
test scaffold: `panopticon --fixtures <dir>` builds every adapter from
recorded fixtures and freezes `now` at a value the fixture set declares.
Baseline already made this possible on purpose — `FixtureTransport` and
`ScriptedRunner` are `pub` rather than `#[cfg(test)]`, and every adapter
method takes an injected `now` — so nothing new is required beneath this
spec. Two runs of a fixture-mode scenario must produce identical frames;
that property is what makes a Plumb NO-GO mean "the layout is wrong"
rather than "time passed."

Fixture mode is also how a human sees the cockpit before any project is
registered, and how the repo demos it in CI.

### The Cloister Bell

The master design names a blocker alert and gives it the best name in
the document; this spec keeps it small. It rings on **transition into**
a blocker state — a verification outcome becoming `Fail`, a Plumb NO-GO
landing, a source becoming unavailable — not continuously while one
holds. A bell that rings every tick is a bell nobody hears.

Presentation: the footer's state changes and the project's rail glyph
goes red, plus one brief screen-level effect (TTUI ships `effects` and
`glitch` for exactly this register). Never a modal, never anything that
swallows a keystroke. The operator must be able to keep working while
something is on fire, because something usually is.

## Non-goals

- **Any control action.** This sub-project is read-only: no labels set,
  no PRs merged, no runs dispatched, nothing written to any repository.
  Baseline's whole `actions` module is deliberately unused here. That is
  sub-project #5, and the confirmation contract it enforces is exactly
  why it gets its own spec.
- **Figure rendering and 3D surfaces.** Sub-project #4. Metrics
  sparklines are in scope because they are a direct `Sparkline` fit and
  the artifact pane is empty without them; half-block PNG previews and
  perspective-projected fields are not.
- **A daemon, a web UI, multi-user, or a hosted service.** Unchanged
  from the master design's non-goals.
- **Changing TTUI.** If an API turns out to be genuinely inadequate, the
  finding is filed as a TTUI issue with the cockpit's use case attached
  — which is the whole value of being an external consumer — and this
  crate works around it in the meantime. Vendoring or forking TTUI is
  not on the table.
- **Persisting anything.** The cockpit holds no state across runs. The
  registry is configuration, not state.

## Testing

- **The view model is tested without a terminal.** Every pane derives
  its contents from a `PlatformState` through a pure function; those
  functions are unit-tested against hand-built states, including the
  ones that matter most — a project with one degraded source, a project
  declaring only `work:`, a work item whose labels are entirely
  unmapped, and an empty registry.
- **Rendering is tested without a terminal too.** TTUI's `Buffer` is
  inspectable in-process, so a test can render a frame at a fixed size
  and assert on cells. This is how the "a degraded source is visible,
  with its reason" property gets asserted rather than eyeballed.
- **The refresh model gets a test with a deliberately slow adapter.** An
  adapter that sleeps must not delay a tick: the assertion is that the
  UI thread completes N ticks while the refresh thread is still in its
  first poll.
- **Fixture mode gets a determinism test.** Two runs, identical frames.
- **Perceptual tier via Plumb**, against fixture mode, with scenarios
  covering the four tabs and the blocker state.

No test in this crate touches the network, and none needs a TTY.

## Critical files

First-cut inventory. A new workspace member alongside `baseline/` and
`plumb/capture/`:

```
panopticon/
  Cargo.toml            - parallax-panopticon; deps: ttui, parallax-baseline
  src/
    main.rs             - arg parsing, registry load, run
    app.rs              - the App impl: update/view/on_tick, channel drain
    refresh.rs          - the refresh thread and its message type
    keys.rs             - the InputBinder key map and action enum
    fixtures.rs         - fixture-mode adapter construction, frozen clock
    view/
      mod.rs            - pane composition and the frame layout
      rail.rs           - project list and status glyphs
      work.rs           - work items and their projected autonomy
      verification.rs   - check standings
      artifacts.rs      - captures and metric series
      sessions.rs       - session activity
      status.rs         - the freshness footer and the Cloister Bell
  tests/
    view_model.rs       - pure derivations from PlatformState
    determinism.rs      - fixture mode renders identical frames twice
    responsiveness.rs   - a slow adapter never delays a tick
  fixtures/             - a registry plus recorded adapter fixtures
```

## Verification

The spec is satisfied when:

- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D
  warnings`, and `cargo fmt --check` are clean across the workspace.
- Fixture mode renders both real manifests — `manifests/ttui.yaml` and
  `manifests/model-experiments.yaml` — with a frozen clock, and two runs
  produce byte-identical frames.
- A project declaring only `work:` renders a valid reduced view: its
  undeclared panes say so, and neither the pane nor the footer reports a
  degradation for a source that was never declared.
- A failing adapter leaves every other pane populated, and its source
  appears in the footer as unavailable **with its reason**.
- An adapter that blocks for 5 seconds does not delay a tick.
- **A full refresh cycle spawns no process**, asserted with a
  `CommandRunner` that panics if it is called at all: the cadence must
  be provably incapable of running `cargo test`.
- No code path in this crate calls anything in
  `parallax_baseline::actions`, asserted by a test over the crate's own
  source — the read-only claim is checkable, so it gets checked.

## Open questions for sign-off

Each of these is a real fork, not a formality. The design above picks a
default for each so implementation is not blocked on all five, but any
of them can be redirected on review.

1. **Where does the registry live, and in what format?** Default
   proposed: `~/.parallax/registry.yaml` listing roots, with
   `--projects-root <dir>` as the scan alternative. A third option — put
   the registry in this repo as a checked-in file — is arguably more
   honest for a single-operator tool and is not chosen only because it
   couples the cockpit's config to its source tree.
2. **Rail-and-detail, or a grid of project cards?** The sketch above is
   rail-and-detail, which scales to more projects and shows one deeply.
   A grid shows all three shallowly and is closer to "watch development
   live" as a wall display. This is a taste call, and Plumb can measure
   it against a `.plumb/taste.md` once this repo has one.
3. **Do the two baseline gaps land as a baseline spec amendment?**
   Recommended, and the alternative — implementing the factory and
   registry inside Panopticon — is called out above as the thing that
   would fork the manifest's meaning.
4. **Is a 100 ms tick / 10 Hz idle wake acceptable?** It is a laptop on
   battery, occasionally. 250 ms would halve the wakeups and remain
   imperceptible for this data.
5. **Does Panopticon stay a workspace member here, or get its own
   repository?** Here, per the master design ("the cockpit lives in the
   platform repo"). Flagged only because it is the last decision that is
   cheap now and expensive after Arc 1.

## Relationship to TTUI's open work

TTUI's event loop has no external-event injection, and its README
advertises multiplexing that Rev A lists as a non-goal. Both are open
questions in that repo, and **neither blocks this spec** — the refresh
model above is built to be correct without either being resolved. If
they are resolved, Panopticon's `refresh.rs` gets shorter. That is the
right shape for a dependency between two projects moving at the same
time: this one names what it needs, works with what exists, and does not
queue behind a change it does not control.
