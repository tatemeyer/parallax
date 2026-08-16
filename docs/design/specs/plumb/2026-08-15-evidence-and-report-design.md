# Plumb Evidence and Report — Design

**Status:** approved 2026-08-15.
**Date:** 2026-08-15
**Relationship to prior work:** extends
`docs/design/specs/plumb/2026-08-14-plumb-design.md` (sub-project #1 of
the Parallax platform). It adds nothing to the review model itself —
the lenses, the blinding contract, the GO / NO-GO / HOLD semantics and
the ruling machinery are unchanged. What it adds is the ability for a
human to check that any of it actually happened.

**Place in the roadmap:** still sub-project #1. This is Plumb's
evidence layer, not a new sub-project. It is a prerequisite for
trusting anything Plumb reports, and therefore for the platform's
thesis that tier-3 verification can be delegated at all.

## Context / Motivation

Plumb currently persists **the images it captured** and **the manifest
it told the lens about**. It does not persist:

- the prompt each lens agent actually read,
- the raw text each lens agent actually returned,
- any of the transformations applied between those replies and the
  verdict, or
- for all but one run to date, the verdict itself.

Eighteen run directories exist. Exactly one contains a `verdict.md`.
None contains a prompt or a reply. `.plumb/runs/` is gitignored, so
even what does exist is disposable working state.

The consequence is exact and serious: **a Plumb verdict cannot be
audited.** A human reading "NO-GO, breakage found a blocker" has no way
to see what the critic was shown, what it said, or what the pipeline
did to what it said. The only available response is to trust the
report — which is the precise failure this tool was built to eliminate.
A reviewer nobody can check is a reviewer nobody should believe.

This matters more than ordinary observability because Plumb's pipeline
**discards things by design**. Findings without a region are dropped.
Severities are clamped to a lens's ceiling. Rulings suppress. Duplicates
merge. Each of those is a place the tool can be wrong, each currently
records only a count, and a genuine critique thrown away by an
over-strict region rule is today invisible in every direction.

There is also direct evidence that this class of defect is real rather
than theoretical. During the implementation of Arcs 1–6, every task
passed `build`, `test`, `fmt` and `clippy` before review, and reviews
still found: a capture that never happened rendering as a clean GO; a
capture format the lens agents could not decode at all; a blinding leak
whose own test could not fire because the fixture used a bare filename;
and a scenario that produced plausible artifacts of the wrong screen
across three consecutive tasks. None of those were reachable by running
the tests again. All of them were found by a reader looking at the
actual artifact.

## Design

### Overview

Two things, in dependency order:

1. **An evidence contract** — a versioned on-disk layout under the run
   directory, written by the subcommands that already hold the data.
2. **A report** — a single self-contained HTML file, generated from a
   run directory, that presents that evidence in the order a human
   actually interrogates it.

The report is the product. The contract exists to serve it and is
deliberately scoped to what the report needs, rather than to what a
maximal archive might want.

A third surface — an interactive TTUI view — is a stated destination
and is **not** in scope here. The contract is defined so that adding it
later reads the same files and requires no change to the pipeline. That
mirrors the platform's own shape: `parallax-baseline` is a headless core
and the cockpit is *a* frontend over it, not the only possible one.

### The evidence contract

```
.plumb/runs/<run-id>/
  run.json                        contract version, run id, timestamps
  <scenario>.manifest.json        unchanged
  <scenario>.png / .gif           unchanged
  lenses/<lens>.<scenario>/
    prompt.txt                    byte-for-byte, as dispatched
    reply.1.raw.txt               attempt 1, unedited
    reply.2.raw.txt               attempt 2, when the skill retried
    parsed.json                   findings extracted from the reply
    dropped.json                  region-less findings, TEXT RETAINED
    clamped.json                  severity lowered, from and to
  merge/
    suppressed.json               ruling id and taste-hash state
    deduped.json                  which finding absorbed which
    survivors.json                what reached the verdict
  verdict.md                      unchanged
```

Three properties are load-bearing.

**The discards are first-class.** `dropped.json` and `clamped.json`
hold the finding's actual text, not a count. A count tells you a
finding was removed; only the text lets you judge whether it deserved
to be. This is the single most valuable thing the contract adds.

**Persisting evidence must never become a channel into a prompt.**
`prompt.txt` is written by the CLI *after* the prompt is built, into a
directory no lens agent can reach — agents declare `tools: Read` and
receive only their image and their prompt. This is the same property
Arc 4 established for rulings, which are applied strictly after
findings return and are never fed to agents, and it is guarded the same
way.

