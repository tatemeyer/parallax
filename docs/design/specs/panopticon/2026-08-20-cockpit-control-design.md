# Cockpit: Full Control (Design)

**Status:** proposed — awaiting sign-off. Nothing here is implemented.
**Date:** 2026-08-20

**Place in the roadmap:** sub-project #5 of the Parallax platform
(`../parallax/2026-08-14-parallax-platform-design.md`), the last of the
five and deliberately last: *control without observation is not useful*.
Observation shipped as sub-project #3 (`2026-08-18-panopticon-observe-design.md`,
outcomes in `2026-08-19-panopticon-outcomes.md`).

**Dependencies, both satisfied:** `parallax-baseline`'s `actions`
module, and the cockpit that will host the verbs.

## Context / Motivation

Baseline shipped the decision layer **complete and entirely
disconnected from the world**. It has the eight-action set, the
reversible/confirmation-required classification, `fingerprint`,
`Confirmation`, `Authorized` with a private constructor, `authorize`,
`ActionExecutor`, `RecordingExecutor`, and `LocalExecutor` — 25 tests,
including a `compile_fail` doctest proving an executor cannot be reached
without passing the confirmation check.

What it does not have is anything that acts. `WorkControl` and
`ProcessControl` are traits with **no implementations outside test
fakes**, so today the platform can decide to merge a pull request, prove
it was confirmed, record what it would have done — and not merge it.

The cockpit, meanwhile, is asserted read-only: `tests/read_only.rs`
fails if any shipped file so much as names `parallax_baseline::actions`.

This sub-project connects the two. It is the smallest remaining gap in
the platform and the one with the sharpest failure mode, which is why it
gets a design rather than an afternoon.

## Design

### The transport learns verbs

`HttpTransport` has exactly one method:

```rust
fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError>;
```

Every control action against GitHub is a POST, PUT, or PATCH. Three
options were weighed:

1. **A second trait, `HttpWrite`.** Keeps reads and writes separable, so
   a read-only frontend could accept a transport that structurally
   cannot write. Costs a second seam and a second fixture type.
2. **Verbs on `HttpRequest`, one `send` method.** One seam, one fixture,
   and `FixtureTransport` records the exact method, URL, and body of
   every write — so a test asserts what *would* have been sent to
   GitHub without sending it.
3. **A separate control client that owns its own `ureq::Agent`.** Least
   disruption to existing code, and abandons the property that
   `UreqTransport` is the only type in the crate touching the network.

**Chosen: option 2.** `HttpRequest` gains `method: Method` and
`body: Option<String>`; the trait's method becomes `send`, with `get`
kept as a provided method that calls it. Every existing *caller* is
unchanged; the two implementors move one method. The recorded-request
property is what makes control testable at all, and option 1's
structural read-only guarantee is already provided one layer up by
`tests/read_only.rs`.

### The two seams get real implementations

**`GithubWorkControl<T: HttpTransport>`** — `set_label` (`POST
/repos/{repo}/issues/{n}/labels`), `request_review` (`POST
/repos/{repo}/pulls/{n}/requested_reviewers`), `merge` (`PUT
/repos/{repo}/pulls/{n}/merge`). Constructed from the same transport the
work adapter uses, so one token configures both.

**`LocalProcessControl`** — `capture` spawns `plumb capture` for the
project's declared scenarios; `push` spawns `git push` in the project
root. Both go through the existing `CommandRunner` seam rather than
`std::process` directly, so both are scriptable in tests and neither
adds a second way to run a subprocess.

### Two actions are deliberately not implemented

`DispatchAgentRun` and `StopAgentRun` stay in the action set and return
`ActionError::NotSupported` with a message naming why.

Starting and stopping an agent session is **harness-specific**: this
machine drives Claude Code jobs, the Pi drives something else, and a
future runner will differ again. Baking one harness's mechanism into the
platform would be exactly the coupling the adapter families exist to
avoid, and `NotSupported` is already in the error enum for a caller that
cannot perform an action.

The spec records this as a scope cut rather than an oversight. When a
harness contract exists, it becomes a `ProcessControl` implementation
and nothing else changes.

### Ruling on a finding needs the findings

The master design calls ruling on a Plumb finding *"the action with the
highest leverage: it is the one input `plumb`'s learned-rejection store
depends on, and it currently has no home."*

The cockpit's artifacts pane shows a capture run as a run id and a
verdict word. To rule on a finding it must show the **findings**, which
means reading `merge/survivors.json` from Plumb's evidence contract —
the same contract the capture adapter already walks. That is an addition
to the artifacts pane, and it comes first: the highest-leverage action
is worth the pane it needs.

Rulings write through `LocalExecutor`'s existing `append_ruling`, which
already appends a JSON line per ruling. Nothing new is invented.

### The confirmation contract, at the surface

