# Panopticon (Observe) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> `superpowers:subagent-driven-development` or
> `superpowers:executing-plans`. Steps use checkbox (`- [ ]`) syntax.
>
> **Structure note:** organized as **Arcs → Slices → Tasks** per
> `docs/design/README.md`.

**Goal:** Build the read-only cockpit — a TUI over `parallax-baseline`
showing work in flight, verification standing, artifacts, sessions, and
the age of every source across every registered project — without
blocking its own event loop and without running a build on a timer.

**Spec:**
`docs/design/specs/panopticon/2026-08-18-panopticon-observe-design.md`,
approved and merged 2026-08-19 with its defaults intact.

**Architecture:** Five arcs, each its own PR, ordered so nothing is
built before the thing it displays. Arc 1 is the view model — pure
functions from `PlatformState` to what a pane shows, with no terminal
anywhere. Arc 2 renders those into a `Buffer`, still with no terminal.
Arc 3 adds the event loop, the key map, and the refresh thread. Arc 4
makes a run reproducible so Plumb can judge it. Arc 5 is the Cloister
Bell and close-out.

The first two arcs are testable without a TTY because TTUI's `Buffer` is
inspectable in-process — the same property that lets TTUI test its own
widgets. That is why rendering comes before the event loop rather than
after it.

**Tech Stack:** Rust (stable, 2021 edition), **`ttui = "1"` from
crates.io** — never a path dependency, since being an external consumer
is half the point — `parallax-baseline` by path, `std::sync::mpsc` and
`std::thread` for the refresh seam. No async runtime. `tempfile` as a
dev-dependency.

---

## Global Constraints

**Read-only, and checkably so.** No code path in this crate calls
anything in `parallax_baseline::actions`. Task 16 asserts it over the
crate's own source rather than trusting review.

**Nothing blocks the UI thread. Ever.** Not a poll, not a scan, not a
build. Every adapter call happens on the refresh thread, including the
operator-initiated ones — `c` and `C` *signal*, they do not run.

**The refresh cycle polls only sources that read.** Verification
adapters reporting `CheckCost::Execute` are excluded from the cadence
and run only when asked. Task 12 asserts this with a `CommandRunner`
that panics if it is called at all, so the cadence is provably incapable
of running `cargo test`.

**No wall clock in any test.** Every `now` is injected; fixture mode
freezes one.

**No network in any test.** Work goes through `FixtureTransport`.

**Soft ceiling of 500 lines per file**, tests included. `view/` is split
by pane from the start for this reason.

**Every `pub` item documented.** `#![warn(missing_docs)]` plus CI's
`-D warnings`.

---

## File Structure

```
panopticon/
  Cargo.toml               — parallax-panopticon; ttui = "1", parallax-baseline
  src/
    main.rs                — args, registry load, run
    app.rs                 — the App impl: update / view / on_tick
    keys.rs                — InputBinder key map and the Action enum
    refresh.rs             — the refresh thread, its requests and responses
    fixtures.rs            — fixture-mode adapters and the frozen clock
    view/
      mod.rs               — frame layout and pane composition
      model.rs             — pure PlatformState -> pane data
      rail.rs              — project list and status glyphs
      work.rs              — work items and projected autonomy
      verification.rs      — check standings
      artifacts.rs         — captures and metric series
      sessions.rs          — session activity
      status.rs            — the freshness footer and the Cloister Bell
  tests/
    view_model.rs          — derivations from hand-built PlatformState
    rendering.rs           — frames rendered into a Buffer, cells asserted
    responsiveness.rs      — a slow adapter never delays a tick
    read_only.rs           — nothing calls parallax_baseline::actions
    determinism.rs         — fixture mode renders identical frames twice
  fixtures/
    registry.yaml, ttui/, model-experiments/, github/, plumb/, metrics/
```

---

## Milestones

- **End of Arc 1** — every pane's contents derive from a
  `PlatformState`, tested against hand-built states including the ones
  that matter: a degraded source, a work-only project, an item whose
  labels are all unmapped, an empty registry.
- **End of Arc 2** — a full frame renders into a `Buffer` at a fixed
  size, and the properties that make the screen trustworthy — every
  source's age visible, a degraded source visible *with its reason* —
  are asserted on cells rather than eyeballed.
