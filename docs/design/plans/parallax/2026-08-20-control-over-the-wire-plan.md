# Control Over the Wire — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> `superpowers:subagent-driven-development` or
> `superpowers:executing-plans`. Steps use checkbox (`- [ ]`) syntax.
>
> **Structure note:** organized as **Arcs → Slices → Tasks** per
> `docs/design/README.md`.

**Goal:** An operator at any one of three machines can act on a project
that lives on another — merge its pull request, label its work, trigger
its captures — with the confirmation decided by the machine that will
execute, and with an action whose answer was lost reported as *unknown*
rather than as a failure.

**Spec:**
`docs/design/specs/parallax/2026-08-20-control-over-the-wire-design.md`.

**Architecture:** Four arcs, each its own PR, ordered so nothing is
built before the thing it carries. Arc 1 is the submission contract
inside baseline — pure types, pure conversions, no network and no new
crate. Arc 2 gives the probe a ledger and a worker and opens the two
routes. Arc 3 teaches the cockpit to route instead of refuse. Arc 4
deploys and closes out.

Arcs 1–3 are testable with no second machine: a submission is a function
from an action to bytes, a ledger is a function from an id to a status,
and `FixtureTransport` already records HTTP. Nothing needs a Raspberry
Pi until Arc 4.

**Tech Stack:** Rust (stable, 2021 edition). No new dependencies —
`serde`, `serde_json`, `sha2`, and `tiny_http` are all already here. **No
async runtime**, matching the constraint panopticon and the probe both
hold; the probe's worker is a `std::thread` and a channel.

---

## Global Constraints

**Authorization is re-decided by the machine that executes.** The probe
calls `authorize` itself. No wire type may deserialize into an
`Authorized`, and Task 3 asserts the classification is the probe's.

**A failed request is never rendered as a failed action.** `Submitted`
has three variants and no `is_ok()`, for the reason `ObservedWire` has
no `freshness()`. Task 4 asserts the third variant survives to the log.

**`not-submitted` requires the ledger to have outlived the
submission.** Otherwise `Unknown`. Tasks 8 and 9 assert both causes —
restart and eviction.

**Control is off unless asked for.** The default probe has no write
path. Task 10 asserts the `403`, and that the body is never parsed.

**No wall clock in any test.** Every `now` is injected, including the
ledger's `started_at`.

**No network in any test.** Submissions go through `HttpTransport`, the
seam peers and GitHub already use.

**`GET /state` still runs no `Execute` adapter**, with control enabled.
The existing assertion is re-run against a control-enabled probe rather
than trusted to still hold.

**Baseline and the probe stay headless.** The probe's audit line is the
one exception and goes to stdout, which is the journal under the unit.

**Soft ceiling of 500 lines per file**, tests included.

**Every `pub` item documented.** `#![warn(missing_docs)]` plus CI's
`-D warnings`.

---

## File Structure

```
baseline/src/actions/
  wire.rs        NEW  ActionId, ActionRequest, Submitted, ActionStatus
  remote.rs      NEW  RemoteExecutor: submit, poll, and the Unknown rule
  mod.rs              re-exports
probe/src/
  control.rs     NEW  the ledger, the worker, started_at
  server.rs           POST /action, GET /action/{id}, the 403
  main.rs             --allow-control, the audit sink
  deploy/parallax-probe.service   the flag, visible in the unit
panopticon/src/
  app.rs              route instead of refuse
  control/mod.rs      an executor per row, local or remote
  control/prompt.rs   the machine in the prompt
```

---

## Milestones

| # | Milestone | Done when |
|---|---|---|
| 1 | The contract exists | An `ActionRequest` round-trips, and a lost answer is `Unknown` rather than an error |
| 2 | A probe acts | `POST /action` on this desktop merges nothing it was not asked to, and running it twice acts once |
| 3 | The cockpit routes | `m` on a fixture peer's row submits rather than refusing, and the prompt names the machine |
| 4 | It runs for real | From the desktop, an action taken on the Pi's `sesh` lands on the Pi |

---

## Arc 1: The submission contract

### Slice 1.1: The types

#### Task 1: `actions::wire`

**Revised during implementation.** Two corrections, both making the
design more honest rather than less.