**`run.json` carries a contract version.** The layout will change; a
report should be able to say "this run was written by an older
contract" rather than misread it.

### The report

`plumb report <run-dir> [--out <file>]` emits one self-contained HTML
file. It is read-only and offline: it runs no capture, dispatches no
agents, and cannot alter a verdict. It may be re-run freely.

A run directory may hold **more than one scenario** — selection routinely
matches several. The report renders the run's overall verdict once at
the top, then one section per scenario, each following the structure
below. A single-scenario run is simply the degenerate case.

It is organised around the five questions a human asks when checking a
verdict, in that order:

1. **What was claimed** — the overall verdict, the scenario, frame
   count, exit code, and each lens's own verdict on one line.
2. **What was seen** — the contact sheet at full resolution, with
   frames numbered over a faint grid.
3. **What each lens said** — per lens: its findings, then its prompt
   and raw reply, both complete and both collapsed by default.
4. **What was discarded** — dropped, clamped and suppressed findings,
   each surfaced as a visible line with its text one click away.
5. **How the verdict was reached** — the attrition chain, e.g.
   `4 raw → 1 dropped → 0 clamped → 0 suppressed → 3 survivors`.

**Region anchoring is the feature that makes a verdict checkable
rather than merely readable.** Every finding names a `region`, and the
report renders that claim beside the pixels it names. Frame geometry is
already known — the manifest carries `frame_count`, and the contact
sheet's grid and gutter width are computed by `contact.rs` — so a claim
naming a frame resolves to a crop of that frame. A critic asserting
that a frame is a solid green fill is then something the reader can
agree or disagree with in one glance, instead of scanning a 5792-pixel
sheet for the region under discussion.

The matcher is deliberately **conservative**: it resolves a region only
when the text unambiguously identifies a frame by index or by grid
position, and falls back otherwise. Where a region names something the
geometry cannot resolve — a quadrant, a named panel, free prose — the
report shows the full sheet and does not guess. A wrong crop is worse
than no crop: it places the reader's attention on the wrong pixels
beside a confident claim, and actively misleads. Erring toward the full
sheet costs a moment of scanning; erring toward a crop costs the
reader's trust in every crop after it.

The file is self-contained: images are embedded as data URIs and no
external resource is referenced, so it survives being moved, committed,
attached to a pull request, or read years later.

### Durability

Run directories stay gitignored and prunable. They are working state.

The **report is the archival unit**: a single portable file that is
deliberately promoted when a run is worth keeping. This keeps the
repository from growing by roughly a third of a megabyte per capture
forever, while making the runs that matter durable in a form that
cannot rot into a broken relative path.

### Data flow

No new orchestration. Each subcommand writes the slice it already
holds:

| Step | Written by | Artifacts |
|---|---|---|
| `plan` builds blinded prompts | CLI | `lenses/*/prompt.txt` |
| dispatch, and retry on unparseable output | skill | nothing; reports the attempt number back |
| `merge` ingests replies | CLI | `reply.N.raw.txt`, `parsed.json`, `dropped.json`, `clamped.json` |
| `merge` aggregates | CLI | `merge/*.json`, `verdict.md`, `run.json` |
| `report` | CLI | one `.html` |

`plan` already receives `--run-dir` and builds the prompts. `merge`
already receives `--run-dir` and the reply files. Both already hold
everything required; nothing currently writes it down.

**Closing the retry gap at the boundary.** The review contract allows
one retry before a lens HOLDs, so a held lens has two replies and only
the second reaches `merge` today. "The first reply was garbled and the
retry was clean" is a materially different story from "it held", and
the difference is audit-relevant. `--report` therefore gains an
optional fourth field — `lens:scenario:file:attempt`, defaulting to `1`
— and an optional `--model lens:scenario:name` records which model
judged. Neither moves any logic into skill prose; both make the skill
state what it already knows.

### Failure handling

The governing rule is the one this project has already paid to learn:
**absence must never read as success.**

- **Missing evidence is labelled, never silently omitted.** A lens
  directory with no reply renders as *"no reply persisted"*, not as a
  lens that returned zero findings. Those are different states, and
  conflating them is exactly how a capture that never happened
  rendered as a clean GO.
- **Malformed evidence renders as "present but unparseable"**, with the
  raw bytes still linked. The report never quietly drops something it
  failed to read.
- **`report` cannot fail a run.** It is a viewer. A broken report is a
  broken viewer, never a changed verdict.
- **A run with no verdict** renders its capture and prompts and states
  that no verdict was reached — which is precisely the state worth
  inspecting after a run HELD or died mid-flight.
