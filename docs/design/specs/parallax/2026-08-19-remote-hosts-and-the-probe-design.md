# Parallax — Remote Hosts and the Probe (Design)

**Status:** proposed — awaiting sign-off. Amends an approved spec.
**Date:** 2026-08-19

**Amends:**
`docs/design/specs/parallax/2026-08-14-parallax-platform-design.md`.
Nothing below contradicts it. It describes a platform; this says what
that platform means when the projects it binds do not share a
filesystem.

**Found by:**
Operating the platform across three machines. TTUI is developed on a
laptop, this repository on a desktop, and SESH on a Raspberry Pi 5 that
builds on itself because cross-compiling to it was rejected in
`SESH/deploy/README.md`. Three registered projects, three disks, one
cockpit that can see one of them.

## Context / Motivation

**The platform is single-machine, and nothing says so.** `Registry::scan`
takes a `&Path`. `RegistryEntry` is a `root: PathBuf`. Every adapter but
`github` reads the local disk — `scan_glob` walks with `walkdir`
(`adapters/artifact.rs:142`), `FilesystemSessionAdapter` calls it and
then `newest_mtime`, and `PlumbVerificationAdapter` reads a `verdict.md`
with `std::fs`. None of that is wrong. It is simply the whole of what
the platform can currently observe.

The concrete failure: a cockpit on the desktop shows GitHub work for all
three projects, because that adapter crosses a network by nature — and
shows **no** sessions, **no** artifacts, and **no** local verification
for the two projects that do not live on that disk. The one pane most
worth having across machines, `sessions.watch`, is the one most tightly
bound to local disk.

### Two seams, one of which exists

**Command execution has a seam.** `CommandRunner`
(`adapters/verification.rs:87`) is a single trait taking a command and a
`cwd`, injected through `from_manifest_with`'s runner factory. Better
still, `LocalProcessControl` (`actions/process.rs:19`) routes through
the same trait rather than reaching for `std::process` a second time.
One implementation would move both verification and control.

**Filesystem observation has none.** Sessions, artifacts, and Plumb
verdicts reach the disk directly. There is no trait to substitute.

So the obvious move — one remote `CommandRunner` — buys the half of the
problem that was already easy and none of the half that motivated the
work.

### The alternative that was rejected

Introduce a `FileSource` trait (glob, read, metadata), thread it through
the artifact, session, and Plumb adapters, and give it a local and an
SSH implementation. It needs no new binary and nothing deployed.

It was rejected on cost at the point of use. `newest_mtime` walks an
entire session tree to find one timestamp; over SSH that is a full
remote tree stat per session, per project, per refresh. The refresh
thread would spend its cycle waiting on a network for data the remote
machine could have computed locally in microseconds. It also needs a
whole new fixture mechanism: `FixtureTransport` records HTTP, and a
recorded *filesystem* is a second thing to build and keep honest.

### The shape chosen

Each host runs a **probe**: a small headless binary that scans its own
disk with the adapters that already exist, and serves the result.
Observation crosses the wire already aggregated. The scan stays local
and native; only the answer travels.

```
  pi5      seshd + parallax-probe ──┐
  laptop   parallax-probe ──────────┼── tailnet ──> panopticon (any host)
  desktop  parallax-probe ──────────┘
```

This reuses the seam that already exists. `HttpTransport` is how baseline
talks to a network, and `FixtureTransport` is how panopticon renders
recorded state deterministically. A peer is another HTTP source, so
recorded remote hosts cost nothing new.

## Design

### The wire type is not the domain type

`PlatformState`, `ProjectState`, and `Observed<T>` carry no serde
derives today. The temptation is to add them and serialize the domain
types directly.

**That cannot work, and the reason is load-bearing rather than
stylistic: an observation changes meaning when it crosses a machine
boundary** (see the next section). A format that were merely derived
would transmit the lie faithfully. Since a transformation is required
anyway, the wire format is written down explicitly.

A `wire` module in `parallax-baseline` holds the serialized contract,
versioned `parallax/v1` like the manifest and the registry file, with
`deny_unknown_fields` for the same reason those have it — a typo'd key
must not silently vanish. Domain types stay free to change without
breaking a running Pi.

```rust
pub struct StateEnvelope {
    pub api_version: String,      // "parallax/v1"
    pub peer: String,             // the probe's self-reported name
    pub now: SystemTime,          // the PROBE's clock, at serialization
    pub projects: Vec<ProjectWire>,
}
```

### An observation is re-stamped on receipt

`Observed::freshness` (`freshness.rs:66`) maps `SourceKind::Watched` to
`Freshness::Live` with no age check. That is exactly right for a
filesystem read — the value was true at the moment it was read — and it
becomes false the instant that read happened on another machine. Every
session and artifact observation is `Watched`. Transmitted unchanged,
the cockpit would render the Pi's sessions as **Live** while the Pi is
unplugged.

