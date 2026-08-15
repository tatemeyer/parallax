---
name: visual-review
description: Orchestrates Plumb's blinded multi-lens visual review — builds the capture binary if needed, selects scenarios from the branch diff (or an explicit --scenario), captures them, fans out one subagent per applicable lens in parallel, and merges the results into a GO / NO-GO / HOLD verdict. Invoked by /plumb:review.
---

## What this skill owns, and what it does not

Everything deterministic — config parsing, scenario selection, running
capture adapters, writing the run manifest, **building the blinded
lens prompts**, merging and deduping findings, and rendering
`verdict.md` — lives in the `plumb` CLI (the `parallax-plumb` crate).
This skill owns exactly one thing the CLI cannot do itself: **dispatch
the lens subagents in parallel and feed their JSON back to the CLI.**

That boundary is load-bearing. **Never** write critique instructions,
selection logic, merge/dedupe logic, or verdict rules here — call the
CLI for all of that. If a step below feels like it's asking you to
invent review content, stop: it isn't, and you've misread it.

## Setup: locate the binary

The capture binary lives in this plugin, at
`${CLAUDE_PLUGIN_ROOT}/capture`. Set:

```
PLUMB="${CLAUDE_PLUGIN_ROOT}/capture/target/release/plumb"
```

(`plumb.exe` on Windows — check for whichever your platform produces.)

All commands below are run from the **consuming project's** working
directory (the repo `/plumb:review` was invoked in), not from inside
the plugin — `.plumb/config.yaml`, `.plumb/taste.md`, and
`.plumb/runs/` are all relative to that project.

## The orchestration procedure

Run these steps in order. Each step's failure behavior is stated
inline — follow it exactly; do not improvise a softer outcome.

### Step 1 — Build the capture binary if needed

If `$PLUMB` does not already exist, build it once and reuse it on every
later invocation in this session:

```
cargo build --release --manifest-path "${CLAUDE_PLUGIN_ROOT}/capture/Cargo.toml"
```

If `cargo` is not available, **stop immediately** with exactly this
message and do nothing else:

> Plumb's capture binary needs a Rust toolchain (rustup.rs). Nothing
> was captured and no verdict was produced.

A clear, actionable message — never a stack trace, never a partial run.

### Step 2 — Scaffold `.plumb/` if absent

If `.plumb/` does not exist in the project, this is the expected
first-run state, not an error. **Offer** to run `"$PLUMB" init` and
**stop, waiting for an answer.** Never scaffold silently and never
error. Only proceed to Step 3 once `.plumb/` exists (either it already
did, or the user said yes and `init` ran).

### Step 3 — Select

If `/plumb:review` was invoked with `--scenario <name>`:

```
"$PLUMB" select --config .plumb/config.yaml --scenario <name>
```

Otherwise, diff the branch against its merge base and pipe the changed
paths in:

```
default_branch=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's@^refs/remotes/origin/@@')
default_branch=${default_branch:-main}
merge_base=$(git merge-base "$default_branch" HEAD)
git diff --name-only "$merge_base"..HEAD | "$PLUMB" select --config .plumb/config.yaml --changed -
```

Read the exit code:

- **0** — one or more scenarios selected. Continue to Step 4 with the
  JSON's `selected` list.