- **A run written before this contract** renders the images and
  manifest it genuinely has, and states plainly that prompts and
  replies were not persisted by the run that produced it. This is one
  honest line, not a compatibility feature.

### Verifying claims already made

Runs that predate this contract cannot be audited, because bytes that
were never written cannot be recovered. Verification of an earlier
claim is therefore done by **re-deriving it under observation**: run the
scenario again with persistence in place and read the resulting report.

At roughly 50–90 seconds per capture, re-establishing the entire
scenario library costs minutes. Unlike reconstruction, it produces real
evidence rather than a plausible account of what probably happened. The
report must state clearly that such a run is a new run, not a
reconstruction of the original.

A `plumb verify --scenario <name>` convenience wrapper — capture, plan,
dispatch, merge, report in sequence — is desirable but **deferred**.
Dispatch is the harness's job, so the wrapper is driven by the skill
rather than by the CLI shelling out to agents. The contract is designed
so that it is a thin sequencing layer rather than a second pipeline.

## Non-goals

- **The interactive TTUI view.** A stated destination, deliberately not
  in this spec. The contract is defined so it can be added over the
  same files.
- **Repeatability and determinism guarantees.** Deliberately back
  burner. The immediate need is to see whether the system works, not to
  prove two runs produce identical bytes.
- **Cross-run comparison, trend views, or dashboards.** One run, one
  report.
- **Reconstructing pre-contract runs.** Impossible, and pretending
  otherwise would manufacture exactly the false confidence this design
  exists to remove.
- **Changing the review model.** No lens, severity rule, blinding
  guarantee, or verdict semantic changes here.
- **CI integration.** The report is for a human. Plumb's gate remains
  harness-level and human-overridable.

## Testing

TDD throughout, per `.claude/rules/development-conventions.md`. The
strategy is shaped by a finding from this project's own history: **a
suite that asserts only on logic passes while the artifact is
unusable.** Every genuine defect found across Arcs 1–6 was invisible to
a green suite. These tests therefore assert about the *output*.

- **The omission test — load-bearing.** For a run containing a dropped
  finding, a clamped finding and a suppressed finding, assert the
  rendered HTML **contains all three texts**. Not that the sections
  exist — that the discarded content is literally present. This is the
  one property the tool exists to provide, and the one a later
  "tidying" of the report would most naturally erode.
- **Crop geometry.** Given a frame count and gutter width, assert the
  crop rectangle for frame *N* is the correct pixels. Pure arithmetic,
  easy to get subtly wrong, and a wrong crop actively misleads.
- **Self-containment.** Assert the emitted HTML references no external
  URL, relative image path, or external font.
- **Blinding.** Assert no evidence path reaches `build_prompt`, both
  structurally and by the source-text guard shape Arc 4 established for
  rulings.
- **Degradation.** For a run missing prompts, assert the "not
  persisted" marker text is **present** — not merely that the section
  is absent.
- **Contract round-trip.** Write evidence, read it back, assert every
  field survives, including the contract version.

**One honest exemption.** Whether the report is legible — whether a
crop reads, whether the hierarchy works — has no unit test, the same
class as this repository's real-TTY and visual-review carve-outs. It is
verified once by hand: generate a report from a real run and look at
it. That manual check is the only thing that can establish the tool
works, and it is the same act the whole tool exists to make possible.

## Critical files

- `plumb/capture/src/evidence.rs` — the contract: layout, versioning,
  writing, reading.
- `plumb/capture/src/report/` — HTML generation, region-to-crop
  resolution, self-containment.
- `plumb/capture/src/cli/report.rs` — the `report` subcommand.
- `plumb/capture/src/cli/plan.rs` — writes `prompt.txt` (modified).
- `plumb/capture/src/cli/merge/mod.rs` — writes replies, findings,
  discards, and the merge chain; gains the `attempt` field and
  `--model` (modified).

## Verification

- `cargo build`, `cargo test`, `cargo fmt --check`, and
  `cargo clippy --all-targets -- -D warnings` clean from the workspace
  root.
- A full run of a real TTUI scenario produces a run directory
  containing every artifact the contract names.
- `plumb report` on that directory emits an HTML file that opens
  standalone, shows the contact sheet, and contains each lens's prompt
  and raw reply in full.
- A finding naming a resolvable frame renders beside a crop of that
  frame; a finding naming an unresolvable region renders beside the
  full sheet without guessing.
- A deliberately dropped finding's text appears in the output.
- The report on a pre-contract run states what was not persisted rather
  than rendering a lens with zero findings.
