# Parallax — Control Over the Wire (Design)

**Status:** proposed. **Date:** 2026-08-20

**Amends:**
`docs/design/specs/parallax/2026-08-19-remote-hosts-and-the-probe-design.md`,
which named this arc in its non-goals and said why it was deferred, and
`docs/design/specs/panopticon/2026-08-20-cockpit-control-design.md`,
which defined the confirmation contract on one machine. Nothing below
contradicts either. The first says a cockpit can *see* three machines;
the second says what it means to *act*; this says what happens when
those are not the same machine.

**Found by:** the refusal this platform ships today. Pressing `m` on the
Pi's `sesh` row prints

> `pi5/sesh is on pi5. This cockpit acts only on the machine it runs on
> — control across the wire is not built yet.`

(`panopticon/src/app.rs:384`). That sentence was the correct thing to
write when observation crossed the wire and control did not. It is also
a promise, and this document is the arc that keeps it.

## Context / Motivation

**A cockpit that can see a machine it cannot act on is half a cockpit,
and the half it is missing is the one that costs an operator a walk to
another room.** The concrete workflow: SESH runs on a Raspberry Pi wired
to a television, an agent session finishes there, its pull request is
green, and the only way to merge it is to open a session on the Pi. The
desktop's cockpit lists that pull request, knows it is green, has an `m`
key bound to merging it, and refuses — correctly, because the executor
behind `m` is built from *local* projects and would have merged the
wrong repository's pull request of the same number.

The previous arc did not merely leave this out. It found that leaving it
out **silently** was a trap: a bare project name dispatched to the local
refresh thread matched the local clone, so `c` on the Pi's `sesh` ran
`cargo test` against this machine's `sesh` — the wrong machine, with the
operator's row never changing to show it. The refusal above is the fix,
and it is deliberately exhaustive: `acts_on_the_selected_project`
(`panopticon/src/keys.rs:72`) classifies every verb, so a verb added
later must decide which side it is on.

That exhaustiveness is what makes this arc tractable. There is exactly
one place where a remote action is refused today, and exactly one
decision to change there: refuse, or route.

### What already exists, and what it is worth

**The confirmation contract is already headless and already the right
shape.** `authorize` (`baseline/src/actions/confirm.rs:113`) takes an
action and an optional `Confirmation` and returns an `Authorized` whose
field is private — so the only path to an executor is through the check.
`Confirmation::of` takes the action itself, so confirming "merge #12"
cannot authorize "merge #99". None of that is cockpit code. A probe can
call it as easily as a TUI can.

**The action set is already serializable.** `Action` derives
`Serialize`/`Deserialize` with `#[serde(tag = "action")]`, and a test
already asserts it round-trips through JSON *because a confirmation must
be fingerprintable*. The wire format for this arc is, in the boring
sense, done.

**The probe already refuses to be written to.** `route` returns
`MethodNotAllowed` for a `POST` to `/state`, under a test that says why:
"Arc 1 is read-only. A POST that fell through to `/state` would be a
control surface nobody specified" (`probe/src/server.rs:179`). This arc
is the specification that test was waiting for.

So the machinery is in place, and this document is mostly about the four
things that are **not** the same once a network is in the middle.

## Design

### Authorization does not travel. It is re-decided where it acts.

The tempting shape is to authorize in the cockpit — where the human
is — and send the resulting approval to the probe. It cannot work, and
the reason is worth stating precisely, because it is the difference
between a type system and a protocol.

`Authorized` is safe locally because its field is private: within one
process, the compiler guarantees that every value of that type came from
`authorize`. **A network erases that guarantee.** Whatever bytes
represent "already authorized" can be typed by hand into `curl`. Sending
an `Authorized` over the wire would be exactly the "conjure a
confirmation from a bare `true`" that the private field exists to
prevent, reintroduced one layer down.

So the wire carries **the action and the fingerprint the operator
confirmed**, and the probe calls `authorize` itself. Two properties
follow, and only one of them is security:

- **The executing machine's classification wins.** Whether `Push` needs
  confirming is decided by the `Reversibility` table compiled into the
  *probe*, not the caller. A cockpit that is out of date, or wrong, or
  hostile, cannot make the Pi treat an irreversible action as a
  reversible one. This is a real structural property and gets a test
  named for it.