*`ActionStatus` gained `Failed`.* An action that ran and failed is a
**real answer** — there is nothing uncertain about a merge conflict —
and folding it into `Refused` (never ran) or into the unknown would have
lost exactly the distinction this arc is about.

*The restart marker is an identity, not a timestamp.* The spec had the
envelope carry `started_at` and the client compare it to when it
submitted. That is a comparison between two machines' wall clocks, which
the previous arc forbids for good reason — the Pi has no RTC. A
`ProbeRun` token identifies one run of one process and needs no clock at
all. See Task 8.

- [x] `ActionId` — an opaque client-generated string, with a constructor that takes the bytes rather than inventing them, so no test needs a random source.
- [x] `ActionRequest { api_version, id, requested_by, action, confirmed }` — `confirmed` is `Option<String>`, a fingerprint, **never a `Confirmation`**.
- [x] `Submitted { Accepted { id }, Refused { reason }, Unknown { id, reason } }` — no `is_ok`, no `Result` conversion, documented as to why.
- [x] `ActionStatus { Running, Done { summary }, Refused { reason }, NotSubmitted }` plus the envelope's `started_at`.
- [x] Unknown fields ignored, matching `wire.rs`'s rule and for the same reason.
- [x] Round-trip tests for every variant.

#### Task 2: The confirmation crosses as a fingerprint

**Revised during implementation.** The plan called for rebuilding a
`Confirmation` from the carried fingerprint, which would have needed a
second constructor on a type whose entire value is having exactly one.
Unnecessary: having checked that the carried fingerprint is the one the
action hashes to, `Confirmation::of(&action)` produces that same value
from the action itself. The wire needs no back door.

- [x] `ActionRequest::new(id, requested_by, action, confirmation)` builds the request from the pair the operator actually approved.
- [x] `authorize_here` checks the carried fingerprint against the action's before authorizing — no `Confirmation::from_fingerprint` was added.
- [x] Test: a request built for "merge #12" and edited to name #99 is refused by `authorize` as a mismatch, over the wire types, not just locally.

### Slice 1.2: The honesty rule

#### Task 3: The executing machine's classification wins

- [x] `ActionRequest::authorize_here()` — resolves the confirmation and calls `authorize` with the *local* `Reversibility` table.
- [x] Test, named for the claim: every member of the confirmation-required group, submitted with no confirmation, is refused whatever the client believed.

#### Task 4: A lost answer is `Unknown`

- [x] Test: a transport that errors after the request is written yields `Submitted::Unknown` carrying the id, never `Refused`.
- [x] Test: `Unknown` cannot be collapsed — asserted by the absence of `is_ok`/`From<Submitted> for Result`, as a compile-fail doc test beside the one guarding `Authorized`.

---

## Arc 2: The probe acts

### Slice 2.1: The ledger

#### Task 5: `probe::control`

- [x] `Ledger { started_at, entries: VecDeque<(ActionId, ActionStatus)>, bound: 256 }`.
- [x] `record`, `status`, and eviction from the front at the bound.
- [x] Test: an id never seen, with the ledger young, is `NotSubmitted`.

#### Task 6: The worker

- [x] A `std::thread` and a channel; `submit` enqueues and returns immediately.
- [x] Test: a blocking executor still yields a response — the submission is not the execution.
- [x] Test: the same id twice records one call on a `RecordingExecutor`, and the second answer reports the first's outcome.

### Slice 2.2: Serving

#### Task 7: The two routes

- [x] `POST /action` and `GET /action/{id}` in `route`, with the existing table's shape.
- [x] The probe resolves the project in **its own** registry; an unknown name is refused by name.
- [x] A qualified name (`pi5/sesh`) is refused as a client that failed to strip its own view.

#### Task 8: A restart is not `not-submitted`

**Revised during implementation:** by run identity rather than by clock.
See Task 1.

- [x] Every reply carries the probe's `ProbeRun`; the client reads a `not-submitted` against the run that accepted the action.
- [x] Test named for the trap: submit, rebuild the ledger with a later `started_at`, ask again — `Unknown`, and the reason names the restart.

#### Task 9: Eviction is not `not-submitted` either

- [x] Test: overflow the bound, ask about the first id, get `Unknown`.