A confirmation-required action opens **an explicit prompt** quoting
`Action::summary()` verbatim, and requires an unambiguous keypress to
proceed. `Confirmation::of(&action)` is built from the exact action
being confirmed, so the fingerprint check that `authorize` performs is
real rather than ceremonial — confirming "merge #12" cannot execute
"merge #99" even if the selection moved between the prompt and the
keypress.

**This is the one modal the cockpit gets, and it must be one.** The
Cloister Bell is never modal because it is information; a confirmation
is a question, and a question that can be answered by accident is not
one. Escape cancels. There is no "confirm all", no remembered
confirmation, and no timeout that answers on the operator's behalf.

### The action log

A fifth detail tab: every action this session attempted, its outcome,
and the effects it reported — `WroteFile`, `CalledApi { method, url }`,
`Spawned`. A cockpit that can act must show what it did, and
`ActionOutcome` already carries exactly this.

The log is in-memory and dies with the process, consistent with the
cockpit holding no state across runs. A durable audit trail is the
event-log shape SESH uses and is not proposed here.

### The read-only boundary moves rather than disappearing

`tests/read_only.rs` currently asserts that **no** shipped file names
`actions`. That test must not simply be deleted — the guarantee it
encodes is what keeps observation observation.

It becomes a narrower assertion: `view/`, `refresh.rs`, `fixtures.rs`
and `bell.rs` still may not name `actions`, and only the new `control`
module may. The observation half stays structurally incapable of
mutating anything, and a future change that reaches for an action from
inside a render path fails the test the way it does today.

## Non-goals

- **Bulk or batch actions.** Every action targets one thing, chosen on
  screen. "Merge all green PRs" is a different tool with a different
  risk profile.
- **Undo.** Rulings are the closest thing the platform has and they are
  additive. An action is confirmed before it happens rather than
  reversed after.
- **Scheduling or automation.** The cockpit acts when the operator acts.
  A daemon remains a non-goal of the master design.
- **Agent dispatch and stop**, as above.
- **Credentials beyond a token**, which the frontend already discovers.
- **Any change to the confirmation classification.** The
  reversible/confirmation-required split is the platform spec's, and
  this sub-project consumes it rather than revisiting it.

## Testing

- **Every write is asserted as a recorded request**, not as a live call:
  `FixtureTransport` keeps method, URL, and body, so a test states
  exactly what GitHub would have received. Live calls stay
  real-external-service exempt, confined to `UreqTransport`.
- **The confirmation contract is asserted at the surface**, not only in
  baseline: a key that triggers a confirmation-required action with no
  confirmation must produce no effect at all, asserted with controls
  that record every call.
- **A stale confirmation is refused**: build a confirmation for one
  action, move the selection, and assert the mismatch is caught — the
  scenario the fingerprint exists for.
- **The read-only boundary test is rewritten, not removed**, and still
  fails if a render path names an action.
- **`NotSupported` is asserted for the two deferred actions**, so the
  deferral is visible in the suite rather than only in prose.

No test performs a live GitHub write, spawns a real `git push`, or needs
a TTY.

## Critical files

```
baseline/src/adapters/http.rs        — Method, body, send(); get() provided
baseline/src/actions/github.rs       — GithubWorkControl
baseline/src/actions/process.rs      — LocalProcessControl
panopticon/src/control/mod.rs        — the surface: verbs, confirmation, log
panopticon/src/control/confirm.rs    — the prompt and its key handling
panopticon/src/view/artifacts.rs     — findings, so rulings have a target
panopticon/tests/control.rs          — the surface-level contract tests
panopticon/tests/read_only.rs        — rewritten boundary
```

## Verification

- Every existing test still passes, including baseline's `compile_fail`
  doctest on `Authorized`.
- A confirmation-required action invoked from the key map with no
  confirmation performs **no** call on either control seam.
- A confirmation built for a different action is refused at the surface.
- `set_label`, `request_review`, and `merge` each produce exactly one
  recorded request, with the method and URL the GitHub API documents.
- A ruling appends one line to `.plumb/rulings.jsonl` and leaves the run
  directory otherwise untouched.
- `DispatchAgentRun` and `StopAgentRun` return `NotSupported` naming the
  harness gap.
- The observation modules still cannot name `actions`.

## Open questions for sign-off

1. **Which keys.** Proposed: `m` merge, `l` label, `R` request review,
   `p` trigger capture, `P` push, `u`/`o` uphold/overrule a finding.
   `m` and `P` and `o` are confirmation-required. The alternative is a
   single `a` opening an action menu for the selected row, which is
   discoverable but slower and needs its own overlay.
2. **Is `y` enough to confirm a merge?** Typing the pull request number
   is the stronger gate and the one most tools reserve for deletion. A
   merge is not reversible by any action this platform has.
3. **Is deferring agent dispatch/stop acceptable for this sub-project**,
   or should it define a minimal harness contract now?
4. **Does the findings list belong in the artifacts pane or its own
   tab?** Rulings need it either way; the pane is already the busiest.
