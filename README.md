# Parallax

A platform binding several development projects into one system: a
shared project registry, a verification-tier ladder, an autonomy model,
and a cockpit for watching and steering agent-driven work across all of
them.

TTUI is the first genuine external consumer, and the Plumb component
below is already running against it for real.

**Status:** four components are built. `plumb` is implemented through
Arc 5 and in use. `baseline` holds manifests, a registry, adapters,
aggregated state with per-source freshness, control actions, and the
wire contract between machines. `panopticon`, the cockpit, observes and
now acts, runs against the real repositories, and renders recorded state
deterministically for review. `probe` serves one machine's state to the
tailnet, so a cockpit on any of them shows all of them. Only the
Model-Experiments views are still sketched.

The platform spans machines as of the remote-hosts arc: TTUI is
developed on a laptop, this repository on a desktop, and SESH on a
Raspberry Pi 5 that builds on itself. Each runs a probe; whichever
machine you are sitting at is the cockpit.

## Repository layout

- **`panopticon/`** — the cockpit: a TUI over `baseline`, showing every
  registered project's work in flight, verification standing, artifacts,
  sessions, and the age of each source — across every machine that runs
  a probe. Observation and control are separate arcs, and the render
  path structurally cannot act. Built on `ttui` as a published crate.
  See [`panopticon/README.md`](panopticon/README.md).
- **`baseline/`** — the platform core: manifest parsing and validation,
  the normalized autonomy axes, the four adapter families, aggregated
  cross-project state with per-source freshness, control actions behind
  a confirmation contract, and the wire contract a probe serves.
  Headless — it never touches a terminal. See
  [`baseline/README.md`](baseline/README.md).
- **`probe/`** — serves one machine's state so a cockpit on another can
  see it. Binds loopback only and is published to the tailnet by
  `tailscale serve`. Headless, and it never links a cockpit. See
  [`probe/README.md`](probe/README.md).
- **`plumb/`** — a Claude Code plugin: capture a terminal UI, then judge
  it. See [`plumb/README.md`](plumb/README.md).
  - `plumb/capture/` — the Rust CLI: capture adapters, contact sheets,
    lens dispatch, verdict merging, HTML reports.
  - `plumb/agents/` — the four blinded critic agents.
  - `plumb/commands/`, `plumb/skills/`, `plumb/templates/` — the plugin's
    command, skill, and scaffolding files.
- **`docs/design/specs/`** — approved design documents, per arc.
- **`docs/design/plans/`** — implementation plans derived from them,
  structured as Arcs → Slices → Tasks.
- **`docs/audits/`** — promoted, self-contained verification reports. The
  one there now is the first Plumb verdict a human could audit without
  taking the tool's word for it.

## Plumb, in one paragraph

Plumb answers "does this actually look right?" without asking the model
that wrote the code. It captures a scenario — driving a real terminal app
through a scripted sequence under a PTY — and hands the result to four
**blinded** critic agents (`breakage`, `intent`, `design`, `motion`),
each of which sees only the image and a run manifest, never the source or
the diff. Their findings merge into a single **GO / NO-GO / HOLD**
verdict. A HOLD is not a GO: it names which lens could not report, and
blocks the same way a NO-GO does.

Multi-frame captures reach the critics as a tiled **contact sheet** rather
than an animated GIF, because agents cannot decode the latter. The GIF is
kept alongside it for a human to watch.

## Components

Named components, each carrying arcs, each arc its own spec → plan cycle.

| Component | What it is | Depends on | Status |
|---|---|---|---|
| `plumb` | perceptual verification | — | shipped through Arc 5 |
| `baseline` | manifests, registry, adapters, state, actions, the wire contract | — | shipped through Arc 7, plus remote hosts |
| `panopticon` | the cockpit | `baseline`, `ttui` | shipped: observe, then control, then peers |
| `probe` | serves one machine's state to the tailnet | `baseline` | shipped: the wire, the probe, peers, the merge |
| Model-Experiments views | visualizing experiment output | `panopticon` | sketched |

`plumb` and `baseline` share no dependency and can proceed in parallel.
Control came after observation deliberately — control without
observation is not useful — and spanning machines came after both,
because a platform has to be able to see a machine before it can act on
one.

**This table used to be numbered, and the numbers are gone on purpose.**
They had stopped describing anything. `docs/design/` was already
organized by named arc with no number in it; #3 and #5 were one
component at two stages rather than two sub-projects; and the five
entries were not the same kind of thing — a tool, a library, two arcs of
one TUI, and a feature of a consumer repo. Growth now means adding an
arc to a component, or a component to this table. It never means
renumbering something that already shipped.

Start with
[the master design](docs/design/specs/parallax/2026-08-14-parallax-platform-design.md)
for the whole picture.

## Development

Rust workspace:

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI runs all four on every push.

Design docs use `<projects-root>/` as a placeholder for wherever the
registered projects live on a given machine.

## License

MIT — see [LICENSE](LICENSE).
