# Panopticon — Outcomes

**Status:** closed. Sub-project #3 is implemented and on `main`.
**Date:** 2026-08-19

Records what the design
(`2026-08-18-panopticon-observe-design.md`) and its plan
(`../../plans/panopticon/2026-08-19-panopticon-observe-plan.md`) turned
into: which open questions the implementation answered, where the design
was wrong, and what building it cost elsewhere. Kept because the plan's
code blocks were meant to be transcribed and a reader needs to know
which ones moved.

## What shipped

| Arc | PR | |
|---|---|---|
| 1 | #23 | the crate and its view model |
| 2 | #26 | rendering into a `Buffer` |
| 3 | #28 | event loop, key map, refresh thread |
| 4 | #29 | fixture mode and determinism |
| 5 | #30 | Cloister Bell, wiring, read-only proof |

`panopticon --projects-root <dir>` runs against real repositories,
`--fixtures <dir>` renders recorded state with a frozen clock, and the
whole crate outside `main.rs` is testable without a terminal.

## The five open questions, answered

1. **Where the registry lives, and in what format.** Settled as
   proposed: the library takes a path and never consults the
   environment, and the frontend chooses. `--registry <file>` and
   `--projects-root <dir>` both exist; nothing defaults to `~`.
2. **Rail-and-detail, or a grid of project cards.** Rail-and-detail,
   as proposed. Two projects fit either way; the question reopens the
   day there are eight.
3. **Whether the baseline gaps land as an amendment.** Yes — spec #11,
   plan #17, arcs #18/#19/#20. Panopticon consumes the registry and the
   factory rather than reimplementing either.
4. **Whether a 100 ms tick is acceptable.** Kept. `app::TICK` is
   100 ms and the refresh cadence is `DEFAULT_POLL_INTERVAL`.
5. **Workspace member or its own repository.** Workspace member, as
   the master design says.

## Where the design was wrong

**`ProjectAdapters` could not cross a thread boundary.** The refresh
model — a worker thread that owns the adapters — was unwriteable as
specified, because the trait objects carried no `Send` bound. The spec
had reasoned carefully about *why* the UI thread must not poll and not
at all about whether the core allowed the alternative. Fixed in #27,
about ninety seconds into Arc 3.

**`Table` is unusable for the work pane.** The plan named TTUI's
`Table` for tabular data. It applies one `col_width` to every column, so
a row holding `#165` and a ninety-character title must choose between a
four-cell title and four ninety-cell columns. Rows are composed into
padded lines and rendered through `List` instead; filed upstream as
tatemeyer/ttui#170, which is the first finding TTUI has had from a
consumer that cannot see inside it.

**A panicking `CommandRunner` would not have worked.** Task 11 called
for one that panics if called, to prove the cadence runs no build. A
panic on the refresh thread unwinds that thread and is invisible to the
test; it counts through an `AtomicUsize` instead. Verified by breaking
the cost split deliberately: the test fails with `left: 10, right: 0`.

**Fixture mode needed the cost split too.** Aggregating outside the
refresh thread ran the build checks, so the demo showed `cargo test`
passing in a run that never invoked it. `refresh::split_by_cost` is
public for that reason — a second answer to "which checks are safe to
poll" is how a cockpit ends up running a build on a timer.

**Nothing read a GitHub token from the environment.** The library
deliberately never does, which the spec said; nobody was doing it
instead. A private repository degraded to a 404 and the bell rang for a
project the cockpit had never been allowed to see (#32).

## What building it cost elsewhere

Two Plumb defects, both found by capturing the cockpit — the first
thing outside TTUI it has been pointed at (#31):

- **An em dash hard-failed the capture.** `font8x8` has no table for
  U+2013/U+2014, so any interface using one was uncapturable. They now
  render as a centred bar.
- **A pty scenario could not use relative paths.** The adapter never
  set the child's working directory, so the cockpit launched, failed to
  find its fixtures, and printed its usage into the capture.

One TTUI issue (tatemeyer/ttui#170), and manifests in three
repositories (tatemeyer/ttui#168, tatemeyer/SESH#21,
tatemeyer/Model-Experiments#103) now that manifests live in the projects
they describe.

## What the honest-state rule caught

The spec's insistence that the cockpit never show a comforting default
turned into four separate catches, which is worth recording because it
is the only design rule here that paid for itself repeatedly:

1. An autonomy axis nothing claims renders `—`, not a default — which
   is every TTUI row today, and visible on screen as
   `unmapped: semver:patch`.
2. A build check nobody asked for reads *not run this session*, not a
   stale green.
3. When the footer cannot fit every source it says how many it hid —
   and writing that test found the count being clipped off the end of
   the line it exists to warn about.
4. The first bell observation never rings, because a cockpit starting
   up in front of a fire is not news.

## What is deliberately still missing

- **No control.** Nothing here mutates anything, asserted by
  `tests/read_only.rs`. That is sub-project #5.
- **No figure previews or 3D surfaces.** Metric series render as text
  today; the artifact pane names them. That is sub-project #4.
- **No Plumb verdict on the cockpit itself.** The capture works and the
  scenario is committed at `.plumb/config.yaml`; dispatching the four
  blinded lenses is a separate, operator-initiated act.
- **No help overlay.** `?` toggles a flag nothing renders yet.