- **End of Arc 3** — it runs. Keys move, the refresh thread feeds it,
  and the two guarantees hold: a slow adapter never delays a tick, and a
  refresh spawns no process.
- **End of Arc 4** — `panopticon --fixtures <dir>` renders the same
  frames twice, which is what makes a Plumb NO-GO mean "the layout is
  wrong" rather than "time passed".
- **End of Arc 5** — the bell rings on transition into a blocker state,
  the read-only claim is asserted, and the README says how to run it.

---

## Arc 1: The view model

Pure data in, pure data out. Nothing in this arc knows a terminal
exists, which is what makes the interesting cases cheap to test.

### Slice 1.1: The crate

**Tags:** admin, git-adjacent

#### Task 1: Add the `panopticon` workspace member

**Files:**
- Modify: `Cargo.toml` (workspace `members`)
- Create: `panopticon/Cargo.toml`, `panopticon/src/main.rs`

**TDD exception: pure scaffolding**, verified by building.

- [ ] **Step 1: Add the member and the manifest**

```toml
[package]
name = "parallax-panopticon"
version = "0.1.0"
edition = "2021"
description = "The Parallax cockpit: a read-only TUI over parallax-baseline."

[[bin]]
name = "panopticon"
path = "src/main.rs"

[dependencies]
ttui = "1"
parallax-baseline = { path = "../baseline" }

[dev-dependencies]
tempfile = "3"
```

`ttui` comes from crates.io deliberately. A path dependency would make
this an in-repo example wearing a crate's clothes, and the API pressure
would evaporate.

- [ ] **Step 2: A `main.rs` that compiles and does nothing yet**

- [ ] **Step 3: Verify**

Run: `cargo build --workspace`, `cargo clippy --workspace --all-targets
-- -D warnings`. Expected: clean, and `ttui v1.0.0` resolved from
crates.io in the lockfile.

- [ ] **Step 4: Commit**

```bash
git commit -m "chore(panopticon): add the cockpit workspace member

ttui comes from crates.io rather than by path: being a genuine external
consumer is what applies the API pressure an in-repo example cannot."
```

---

### Slice 1.2: What a pane shows

**Tags:** coding

#### Task 2: Project status and the rail

**Files:**
- Create: `panopticon/src/view/mod.rs`, `panopticon/src/view/model.rs`

**Interfaces:**
- Consumes: `parallax_baseline::state::{PlatformState, ProjectState}`,
  `freshness::Freshness`, `adapters::verification::VerificationOutcome`.
- Produces:
  - `pub enum Health { Ok, Pending, Broken }`
  - `pub struct RailRow { pub name: String, pub language: Option<String>, pub health: Health }`
  - `pub fn rail_rows(state: &PlatformState, now: SystemTime) -> Vec<RailRow>`

**Health is the worst of what a project knows about itself**, over two
inputs: the stalest source, and the verification outcomes. `Broken` for
a failed check or an unavailable source, `Pending` for stale or a
still-running check, `Ok` otherwise. A project with nothing declared is
`Ok` — it has no bad news, and inventing some would be a lie.

- [ ] **Step 1: Write the failing tests**

One per rule, against hand-built `ProjectState`s: all fresh and passing
is `Ok`; one stale polled source is `Pending`; a `Fail` outcome is
`Broken`; a degradation is `Broken`; a `Hold` is `Broken` (a hold is
never upgraded, and the cockpit is not where that gets softened); an
empty `PlatformState` yields no rows; rows follow
`PlatformState::projects` order.