The rule:

> A `Watched` observation received from a peer becomes
> `Polled { interval }` at the client, where `interval` is the peer's
> poll interval. **A remote observation is never `Live`.**

`Live` means "I read this myself." Nothing received over a network may
claim it. `Freshness` already carries `Unavailable { last_success }`,
which is precisely the sleeping-laptop case, so the vocabulary needs no
extension — only the discipline to use it.

### Clocks are re-based, never compared

`observed_at` is a `SystemTime` from the probe's clock. A Pi 5 has no
RTC and comes up wrong until NTP lands, so a client that compared the
two machines' wall clocks directly would compute ages that are
confidently absurd.

The envelope carries the probe's own `now`. The client computes age on
the probe's clock, where both values came from the same clock and skew
cancels, then re-bases onto its own:

```
age          = envelope.now - observed_at     // probe's clock; skew cancels
observed_at' = client_now - age               // client's clock
```

`Observed::age` already saturates at zero when `now` precedes the
observation, so a probe whose clock jumps backwards mid-scan yields a
zero age rather than a panic or a wrapped duration.

### The cost model survives the wire

`CheckCost` already separates a check that reads state from one that
produces it, and the cockpit's rule is that the refresh cycle never runs
a build. That maps onto HTTP without inventing anything:

- `GET /state` runs **only** `CheckCost::Read` adapters.
- An `Execute` adapter is reported as `VerificationOutcome::NotRun` with
  a detail naming what would run it.

A probe that ran `cargo test` because a cockpit refreshed is the same
category error the cost model was introduced to prevent, now with a
network in front of it and three machines able to trigger it at once.
This is also the cleanest argument for deferring control: `Execute`
belongs behind a `POST`, and `POST` is the next spec.

### Peers, not per-project hosts

The registry file gains a `peers:` list. It does **not** gain a `host:`
on each project entry.

```yaml
apiVersion: parallax/v1
projects:
  - root: C:/Users/tatem/Dev/Parallax
peers:
  - url: https://pi5.tail-scale.ts.net
  - url: https://laptop.tail-scale.ts.net
```

SESH's own manifest makes this argument already, against a different
target: it omits `root:` because "a checked-in absolute path would be
wrong on every machine but one." A desktop registry enumerating which
projects live on the Pi, and where, is that same wrong path with an
extra hop. The Pi knows what is on the Pi. A peer self-describes, and
the client configures endpoints, not inventories.

Consequences, all of them wanted:

- A project added on the Pi appears in the cockpit with no desktop edit.
- A peer that cannot be reached is **one degraded source**, exactly as a
  broken project is one `RegistryError` — the rule `Registry` and
  `aggregate` already share.
- Project names are qualified by peer, so two machines holding a clone
  of the same repository are two rows rather than a collision. A local
  project and a remote one with the same name is the ordinary case, not
  an error: the desktop holds clones of all three.

### Reachability: the probe never listens on a network

The probe binds `127.0.0.1` and nothing else. It is published on the
tailnet by `tailscale serve`, which terminates TLS with a real
certificate and forwards to the loopback port:

```
tailscale serve --bg --https=443 http://127.0.0.1:8737
```

**The probe therefore implements no TLS, no authentication, and no
authorization**, because it is not reachable by anything that would need
to be turned away. That is a property worth testing rather than
documenting: a probe that binds a routable address has lost its entire
security argument, so it refuses to start.

`tsnet` was specified first and withdrawn on investigation.
`tailscale-rs` 0.5 has no TCP listeners, relays everything through DERP
rather than the LAN two of these machines share, and ships unaudited
cryptography behind a `TS_RS_EXPERIMENT` flag. `libtailscale` starts a
Go runtime inside the process via cgo, which would have to be
cross-compiled for Windows x86_64 and Linux aarch64. The cost of
`tailscale serve` is that the probe carries its host's tailnet identity
rather than its own, which for three machines under one owner buys
nothing that a per-machine ACL does not already give.

### The probe

A new workspace member, `probe/`, depending on `parallax-baseline` and
nothing else in the workspace. Headless by the same rule baseline
follows — it never links panopticon, and a cockpit is not the only
possible client.

- `GET /state` — the envelope above, for every project in the probe's
  own local registry.
- `GET /health` — liveness without a scan, so an unreachable peer and a
  slow one are distinguishable.

The probe is close to trivial by construction: it runs `Registry::scan`
or `Registry::from_file`, calls `from_manifest` and `aggregate`, and
serializes. Every hard question it might have had was answered when
those were written.

### The client

Panopticon merges peers into the `PlatformState` it already renders.
Local projects load as they do now; each peer contributes its projects
with observations re-stamped and re-based on receipt. A peer is fetched
on the poll interval, on the refresh thread that already exists.

