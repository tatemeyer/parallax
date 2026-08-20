# The Recorded Submitter — Implementation Plan

> **Structure note:** organized as **Arcs → Slices → Tasks** per
> `docs/design/README.md`.

**Goal:** The confirmation prompt that names a remote machine, and the
log entry for an action whose fate is unknown, are captured and judged
like every other pane in this cockpit.

**Spec:**
`docs/design/specs/parallax/2026-08-20-the-recorded-submitter-design.md`.

**Depends on:** the control-over-the-wire arc (#41). This builds on its
`Submitter` trait and its courier.

**Architecture:** One arc, three slices. The guard comes first, because
it is what makes the rest safe to build. Then the recording, then the
frames it exists for.

---

## Global Constraints

**No network in any test, and none in fixture mode.** The whole point:
`FixtureTransport` everywhere, and a fixture peer name that could not
resolve even if it were.

**Local actions in fixture mode stay `Nowhere`.** Untouched. A test
asserts it still holds after this arc, because that is exactly the
guarantee an arc like this one erodes by accident.

**The submitter is `RemoteExecutor`, not a double.** If a test needs a
hand-written `Submitter`, that is a test's business; fixture mode gets
the shipping type.

**Soft ceiling of 500 lines per file**, tests included.

---

## Arc 1: A recorded cockpit can be asked

### Slice 1.1: The guard

#### Task 1: A fixture peer's name cannot resolve

- [x] `load_peers` refuses a name containing a dot, or one that parses as an `IpAddr`.
- [x] The message names the file and says why, in the register `bind_address` uses.
- [x] Test: `pi5` is accepted; `pi5.tail-scale.ts.net`, `10.0.0.4`, and `::1` are each refused.
- [x] Test: the refusal names the offending file, because a fixture set is data and the operator is looking at a directory.

### Slice 1.2: The recording

#### Task 2: The control file

**Revised during implementation.** The bodies are stored as raw JSON
rather than parsed into `SubmitReply` and `StatusReply`. A probe
answering something this version cannot read is a case the cockpit has
distinct and important behaviour for, and a fixture format that
validated the bodies could not record it. The *file* is still strict —
unknown keys are refused, because a person types it.

- [x] `peers/<name>.control.json`: `{ "submit": <body>, "status": { "<id>": <body> } }`.
- [x] Absent file means no control for that peer — the fixture-set spelling of a probe without `--allow-control`.
- [x] A present but unreadable file is an error, not a silent no-control: a fixture that half-loads is worse than one that refuses.
- [x] ~~Recorded into the same `FixtureTransport` as that peer's `/state`~~ — see Task 3: it is a second transport, in a different file.

#### Task 3: The submitter

**Revised during implementation, by a test.** The plan had `fixtures.rs`
build the submitters. `tests/read_only.rs` refused it: that test allows
exactly four files to so much as *name* baseline's actions module, and
its own documentation says the fixtures are observation and "stay
structurally unable to act".

Widening the exemption to a fifth file was the obvious fix and the wrong
one. The rule's principle is that the composition root decides what a
run may act on — and `main.rs` is already that root and already allowed.
So `fixtures.rs` returns *recordings*, which are data, and `main.rs`
turns them into submitters. The guard stayed at four files, and the
transport is built where the executor is.

- [x] `FixtureSet` gains `control: Vec<RecordedControl>` — recordings, not submitters.
- [x] `main.rs` builds `RemoteExecutor::new(transport, url, name, "fixture", 0)` — a pinned client and run, so ids are `fixture-0-1`, `fixture-0-2`, and a recording can name them.
- [x] `load` in `main.rs` supplies the courier's submitters in both modes; `courier_for` is gone, and `Loaded` became a struct because `live` has to be readable at a glance.
- [x] Test: a fixture peer with a control file is carried to; one without is not.
- [x] Test: **every local destination is still `Nowhere` in fixture mode.** The guarantee this arc is most likely to break.
- [x] Test: an unreadable recorded reply becomes `Unknown` and never `Refused`.

### Slice 1.3: The frames

#### Task 4: A second fixture peer

**Revised during implementation.** `pi5` records a reply this version
*cannot read*, rather than an acceptance followed by a status. Both
produce the `unknown` entry, but only one is deterministic: reaching it
through a status poll needs the refresh cadence to tick, and a scenario
that waits on a cadence is exactly the nondeterminism fixture mode
exists to remove. The unreadable reply arrives synchronously, on the
keystroke. It is also the more honest recording — a probe one version
ahead of its cockpit is how this case really turns up. Explained in
`fixtures/peers/README.md`, so nobody later "fixes" it.

A test also had to be added that was not in the plan:
`every_recorded_machine_in_the_shipped_set_answers`. A fixture peer with
a malformed envelope does not fail loudly — it renders as a machine that
did not answer, which is a real state, so the screen looks plausible and
the fixture is silently broken. That is exactly what `pi5` did on its
first run, with the wrong spelling of one field.

- [x] `peers/pi5.json` — a machine with work in flight, so there is something on it worth confirming.
- [x] `peers/pi5.control.json` — answers something this version cannot parse, which is the `unknown` case.
- [x] `tates-laptop` keeps no control file, so the refusal scenario keeps its subject.
- [x] Test: every recorded machine in the shipped set actually answers.

#### Task 5: The scenarios

- [x] `cockpit-remote-confirm`: the prompt naming the machine.
- [x] `cockpit-unknown`: the log entry for an action whose fate is not known.
- [x] Existing scenarios re-captured; intents corrected wherever the new row changed what is on screen.

#### Task 6: Close-out

- [x] Task 17 of the control-over-the-wire plan is checked, with what unblocked it.

---

## Spec coverage

| Spec section | Task |
|---|---|
| A fixture peer's name must not resolve | 1 |
| A fixture peer may carry a recording | 2 |
| The submitter is the real one | 3 |
| Local actions stay inert | 3 |
| The frames this exists for | 4, 5 |
