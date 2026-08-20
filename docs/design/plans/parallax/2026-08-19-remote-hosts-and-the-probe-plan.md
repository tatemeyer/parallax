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

- [x] `probe/Cargo.toml`: `parallax-baseline` by path, `tiny_http`, `serde_json`. Nothing else from the workspace — it must not link panopticon.
- [x] Add `"probe"` to the workspace `members`.
- [x] `#![warn(missing_docs)]` in `main.rs`.
- [x] A test asserting `Cargo.toml` never grows a `panopticon` dependency, in the shape baseline already uses to keep Plumb out of itself — plus one that it grows no terminal machinery, since headless is the property that lets it run on a Pi with no TUI.

**Verified:** builds clean; `tiny_http` brought only `ascii`,
`chunked_transfer`, and `httpdate` with it, which matters on a Pi that
compiles its own binaries.

#### Task 6: Registry to envelope

The pure half of the probe, testable with no socket.

- [x] `state::envelope(registry, config, peer, now) -> StateEnvelope`.
- [x] Runs `from_manifest` and `aggregate_project`, exactly as a local frontend would — the probe invents no aggregation of its own.
- [x] The peer name defaults to the OS hostname and is overridable by `--peer <name>`. No dependency for it: `COMPUTERNAME`, then `HOSTNAME`, then `/etc/hostname` — the last being the case the first two miss on the Pi.
- [x] A project that fails to load is a degraded row in the envelope, not a dropped one.

**Unplanned change, and the reason for it.** `split_by_cost` lived in
`panopticon::refresh`, and its own doc comment said it was public so
that "a future daemon" could apply the same rule. The probe is that
daemon and cannot depend on a cockpit, so the function **moved down into
`parallax-baseline`** and panopticon now re-exports it. One
implementation, three consumers, which is what the comment was asking
for. A second implementation of "which checks are safe to poll" is how
something ends up running `cargo test` on a timer.

**Verified:** against the real `C:/Users/tatem/Dev` — two registered
projects, TTUI's work feed returning four live GitHub items, SESH's
degrading with the 404 its private repo actually returns.

### Slice 2.2: Serving

#### Task 7: `GET /state` and `GET /health`

- [x] `GET /state` → the envelope as JSON, `Content-Type: application/json`.
- [x] `GET /health` → 200 with no scan, so a slow probe and a dead one are distinguishable.
- [x] Any other path → 404. Any other method → 405 — a `POST` that fell through to `/state` would be a control surface nobody specified.
- [x] `--port` defaulting to 8737, `--projects-root` and `--registry` mirroring panopticon's flags so the two agree on how a registry is named.
- [x] `route()` is a pure function of method and URL, so the table is tested without a socket. A trailing slash and a query string do not change the route.

**Verified:** live against the running probe — `/health` returned `ok`,
`/state` returned 4,416 bytes of valid `parallax/v1` JSON.

**And now as the task originally asked.** That live `curl` was evidence,
not a test; `probe/tests/serving.rs` binds an ephemeral loopback port,
serves from it, and reads it back with the same `PeerClient` a cockpit
uses — so the bytes, the JSON, and the re-stamping are exercised by the
code that ships rather than by a stand-in. Six cases, including two
probes at once (which is how the two-machine shape is reachable on one
machine) and an empty machine, which must answer rather than be
indistinguishable from an unreachable one.

The probe became a library as well as a binary to allow it. `Probe` now
owns its listener, so `tiny_http` no longer appears in the public API and
`main.rs` is thinner — and `Probe::bind(0)` asks the OS for a free port,
which is what makes the tests safe to run on a machine that is already
serving a probe. On these machines, that is all of them.

#### Task 8: The loopback refusal

- [x] The bind address is not configurable. The probe binds `127.0.0.1` and nothing else.
- [x] A non-loopback address is refused with a message naming `tailscale serve` as the intended path.
- [x] The security argument is documented where the bind happens, not only in the spec.

**Verified:** `0.0.0.0`, a LAN address, `::`, and **`100.67.55.58` — this
machine's own tailnet address** are all refused. The tailnet row is the
one that matters: binding it looks safe and would publish the probe to
the whole tailnet directly, bypassing `tailscale serve` and its ACLs.

#### Task 9: The refresh runs no build

- [x] `GET /state` calls only verification adapters reporting `CheckCost::Read`, via the shared `split_by_cost`.
- [x] An `Execute` adapter is replaced by a stand-in reporting `VerificationOutcome::NotRun` with a detail naming the command. Removing it outright would read as "this project has no tests" rather than "nobody has run them".

**Verified twice.** A unit test builds an envelope over a manifest
declaring `cargo test` inside a temp directory — which would fail slowly
and loudly if spawned — and gets `NotRun`. And live: SESH declares
`cargo clippy`, `cargo test`, `cargo fmt --check`, and an `npm` gate;
all four came back `NotRun` and the whole response took under a second.

---

## Arc 3: Peers

#### Task 10: `peers:` in the registry file