The read-only boundary is untouched: this spec adds no action, and
`tests/read_only.rs` continues to assert that only `control` may name
one.

## Non-goals

- **Control over the wire.** `POST`, the confirmation contract crossing
  a network, and executing a confirmed action through `LocalExecutor` on
  the host that owns the project. Its own spec, for the reason the
  roadmap already gives: control without observation is not useful.
- **A remote `CommandRunner`.** Once a probe exists, the machine that
  owns a project runs its own commands locally through the executor it
  already has. SSH would be a second way to do the same thing.
- **Push or streaming.** Polling is what the freshness model describes
  and what the refresh thread already does.
- **Discovery.** Peers are listed. Three machines do not need a registry
  service, and one that guessed would be harder to reason about than one
  that reads a file.
- **Authentication.** The tailnet is the boundary; see above.
- **Anything SESH-specific.** The probe serves the platform contract. A
  living room is not a platform concept.

## Testing

- **Round-trip.** Every wire type serializes and parses back equal, and
  an unknown key is rejected rather than ignored.
- **A remote observation is never `Live`.** A `Watched` observation from
  a peer, received and re-stamped, reports `Fresh` or `Stale` and never
  `Live`. This is the spec's central claim and gets a test that names it.
- **An unreachable peer is `Unavailable`, not empty.** And it degrades
  only itself: the local projects and every other peer still render.
- **Clock skew.** A probe reporting an `observed_at` an hour ahead of the
  client's clock still yields the correct age after re-basing.
- **The probe refuses a routable bind address.** Its security argument
  is that nothing off-box can reach it; a test asserts it enforces that
  rather than assuming it.
- **`GET /state` runs no `Execute` adapter.** Asserted with a
  `ScriptedRunner` that records calls: the list must be empty.
- **Recorded peers.** `FixtureTransport` serves a canned envelope, so a
  fixture-mode cockpit renders three machines deterministically with no
  network — the property panopticon's fixture mode already depends on.

## Critical files

| file | change |
|---|---|
| `baseline/src/wire.rs` | new — the serialized contract and its version |
| `baseline/src/freshness.rs` | the re-stamping and re-basing rules |
| `baseline/src/registry.rs` | `peers:` in `RegistryFile`; peer-qualified names |
| `baseline/src/state.rs` | merging peer state into `PlatformState` |
| `probe/` | new workspace member: registry, aggregate, serialize, serve |
| `panopticon/src/app.rs` | peers on the refresh cycle |
| `panopticon/src/main.rs` | peers from the registry file |
| `Cargo.toml` | `probe` added to workspace members |

## Verification

- `cargo clippy --all-targets -- -D warnings` and `cargo test` pass at
  the workspace root.
- `panopticon --registry <file>` against a registry naming two
  unreachable peers starts, renders local projects, and shows both peers
  as unavailable.
- With probes running on all three machines, `panopticon` on any one of
  them lists TTUI, Parallax, and SESH, each with its sessions.
- Unplugging the Pi moves SESH's sources to unavailable within one poll
  interval, and no pane claims `Live`.

## Open questions for sign-off

1. **Resolved — the roadmap stops numbering.** The question was whether
   this is sub-project #6. The better answer is that the numbers have
   already stopped earning their place, and adding a sixth would make
   that worse rather than settle it.

   Three signs of it. `docs/design/` **already abandoned numbering** —
   specs live under `parallax/`, `plumb/`, and `panopticon/`, and no
   directory is named for a number. The numbers already double-count:
   #3 and #5 are one component at two stages, not two sub-projects. And
   the five entries are not even the same kind of thing — a tool
   (`plumb`), a library (`baseline`), two arcs of one TUI, and a feature
   of a consumer repo.

   So the roadmap converts to **named components, each progressing
   through arcs** — the vocabulary the plans and the docs tree already
   use. `panopticon` becomes one row with two shipped arcs rather than
   #3 and #5. Growth then means adding an arc to a component, or a
   component to the table, which is what actually happens; it never
   means renumbering something that shipped.

   The probe is a component. This document stays under the `parallax/`
   arc, because what it specifies first is a platform-wide contract —
   the wire format — and that is the same reason the registry and
   adapter-factory design lives there rather than under `baseline`.
2. **Should the probe serve the local registry, or a configured subset?**
   Serving everything with a `parallax.yaml` is the zero-configuration
   answer and matches `Registry::scan`. A host that wanted to publish
   only some of its projects has no way to say so.
3. **How are peer-qualified names rendered?** `sesh@pi5` is the obvious
   spelling and it is a cockpit decision, not a baseline one — but the
   qualification has to exist in the state for the cockpit to render it.
4. **Does the probe need `GET /state` per project?** Fetching one
   project is cheaper when a cockpit is focused on one row, and it is
   speculative until the refresh cycle is measured against three real
   machines.