- [ ] **Step 2: Run to verify they fail, then implement**

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(view): derive each project's health for the rail"
```

---

#### Task 3: The four detail panes

**Files:**
- Create: `panopticon/src/view/{work,verification,artifacts,sessions}.rs`

**Interfaces:**
- Produces, one per pane, all pure:
  - `pub struct WorkRow { pub number: u64, pub kind: char, pub state: &'static str, pub implement: &'static str, pub merge: &'static str, pub readiness: &'static str, pub checks: String, pub title: String }` and `pub fn work_rows(&ProjectState) -> Vec<WorkRow>`
  - `pub struct VerificationRow { pub kind: String, pub standing: Standing, pub detail: Option<String> }`, `pub enum Standing { Pass, Fail, Hold, NotRun, NotRunThisSession }`, and `pub fn verification_rows(&ProjectState, ran: &BTreeSet<String>) -> Vec<VerificationRow>`
  - `pub struct ArtifactRow { … }` and `pub fn artifact_rows(&ProjectState) -> Vec<ArtifactRow>`
  - `pub struct SessionRow { pub name: String, pub active: bool, pub idle_for: Duration }` and `pub fn session_rows(&ProjectState, now: SystemTime) -> Vec<SessionRow>`

**`NotRunThisSession` is the spec's rule, made a type.** A command check
the operator has not asked for shows *not run this session*, never a
stale green — a check whose last result predates the code on disk looks
like an answer and is not one. The `ran` set is the projects whose
command checks have run since the cockpit started.

**An undeclared family is not an empty one.** A project that declares no
`sessions:` renders "not declared", distinct from "declared, and there
are none". Both are true statements and only one is honest.

- [ ] **Step 1: Write the failing tests**

Including, specifically: an item whose labels are all unmapped renders
`—` on every axis rather than a default; `unmapped_labels` is surfaced
rather than dropped; a `NotRun` from baseline (Plumb never ran) is
distinct from `NotRunThisSession` (we have not asked); a metrics
artifact yields its series names; a session past `DEFAULT_IDLE_AFTER` is
inactive.

- [ ] **Step 2: Implement; commit**

```bash
git commit -m "feat(view): derive the four detail panes

A command check the operator has not asked for reads 'not run this
session' rather than a stale green: a result that predates the code on
disk looks like an answer and is not one."
```

---

#### Task 4: The footer

**Files:**
- Create: `panopticon/src/view/status.rs`

**Interfaces:**
- Produces: `pub struct SourceCell { pub label: String, pub age: String, pub alarming: bool }` and `pub fn footer(&ProjectState, now: SystemTime) -> Vec<SourceCell>`

**This is the pane that makes the screen trustworthy**, so it renders
`ProjectState::sources(now)` verbatim and adds only formatting: `live`,
`12s`, `45s !`, or the unavailable reason. A degraded source keeps its
reason — truncated, never dropped.

- [ ] **Step 1: Write the failing tests**

`Live` renders `live`; `Fresh` renders whole seconds; `Stale` is marked
alarming; `Unavailable` carries its reason text and is alarming; a
project with no sources yields an empty footer rather than a fabricated
row.

- [ ] **Step 2: Implement; commit**

---

## Arc 2: Rendering

### Slice 2.1: The frame

**Tags:** coding

#### Task 5: Layout and composition

**Files:**
- Modify: `panopticon/src/view/mod.rs`

**Interfaces:**
- Produces: `pub struct Frame<'a> { … }` and `pub fn render(frame: &Frame<'_>, area: Rect, buf: &mut Buffer)`

**Layout, per the spec's sketch:** a vertical split into detail and
footer (`Constraint::Fill(1)`, `Constraint::Fixed(3)`), then a
horizontal split of the upper region into rail and detail
(`Constraint::Fixed(18)`, `Constraint::Fill(1)`). `Block::render`
returns the inner `Rect`, which is what each pane draws into.

**Degrade rather than panic on a small terminal.** A frame narrower than
the rail plus a usable detail column drops the rail and renders the
detail alone; below that, it renders a single line saying the terminal
is too small. TTUI's `Layout` clamps rather than panicking, but a
zero-width pane renders nothing at all, which reads as a bug.

- [ ] **Step 1: Write the failing tests**

Render into `Buffer::new(100, 30)` and assert on cells: the footer
occupies the bottom three rows; the rail occupies the left 18 columns;
the selected project's row carries the selection background. Then
`Buffer::new(20, 8)` renders the detail alone, and `Buffer::new(8, 3)`
renders the too-small message.

- [ ] **Step 2: Implement; commit**

---

#### Task 6: The panes

**Files:**
- Modify: `panopticon/src/view/{rail,work,verification,artifacts,sessions,status}.rs`

**Interfaces:** each pane gains `pub fn render(rows: &[Row], area: Rect, selected: usize, buf: &mut Buffer)`.

TTUI's widgets take borrowed slices — `List::new(&[String], selected)`,
`Table::new(&[String], &[Vec<String>], selected, col_width)` — so each
pane builds its owned `Vec`s locally in `render` and lends them. That is
one allocation per pane per frame, at a 100 ms tick, over at most a few
dozen rows.

- [ ] **Step 1: Write the failing tests**

The two properties the spec names, asserted on cells rather than
eyeballed:

1. **Every source's age is visible.** For a state with four sources, all
   four labels appear in the footer region.
2. **A degraded source is visible with its reason.** Aggregate a state
   whose work adapter failed with `rate limit exceeded`, render, and
   assert the rendered text contains `rate limit`.

Plus: a work item with unmapped labels renders `—`; a `NotRunThisSession`
check renders text containing `not run`; a metrics artifact renders a
sparkline row whose cells are non-blank.

- [ ] **Step 2: Implement; commit**

```bash
git commit -m "feat(view): render the rail, the four panes, and the footer

