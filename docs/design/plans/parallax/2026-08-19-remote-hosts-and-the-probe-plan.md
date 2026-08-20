# Remote Hosts and the Probe — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> `superpowers:subagent-driven-development` or
> `superpowers:executing-plans`. Steps use checkbox (`- [ ]`) syntax.
>
> **Structure note:** organized as **Arcs → Slices → Tasks** per
> `docs/design/README.md`.

**Goal:** A cockpit on any of three machines shows all three machines'
projects — work, verification, artifacts, and sessions — with every
remote observation honestly aged, and with an unplugged machine visibly
unavailable rather than silently stale.

**Spec:**
`docs/design/specs/parallax/2026-08-19-remote-hosts-and-the-probe-design.md`.

**Architecture:** Five arcs, each its own PR, ordered so nothing is
built before the thing it carries. Arc 1 is the wire contract inside
baseline — pure types and pure conversions, no network and no new crate.
Arc 2 is the probe: the crate, then the server. Arc 3 teaches the
registry about peers. Arc 4 makes the cockpit merge them. Arc 5 deploys
and closes out.

Arcs 1 and 2 are testable with no second machine, because a peer is a
function from bytes to state and a probe is a function from a directory
to bytes. Nothing in this plan needs a Raspberry Pi until Arc 5.

**Tech Stack:** Rust (stable, 2021 edition). `parallax-baseline` by
path. `serde` and `serde_json`, both already dependencies. **`tiny_http`
for the probe's server** — synchronous, one dependency, no runtime.
**No async runtime anywhere**, matching the constraint panopticon
already holds. `tempfile` as a dev-dependency.

---

## Global Constraints

**The probe never binds a routable address.** Its entire security
argument is that `tailscale serve` is the only path to it. Task 8
asserts the refusal rather than trusting the deployment to get it right.

**A remote observation is never `Live`.** This is the spec's central
claim. Task 3 asserts it by name, and the assertion is the reason the
wire types are hand-written rather than derived.

**No wall clock in any test.** Every `now` is injected — including the
probe's, which is why the envelope carries one rather than calling
`SystemTime::now()` at serialization time inside a function a test wants
to drive.

**No network in any test.** Peers go through `FixtureTransport`, the
same seam work already uses.

**`GET /state` runs no `Execute` adapter.** Task 9 asserts it with a
`CommandRunner` that panics if called at all, mirroring how panopticon's
Task 11 proved the same property for the refresh cycle.

**Baseline stays headless, and so does the probe.** No terminal, no
`print!` outside tests, in either crate.

**Soft ceiling of 500 lines per file**, tests included.

**Every `pub` item documented.** `#![warn(missing_docs)]` plus CI's
`-D warnings`.

---

## File Structure

```
baseline/src/
  wire.rs          NEW  the serialized contract, its version, conversions
  freshness.rs          re-stamping and re-basing
  registry.rs           `peers:` in RegistryFile
  state.rs              merging peer projects into PlatformState
probe/
  Cargo.toml       NEW
  src/
    main.rs        NEW  argument parsing, bind, serve
    state.rs       NEW  registry -> aggregate -> envelope
    server.rs      NEW  routes, loopback guard
panopticon/src/
  app.rs                peers on the refresh cycle
  main.rs               peers from the registry file
  fixtures.rs           recorded peers
```

---

## Milestones

| # | Milestone | Done when |
|---|---|---|
| 1 | The contract exists | A `StateEnvelope` round-trips, and a re-stamped remote observation cannot report `Live` |
| 2 | A probe serves | `curl 127.0.0.1:8737/state` on this desktop returns all three local clones |
| 3 | A peer is configurable | A registry file naming an unreachable peer loads, and says so |
| 4 | The cockpit spans machines | `panopticon` renders local projects plus a fixture peer's, ages correct |
| 5 | It runs for real | Probes on desktop, laptop, and Pi; one cockpit shows nine panes' worth of truth |

---

## Arc 1: The wire contract

### Slice 1.1: The format

#### Task 1: The `wire` module

**Revised during implementation.** The spec first called for
hand-written DTOs mirroring every domain type. Two corrections, both
recorded back into the spec:

*The boundary is narrower than "the domain".* Only `Observed<T>` changes
meaning as it crosses. `WorkItem`, `Artifact`, `Session`, and
`VerificationStatus` are inert data, and mirroring them adds several
hundred lines that decouple nothing — a mirror of a type with no
behaviour is a second place to forget a field. So the structure is
hand-written and the leaves derive.

*`deny_unknown_fields` was wrong here.* It is right for a manifest,
which a human types and can typo. It is backwards for a wire format
between machines that upgrade at different times: it would break an
older client on exactly the newer probe it should tolerate.