- **The fingerprint is a consistency check, not an authentication.** A
  client can trivially compute the fingerprint of whatever action it is
  sending. What the check catches is a client that confused two
  actions — a real bug class, and the one the local contract was built
  against — and what it does not catch is a client that is lying. Saying
  so here is the point. The security boundary for control is the same as
  for reads: the probe binds loopback, `tailscale serve` publishes it,
  and the tailnet's ACLs decide who may reach it. Anyone who can `POST`
  could equally have opened an SSH session to the same machine.

### A submission is not an execution

`POST /action` **enqueues**. It does not run the action and return the
result.

This is not deferred work dressed as a design. The read path's timeouts
exist because a sleeping machine accepts a connection and goes quiet,
and a synchronous control request has a worse version of that problem:
the actions worth taking remotely are the slow ones. `TriggerCapture`
drives a PTY through a scripted scenario. `DispatchAgentRun` starts an
agent. A build check on a Raspberry Pi 5 is minutes. A request that
holds a socket open for the duration is a request that cannot be given
an honest timeout — short enough to detect a dead machine, long enough
to finish a build, pick one.

So the probe accepts the action, hands it to a worker, and answers
immediately with the id the client sent. The outcome is fetched later.
The client's timeout can then be short, because the only thing it is
waiting on is a machine saying "I have this".

**The pleasing consequence: an action's result becomes an observation.**
It comes back through the read path this platform already made honest,
rather than through a second mechanism with its own freshness rules.

### A failed request is not a failed action

This is the arc's central claim, and the analogue of the last arc's
"a remote observation is never `Live`".

When a `POST` times out, three things are equally consistent with what
the client saw: the request never arrived; it arrived and the action ran;
it arrived, ran, and the *answer* was lost. **Rendering that as a
failure is a lie of exactly the kind `Live` was** — and a more expensive
one, because an operator who reads "merge failed" will press `m` again.

The enforcement is structural rather than a convention. Local execution
returns `Result<ActionOutcome, ActionError>`, a two-valued shape in
which everything that is not success is failure. A remote submission
does not get to use it. It returns

```rust
pub enum Submitted {
    /// The probe has the action and will run it. Ask again by id.
    Accepted { id: ActionId },
    /// The probe refused it outright, and this is final.
    Refused { reason: String },
    /// The request did not complete. Whether the action ran is unknown.
    Unknown { id: ActionId, reason: String },
}
```

