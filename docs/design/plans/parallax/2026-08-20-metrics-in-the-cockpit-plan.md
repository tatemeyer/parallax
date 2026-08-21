# Metrics in the Cockpit — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> `superpowers:subagent-driven-development` or
> `superpowers:executing-plans`. Steps use checkbox (`- [ ]`) syntax.
>
> **Structure note:** organized as **Arcs → Slices → Tasks** per
> `docs/design/README.md`.

**Goal:** An operator watching the cockpit can read what an experiment
concluded — including when it concluded *nothing*, which is the harder
and more common case — without opening the repository that produced it.

**Spec:**
`docs/design/specs/parallax/2026-08-20-metrics-in-the-cockpit-design.md`.

**Tech Stack:** Rust (stable, 2021). No new dependencies. No async
runtime. `ttui` already has `Sparkline` and `BarChart`; `baseline`
already has `MetricsArtifactAdapter`.

---

## What changed between the spec and this plan

The spec was written against the *idea* of a metrics feed. This plan was
written after reading the one that exists, and three of its assumptions
did not survive. They are recorded here rather than quietly fixed,
because two of them are load-bearing and one of them changes what the
arc builds.

### 1. Model-Experiments is registered. Open question 2 is answered — and its recommendation was wrong.

The spec asks whether the project "already declares a `parallax.yaml`
with a `metrics` artifact feed, or whether writing one is the first
slice", and recommends assuming the latter.

It declares one. `Model-Experiments@main:parallax.yaml`:

```yaml
artifacts:
  - kind: figure
    watch: projects/*/results/**/*.png
  - kind: metrics
    adapter: jsonl
    watch: projects/*/results/**/*.jsonl
```

**But nothing backs it.** A code search for `jsonl` across that
repository returns no hits, there is no `results/` directory in the
tree, and no code writes one. The feed is empty by construction — which
is the exact failure its own manifest names, three lines further down,
as the reason it declines to declare a `perceptual` check:

> a declared check nothing backs is worse than an absent one

So the first slice is not "write a manifest". It is **decide what the
feed points at**, and that decision is in this plan rather than assumed.

### 2. The real data is long-format. `parse_metrics` renders it wrong, silently.

Arc 1's durable record is `projects/jepa/results.csv` — checked in, 318
rows, one row per measurement:

```
issue,experiment_slug,variant,seed,metric,value,params,date
69,001-baseline-collapse-avoidance,full,0,effective_rank,2.779,{...},2026-08-03
```

`parse_metrics` assumes the **wide** shape: one record per timestep,
each numeric field a metric. Fed the shape that exists, the shipping
parser produces this — run, not predicted:

```
series issue    n=12  points=[69.0, 69.0, 69.0, ...]
series seed     n=12  points=[0.0, 1.0, 2.0, 0.0, 1.0, 2.0, ...]
series value    n=12  points=[2.779, 2.352, 2.791, 1.389, 1.459, 1.25,
                              2.934, 2.461, 2.437, 0.4951, 0.5311, 0.4933]
```

Three failures, none of which announce themselves:

- **`issue` and `seed` are charted.** They are identifiers. The pane
  would draw a sparkline of an issue number and a sawtooth of a seed
  index, indistinguishable from measurements.
- **`value` concatenates unrelated metrics.** `effective_rank`
  (1.25–2.93) and `embedding_std` (0.49–0.53) land in one series on one
  axis. It renders as a metric that fell off a cliff at point 9. There
  is no cliff.
- **`variant` and `metric` are dropped**, because they are strings —
  and they are the two columns the finding actually lives in.

The parser is not careless; its contract says "non-numeric fields and
unparseable lines are skipped, never coerced and never fatal", and that
is a good rule for a ragged producer. The bug is that skipping a string
field is right when it is an *annotation* and wrong when it is a
*dimension*, and nothing distinguishes those today.

### 3. Record order is not an order. This is the one that changes the design.

`parse_metrics` uses record order as the x-axis. For a training curve
that is correct — successive records are successive steps. For a sweep
it is not: `results.csv` is ordered by metric, then variant, then seed,
because that is the nesting of the loop that wrote it.

> **A sparkline over record order is a chart of the producer's loop
> nesting.** Its shape is a fact about the writing code, not about the
> data, and re-ordering the loop would change the picture without
> changing a single measurement.

The spec already carries the rule this generalizes — *"one point is not
a curve"*, because a flat line makes the much stronger claim that
something was measured repeatedly and did not change. The same argument
applies to three unordered measurements: drawing them left-to-right
asserts a progression that the data does not contain.

So a metric's points are a curve **only if something orders them**. That
is a property the feed has to state, not one the renderer may assume.

---

## What a null result needs on screen

The spec's pane is a row per series with a sparkline. That is the right
pane for a training curve. It is the wrong pane for the result this
platform's first real consumer just produced, and the difference is not
cosmetic.

