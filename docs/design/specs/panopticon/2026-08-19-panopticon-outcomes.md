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

## What the perceptual tier found (run `20260820T020000Z`)

The cockpit was captured and put through three blinded lenses —
`breakage`, `intent`, and `motion`, each seeing only the image and the
run manifest. Verdict: **GO**, six findings, no blockers. All six were
fixed rather than filed, and the four distinct defects behind them are
worth recording because of what found them.

**`readiness` could not say "nothing claimed".** The intent lens counted
two em dashes in a row whose declared intent said three, and asked
whether the third column was blank or whether `verifiable` *was* the
third column. It was the latter: `Autonomy::readiness` was a
`Readiness`, not an `Option<Readiness>`, and `resolve` finished with
`unwrap_or_default()`. Two axes obeyed the honest-state rule; the third
quietly asserted that "done" was defined for every work item nobody had
said anything about — including all four of TTUI's, on screen, in the
demo. The master design's own table
(`../parallax/2026-08-14-parallax-platform-design.md`) said `verifiable`
on six rows that declare no readiness, and the code was faithful to it,
and the unit tests asserted the table. One of them was even named
`an_item_whose_labels_are_all_unmapped_renders_dashes` and asserted
`readiness == "verifiable"` with the comment *"readiness always lands
somewhere"*. The spec, the type, the render and the test all agreed with
each other and none of them agreed with the design rule they were there
to enforce. What broke the loop was that the lens could not read any of
them.

**Titles were clipped with no marker.** Flagged independently by
`breakage` and `intent`: `#141`'s title ended `…do with a singl`, flush
against the border, indistinguishable from a title that simply ended
there. The footer had solved exactly this problem — it reserves room to
say `(+3)` rather than hide that it hid anything — and the detail pane
was not doing it. Now every over-wide line ends in `...`; ASCII
deliberately, because U+2026 has no glyph in the rasterizer the
perceptual tier captures through, and a cockpit that cannot be captured
cannot be judged.

**The blocker banner was not attributable.** `motion` noticed a
`** BLOCKER **` raised over `sesh`'s sources while `sesh` was selected,
healthy, and showing nothing in flight — the fire was `ttui`'s. The bell
rings for the platform; the box below it holds one project's sources.
The banner now names the projects: `** BLOCKER: ttui **`, and falls back
to the bare word only when the bell is still ringing after every project
has recovered, which it can be, by design.

**A quarter of the capture was a duplicate frame.** `breakage` and
`motion` both observed frames 3 and 4 to be pixel-identical: the script
ended on a `wait_ms` after the screen had already settled. The last step
is now a keypress that moves the selection, so every frame in the sheet
differs from the one before it. A scenario that spends a frame on
nothing is paying a lens to look at nothing.

The tier justified itself here. Three of the four are things a person
would notice in a screenshot and no test would ever assert, and the
fourth is a design defect that four layers of the stack agreed on
because they were all reading each other.

## What is deliberately still missing

- **No control.** Nothing here mutates anything, asserted by
  `tests/read_only.rs`. That is sub-project #5.
- **No figure previews or 3D surfaces.** Metric series render as text
  today; the artifact pane names them. That is sub-project #4.
- **No `design` lens.** Parallax has no `.plumb/taste.md`, so the lens
  that judges against a declared aesthetic is skipped every run. That is
  a taste call and not one this document can make.
- **No help overlay.** `?` toggles a flag nothing renders yet.
