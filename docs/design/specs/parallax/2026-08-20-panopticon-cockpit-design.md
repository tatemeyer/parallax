# Panopticon — Cockpit: observe

**Status:** approved by standing directive, built same session. Design
calls in this document are the agent's, made under an explicit
instruction to decide rather than ask; they are the first thing to
review.

Sub-project **#3** of the Parallax platform. Depends on **#2**
(`parallax-baseline`, complete through Arc 5) and the `ttui` crate
(2.0.0, published).

## Goal

A read-only TUI over `parallax-baseline`: every registered project's
work in flight, verification standing, artifacts, session activity,
autonomy distribution, and per-source freshness — on one screen.

**Read-only is load-bearing.** Control actions are sub-project #5. This
renders state and never mutates it. No action key, no confirmation
prompt, nothing that writes.

## Why this shape

The platform design says the cockpit "lands before any control surface
because control without observation is not useful, and because this is
where the 'watch development' value actually sits."

Baseline already answers every question the screen asks. `ProjectState`
carries `work`, `verification`, `artifacts`, `sessions`, `autonomy`,
`unmapped_labels`, `degradations`; `sources(now)` returns a uniform
`Vec<SourceStatus>`; `stalest(now)` and `degraded()` are precomputed.
The frontend's job is to *lay this out*, not to re-derive it.

## The three states the UI must keep distinct

Baseline preserves these deliberately, and the cockpit is the reason:

| State | In `ProjectState` | Renders as |
|---|---|---|
| **not declared** | field `None`/empty **and** no `sources()` row | `—` (dim) |
| **fetch failed** | `sources()` row with `Freshness::Unavailable`, plus a `degradations` entry | `!` (alert colour) |
| **fetched, empty** | field present, ordinary freshness, no degradation | `0` (normal) |

Collapsing any two would make a project with **no CI configured** look
identical to one whose **CI is down**. That distinction is most of the
value of a status screen, so it is a hard requirement, not a nicety.

## Screens

Two, plus an overlay. More would be scope; fewer would not fit.

### 1. Overview — the project table

One row per registered project. Columns, left to right:

```
PROJECT        WORK  CHECKS  ARTIFACTS  SESSIONS  AUTONOMY   OLDEST SOURCE
ttui             12   4/4          37         2  8a/3g/1h   verification:fmt 4m
model-exp         5   —             9         —  2s/1r      work:github 22m
```

Six narrow fixed columns and one wide one. That is exactly the layout
that could not be expressed before `ttui` 2.0 — `Table::new` took a
single `col_width`, so either the counts wasted space or the source
label was truncated to nothing (ttui#170). This screen is the reason
that issue was filed, and it is now expressible:

```rust
.widths(&[
    Constraint::Fixed(14),  // PROJECT
    Constraint::Fixed(6),   // WORK
    Constraint::Fixed(8),   // CHECKS
    Constraint::Fixed(11),  // ARTIFACTS
    Constraint::Fixed(10),  // SESSIONS
    Constraint::Fixed(12),  // AUTONOMY
    Constraint::Fill(1),    // OLDEST SOURCE — takes the rest
])
.spacing(1)
```

`AUTONOMY` is the distribution across that project's work items,
abbreviated by axis value. A count is more useful than a percentage at
this width, and it does not lie about precision when *n* is 3.

`OLDEST SOURCE` is `stalest(now)` — one number that answers "how much of
this screen should I believe."

### 2. Detail — one project

Selected with `Enter`. Sections, top to bottom:

- **Sources** — every `SourceStatus`, label and freshness, worst first.
  A degraded source stays in the list as `Unavailable`; it never
  disappears, because a source that vanished when it broke would be the
  worst possible behaviour for a monitoring screen.
- **Work in flight** — items from the `work` snapshot with their
  projected autonomy: `implement` / `merge` / `readiness`, with `—` for
  *no claim*. Never a default; a `—` is information.
- **Verification** — each check's standing.
- **Artifacts** and **Sessions** — counts and most recent.
- **Unmapped labels**, when any. Not an error — a prompt to extend the
  map, or evidence a label is doing nothing.

### 3. Cloister Bell — the blocker overlay

The platform design names it: *"rings only for impending catastrophe,
which is the correct frequency for a blocker alert."*

Here that means **degraded sources**, and nothing else. If any project
has a `degradations` entry, a banner shows the count and the worst
offender. It is dismissible with `Esc` and reappears on the next change
of degradation set — not on every refresh, or it becomes wallpaper and
stops meaning anything.

## Keys

Read-only, so the whole map is navigation:

```
Up / Down     select project
Enter         open detail
Esc           back, or dismiss the bell
r             refresh now
q             quit
```

## Data flow, and why it works offline

```
manifests/*.yaml → parse → validate → adapters → aggregate → PlatformState → render
```

The cockpit runs the **real** adapters. When GitHub is unreachable, or a
verification command cannot spawn, that source becomes
`Freshness::Unavailable` and lands in `degradations` — and the screen
**shows** it.

That is the design working, not a fallback: a cockpit that only renders
when everything is reachable is useless precisely when you need it. It
therefore runs with no network, no token, and no configuration, and
tells the truth about what it could not see.

Refresh is manual (`r`) rather than on a timer. An automatic poll would
hammer GitHub's rate limit for a screen a human looks at intermittently,
and `Observed`'s ETag-conditional support already makes a manual refresh
cheap.

## What this deliberately is not

- **Not control.** Sub-project #5.
- **Not Model-Experiments visualization.** Sub-project #4.
- **Not a log viewer.** Sessions are counted and dated, not streamed.
- **Not configurable.** No themes, no layouts, no preferences. A second
  screenful of settings for a tool with two screens is not worth it.

## Testing

The renderers are pure functions from `&ProjectState` to a `Buffer`, so
they test without a TTY, exactly as Baseline's adapters test without a
network:

- The three states each render distinctly — the hard requirement above,
  asserted per column.
- A partial manifest renders as a normal row with `—`s, never as an
  error.
- Autonomy `—` (no claim) is distinguishable from a concrete value.
- The table survives a narrow terminal without panicking — `Fill(1)`
  collapsing to near-zero is the interesting case.
- The bell appears with degradations and stays gone once dismissed until
  the degradation set changes.

## Risks

1. **Live adapters make the first run slow or noisy.** Mitigated by the
   offline path above: unreachable is a rendered state, not an error.
2. **`Fill(1)` at a narrow width** is where `ttui`'s truncation and the
   ellipsis get exercised for real. This is the first external consumer
   of that code path; a defect here is a `ttui` finding.
3. **Autonomy abbreviations** (`8a/3g/1h`) are terse. They are legible
   next to the header once, and the detail screen spells them out.