- **3** — nothing matched, and no `--scenario` was named. **Stop here
  and say so.** Report that no scenario's `touches` matched, list the
  changed paths that matched nothing (the JSON's `unmatched` field),
  and end the run. **Never widen to every scenario.**
- **1** — a real error (bad/missing config, bad arguments). Stop and
  show the CLI's error text verbatim. No run happened; this is
  distinct from a HOLD verdict, since no verdict was ever attempted.

### Step 4 — Capture

Generate one run id for the whole review (all selected scenarios share
it), matching the CLI's own `YYYYMMDDTHHMMSSZ` format:

```
run_id=$(date -u +%Y%m%dT%H%M%SZ)
run_dir=".plumb/runs/${run_id}"
```

For each selected scenario, in turn:

```
"$PLUMB" capture --config .plumb/config.yaml --run-dir "$run_dir" --scenario "<name>"
```

- **0** — captured; its manifest now lives under `$run_dir`.
- **2** — capture failed (HOLD). Record `<scenario>:<the printed
  reason>` for Step 7's `--capture-failure`, and **continue to the
  next scenario** — one failed capture does not stop the others. The
  run's overall verdict will not be a GO regardless of what else is
  clean.
- **1** — a usage error (e.g. an unknown scenario name); this
  shouldn't happen for anything Step 3 selected, but if it does, treat
  it the same as a Step 3 exit-1: stop and show the error.

### Step 5 — Plan the fan-out

```
taste_arg=()
if [ -f .plumb/taste.md ]; then taste_arg=(--taste .plumb/taste.md); fi
"$PLUMB" plan --run-dir "$run_dir" "${taste_arg[@]}" > "$run_dir/plan.json"
```

(Omit `--taste` entirely when `taste.md` doesn't exist — passing a
path that doesn't exist is an I/O error, not "no taste profile
declared"; the CLI already treats a *missing flag* as the latter.)

Parse `plan.json`: it has `batches` (a list of batches, each a list of
dispatch entries with `lens`, `agent`, `scenario`, `image`, `prompt`),
`skipped` (scenario/lens/reason triples for lenses that didn't apply),
and `cap`.

**Report the skipped lenses now** (e.g. "design skipped for
`omnitrix-dial-rotate`: no taste.md") — every one, not a summary count.

**Immediately build the full `--expected lens:scenario` list** by
flattening every dispatch entry across every batch (deduplicated).
This is the entire `--expected` handling for this run — see
"The `--expected` decision" below for why it's built this way and
never hand-typed.

**If `plan.json` has more than one batch**, say so before dispatching:
state the total dispatch count and how many batches it took under the
cap. This is what "report what was deferred" means here — nothing is
ever dropped by the cap (every entry in `batches` still runs, in a
later wave), but running in several sequential waves must never be
reported as if it were one flat parallel fan-out that covered
everything at once.

### Step 6 — Dispatch, batch by batch

Process `plan.json`'s batches **in order**; within one batch, dispatch
every entry **in parallel** (one Agent tool call per entry, all issued
in the same turn), and wait for the whole batch to finish before
starting the next.

For each dispatch entry:

1. Resolve the image to an absolute path: `run_dir` joined with the
   entry's `image` field (the CLI stores this as a bare filename —
   see "Getting the image to the agent" below for why an absolute path
   has to reach the agent through a channel other than the prompt
   text itself).
2. Launch the entry's `agent` (its `subagent_type`/agent name is the
   plan's `agent` field, e.g. `critic-breakage`) with a prompt built as
   exactly:

   ```
   Image file to read: <absolute path>

   <the plan's "prompt" field, verbatim>
   ```

   The one leading line naming the image path is the **only** thing
   ever placed before the verbatim prompt, and nothing is ever placed
   after it or interleaved into it. **You must not add anything else
   to that prompt.** Do not paste the diff, name the files that
   changed, mention that anything changed, say the work is yours, or
   ask the agent to confirm anything. The prompt is constructed to be
   blind; appending review-relevant content to it destroys the one
   property the whole tool rests on. (The image-path line carries no
   information about the change, the diff, or authorship — only where
   the artifact under review happens to sit on disk — which is why it
   is the one exception.)

3. Collect the agent's raw returned text.

### Step 7 — Retry once, then let the CLI decide

For each dispatch's returned text, do a cheap sanity check only — not
a real parser (that's the CLI's job, and it already does prose/fenced-
block extraction before declaring output unparseable):

- The agent errored or returned nothing at all, **or**
- the trimmed text (after stripping at most one surrounding markdown
  fence) does not start with `[`.

If either is true, **re-dispatch that one entry once**, with the
identical prompt built in Step 6 — same image line, same verbatim plan
prompt, no changes. Take whichever text you have after that (the retry
result if you retried, the original otherwise) and move on regardless
of whether it now looks better; do not retry a third time.

For every dispatch you have final text for (including a still-bad
second attempt — let the CLI's parser be the one to call it
unparseable rather than discarding it yourself):

```
mkdir -p "$run_dir/reports"
# write the raw text to:
"$run_dir/reports/<lens>-<scenario>.txt"
```

and record `<lens>:<scenario>:<path>` for Step 8's `--report`.

If an agent produced no text at all even after the retry (a hard
dispatch failure, not just bad content), write nothing for it — its
`--expected` entry from Step 5 is what turns its absence into a HOLD.

### Step 8 — Merge

```
"$PLUMB" merge --run-dir "$run_dir" \
  --report lens1:scenario1:path1 --report lens2:scenario2:path2 ... \
  --expected lens1:scenario1 --expected lens2:scenario2 ... \
  --capture-failure scenario:reason ...
```

Include every `--report` collected in Step 7, every `--expected` built
in Step 5, and every `--capture-failure` recorded in Step 4. Read the
exit code:

- **0 — GO.**
- **1 — NO-GO.**
- **2 — HOLD.**

### Step 9 — Report the verdict

Show `$run_dir/verdict.md`'s contents (or a faithful summary of it —
never a softened one).

- **On NO-GO:** state plainly that the task may not be claimed
  complete and no PR may be opened until every blocker is **fixed**,
  **overruled** (which writes a ruling — not implemented yet; if
  overruling is the intended path, say so and stop rather than
  attempting it), or **deferred with a note**.
- **On HOLD:** name which lens(es) could not report and why (capture
  failure, or two malformed attempts), and state plainly that this is
  **not** a GO — it means the check could not be run, not that it
  passed.
- **On GO:** report it as a GO, plus any advisory (non-blocking)
  findings the verdict carries.

## The `--expected` decision

`plumb merge --expected <lens>:<scenario>` splits on the first colon,
so a hand-typed, colon-bearing, or misspelled scenario name would
silently produce a spurious HOLD instead of a usage error — a
fail-safe direction, but still an avoidable failure mode.

**This skill never hand-types an `--expected` value.** Step 5 builds
the entire `--expected` list mechanically, by flattening `plan.json`'s
own `batches` — the exact lens/scenario pairs `plumb plan` says it
dispatched, taken verbatim from that JSON. `plumb plan` already knows
precisely what it planned to dispatch; passing those pairs straight
through removes the hand-typing error class entirely rather than
asking whoever runs this to reconstruct the list by hand. There is no
other source of `--expected` values anywhere in this procedure.

## Getting the image to the agent

Lens agents (`critic-breakage`, etc.) declare `tools: Read` and see
"Image: `<filename>`" in their prompt — the CLI writes only the bare
filename there, deliberately, so the prompt never leaks a run
directory or an absolute path (`prompt/text.rs`'s blinding guarantee).
The plan JSON's own `image` field carries that same bare filename for
the same reason. Neither is enough on its own for the agent's `Read`
tool call to resolve — it needs an absolute path, and no field on the
`Agent` tool exists to hand one over except the prompt text itself.
Step 6 resolves this by prepending exactly one plain path line before
the verbatim prompt. That line is mechanical addressing information
only (which file to open), carries no statement that anything changed
or who produced it, and is the one exception to "never add to the
prompt" for that reason — everything else about the addition
prohibition still holds.

## Do not

- Never review everything when Select finds no match. Stop and say so.
- Never omit a deferred batch from the report — every wave the plan
  took to run must be visible, not just the first.
- Never upgrade a HOLD to a GO, or a NO-GO to a GO. Trust the CLI's
  exit code exactly as given; do not reinterpret it.
- Never add anything to a dispatched prompt beyond the single
  mechanical image-path line in Step 6 — no diff, no file names, no
  "this is my work," no request to confirm.
- Never feed a ruling, a suppression, or any prior verdict to a lens
  agent. Rulings (when they exist) are a post-hoc filter the CLI
  applies to the *report*, never an input to the agent's judgment.
