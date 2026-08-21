# Design docs

Same structure as TTUI's `docs/design/`, and for the same reason: specs
are approved design documents produced by `superpowers:brainstorming`,
plans are their implementations produced by `superpowers:writing-plans`,
organized **Arcs → Slices → Tasks**.

Filename convention: `specs/<arc>/YYYY-MM-DD-<topic>-design.md` and the
equivalent under `plans/`.

## Arcs

Directories here are named for **components**, not numbers. The roadmap
used to number five sub-projects and no longer does; see the README for
why. This tree was always organized this way, which was most of the
argument for retiring them.

- `parallax/` — platform-wide contracts, and the master design binding
  TTUI, Model-Experiments, and Plumb into one system. Verification
  tiers, the three-axis autonomy model, the `parallax.yaml` manifest,
  the registry and adapter factory, and — because it is a contract every
  component shares rather than one component's concern — the wire format
  a probe serves and a cockpit reads, and the trust tiers that say which
  values on that wire may be interpreted rather than merely passed.
- `plumb/` — perceptual verification. Portable capture adapters plus a
  blinded, adversarial multi-lens reviewer rendering GO / NO-GO / HOLD.
- `panopticon/` — **implemented**: the cockpit. A TUI over
  `parallax-baseline` showing work in flight, verification standing,
  artifacts, sessions, and the age of every source, built with `ttui`
  as a published crate. Two arcs so far — observe, then control — and
  the render path structurally cannot reach an action. Designed here
  rather than in TTUI because it is a platform frontend, not a framework
  example.

A component's design lives under the arc that owns the contract it
changes, not under the directory its code happens to sit in. The probe
is specified under `parallax/` for that reason: what it introduces first
is a platform-wide wire format, exactly as the registry and adapter
factory were specified there rather than under a `baseline/`.

The `parallax/` and `plumb/` documents were designed in the TTUI repo
(TTUI is Plumb's consumer #1) and moved here when this repository was
created, per each spec's own "Home: its own repository" note. TTUI
retains pointer stubs. `panopticon/` was designed here.

## Governing this repo

Per the Parallax spec's "Governing this repo" section: work that
creates or changes a contract other units depend on is
**methodology-first** (brainstorm → spec → plan before code); everything
behind an already-settled contract is **outcome-first** (state the
end state and how to verify it, let CI decide done).

The tiebreaker, when the rule is unclear: try to write the
machine-checkable success criterion. If you can, it is outcome-first
work by definition. If you cannot, that inability is the signal the
contract is not settled and needs a design pass first.