Arc 1 (Model-Experiments #107) concluded: **stop-gradient is the
mechanism; EMA momentum, masking ratio and predictor depth are all
inert; training duration is the axis that mattered and was never
varied.** The evidence is a non-effect. Its quantitative form, in that
project's own words:

> Slice 3's within-cell seed spread reached 1.333, comparable to the
> entire range across all nine configurations, which is why that grid
> returned F < 1.

> Every cell of Slice 3's 3000-step grid overlaps the *untrained*
> random-init band.

A non-effect is invisible unless the screen shows **spread** and a
**reference**. Grouped by metric × variant, Slice 1's own numbers say it
outright:

```
effective_rank   n     range
  random_init    3     2.437 .. 2.934
  full           3     2.352 .. 2.791
  no_ema         3     1.250 .. 1.459
```

`full` sits almost entirely *inside* `random_init` — the trained model
is not distinguishable from an untrained one on this metric — while
`no_ema` separates cleanly below it. That is the finding, legible in
three rows. Averaged into one number per variant it is still there but
much weaker; drawn as a twelve-point sparkline it is destroyed.

**Design consequence:** the pane needs two row shapes, chosen by the
data rather than by the operator.

- **Ordered series** → sparkline, as specced. A training curve.
- **Unordered group** → an interval: `min ── median ── max`, with `n`.
  A sweep cell.

And one hard rule that falls out of the same evidence:

> **A shared axis per metric; never across metrics.** Overlap is only
> readable if the variants of one metric are drawn against one scale —
> and `effective_rank` (~2.5) sharing a scale with `embedding_std`
> (~0.5) is the concatenation bug rebuilt in the renderer.

---

## Architecture

Four slices, one arc, ordered so nothing is built before the thing it
carries. Slices 1–3 need no second machine and no Model-Experiments
checkout; Slice 4 is where it meets the real producer.

The arc is bigger than the spec's "one arc" estimate by roughly one
slice, and the reason is Slice 1 — which the spec did not know it
needed, because it had not read the feed. That is stated rather than
absorbed.

## Global Constraints

**Observed data must not steer the display.** The rule
`panopticon/src/view/sanitize.rs` exists for. Series names come from a
producer, so they are sanitized and elided like every other observed
string. Colour remains a non-goal of this arc.

**The renderer never invents an ordering.** If the feed does not say the
points are ordered, they are drawn as a group. A sparkline is a claim.

**No wall clock in any test.** Every `now` is injected, including the
one the two ages are computed against.

**No network in any test.** Metrics reach the cockpit through the same
`HttpTransport` seam peers already cross.

**A remote observation is never `Live`.** Unchanged, and the reason the
two ages are two.

---

## Slice 1 — The parser stops inventing curves

*Crate: `baseline`. No UI. This slice is entirely about not being
confidently wrong.*

- [ ] **Task 1.1 — Characterize today's behaviour.** A test feeding real
      long-format records to `parse_metrics` and asserting the current
      output *including* the `issue` and `seed` series and the
      concatenated `value`. It is a characterization test: it locks in
      the wrong answer so the diff that fixes it is visible, and it is
      deleted in Task 1.4. Without it the fix looks like a refactor.
- [ ] **Task 1.2 — Long-format detection.** A record carrying a string
      `metric` and a numeric `value` is an *observation*, not a
      timestep. The series name is the value of `metric`; the
      measurement is `value`; no other numeric field is charted. This
      one rule kills both the identifier series and the concatenation.
      Wide records are unchanged — the existing behaviour is the
      fallback, so no current producer breaks.
- [ ] **Task 1.3 — Dimensions, not annotations.** Remaining string
      fields on an observation are its dimensions and become part of the
      grouping key (`effective_rank` × `full`). Assert the JEPA rows
      group into the three bands above, by value, from the real CSV
      converted to JSONL — the fixture is the real data, not a
      hand-written imitation of it.
- [ ] **Task 1.4 — An ordering claim on the type.** `Series` gains a
      statement of whether its points are ordered, and by what.
      `parse_metrics` sets it only when the feed justifies it — a
      monotonic `step`/`epoch`/`iteration` field on wide records — and
      never for observations. Delete Task 1.1's characterization test in
      the same commit that makes it false.
- [ ] **Task 1.5 — A `compile_fail` doctest** asserting the ordering
      claim cannot be constructed from outside, the way `Authorized`
      cannot. A renderer must not be able to promote a group to a curve.

**Verification:** `effective_rank`, `embedding_std`, `loss_slope`,
`final_loss`, `probe_r2` come out as five separately-keyed metrics with
their variants intact, and `issue` and `seed` come out not at all.

## Slice 2 — The foreign clock

*Crate: `baseline`. The spec's own contribution, and independent of
shape — do it second so Slice 3 has both ages to render.*

- [ ] **Task 2.1 — A failing test first.** A peer whose clock is four
      minutes fast, whose `Artifact::modified` therefore lies in the
      receiver's future. Assert the current saturating subtraction
      renders `0s` — a stalled run displayed as the freshest thing on
      screen. This is the spec's stated bad failure and it must be
      reproduced before it is fixed.
- [ ] **Task 2.2 — `Artifact` crosses the wire as a wire type.** The
      previous arc declined to write one because `Artifact` is "inert
      data". A timestamp inside a value is not inert: `receive()`
      re-bases `observed_at` and passes `self.value` through untouched,
      so `modified` crosses raw and every duration computed from it is
      the cross-machine clock comparison this platform forbids.
- [ ] **Task 2.3 — Re-base `modified` on receipt**, the way
      `observed_at` already is. Producer recency becomes a duration
      measured against the peer's own clock, then re-expressed against
      the receiver's — never a subtraction across two clocks.
- [ ] **Task 2.4 — A producer age that can be unknown.** A peer with no
      RTC (the Pi) can report a `modified` that means nothing. The type
      must be able to say so; rendering `56 years ago` is a bug and
      rendering `0s` is a worse one.

**Verification:** the four-minutes-fast peer renders "produced: unknown"
or a correctly re-based age, and never `0s`.

## Slice 3 — The pane

*Crate: `panopticon`.*

- [ ] **Task 3.1 — A `METRICS` pane, per project**, one group per
      metric, one row per series or observation-group within it.
- [ ] **Task 3.2 — Two row shapes.** Ordered → sparkline over the points
      the row has width for, with the point count beside it. Unordered →
      `min ── median ── max` with `n`. The shape is read off Slice 1's
      ordering claim; the renderer never chooses.
- [ ] **Task 3.3 — One axis per metric.** Every row inside a metric
      group is drawn against that metric's own min/max so overlap is
      readable. Explicitly assert that two metrics on different scales
      never share an axis.
- [ ] **Task 3.4 — The spec's rendering rules**, each with a test: one
      point renders as a value and not a flat line; an empty series says
      so and does not render zero; a series that stopped is
      distinguishable from a series that is flat.
- [ ] **Task 3.5 — The two ages, distinctly labelled**, from Slice 2.
- [ ] **Task 3.6 — `read_only.rs` still passes.** The pane observes; it
      does not act. No entry is added to `MAY_ACT`.

## Slice 4 — Point it at the real producer

- [ ] **Task 4.1 — Decide the feed, and record why.** The manifest
      declares `adapter: jsonl` over a gitignored `results/` glob that
      nothing writes. Two honest options:
      **(a)** Model-Experiments emits JSONL alongside `results.csv`
      (work in that repository, and its run output is deliberately
      ephemeral); **(b)** the manifest points at `results.csv` and
      `baseline` grows a long-format CSV reader.
      **Recommendation: (b).** `results.csv` is that project's curated,
      checked-in record of record — it is the artifact Arc 1 concluded
      *in* — and it exists today. (a) renders a file that does not.
      Long-format CSV and long-format JSONL are the same shape after
      Slice 1, so the reader is small.
- [ ] **Task 4.2 — Register Model-Experiments** in the worked registry
      alongside Parallax, TTUI and SESH.
- [ ] **Task 4.3 — A recorded fixture from the real feed**, so the pane
      is exercised against a messy producer rather than a tidy
      invention — six metrics, 18 variants, ragged coverage.
- [ ] **Task 4.4 — A Plumb scenario whose written intent is the
      finding.** Not "the metrics pane renders". The judged intent is:
      *an operator can see that `full` and `random_init` overlap and
      that `no_ema` does not.* If the cockpit cannot make Arc 1's
      conclusion legible, the arc has not landed, and a perceptual judge
      is the only thing that can say so.

---

## Open questions for sign-off

1. **The feed decision in Task 4.1** is the one that spans two
   repositories, and it is the one I would most like confirmed rather
   than assumed. The recommendation is (b): read `results.csv`. If you
   would rather Model-Experiments start emitting JSONL — reasonable,
   since it keeps `parallax.yaml` honest as written and keeps Parallax
   out of another project's file formats — that is a change in that
   repository and belongs in its own issue there, and this arc should
   wait for it rather than render an empty glob.

2. **Spec open question 3 needs revisiting with a real number.** It asks
   which series to show when there are many, and recommends "show all,
   sorted by name". Against the real feed that is **60 rows** for one
   project — 6 metrics × 18 variants, ragged. Sorted by name they
   interleave across experiments, so `002`'s momentum ladder and `004`'s
   depth grid land adjacent and unrelated. **Revised recommendation:
   group by metric, and collapse groups by default**, which keeps "the
   cockpit never silently picks" while not scrolling forever. Still no
   ranking, still no thresholds — collapsing is reversible and a filter
   is not.

3. **Should the pane show a baseline reference explicitly?** Arc 1's
   conclusion depends on comparison against `random_init`, but which
   variant is the control is *the experiment's* knowledge, not the
   platform's. **Recommendation: no.** Drawing every variant of a metric
   on one axis makes the overlap visible without Parallax having to know
   which row is the control — and a cockpit that guessed at a control
   would be ruling on the experiment, which is already a stated
   non-goal.
