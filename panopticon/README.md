# panopticon

The Parallax cockpit: one screen for every registered project's work in
flight, verification standing, artifacts, agent sessions, and — the part
that makes the rest trustworthy — how current each of those answers is.

**Read-only.** It observes and mutates nothing: no labels set, no pull
requests merged, no runs dispatched, nothing written to any repository.
`parallax-baseline` ships a whole `actions` module that could do all of
it, and `tests/read_only.rs` asserts this crate never names it. Control
is sub-project #5 and gets its own spec.

Sub-project #3 of [Parallax](../README.md), built on
[`parallax-baseline`](../baseline/README.md) and on `ttui` as a
published crate — which makes it TTUI's first genuine external consumer,
and the reason [tatemeyer/ttui#170](https://github.com/tatemeyer/ttui/issues/170)
exists.

## Running it

```
panopticon --projects-root C:/Users/tatem/Dev   # every sibling with a parallax.yaml
panopticon --registry ~/.parallax/registry.yaml # the roots a registry file lists
panopticon --fixtures panopticon/fixtures       # recorded state, frozen clock
```

With none of those it still starts, and says what it looked for. "No
projects registered" is a common answer — a project joins the platform
by dropping a `parallax.yaml` in its root — and a cockpit that exits on
it teaches nothing about why.

| key | |
|---|---|
| `j` / `k` | move within the detail pane |
| `Tab` | next project |
| `1`–`4` | work / verify / artifacts / sessions |
| `r` | refresh the sources that only read |
| `c` / `C` | run this project's / every project's build checks |
| `?` | help |
| `q` | quit |

## Two rules worth knowing

**The refresh cycle never runs a build.** Verification adapters report
what calling them costs, and only the readers go on the cadence. TTUI's
manifest declares `cargo clippy --all-targets -- -D warnings` and `cargo
test`; polling those every thirty seconds, for every project, on the
machine running the agent sessions, is a category error — a cadence is
the right shape for observing state and the wrong shape for producing
it. So they run when you press `c`, and until then the pane reads **not
run this session** rather than showing a stale green. A result that
predates the code on disk looks like an answer and is not one.

`tests/responsiveness.rs` holds that line: five refresh cycles run zero
commands, and an explicit request runs exactly the declared ones.

**Nothing blocks the event loop.** The adapters live on a refresh thread
that owns them; `on_tick` drains a channel and returns. A five-second
poll costs the UI nothing, and a hung socket cannot hang the cockpit.
`r`, `c`, and `C` send a request rather than doing the work — a key that
performs I/O on the UI thread is the same mistake wearing a different
hat.

## Fixture mode

`--fixtures <dir>` builds every adapter from recorded responses with a
clock the fixture set declares, so two runs render byte-identical
frames. That is what lets Plumb judge the cockpit: a NO-GO then means
the layout is wrong rather than that time passed.

It is a shipped feature rather than a test scaffold — it is also how a
human sees the cockpit before registering anything. A fixture directory
without a `clock.txt` is rejected rather than falling back to the system
clock, because that fallback is the exact bug the mode exists to
prevent.

The adapters come from `parallax-baseline`'s own factory with a fixture
transport, so fixture mode exercises the same manifest translation
production does, not a parallel one.

## What it shows, and what it refuses to invent

- An autonomy axis nothing claims renders `—`, never a default. TTUI
  declares no autonomy map today, so every one of its rows shows three
  dashes plus the labels its manifest never mentions — which is the
  honest picture, not a broken one.
- A degraded source appears in the footer **with its reason**, alongside
  the sources that did report. A cockpit that renders "unavailable" and
  drops the why has taken a fact and returned a shrug.
- When the footer cannot fit every source it says how many it hid.
- The Cloister Bell rings when a project *enters* a blocker state, not
  continuously while one holds, and never as a modal. The first
  observation never rings however bad it is: the cockpit has just
  started and is reporting the world as found, not something that
  happened.

## Testing

Everything but `main.rs` is testable without a terminal, because TTUI's
`Buffer` is inspectable in-process — the view model is pure, and
rendering is asserted on cells.

```
cargo test -p parallax-panopticon
```

No test touches the network, a TTY, or a wall clock.