- [x] Add `pub mod wire;` to `lib.rs`.
- [x] `StateEnvelope { api_version, peer, now, projects }`, `rename_all = "camelCase"`.
- [x] `ObservedWire<T> { value, observed_at, source }` — **with no `freshness` method**, so a received observation structurally cannot be asked how fresh it is.
- [x] `SourceKindWire`, mirroring `SourceKind` because the two do not mean the same thing on both ends.
- [x] `ProjectWire`, every collection `#[serde(default)]` so a reduced project crosses as a reduced project.
- [x] Unknown fields **ignored**, with the reason documented at the module level.
- [x] `pub const WIRE_API_VERSION: &str = "parallax/v1";`
- [x] Leaf types derive `Serialize`/`Deserialize` (`autonomy.rs` already had them — it is parsed from manifests).
- [x] `SystemTime` uses serde's own representation rather than a hand-rolled millis encoding; it is stable and cross-platform, and a custom one was unjustified complexity.

**Verified:** `cargo test -p parallax-baseline wire::` — 11 tests pass.

#### Task 2: Domain ↔ wire, and round-trip

- [x] `ObservedWire::send` / `ProjectWire::send` / `StateEnvelope::send`.
- [x] `receive` on each, taking the probe's `now`, the client's `now`, and the peer's interval.
- [x] `WireError { source, problem }` — the shape `RegistryError` already uses, so a peer that answers with nonsense degrades like a project that fails to load.
- [x] An envelope whose `apiVersion` is not `parallax/v1` is rejected before any project is read.

**Verified:** a full envelope round-trips through JSON unchanged; a
project declaring nothing but a name survives the trip as a reduced
view with no degradations, because absent is not degraded.

### Slice 1.2: The two honesty rules

#### Task 3: A remote observation is never `Live`

The spec's central claim, asserted by name.

- [x] On receipt, `SourceKind::Watched` becomes `SourceKind::Polled { interval }`, where `interval` is the peer's configured poll interval.
- [x] `Polled` observations keep their interval unchanged — they were already honest about being periodic.
- [x] Document the rule on the conversion function, with the reason: `Live` means "I read this myself."

**Verified:** `a_remote_observation_is_never_live` asserts it across
every `now` a client could plausibly hold — including the instant the
probe claims it read the file, which is the case a naive implementation
gets wrong. A companion test confirms the re-stamped value does go
`Stale` once the peer interval lapses, so the rule makes it honest
rather than merely pessimistic.

#### Task 4: Clocks are re-based, never compared

- [x] `receive` computes `age = probe_now - observed_at` on the probe's clock, then sets `observed_at = client_now - age`.
- [x] Saturate at zero when the probe reports an `observed_at` after its own `now`.
- [x] Never compare `observed_at` against the client's clock directly, anywhere.

**Verified:** three tests. A probe an hour ahead and a probe an hour
behind both yield a true 30-second age. A probe whose clock jumped
backwards mid-scan, reporting an observation after its own `now`, ages
to zero rather than wrapping.

---

## Arc 2: The probe

### Slice 2.1: The crate

#### Task 5: Add the `probe` workspace member

- [ ] `probe/Cargo.toml`: `parallax-baseline` by path, `tiny_http`, `serde_json`. Nothing else from the workspace — it must not link panopticon.
- [ ] Add `"probe"` to the workspace `members`.
- [ ] `#![warn(missing_docs)]` in `main.rs`.
- [ ] A test asserting `Cargo.toml` never grows a `panopticon` dependency, in the shape baseline already uses to keep Plumb out of itself.

**Verify:** `cargo build -p parallax-probe` and workspace clippy pass.

#### Task 6: Registry to envelope

The pure half of the probe, testable with no socket.

- [ ] `state::envelope(registry: &Registry, config: &AdapterConfig, peer: &str, now: SystemTime) -> StateEnvelope`.
- [ ] Runs `from_manifest` and `aggregate`, exactly as a local frontend would — the probe invents no aggregation of its own.
- [ ] The peer name defaults to the OS hostname and is overridable by `--peer <name>`.
- [ ] A project that fails to load is a degraded row in the envelope, not a dropped one.

**Verify:** point it at `panopticon/fixtures` and assert the envelope
names every fixture project.

### Slice 2.2: Serving

#### Task 7: `GET /state` and `GET /health`

- [ ] `GET /state` → the envelope as JSON, `Content-Type: application/json`.
- [ ] `GET /health` → 200 with no scan, so a slow probe and a dead one are distinguishable.
- [ ] Any other path → 404. Any other method → 405.
- [ ] `--port` defaulting to 8737, `--projects-root` and `--registry` mirroring panopticon's flags so the two agree on how a registry is named.

**Verify:** an integration test binding an ephemeral loopback port,
issuing both requests, and parsing the response back into a
`StateEnvelope`.

#### Task 8: The loopback refusal

- [ ] The bind address is not configurable. The probe binds `127.0.0.1` and nothing else.
- [ ] If a future flag or environment variable ever supplies a non-loopback address, the probe exits non-zero with a message naming `tailscale serve` as the intended path.
- [ ] Document the security argument where the bind happens, not only in the spec.

**Verify:** a unit test over the bind-address helper asserting that
every non-loopback address — `0.0.0.0`, a LAN address, and a `100.x`
tailnet address — is refused. The tailnet address is the important case:
it is the one a reasonable person would think is safe.

#### Task 9: The refresh runs no build

