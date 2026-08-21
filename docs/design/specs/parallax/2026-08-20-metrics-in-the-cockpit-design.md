# Metrics in the Cockpit

**Status:** proposed — awaiting sign-off. Nothing here is implemented.
**Date:** 2026-08-20
**Size:** one arc.
**Amends:** nothing. It is the first arc of the last component the
README still lists as `sketched`, specified in outline by
`2026-08-14-parallax-platform-design.md` under *Visualizing
Model-Experiments*.

## Why this is the next capability

Everything else has shipped. `plumb` verifies, `baseline` models,
`panopticon` observes and acts, `probe` serves and executes, and control
now crosses the wire to three machines. The README's component table has
one row left that does not say "shipped":

| Component | Status |
|---|---|
| Model-Experiments views | sketched |

Two arguments for taking it now, and the second is the real one.

**It is the only thing left on the roadmap.** The platform spec called
it "a stated priority" and deliberately ordered it *ahead* of full
control. In the event control went first, twice — which is defensible,
because control was blocking two other projects. Nothing is blocking
this one except that it has never been anybody's turn.

**It is the first capability that serves a project other than Parallax
itself.** This is the argument that decides it. Everything shipped so
far is Parallax watching software get built — its own work, TTUI's,
SESH's. Every pane answers a question about *development*: what is in
flight, what is green, what an agent is doing. Model-Experiments is not
a project being developed on the television in the corner; it is a
project whose **output is the point**. Loss curves, probe accuracy,
spectral error. A cockpit that can show three machines' pull requests
and cannot show the numbers the experiments produced is a development
dashboard, not a platform — and the difference between those two things
is exactly this arc.

## Scope: metrics, and nothing else that moves

The platform spec sketches three kinds of artifact and they are not
equally ready. This arc takes the first and only the first.

- **Metrics — JSONL scalar series. In scope.** `baseline` already parses
  them: `ArtifactDetail::Metrics { series: Vec<Series> }`, where
  `Series { name: String, points: Vec<f64> }`, sorted by name. `ttui`
  already has `Sparkline` and `BarChart`. Almost nothing has to be
  invented, which is why this is one arc and not three.
- **Fields and 3D surfaces. Out of scope.** Genuinely interesting, and
  a different problem — `ttui` has `perspective.rs` and `canvas.rs` and
  neither has been pointed at this.
- **Pre-rendered figures. Out of scope**, with a correction recorded
  below: the reason previously written down for deferring them is
  wrong, and the real reason is only that this arc should be small.

## The part that is not a chart

Drawing a sparkline is the easy half and is not what this document is
about. The hard part is that **a metric has two ages, and every screen
this platform has drawn so far has needed only one.**

Every value the cockpit renders today is an *observation*: a thing read
from somewhere at a known moment, aged against the clock of the machine
doing the reading. `Observed<T>`, `Freshness`, "a remote observation is
never `Live`" — the whole model answers one question, *how long ago did
we look?*

A metric answers to a second clock. A loss curve read two seconds ago
from a training run that died an hour ago is **fresh and stalled at the
same time**, and both facts are true and neither is the other. The
observation could not be more current. The data could not be more dead.

A screen that shows only observation freshness reports a live-looking
loss curve for a job that is not running. That is the same class of lie
as `Live` on a value fetched from another machine — and this platform
has already decided, twice, that a lie of that class is worth an arc to
prevent.

So:

**A metrics row shows two ages, and names them differently.** How
current the reading is, and how long since the series last advanced. The
second comes from `Artifact::modified` — the feed's own file
modification time, which `baseline` already carries and which nothing
currently renders.

## The foreign clock

That second age is where this arc gets sharp, because `Artifact::modified`
is a `SystemTime` **stamped by the machine that produced it**.

The wire re-bases observations and does not touch values.
`ObservedWire::receive` computes an age on the probe's clock, subtracts
it from the client's, and passes `self.value` through untouched — which
is right, and is why "clocks are re-based, never compared" holds today.
But `modified` lives *inside* the value. It crosses the wire as a raw
foreign timestamp, and any duration computed from it against the local
clock is precisely the comparison the platform forbids.

**This is latent rather than live.** Nothing renders `modified` today —
it appears in one test fixture and nowhere else — which is why it has
not bitten. A metrics pane is the first thing that would need it, and it
would need it on a Raspberry Pi with no RTC, where a machine that boots
before NTP settles reports mtimes near the epoch. "Last advanced: 56
years ago" is the good failure. The bad one is a peer whose clock is
four minutes fast, reporting a series that advanced in the future, and a
saturating subtraction rendering it as `0s` — a stalled run displayed as
the freshest thing on screen.

The fix belongs in the wire, not in the view. See open question 1.

## What goes on the screen

A `METRICS` pane, per project, listing one row per series:

- the series name, elided against the pane rather than the data;
- a sparkline over the points the row has width for;
- the last value, rendered at a precision the source justifies;
- the count of points, because a sparkline over four points and one
  over four thousand look identical and are not;
- the two ages, distinctly labelled.

