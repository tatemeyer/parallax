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

- [ ] `ActionId` — an opaque client-generated string, with a constructor that takes the bytes rather than inventing them, so no test needs a random source.
- [ ] `ActionRequest { api_version, id, requested_by, action, confirmed }` — `confirmed` is `Option<String>`, a fingerprint, **never a `Confirmation`**.
- [ ] `Submitted { Accepted { id }, Refused { reason }, Unknown { id, reason } }` — no `is_ok`, no `Result` conversion, documented as to why.
- [ ] `ActionStatus { Running, Done { summary }, Refused { reason }, NotSubmitted }` plus the envelope's `started_at`.
- [ ] Unknown fields ignored, matching `wire.rs`'s rule and for the same reason.
- [ ] Round-trip tests for every variant.

#### Task 2: The confirmation crosses as a fingerprint

- [ ] `ActionRequest::confirming(action, confirmation)` builds the request from the pair the operator actually approved.
- [ ] `ActionRequest::confirmation()` rebuilds a `Confirmation` from the carried fingerprint, so the probe calls the existing `authorize` unchanged.
- [ ] Test: a request built for "merge #12" and edited to name #99 is refused by `authorize` as a mismatch, over the wire types, not just locally.

### Slice 1.2: The honesty rule

#### Task 3: The executing machine's classification wins

- [ ] `ActionRequest::authorize_here()` — resolves the confirmation and calls `authorize` with the *local* `Reversibility` table.
- [ ] Test, named for the claim: every member of the confirmation-required group, submitted with no confirmation, is refused whatever the client believed.

#### Task 4: A lost answer is `Unknown`

- [ ] Test: a transport that errors after the request is written yields `Submitted::Unknown` carrying the id, never `Refused`.
- [ ] Test: `Unknown` cannot be collapsed — asserted by the absence of `is_ok`/`From<Submitted> for Result`, as a compile-fail doc test beside the one guarding `Authorized`.

---

## Arc 2: The probe acts

### Slice 2.1: The ledger

#### Task 5: `probe::control`

- [ ] `Ledger { started_at, entries: VecDeque<(ActionId, ActionStatus)>, bound: 256 }`.
- [ ] `record`, `status`, and eviction from the front at the bound.
- [ ] Test: an id never seen, with the ledger young, is `NotSubmitted`.

#### Task 6: The worker

- [ ] A `std::thread` and a channel; `submit` enqueues and returns immediately.
- [ ] Test: a blocking executor still yields a response — the submission is not the execution.
- [ ] Test: the same id twice records one call on a `RecordingExecutor`, and the second answer reports the first's outcome.

### Slice 2.2: Serving

#### Task 7: The two routes

- [ ] `POST /action` and `GET /action/{id}` in `route`, with the existing table's shape.
- [ ] The probe resolves the project in **its own** registry; an unknown name is refused by name.
- [ ] A qualified name (`pi5/sesh`) is refused as a client that failed to strip its own view.

#### Task 8: A restart is not `not-submitted`

- [ ] The envelope carries `started_at`; the client compares it to when it submitted.
- [ ] Test named for the trap: submit, rebuild the ledger with a later `started_at`, ask again — `Unknown`, and the reason names the restart.

#### Task 9: Eviction is not `not-submitted` either

- [ ] Test: overflow the bound, ask about the first id, get `Unknown`.

### Slice 2.3: Opt-in

#### Task 10: `--allow-control`

- [ ] Off by default. `POST /action` without it is `403` naming the flag, refused at the route before the body is read.
- [ ] Test: `/state` still serves with control off.
- [ ] Test: `GET /state` runs no `Execute` adapter **with control on**.

#### Task 11: The audit line

- [ ] Every accepted action logs id, action summary, and `requested_by` **labelled as a claim**.
- [ ] Test: the sink is injected, and records the claim verbatim.

---

## Arc 3: The cockpit routes

#### Task 12: `RemoteExecutor`

- [ ] In `baseline/src/actions/remote.rs`, over `HttpTransport`, so `FixtureTransport` records a submission exactly as it records a fetch.
- [ ] Short timeouts — the only thing being waited on is "I have this".

#### Task 13: Route instead of refuse

- [ ] `app.rs` consults the row's peer and picks an executor; the refusal survives for a peer with no control.
- [ ] Test: the old refusal test becomes a routing test, and a *new* refusal test covers a control-disabled peer.

#### Task 14: The prompt names the machine

- [ ] A remote action's prompt says which machine it will run on; a local one does not gain noise.
- [ ] Test both halves.

#### Task 15: `Unknown` in the log

- [ ] Its own rendering, neither `ok` nor error, naming the machine and quoting the action.
- [ ] Test: the entry for an `Unknown` is not marked `ok` and does not read as a failure.

---

## Arc 4: Close-out

#### Task 16: Deployment

- [ ] The unit file gains the flag, commented so the decision is visible.
- [ ] `probe/README.md` documents enabling control and what it costs.

#### Task 17: Perceptual

- [ ] A Plumb scenario for the remote prompt, the `unknown` entry, and the control-disabled refusal.

#### Task 18: The roadmap

- [ ] `panopticon` and `probe` gain the arc; the README's refusal sentence is retired.

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