### Slice 2.3: Opt-in

#### Task 10: `--allow-control`

- [x] Off by default. `POST /action` without it is `403` naming the flag, refused at the route before the body is read.
- [x] Test: `/state` still serves with control off.
- [x] Test: `GET /state` runs no `Execute` adapter **with control on**.

#### Task 11: The audit line

- [x] Every accepted action logs id, action summary, and `requested_by` **labelled as a claim**.
- [x] Test: the sink is injected, and records the claim verbatim.

---

## Arc 3: The cockpit routes

**Revised during implementation: a courier thread, not the refresh
thread.** The plan assumed submissions would ride the existing refresh
cycle. Two things ruled that out, and the second is the one that
decided it.

`tests/read_only.rs` encodes that observation may not name an action,
and names the refresh thread as observation. Putting control there would
have meant deleting that guarantee rather than keeping it.

And it would have been slow in the way that matters. A read sweep walks
every project and then every peer, and an asleep peer costs the connect
timeout — so `m` pressed at the wrong moment would have waited behind
two dead machines before being *sent*. A keystroke that acts must not
queue behind a poll that only looks.

`panopticon/src/courier.rs` is that thread, and it joins the read-only
exemption list with the argument written down beside it.

**Also revised: a `Target`, not a destination per row.** Local rows sit
still in registry order, but a peer's rows arrive and leave as that
machine answers — a destination looked up by row number means something
different one frame later. The machine is named instead, and the target
travels with the prompt so an answer cannot land on a machine the
operator was not asked about.

#### Task 12: `RemoteExecutor`

- [x] In `baseline/src/actions/remote.rs`, over `HttpTransport`, so `FixtureTransport` records a submission exactly as it records a fetch.
- [x] Short timeouts — the only thing being waited on is "I have this".

#### Task 13: Route instead of refuse

- [x] `app.rs` consults the row's peer and picks an executor; the refusal survives for a peer with no control.
- [x] Test: the old refusal test becomes a routing test, and a *new* refusal test covers a control-disabled peer.

#### Task 14: The prompt names the machine

- [x] A remote action's prompt says which machine it will run on; a local one does not gain noise.
- [x] Test both halves.

#### Task 15: `Unknown` in the log

- [x] Its own rendering, neither `ok` nor error, naming the machine and quoting the action.
- [x] Test: the entry for an `Unknown` is not marked `ok` and does not read as a failure.

---

## Arc 4: Close-out

#### Task 16: Deployment

- [x] The unit file gains the flag, commented so the decision is visible.
- [x] `probe/README.md` documents enabling control and what it costs.

#### Task 17: Perceptual

**Only partly done, and the rest is a question rather than an
omission.** The control-disabled refusal is captured: `cockpit-peer`
already drives it, and its intent was rewritten, because the refusal it
described word for word is not the refusal the cockpit now gives.

The remote prompt and the `unknown` log entry **cannot** be captured as
things stand. Both need a cockpit that has submitted something, and
fixture mode builds no courier — which the spec lists as a non-goal in
as many words: "a recorded cockpit stays inert". Capturing them means
either pointing a recorded cockpit at a real machine, which is the demo
with a loaded weapon the fixture rule exists to prevent, or giving
fixture mode a *recorded* submitter over `FixtureTransport`, exactly as
peers already work — which cannot reach any machine and would be
deterministic.

The second is probably right and is not this arc's to decide, because it
amends an approved non-goal. Raised as open question 4 on the spec.

- [x] The control-disabled refusal, in `cockpit-peer`, with its intent corrected.
- [ ] The remote prompt and the `unknown` entry — blocked on the question above.

#### Task 18: The roadmap

- [x] `panopticon` and `probe` gain the arc; the README's refusal sentence is retired.

---

## Spec coverage

| Spec section | Task |
|---|---|
| Authorization does not travel | 2, 3 |
| A submission is not an execution | 6 |
| A failed request is not a failed action | 4, 15 |
| `not-submitted` is a claim about a ledger | 8, 9 |
| The ledger is bounded | 5, 9 |
| Control is opt-in per machine | 10, 16 |
| The requester is a claim | 11 |
| Routing, and the name that crosses | 7, 13 |
| What the confirmation prompt says | 14 |