### Rendering rules, in the register the rest of this cockpit uses

**A sparkline is a summary and must not imply it is the data.** Fifty
cells cannot hold four thousand points; whatever downsampling happens
is a claim about shape, and the point count beside it is what keeps
that claim honest.

**One point is not a curve.** A series with a single point renders as a
value, not as a flat line — a flat line says "this was measured
repeatedly and did not change", which is a different and much stronger
statement.

**An empty series is not zero.** A feed that parsed but held no points
says so. Zero is a measurement.

**A series that stopped is not a series that is flat.** This is what the
second age is for, and it is the rule the pane exists to keep.

## Non-goals

- **Fields, 3D surfaces, and figure previews.** Later arcs.
- **Colour.** Available — see the correction below — and deliberately
  unused. The cockpit is monochrome today, and an arc that introduced a
  new pane *and* the first colour in the interface would be two arcs
  reviewed as one. The pane should be legible before it is pretty.
- **Ruling on a metric.** Plumb rulings address findings, not numbers.
- **Producing metrics.** The cockpit reads the feed. It does not launch
  the run, and `DispatchAgentRun` already exists for that if wanted.
- **Alerting or thresholds.** "Loss went up" is a judgement, and this
  platform's habit is to show the number and let the operator judge.
  A threshold would need a manifest to declare it, which is a different
  arc with a different open question.

## A correction this arc depends on

`panopticon/src/view/sanitize.rs` says, in its module documentation:

> The escapes are stripped, never interpreted. Turning them into real
> colours would be the same "observed data steers the display" problem
> in a nicer suit — and `ttui` has no per-cell foreground colour to turn
> them into.

**The second clause is false, and I wrote it.** `ttui::buffer::Cell` has
`pub fg: Color` and `pub bg: Color`, `Color` is `crossterm::style::Color`
and therefore includes `Rgb { r, g, b }`, `Buffer::set` is public, and
`terminal.rs` emits `SetForegroundColor(d.cell.fg)` per differing cell.
Per-cell 24-bit colour is fully supported and always was. What is true
is that *panopticon* has never set `fg` anywhere, which is what I
generalised from.

The claim is corrected in this pull request, because the sentence sits
in a module a reader would consult before attempting exactly the work
this document schedules. The first clause — the actual argument, that
observed data must not steer the display — is untouched and still
decides the question: escapes stay stripped.

The consequence for the roadmap is that the half-block figure preview
the platform spec sketched, `▀` with independent foreground and
background at 24-bit colour, is **buildable today and blocked on
nothing**. It is still not in this arc.

## Testing

- **A series with one point does not render as a line.** The rule that
  is easiest to get wrong and hardest to notice.
- **Two ages are distinct.** A fixture whose observation is seconds old
  and whose feed is hours old renders both, and a test asserts the two
  do not collapse to one.
- **A stalled remote series does not read as current.** The foreign
  clock case, with a peer whose recorded `modified` is ahead of the
  client's clock — the case a saturating subtraction silently turns
  into `0s`.
- **A metrics pane over a fixture is captured by Plumb** and judged on
  whether an operator could mistake a stopped run for a running one.
  That is the question the arc is for, so it is the question the
  scenario asks.

## Open questions for sign-off

1. **How does a producer timestamp cross the wire?** `Artifact::modified`
   is a foreign clock inside a value, and the platform's rule is that
   clocks are re-based and never compared.

   *(a)* Re-base it on receipt, inside `ProjectState::receive`, walking
   into artifact values to fix their timestamps. Small, and it makes the
   wire's "values are inert" property no longer true — the next nested
   timestamp will be someone's to remember.

   *(b)* Never send a producer timestamp at all. The probe converts
   `modified` to an **age at observation** — a `Duration` — in the wire
   type, and the client adds it to its own clock. A duration has no
   clock in it and nothing to compare.

   **Recommendation: (b).** It is the same argument that gave
   `ObservedWire` no `freshness()` method: make the dishonest thing
   unrepresentable rather than remembering to correct it. It costs a
   wire type for artifacts, which the last arc deliberately did not
   write — and the reason it gave was that `Artifact` is "inert data".
   This document is the evidence that it is not.

2. **Is Model-Experiments registered anywhere?** This arc renders a feed
   that has to exist. The worked registry lists Parallax, TTUI and SESH;
   Model-Experiments appears in no manifest in this repository. Whoever
   signs this off should say whether the project already declares a
   `parallax.yaml` with a `metrics` artifact feed, or whether writing
   one is the first slice of the plan.
   **Recommendation: assume it is the first slice**, and have that slice
   produce a real manifest against the real directory rather than a
   fixture, so the arc is exercised against a messy producer the way the
   platform spec intended.

3. **Which series, when there are many?** A training run can emit
   dozens. Showing all of them makes a pane that scrolls forever;
   showing some means choosing, and a cockpit that silently picks is a
   cockpit that hides. **Recommendation: show all, sorted by name,
   scrollable like every other pane** — and revisit only when a real
   feed makes it unusable, which is the same answer this project gave
   for the ledger bound.