- [x] `RegistryFile` gains `peers: Vec<Peer>`, defaulting to empty — a registry file with no peers is the current file, unchanged.
- [x] `Peer { url: String }`, with `deny_unknown_fields`.
- [x] `Registry::peers()` alongside `projects()` and `failures()`.
- [x] `Registry::scan` yields no peers: a directory scan is a statement about a disk, and a peer is not on it.
- [x] `projects:` also defaults to empty, and `is_empty()` now means no local project **and** no peer — a cockpit on a machine holding no checkouts, watching the other two, is the configuration this whole arc exists to enable, and a frontend that refused to start on it would refuse the point.

**Verified:** a registry file written before peers existed still loads. A
mistyped `peer:` and an unknown key inside a peer entry are both refused
by name, rather than silently yielding zero peers — which would read
exactly like a machine that is switched off.

#### Task 11: Peer-qualified identity

- [x] `ProjectState::peer` carries which machine a row came from; local projects carry `None`.
- [x] `qualified_name()` — `sesh` locally, `sesh@pi5` from a peer. Baseline needs *a* unique key; how a cockpit spells it is the cockpit's business.
- [x] `PlatformState::project` finds local projects only, and `qualified` finds any. A bare name means the checkout on this disk; returning the Pi's would be the collision the qualification exists to stop.
- [x] `extend_from_peer` appends and stamps, keeping order deterministic: local first, then each peer as listed.

**Verified:** a local `sesh` and the Pi's `sesh` are two rows with
distinct keys and a stable order. A local refresh cannot overwrite a
peer's row — every local lookup in the cockpit now requires
`peer.is_none()`, which it did not before this arc and would have been
a live bug the moment the Pi came up.

---

## Arc 4: The cockpit merges peers

#### Task 12: Fetching a peer on the refresh thread

- [x] `PeerClient` in baseline fetches, parses, re-stamps, re-bases, and tags — reusing `HttpTransport`, so a peer records exactly as GitHub does.
- [x] `Refresher::spawn_with_peers`; `spawn` delegates to it with none, so all seven existing call sites are untouched.
- [x] Fetch, parse, and re-stamp happen on the refresh thread — the constraint that nothing blocks the event loop is unchanged and now has a network behind it.
- [x] `Update::PeerState` carries a peer's **whole** list, because it is also the answer to "what is no longer there" — a project deleted on the Pi has to leave the rail, and per-project updates could never say so.
- [x] Peers are fetched after local projects: a disk read is cheaper than a round trip, so the rows this machine can answer for appear first.

**Unplanned addition.** `impl HttpTransport for Box<T>` in baseline —
a cockpit holds a live transport and fixture mode holds a recorded one,
and one list cannot hold two concrete types.

**Verified:** a peer's projects arrive as `sesh@pi5` with correct ages
under a frozen clock, with no network in the test.

**Follow-on, once peers could be slow.** The cadence fires every poll
interval whether or not the last cycle finished. That was harmless when
a cycle meant a disk read and a GitHub poll; a peer that has to time out
costs most of an interval by itself, so two dead machines and the sweep
no longer fits between two ticks — and every sweep that runs late makes
the next one later, permanently. Read-refreshes that queue up while one
is running now collapse into a single sweep. Nothing else collapses:
`Stop` must arrive and a `RunChecks` somebody pressed must run, because
losing asked-for work is a worse bug than being slow.

#### Task 13: An unreachable peer degrades only itself

- [x] A failed fetch becomes `Update::PeerFailed`, and the cockpit adds a degradation to that peer's rows — which `sources()` already reports as `Freshness::Unavailable`.
- [x] **The rows keep their last-known values.** The degradation says why they stopped moving rather than blanking them: the last thing a machine said is still the last thing it said, and it goes stale on its own.
- [x] A peer that has never answered gets one row named for itself, because a machine configured and unreachable is a fact worth showing — nothing at all is indistinguishable from never having configured it.
- [x] Malformed JSON and an unreadable version are failures with reasons, not panics on the refresh thread.

**Verified:** three peers, the middle one unreachable. The other two
answered, the failure named `laptop` and carried `connection refused`,
and nothing else lost a row. A peer answering `<html>gateway timeout` and
one speaking `parallax/v2` each fail by name.

**That verification was incomplete, and the gap was real.** Every one of
those cases fails *immediately*, which is what a refused connection looks
like. The failure that actually matters is a machine that accepts and
then says nothing — a laptop asleep mid-conversation, a tailnet route
that stopped forwarding — and `UreqTransport` built its agent with no
timeouts at all, so that read was bounded only by the operating system.

Because peers are fetched one after another on the refresh thread, one
blackholed machine would have stalled every peer behind it *and* never
been reported unavailable itself, since the fetch it was stuck inside is
the thing that reports it. Precisely the failure the freshness model
exists to prevent, arriving through the one path it did not cover.