The two assertions that matter are on cells: every source's age is on
screen, and a degraded source is on screen with its reason. A cockpit
that quietly drops either is worse than no cockpit."
```

---

## Arc 3: The event loop

### Slice 3.1: Keys

**Tags:** coding

#### Task 7: The key map

**Files:**
- Create: `panopticon/src/keys.rs`

**Interfaces:**
- Produces: `pub enum Action { Up, Down, NextPane, Tab(u8), Refresh, RunChecks, RunAllChecks, Help, Quit }` and `pub fn binder() -> InputBinder<Action>`

Bindings, per the spec: `j`/`k`, `Tab`, `1`–`4`, `r`, `c`, `C`, `?`,
`q`. Nothing else — every remaining verb belongs to sub-project #5, and
binding it now means binding it twice.

`InputBinder<A: Copy>` resolves single keys and chords against an
app-defined action type, so chords cost nothing to add later.

- [ ] **Step 1: Write the failing tests**

Feed `Event::Key` values and assert the action; assert `c` and `C` are
distinct (a shifted key is a different `KeyPress`); assert an unbound
key yields `None`.

- [ ] **Step 2: Implement; commit**

---

### Slice 3.2: The refresh seam

**Tags:** coding

#### Task 8: The refresh thread

**Files:**
- Create: `panopticon/src/refresh.rs`

**Interfaces:**
- Produces:
  - `pub enum Request { RefreshReads, RunChecks { project: String }, RunAllChecks, Stop }`
  - `pub enum Update { Project(Box<ProjectState>), ChecksRan { project: String, checks: Vec<Observed<VerificationStatus>> }, Failed { project: String, problem: String } }`
  - `pub struct Refresher { … }` with `pub fn spawn(projects: Vec<(Validated, ProjectAdapters)>, config: RefreshConfig) -> Self`, `pub fn request(&self, r: Request)`, `pub fn drain(&self) -> Vec<Update>`, `pub fn stop(self)`

**The whole design is one sentence: the thread owns the adapters and the
UI owns the state.** Nothing is shared, nothing is locked, and no lock is
ever held across a render. `drain` is `try_recv` until empty and returns
immediately.

**Per-project sends, not one batch.** One slow project must not withhold
the others, and a rail that updates row by row is more honest than a
screen that changes all at once.

**The thread splits its own adapters by `CheckCost`.** Reading checks go
in the cadence; executing ones are held back and run only on a
`RunChecks` request. This is the single reason Arc 1 of the registry
plan existed.

- [ ] **Step 1: Write the failing tests**

A refresher over two stub projects sends two `Update::Project` values;
`drain` on an idle refresher returns empty immediately; a `RunChecks`
request produces `ChecksRan` for that project only; an adapter that
errors produces `Update::Failed` naming the project rather than killing
the thread; `stop` joins.

- [ ] **Step 2: Implement; commit**

---

#### Task 9: The `App`

**Files:**
- Create: `panopticon/src/app.rs`

**Interfaces:**
- Produces: `pub struct Panopticon { … }` implementing `ttui::app::App`, with `pub fn new(state: PlatformState, refresher: Refresher) -> Self`.

`tick_rate()` returns `Some(100ms)`. `on_tick` drains the refresher,
replaces the matching entries in its own `PlatformState`, expires the
input binder, and returns. `update` feeds the event to the binder and
applies the action — `Refresh` and `RunChecks` *send a request*; neither
touches an adapter.

**`r` does not fetch.** A key that performs I/O on the UI thread is the
rejected design wearing a different hat.

- [ ] **Step 1: Write the failing tests**

`on_tick` with three queued updates applies all three in one tick; an
update naming an unknown project is ignored rather than appended
(the registry is the source of which projects exist); `q` sets
`should_quit`; `j`/`k` move the selection and clamp at the ends;
`Tab(3)` selects the artifacts pane.

- [ ] **Step 2: Implement; commit**

---

### Slice 3.3: The two guarantees

**Tags:** coding

#### Task 10: A slow adapter never delays a tick

**Files:**
- Create: `panopticon/tests/responsiveness.rs`

Spawn a refresher whose work adapter sleeps 5 seconds, then drive the
app's `on_tick` in a loop with an injected clock and assert it completes
at least 20 ticks before the first update arrives. The assertion is
about the UI thread never waiting, so it measures ticks completed rather
than wall time.

- [ ] **Step 1: Write it; run; commit**

---

#### Task 11: A refresh spawns no process

**Files:**
- Create: `panopticon/tests/refresh_spawns_nothing.rs`

Build a project whose manifest declares two `command` checks, with a
`CommandRunner` that **panics if called**. Drive a full refresh cycle
and assert it completes. Then send `RunChecks` and assert the runner
*is* called — the refusal is a gate, not a wall.

This is the spec's rule made unbreakable: a future task that adds
command checks to the cadence fails this test immediately rather than
quietly running `cargo test` every 30 seconds on the machine running the
agent sessions.

- [ ] **Step 1: Write it; run; commit**

```bash
git commit -m "test(refresh): the cadence is provably incapable of running a build"
```

---

## Arc 4: Fixture mode

### Slice 4.1: A reproducible run

**Tags:** coding

#### Task 12: `--fixtures <dir>` and the frozen clock

**Files:**
- Create: `panopticon/src/fixtures.rs`, `panopticon/fixtures/…`
- Modify: `panopticon/src/main.rs`

**Interfaces:**
- Produces: `pub struct FixtureSet { pub now: SystemTime, pub projects: Vec<(Validated, ProjectAdapters)> }` and `pub fn load(dir: &Path) -> Result<FixtureSet, String>`

The fixture directory holds a registry, two project trees, and recorded
adapter responses. `load` builds every adapter through
`parallax_baseline::adapters::factory::from_manifest_with` with a
`FixtureTransport` and a `ScriptedRunner` — the *same* translation
production uses, which is the reason that signature takes factories.

`now` is declared by the fixture set (`now: 1700000000` in its
`registry.yaml`, or a sibling `clock.txt`) rather than sampled, so ages
render identically on every run.

- [ ] **Step 1: Write the fixtures** (TDD exception: data)
- [ ] **Step 2: Write the failing test, implement, commit**

---

#### Task 13: Determinism

**Files:**
- Create: `panopticon/tests/determinism.rs`

Render a full frame from a loaded `FixtureSet` twice into two
`Buffer`s and assert every cell is equal — symbol, fg, bg, style. Then
render at a second size and assert the same property, so determinism is
not an accident of one geometry.

**If this fails, the cause is a `SystemTime::now()` that escaped**, and
the fix is to find it rather than to loosen the assertion.

- [ ] **Step 1: Write it; run; commit**

```bash
git commit -m "test(fixtures): two runs, identical frames

