# Plumb

Plumb is a single git-installable Claude Code plugin for perceptual
verification of terminal and image output. Capture is a *contract* (an
adapter is anything that writes images to a path), judgment is a
fan-out of narrow, blinded subagents, and per-project state lives in a
`.plumb/` directory the plugin scaffolds on first use. It gives visual
review eyes and opinions: portable capture adapters plus a blinded,
adversarial multi-lens reviewer rendering a GO / NO-GO / HOLD verdict.

Plumb is sub-project #1 of the [Parallax
platform](../docs/design/specs/parallax/2026-08-14-parallax-platform-design.md),
where it serves as the perceptual verification provider — tier 3 of
that platform's verification ladder, the first rung above what CI can
reach. It has no dependency on the platform and ships standalone.

## Capture adapters

Three adapters, one contract: given args, write one or more images to
a declared path, or fail with a typed error. Nothing downstream knows
or cares which adapter produced a frame.

- **`pty`** — spawns an arbitrary command under a pseudo-console,
  drives it with a scripted key/wait sequence, and rasterizes the
  resulting byte stream to PNG or GIF. Cross-platform, no external
  binary, no human install.
- **`window`** — captures a native OS window by title, Windows-only in
  v1. **Deferred — no consumer yet.**
- **`command`** — runs any shell command that writes images to a
  declared path. The escape hatch that makes adoption free.

## Design of record

- [`docs/design/specs/plumb/2026-08-14-plumb-design.md`](../docs/design/specs/plumb/2026-08-14-plumb-design.md)
- [`docs/design/specs/parallax/2026-08-14-parallax-platform-design.md`](../docs/design/specs/parallax/2026-08-14-parallax-platform-design.md)