Fixed with connect and read timeouts sized against the poll interval —
15 seconds total, so two dead peers still finish inside a 30-second
cycle. `baseline/tests/peer_timeouts.rs` holds a socket open and says
nothing; the suite takes 10.12 seconds, which is the read timeout doing
the bounding rather than a connection failing for some other reason.

#### Task 14: Recorded peers in fixture mode

- [x] `fixtures/peers/<name>.json`, one recorded envelope per machine, served by a `FixtureTransport`. The URL is synthesized from the file name — a fixture that could reach a network is not a fixture.
- [x] Files are sorted, so two runs put the same machine in the same row.
- [x] An absent `peers/` directory is a set with no peers, not an error: every recording made before this arc still loads.
- [x] A fixture set may now hold peers and no local projects.

**Verified:** the shipped fixture set carries `tates-laptop`, loads
identically twice down to every source's rendered freshness, and its
`Watched` observations arrive re-stamped rather than `Live`. Without
this, remote hosts would be the one part of the cockpit Plumb could
never judge — a NO-GO on a screen holding a live peer would mean "the
laptop answered differently", not "the layout is wrong".

---

## Arc 5: Close-out

#### Task 15: Deployment

- [x] `probe/README.md`: what it is, both rules, the flags, the routes, and the install.
- [x] A systemd **user** unit, matching how `seshd` is installed rather than inventing a second pattern.
- [x] Note that the Pi builds on itself, so the probe is built there too.
- [x] A worked `registry.yaml` naming all three machines.

**Two things the unit gets deliberately different from `seshd`.** It is
**not** tied to `graphical-session.target`: `seshd` needs
`WAYLAND_DISPLAY` because it puts things on a television, while the probe
draws nothing and answers a cockpit that may be in another room —
stopping it when the TV session ends would stop it exactly when the
machine is most likely to be watched from elsewhere. And it needs
`loginctl enable-linger`, called out in the README because without it the
Pi answers only while somebody is logged in, and that failure looks
precisely like a network problem.

**Unplanned addition, found while writing the unit.** The probe built its
adapters with `AdapterConfig::default()`, so it had no GitHub token and
every private repository degraded to a 404 — which is exactly what SESH
did in the Arc 2 live run. It now takes `--github-token` and falls back
to `$GITHUB_TOKEN` then `$GH_TOKEN`, mirroring panopticon, and the unit
reads them from an `EnvironmentFile` so the token stays out of a file
that gets committed.

**Verified on the real machines, 2026-08-20.** A probe on `tatepi`
published with `tailscale serve --https=443`, reached from the desktop
over a direct connection at 16ms. It serves `parallax` and `sesh`, and
`sesh` carries **17 agent sessions** — which is the thing this whole arc
was for: a cockpit on one machine listing the work in flight on another.

Two deployment notes were wrong before that worked, and both are fixed
above: the unit's `--projects-root` assumed the desktop's `Dev/` layout,
and the install sequence assumed this repository was already on the Pi.

**Milestone 5, both halves.** The cockpit rendered six rows across three
machines with the Pi's sources aged in seconds where local ones read
`live`. Stopping the probe produced two *different* failures at once,
each naming itself and neither costing the others a row:

- `peer:tatepi http 502:` — the machine is up and `tailscale serve` is
  still proxying, so the peer answers with an error rather than being
  unreachable. Worth knowing that this is the common case.
- `peer:tates-laptop timed out … connection timed out` — a machine on
  the tailnet with no probe. It *timed out* rather than hanging, which
  is the transport timeout added earlier doing visible work.

That run was a cold start, though, and a cold start is the easier half:
with nothing remembered there is nothing to lose.
`baseline/tests/peer_transition.rs` holds the other half on a socket —
a peer that answers once and then disappears keeps the rows it served,
reports every one of them `Unavailable`, and leaves nothing behind that
can still claim to be fresh.

#### Task 16: The roadmap converts to named components

Per the spec's resolved open question 1.

- [x] `README.md`: the roadmap table becomes named components with arcs. `panopticon` is one row with its arcs listed, replacing #3 and #5.
- [x] Add `probe` as a component, and to the repository layout.
- [x] The status paragraph was stale — it still called control "sketched" after it shipped. Rewritten, and it now says the platform spans machines.
- [x] `docs/design/README.md`: says directories are named for components, that numbers are retired, and where a component's design lives when it changes a shared contract.
- [x] The master design gets a pointer, not a rewrite, with a translation table for its five numbers. It is an approved document; editing the numbers out would erase the decision rather than supersede it.

**Correction to this task as planned.** It said "add the `probe` arc" to
`docs/design/README.md`. There is no `probe/` spec directory and there
should not be: what the probe introduced first is a platform-wide wire
contract, so it is specified under `parallax/` — the same reason the
registry and adapter factory live there rather than under a `baseline/`.
The design README now states that rule instead of listing a directory
that does not exist.

**Verified:** the only surviving "sub-project" wording outside the
historical approved specs is the sentence in the README explaining why
the numbering was retired.

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