Determinism is what makes a Plumb NO-GO mean the layout is wrong rather
than that time passed."
```

---

## Arc 5: The bell, and close-out

### Slice 5.1: Cloister Bell

**Tags:** coding

#### Task 14: Ring on transition, not on state

**Files:**
- Modify: `panopticon/src/view/status.rs`, `panopticon/src/app.rs`

**Interfaces:**
- Produces: `pub struct Bell { … }` with `pub fn observe(&mut self, state: &PlatformState) -> bool` and `pub fn ringing(&self, now: SystemTime) -> bool`

Rings when a project **enters** a blocker state — a check becoming
`Fail`, a Plumb NO-GO landing, a source becoming unavailable — not
continuously while one holds. A bell that rings every tick is a bell
nobody hears.

Presentation is the footer's state plus the rail glyph; **never a modal,
never anything that swallows a keystroke.**

- [ ] **Step 1: Write the failing tests**

A first observation of an already-broken project does **not** ring (the
cockpit just started; that is not news); a transition from `Ok` to
`Broken` rings; holding `Broken` across ten observations rings once;
recovering and breaking again rings again; the bell decays after a fixed
duration against an injected clock.

- [ ] **Step 2: Implement; commit**

---

### Slice 5.2: Close-out

**Tags:** coding, admin

#### Task 15: Wire `main.rs`

**Files:**
- Modify: `panopticon/src/main.rs`

Args: `--registry <file>`, `--projects-root <dir>`, `--fixtures <dir>`,
`--help`. With none of them and nothing found, the cockpit renders an
**empty state naming what it looked for** rather than exiting — issue
#21 means the honest answer today is often "no projects", and a cockpit
that vanishes on that teaches nothing.

`main` is the one place `SystemTime::now()` is sampled outside fixture
mode.

- [ ] **Step 1: Implement; run it by hand against `--projects-root`; commit**

---

#### Task 16: The read-only assertion, and the README

**Files:**
- Create: `panopticon/tests/read_only.rs`, `panopticon/README.md`

The test walks `panopticon/src/**/*.rs` and asserts no file mentions
`actions::` or `parallax_baseline::actions`. Crude, and exactly
proportionate: the claim is "this sub-project cannot mutate anything",
the enforcement is that the module it would have to reach for is never
named, and a grep proves that in a way a reviewer's memory does not.

README: what it is, how to run it, the key map, and the two rules a
reader needs — that the cadence never runs a build, and that fixture
mode is how it is demonstrated and judged.

- [ ] **Step 1: Write both; run every gate; commit**

```bash
cargo build && cargo test && cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo doc -p parallax-panopticon --no-deps
```

---

## Spec coverage

| Spec section | Tasks |
|---|---|
| The four questions | 2, 3 |
| Layout: rail, detail, footer | 5, 6 |
| Key map | 7, 9 |
| Refresh thread, drained on tick | 8, 9 |
| The refresh cycle polls only readers | 8, 11 |
| `c` / `C` operator-initiated | 7, 8, 9 |
| "not run this session" | 3, 6 |
| Fixture mode, frozen clock | 12 |
| Determinism: two runs, identical frames | 13 |
| Cloister Bell on transition | 14 |
| Read-only, asserted | 16 |
| Every source's age visible | 4, 6 |
| A degraded source visible with its reason | 4, 6 |
| A slow adapter never delays a tick | 10 |
| A work-only project renders a reduced view | 3, 6 |

Non-goals stay non-goals: no control action, no PNG rendering, no
perspective-projected fields, no daemon, no persistence, and no change
to TTUI — a finding there is filed as a TTUI issue with this crate's use
case attached, which is the whole value of being an external consumer.

---

## Judgment calls made while planning

1. **`c` / `C` run on the refresh thread, not the UI thread.** The spec
   makes command checks operator-initiated and does not say where they
   execute. Running `cargo test` synchronously in `update` would freeze
   the cockpit for minutes — the rejected design, arriving through a
   different door. Decided (Task 8): they are requests like any other,
   and the pane shows the check as running until its result lands.

2. **Health is the worst of freshness and outcome, and a project with
   nothing declared is `Ok`.** The spec describes the glyph and not its
   derivation. The alternative — a fourth "unknown" state for a project
   with no declared sources — adds a colour to the rail that means
   "this project told us nothing", which is already visible as an empty
   detail pane.

3. **An update naming an unknown project is dropped.** The registry is
   the source of which projects exist; a refresher that invented one
   would put a row on screen that no manifest backs.

4. **The read-only assertion is a grep over source text.** Crude, and it
   catches the thing that would actually happen — someone reaching for
   `actions::` — where a type-level guarantee would need a facade crate
   for the same result.

5. **Empty state rather than exit when no projects are found.** Given
   issue #21, "no projects" is the *common* case today, and a cockpit
   that exits on it teaches nothing about why.

---

## Execution handoff

Everything executes in `<projects-root>/Parallax` on a worktree branch
per `git-github-standards.md`, one Gated PR per Arc, squash-merged.

Three notes:

- **Arcs 1 and 2 need no threads and no terminal.** If anything in them
  reaches for `SystemTime::now()` or a `Terminal`, that is a mistake in
  the task, not a necessity.
- **Task 11 is a tripwire.** Its value is that it fails loudly the day
  someone adds command checks to the cadence.
- **Nothing here depends on issue #21 being resolved.** Fixture mode is
  in the spec precisely so the cockpit can be built, demonstrated, and
  judged before a single project is registered on a given machine.