`Unknown` is a variant, not an `Err`, and carries the id — which is what
makes it recoverable rather than merely honest: the client can ask the
probe about that id and find out. The cockpit's action log renders it as
its own state, with its own words ("unknown — `pi5` did not answer;
it may have merged #12"), never as a failure.

There is deliberately no `is_ok()` on this type, for the same reason
`ObservedWire` has no `freshness()` (`baseline/src/wire.rs:71`): a
caller that could collapse three states into two would, and the state it
would drop is the only one that needed saying.

### `not-submitted` is a claim about a ledger, not about the world

The probe records what it has executed in an in-memory ledger keyed by
the client's id, so that asking twice is safe and re-submitting the same
id does not run the action twice. That gives `GET /action/{id}` four
answers: `running`, `done`, `refused`, `not-submitted`.

**The fourth is a trap, and closing it is the sharp part of this
design.** A probe restarts — deployed, updated, the Pi rebooted. The
ledger is memory and dies with it. An action submitted before the
restart now answers `not-submitted`, and a client that believes that
will re-run a merge that already happened.

So the envelope carries the probe's `started_at`, and **`not-submitted`
is only meaningful for an id submitted after it.** A client that
submitted earlier is told `Unknown`, with the reason naming the restart.
The check lives on the client, next to the re-basing rule it resembles:
the probe reports a fact about itself, and the client is the one that
knows when it asked.

This is the same failure the last arc found in "a machine nobody has
heard from is not `ok`" — a check that never happened, reported as a
check that passed.

### The ledger is bounded, and forgetting is not `not-submitted` either

An unbounded map keyed by client nonces is a memory leak on a machine
with 8GB and a kiosk on it. The ledger keeps the most recent N entries
(N = 256, which is far more actions than an operator takes in a session)
and the id-count watermark of the oldest it still holds. An id it has
evicted gets `Unknown`, not `not-submitted` — same rule as the restart,
same reason.

### Control is opt-in per machine

Deploying a probe must not silently make a machine remotely
commandable. `/state` is a disclosure; `POST /action` is a shell. They
should not arrive together by default.

The probe therefore serves control only when started with
`--allow-control` (`ParallaxProbeAllowControl=1` in the unit file, so
the decision is visible in the deployment). Without it, `POST /action`
is `403` with a message naming the flag — not `404`, because a client
that gets `404` cannot tell "control is off here" from "this probe is
too old to have control at all", and those call for different operator
actions.

The refusal is at the route, before the body is parsed, so a
control-disabled probe never deserializes an `Action` at all.

### The requester is a claim, not an identity

The request carries the name the calling cockpit calls itself, and the
probe writes it to the audit line it emits for every action it accepts.
**It is recorded as a claim and labelled as one**, because there is no
authentication: the field says who *says* they asked. It is worth having
anyway — with three machines, "who" is nearly always a debugging
question rather than a forensic one, and an audit line that says
`desktop` beats one that says `127.0.0.1` (which is what the socket
reports for every request, since `tailscale serve` is the thing
connecting).

Every accepted action is written to the probe's log — stdout, which
under the systemd user unit is the journal. **The machine that did the
thing keeps the record**, which is the property the cockpit's in-memory
log explicitly does not provide.

### Routing, and the name that crosses

The cockpit already knows which machine a row is on: `ProjectState::peer`
is `Some` for a peer's row and drives both the qualified name and
today's refusal. The change at `app.rs:378` is to consult a route rather
than refuse: a local row goes to the `LocalExecutor` it goes to now; a
peer's row goes to a `RemoteExecutor` bound to that peer's URL.

**The action carries the bare project name.** Qualification (`pi5/sesh`)
is a client-side identity for telling two rows apart on one screen; it
means nothing on the Pi, where the project is just `sesh`. The probe
resolves the name in **its own registry**, which is precisely the
property that makes remote control correct where the old bug was wrong —
the machine that owns the project is the one that looks it up.

A name the probe does not have is refused by name (``no project `ttui`
on this machine``), not ignored. The last arc's rule holds here too: a
check that cannot happen must say so rather than pass.

### What the confirmation prompt says

The prompt is raised in the cockpit, as it is today, and gains one
sentence: **which machine the action will run on.** "merge pull request
#12" and "merge pull request #12 **on pi5**" are different decisions,
and the operator is the only one who can tell whether the first was
meant. The summary that travels is the one the operator saw.

## Non-goals

- **Authentication and TLS between cockpit and probe.** The tailnet is
  the boundary, unchanged from the read arc, and now stated for a write
  path where it matters more. If control ever leaves the tailnet, this
  is the first thing that must change, and it is not a small change.
- **Per-action allowlists.** A machine's control is on or off. A
  four-line table of which verbs each peer will accept is configuration
  that has to be right in two places, and with three machines the honest
  version is the flag.
- **A durable action ledger.** In memory, bounded, and dies with the
  process — consistent with the cockpit's own log, and with the
  `Unknown` rule that makes the volatility safe rather than silent. A
  durable audit trail is a different thing and is not this.
- **Cancelling a submitted action.** `StopAgentRun` exists as an action
  in its own right; cancelling *the submission* is a second control
  plane over the first.
- **Streaming an action's output back.** The outcome is a result, not a
  terminal. Watching a build run on another machine is what the sessions
  pane already shows.
- **Control in fixture mode.** A recorded cockpit stays inert, exactly
  as it is now, and says so rather than appearing to work.

## Testing

- **An `Authorized` cannot be transmitted.** A compile-fail test, beside
  the one that already guards its constructor: the wire request type
  holds an `Action` and a fingerprint, and there is no serialization
  path that produces an `Authorized` on the far side.
- **The executing machine's classification wins.** A request that
  presents an irreversible action with no confirmation is refused by the
  probe, whatever the client believed — asserted for every member of the
  confirmation-required group, so a reclassification cannot pass silently.
- **A confirmation for a different action is refused across the wire**,
  reusing the fingerprint mismatch the local contract already defines.
- **A submission is not an execution.** `POST /action` answers before
  the action completes: a scripted runner that blocks until released
  still yields a response, and the response carries the id.
- **The same id twice runs the action once.** Asserted with a recording
  executor: two identical submissions, one recorded call, and the second
  response reports the first's outcome.
- **A lost answer is `Unknown`, never a failure.** A transport that
  accepts and then dies yields `Submitted::Unknown` carrying the id, and
  the cockpit's log entry for it is neither `ok` nor an error.
- **An id from before the probe restarted is `Unknown`, not
  `not-submitted`.** The test that names the trap: submit, restart the
  ledger with a later `started_at`, ask again.
- **An id the ledger has evicted is `Unknown` too.** Same rule,
  different cause; overflow the bound and ask about the first.
- **A control-disabled probe refuses `POST /action` with `403` naming
  the flag, and never parses the body.**
- **A control-disabled probe still serves `/state`.** Turning control
  off must not cost observation.
- **The probe resolves the project in its own registry**, and refuses an
  unknown name by name.
- **A qualified name is refused.** `pi5/sesh` sent to the Pi is a client
  that failed to strip its own view of the world.
- **`GET /state` still runs no `Execute` adapter**, unchanged, with
  control enabled — the read path does not become a write path because
  a write path exists next to it.
- **Every accepted action is audited**, with the requester recorded as a
  claim.
- **The confirmation prompt names the machine** for a remote row, and
  does not for a local one.
- **A recorded cockpit is still inert.**

## Critical files

| file | change |
|---|---|
| `baseline/src/actions/wire.rs` | new — `ActionRequest`, `ActionId`, `Submitted`, `ActionRecord` |
| `baseline/src/actions/remote.rs` | new — `RemoteExecutor`, submission over `HttpTransport` |
| `baseline/src/actions/mod.rs` | re-exports; the action set is unchanged |
| `probe/src/control.rs` | new — the ledger, the worker, `started_at` |
| `probe/src/server.rs` | `POST /action`, `GET /action/{id}`, the `403` |
| `probe/src/main.rs` | `--allow-control`, and the audit sink |
| `probe/deploy/parallax-probe.service` | the flag, visible in the unit |
| `panopticon/src/app.rs` | route instead of refuse |
| `panopticon/src/control/mod.rs` | a remote executor per peer row |
| `panopticon/src/control/prompt.rs` | the machine in the prompt |

## Verification

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo fmt --check` clean at the workspace root.
- A probe with control disabled: `/state` serves, `POST /action` is
  `403` and names the flag.
- Two probes on one machine, one with control and one without, exercise
  both paths with no second machine.
- On the real three: from the desktop, merge a pull request on the Pi's
  `sesh`, and see the merge land on GitHub and the Pi's row update on
  the next poll.
- With the probe stopped mid-action, the cockpit's log shows `unknown`
  naming the machine, and re-submitting the same id after the probe
  returns does not act twice.
- A Plumb scenario for the remote prompt, the `unknown` log entry, and
  the refusal of a control-disabled peer.

## Open questions for sign-off

1. **Does the confirmation prompt for a remote action need a second
   step?** The argument for: `m` on a row one keystroke away from a
   different machine's row merges a real pull request on a machine the
   operator is not sitting at. The argument against: the prompt already
   quotes the action and would now name the machine, and a confirmation
   an operator learns to dismiss twice is worth less than one they read
   once. **Recommendation: no second step, name the machine loudly.**
2. **Should `--allow-control` be per-peer on the client too** — a
   cockpit that declines to offer control on a peer it can reach? It is
   cheap, and it is also the per-action allowlist argument in a smaller
   coat: two places to be right. **Recommendation: no. The machine that
   would execute is the one that gets to say.**
3. **Is 256 the right ledger bound?** It is far more actions than an
   operator takes in a session, and a count has no clock in it — which
   on a machine with no RTC is worth something. **Recommendation: keep
   it; revisit if an operator ever overflows it, which the log shows.**