- [ ] `GET /state` calls only verification adapters reporting `CheckCost::Read`.
- [ ] An `Execute` adapter is reported as `VerificationOutcome::NotRun`, with a detail naming the command an operator would run.

**Verify:** build a probe over a manifest declaring `cargo test` with a
`CommandRunner` that panics if called. `GET /state` must succeed and the
runner must never fire. This is panopticon's Task 11 property, now with
three machines able to trigger it at once.

---

## Arc 3: Peers

#### Task 10: `peers:` in the registry file

- [ ] `RegistryFile` gains `peers: Vec<PeerEntry>`, defaulting to empty — a registry file with no peers is the current file, unchanged.
- [ ] `PeerEntry { url: String }`, with `deny_unknown_fields`.
- [ ] `Registry::peers()` alongside `projects()` and `failures()`.
- [ ] `Registry::scan` yields no peers: a directory scan is a statement about a disk, and a peer is not on it.

**Verify:** an existing registry file still parses. A file with a
mistyped `peer:` is rejected rather than silently yielding zero peers —
the failure mode `deny_unknown_fields` exists to prevent.

#### Task 11: Peer-qualified identity

- [ ] A project carries which peer it came from; local projects carry `None`.
- [ ] Two projects with the same name from different peers are two distinct rows, not a collision — the desktop holds clones of all three projects, so this is the ordinary case.
- [ ] Ordering: local projects first in registration order, then each peer's in the order peers are listed. `PlatformState::projects` stays documented as deterministic.

**Verify:** merge a local `sesh` and a peer's `sesh` and assert two rows
with distinguishable identity, in a stable order across runs.

---

## Arc 4: The cockpit merges peers

#### Task 12: Fetching a peer on the refresh thread

- [ ] The refresh thread fetches each peer through `HttpTransport`, on the existing poll interval.
- [ ] Fetch, parse, and re-stamp happen off the UI thread — the constraint that nothing blocks the event loop is unchanged and now has a network behind it.
- [ ] A peer's projects enter `PlatformState` through the merge from Task 11.

**Verify:** `FixtureTransport` serving a canned envelope; the cockpit's
view model shows the peer's projects with correct ages under a frozen
clock.

#### Task 13: An unreachable peer degrades only itself

- [ ] A peer that fails to answer becomes `Freshness::Unavailable { last_success }` on its projects' sources.
- [ ] Local projects and every other peer still render — the rule `Registry` and `aggregate` already share, now applied to peers.
- [ ] A peer that answers with malformed JSON is unavailable with a *reason*, not a panic and not silence.

**Verify:** three peers, one unreachable and one serving garbage. The
third peer's projects and all local projects render normally, and both
failures are visible and named.

#### Task 14: Recorded peers in fixture mode

- [ ] `panopticon --fixtures <dir>` picks up recorded peer envelopes alongside the recorded projects it already loads.
- [ ] Determinism holds: two runs against the same fixture set render identical frames, peers included.

**Verify:** the determinism test panopticon already has, extended to a
fixture set containing a peer.

---

## Arc 5: Close-out

#### Task 15: Deployment

- [ ] `probe/README.md`: what it is, the loopback rule, and the one `tailscale serve --bg --https=443 http://127.0.0.1:8737` line.
- [ ] A systemd **user** unit for the probe on the Pi, matching how `seshd` is installed rather than inventing a second pattern — see `SESH/deploy/`.
- [ ] Note that the Pi builds on itself, so the probe is built there too.
- [ ] A worked `registry.yaml` naming all three machines.

**Verify:** probes on all three machines; `panopticon` on the laptop
lists TTUI, Parallax, and SESH. Unplug the Pi and SESH's sources go
unavailable within one poll interval, with nothing claiming `Live`.

#### Task 16: The roadmap converts to named components

Per the spec's resolved open question 1.

- [ ] `README.md`: the roadmap table becomes named components with arcs, not numbered sub-projects. `panopticon` is one row with two shipped arcs, replacing #3 and #5.
- [ ] Add `probe` as a component.
- [ ] `docs/design/README.md`: add the `probe` arc, and say that components carry arcs and numbers are retired.
- [ ] Update the master design's "five sub-projects" framing with a pointer rather than a rewrite — it is an approved document, and amending it in place would erase what it decided.

**Verify:** no numbered sub-project reference survives outside the
historical specs that were approved using them.

---

## Spec coverage

| Spec section | Tasks |
|---|---|
| The wire type is not the domain type | 1, 2 |
| An observation is re-stamped on receipt | 3 |
| A remote observation is never `Live` | 3, 12 |
| Clocks are re-based, never compared | 4 |
| The cost model survives the wire | 9 |
| Peers, not per-project hosts | 10, 11 |
| The probe never listens on a network | 8, 15 |
| The probe | 5, 6, 7 |
| The client | 12, 13, 14 |
| Testing: round-trip | 2 |
| Testing: unreachable peer | 13 |
| Testing: clock skew | 4 |
| Testing: routable bind refused | 8 |
| Testing: `GET /state` runs no `Execute` | 9 |
| Testing: recorded peers | 14 |
| Resolved: roadmap stops numbering | 16 |
