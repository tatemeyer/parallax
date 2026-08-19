# Parallax

A platform binding several development projects into one system: a
shared project registry, a verification-tier ladder, an autonomy model,
and a cockpit for watching and steering agent-driven work across all of
them.

TTUI is the first genuine external consumer, and the Plumb sub-project
below is already running against it for real.

**Status:** early. Sub-project #1 (Plumb) is implemented through Arc 5
and in use; sub-project #2 (Baseline) is implemented through Arc 7 —
manifests, adapters, aggregated state, and control actions — and is the
deliverable the cockpit will consume. Everything else is specced or
sketched, not built.

## Repository layout

- **`baseline/`** — sub-project #2, the platform core: manifest parsing
  and validation, the normalized autonomy axes, the four adapter
  families, aggregated cross-project state with per-source freshness,
  and control actions behind a confirmation contract. Headless — it
  never touches a terminal. See [`baseline/README.md`](baseline/README.md).
- **`manifests/`** — the registered projects' `parallax.yaml` files.
- **`plumb/`** — sub-project #1. A Claude
  Code plugin: capture a terminal UI, then judge it. See
  [`plumb/README.md`](plumb/README.md).
  - `plumb/capture/` — the Rust CLI: capture adapters, contact sheets,
    lens dispatch, verdict merging, HTML reports.
  - `plumb/agents/` — the four blinded critic agents.
  - `plumb/commands/`, `plumb/skills/`, `plumb/templates/` — the plugin's
    command, skill, and scaffolding files.
- **`docs/design/specs/`** — approved design documents, per sub-project.
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

## Roadmap

Five sub-projects, each with its own spec → plan cycle.

| # | Sub-project | Depends on | Status |
|---|---|---|---|
| 1 | `plumb` — perceptual verification | — | implemented through Arc 5 |
| 2 | `parallax-baseline` — registry, manifest, transport | — | implemented through Arc 7 |
| 3 | Cockpit: observe | 2, `ttui` | spec proposed, awaiting sign-off |
| 4 | Model-Experiments visualization | 3 | sketched |
| 5 | Cockpit: full control | 3 | sketched |

#1 and #2 share no dependency and can proceed in parallel. Control comes
last deliberately — control without observation is not useful.

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
