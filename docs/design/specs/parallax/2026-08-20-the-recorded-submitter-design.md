# The Recorded Submitter

**Status:** proposed
**Amends:** `2026-08-20-control-over-the-wire-design.md`, one non-goal.
**Size:** one arc.

## Why this exists

Control over the wire put two new things on the cockpit's screen: a
confirmation prompt that names the machine an action would run on, and a
log entry for an action whose fate is genuinely unknown. Neither can be
judged, because neither can be captured.

Every other pane in this cockpit is verified perceptually. A scenario
runs the real binary against a recorded fixture set, and an agent judges
the frames against a written intent. The two pieces control added are
exempt only by accident: fixture mode builds no courier, so a recorded
cockpit cannot be asked to do anything, so there is nothing to
photograph.

That exemption was deliberate, and the reason was good:

> **Control in fixture mode.** A recorded cockpit stays inert. A demo
> that could merge a real pull request is a demo with a loaded weapon in
> it.

This document keeps the reason and drops the rule, because they are not
the same thing.

## The distinction the old rule missed

A fixture-mode cockpit holds two kinds of destination, and they are not
equally dangerous.

**A local action is dangerous, and stays inert.** `Destination::Local`
wraps an executor that shells out on *this* machine. Running it because
a recording said so would merge a real pull request in a real
repository. Fixture mode replaces every local destination with
`Destination::Nowhere`, and this document does not touch that.

**A remote submission cannot be dangerous, because it cannot arrive.** A
remote action is an HTTP request through a `HttpTransport`. In fixture
mode that transport is a `FixtureTransport`, which owns a `HashMap` of
recorded responses and has no socket, no client, and no address
resolution anywhere in it. It is the same seam every recorded peer
already crosses: `GET /state` in fixture mode reaches no machine for
exactly this reason, and nobody has ever called that a loaded weapon.

The old rule said "no courier." The property it was protecting was "no
cockpit rendering a recording can cause a real machine to act." A
submitter over `FixtureTransport` keeps the property and loses the rule.

## What is being added

**A fixture peer may carry a recording of its control surface.** Beside
`peers/<name>.json`, which holds exactly what that machine's probe would
have served from `GET /state`, an optional `peers/<name>.control.json`
holds what it would have answered from `POST /action` and
`GET /action/<id>`.

**Its presence is what enables control for that peer** — mirroring
`--allow-control` on a real probe, where the machine that would execute
is the one that decides. A fixture peer with no control file is a
machine watched but not acted on, which is the default in the fixture
set exactly as it is in a deployment.

**The submitter is the real one.** `RemoteExecutor<FixtureTransport>`,
not a hand-written double. The id generation, the JSON, the 4xx-versus-
everything-else reading of a failure, the ledger-run comparison — all of
it is the shipping code. Only the bytes are recorded. A fake submitter
would be a second implementation of the thing being photographed, and a
photograph of a second implementation proves nothing about the first.

## The rule that replaces the old one

**A fixture peer's name must not be able to resolve.**

Peer URLs are synthesized from file names: `peers/pi5.json` becomes
`https://pi5`. That was already documented as a rule — "no fixture set
should contain a real address, because a fixture that could reach a
network is not a fixture" — and it was a comment. It becomes a check.

A name is refused if it contains a dot or parses as an IP address. Both
are the shapes that could name a machine on a network; a bare label
cannot, and that is the whole of the requirement.

This matters more now than it did. A recorded `GET /state` against a
name that resolved would leak a fixture run onto a network. A recorded
`POST /action` against one would be the loaded weapon the old non-goal
was written to prevent — so the guarantee moves from "we do not build
the thing" to "the thing cannot be pointed anywhere", which is checked
rather than promised.

`FixtureTransport` makes the check redundant in fixture mode: it has no
socket, so a resolvable name would still reach nothing. It is here
because the guarantee should not rest on the current implementation of a
transport that could reasonably grow a passthrough for recording, and
because a fixture set is data, which is the part of a system that gets
copied without its reasons.

## Non-goals

**Local actions in fixture mode.** Unchanged and unchallenged: a
recorded cockpit does not run commands on the machine it is displayed
on.

**Recording a real session.** The control files are written by hand, the
way every other fixture in this repository is. A record-and-replay
harness pointed at a live probe would be a genuinely useful thing and is
not this.

**Fidelity of timing.** A recorded submission answers instantly. The
real one takes as long as a tailnet takes. The scenarios are about what
the screen says, not how long it takes to say it.

## Open questions for sign-off

1. **Should a fixture peer's control file be able to record a
   *sequence*** — accept, then running, then done — rather than one
   reply per endpoint? The `unknown` and `refused` entries need one
   reply each, and a status poll returning the same answer twice is
   what the cockpit already tolerates. A sequence would let a scenario
   show an action moving from `..` to `ok` on screen, which is the one
   piece of the log's behaviour a single reply cannot show.
   **Recommendation: one reply now, sequence when a scenario needs it.**
   The shape is additive and the arc is small; inventing the sequence
   before there is a frame that needs it is the more expensive mistake.
