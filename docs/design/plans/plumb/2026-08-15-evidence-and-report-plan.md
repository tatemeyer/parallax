# Plumb Evidence and Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Structure note:** organised as **Arcs → Slices → Tasks** per
> `docs/design/README.md`. Arc/Slice headings are pure grouping; tasks
> follow the skill's bite-sized TDD step structure.

**Goal:** Make a Plumb verdict auditable by a human — persist the prompt
each lens read, the raw reply it returned, and every transformation
applied to that reply, then render it all as one self-contained HTML
report in which a finding's stated region sits beside the pixels it
names.

**Architecture:** A new `evidence` module owns a versioned on-disk
layout under the run directory. The subcommands that already hold the
data write their own slice: `plan` writes prompts, `merge` writes
replies and the transformation chain. A new `report` module reads that
layout and emits a single self-contained HTML file. Nothing new
orchestrates anything, and `report` is read-only — it cannot alter a
verdict.

**Tech Stack:** Rust (stable, 2021 edition), `serde` + `serde_json`
(contract), `image` (frame cropping), `base64` via manual encoding of
`image`'s PNG bytes for data URIs, `clap` (the subcommand). All already
declared except `base64`, addressed in Task 7.

## Global Constraints

Copied from
`docs/design/specs/plumb/2026-08-15-evidence-and-report-design.md`.
Every task's requirements implicitly include this section.

- **Absence must never read as success.** A missing artifact renders as
  an explicit marker, never as an empty or omitted section. A lens with
  no persisted reply is *"no reply persisted"*, never a lens that
  returned zero findings.
- **Discards keep their text.** `dropped`, `clamped` and `suppressed`
  findings retain the finding itself, not a count. A count says a
  finding was removed; only the text lets a reader judge whether it
  deserved to be.
- **Persisting evidence must never become a channel into a prompt.**
  Evidence is written by the CLI after a prompt is built, into
  directories no lens agent can read. No evidence path, type, or field
  may reach `prompt::build_prompt`. Guarded structurally and by test.
- **`report` is read-only.** It runs no capture, dispatches no agents,
  makes no network call, and never writes into the run directory it
  reads. It may be re-run freely.
- **The report is self-contained.** Images embedded as data URIs; no
  external URL, relative image path, or external font. Evidence that
  breaks when moved is not evidence.
- **The region matcher is conservative.** Resolve a crop only when the
  text unambiguously identifies a frame by index or grid position;
  otherwise show the full sheet. A wrong crop is worse than no crop.
- **Contract versioning.** `run.json` carries `contract_version`. A
  report reading an unknown or absent version says so rather than
  misreading the run.
- **Existing behaviour is preserved.** No lens, severity rule, blinding
  guarantee, or verdict semantic changes. `manifest.rs`'s
  `serialized_manifest_carries_no_command_line_and_no_source_paths` and
  `prompt/tests.rs`'s 29-item forbidden-substring test must both pass
  **unmodified**.
- **Repo conventions.** Conventional Commits with a body, plus the
  `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`
  and `Claude-Session: https://claude.ai/code/session_016nwJYRXRJ6min9Z8NCQTbX`
  trailers. One commit per task. TDD mandatory. Every `pub` item gets a
  one-line `///`; every module a `//!` header. Soft ceiling 500 lines
  per file. Every task's commit must pass `cargo build`, `cargo test`,
  `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`
  from the workspace root, run in the **foreground**.

**Baseline at plan time:** 262 tests passing.

## File Structure

```
plumb/capture/src/
  evidence.rs          NEW  — contract: layout, paths, run.json, write/read
  finding.rs           MOD  — ParsedFindings retains discarded findings
  contact.rs           MOD  — expose frame geometry for cropping
  report/
    mod.rs             NEW  — orchestration: run dir -> HTML string
    geometry.rs        NEW  — frame rects, crop extraction, data URIs
    region.rs          NEW  — conservative region -> frame index matcher
    render.rs          NEW  — HTML emission
  cli/
    plan.rs            MOD  — writes prompt.txt per dispatch
    merge/mod.rs       MOD  — writes replies + chain; --report gains attempt
    report.rs          NEW  — the `report` subcommand
  main.rs              MOD  — register the subcommand
```

## Milestones

- **End of Arc 1** — a real run leaves a complete evidence directory:
  every prompt, every raw reply, every discarded finding with its text.
  No viewer yet.
- **End of Arc 2** — `plumb report <run-dir>` emits a self-contained
  HTML file showing the contact sheet, each lens's findings with region
  crops, the full prompt and reply, and the attrition chain.
- **End of Arc 3** — a report generated from a real TTUI scenario has
  been opened and read by a human, which is the only check that
  establishes the tool works.

---

## Arc 1: The evidence contract

### Slice 1.1: Retain what the pipeline discards

#### Task 1: `ParsedFindings` keeps dropped and clamped findings

**Files:**
- Modify: `plumb/capture/src/finding.rs`

**Interfaces:**
- Consumes: existing `Finding`, `Severity`, `Lens`, `parse_findings`.
- Produces: `ParsedFindings.dropped: Vec<Finding>` and
  `ParsedFindings.clamped_records: Vec<ClampRecord>`, plus
  `pub struct ClampRecord { pub finding: Finding, pub from: Severity }`.
  Task 4 serialises both; Task 8 renders them.
  **Note the name:** the field is `clamped_records`, not `clamped` —
  `clamped: usize` already exists and is read by the verdict layer, so
  the new vector sits beside it rather than shadowing it.

**Why additive.** `dropped_no_region: usize` and `clamped: usize` are
already read by the verdict layer. Adding vectors beside them, rather
than replacing them, keeps every existing caller and test untouched.

- [ ] **Step 1: Write the failing test**

Add to `finding.rs`'s `mod tests`:

```rust
#[test]
fn a_dropped_finding_keeps_its_text_not_just_a_count() {
    let raw = r#"[
      {"lens":"breakage","scenario":"s","severity":"major","region":"",
       "claim":"the left gutter is doubled","evidence":"e","confidence":"high"},
      {"lens":"breakage","scenario":"s","severity":"major","region":"top row",
       "claim":"kept","evidence":"e","confidence":"high"}
    ]"#;
    let p = parse_findings(Lens::Breakage, "s", raw).expect("parses");
    assert_eq!(p.kept.len(), 1);
    assert_eq!(p.dropped_no_region, 1);
    assert_eq!(p.dropped.len(), 1);
    assert_eq!(p.dropped[0].claim, "the left gutter is doubled");
}

#[test]
fn a_clamped_finding_records_the_severity_it_came_from() {
    let raw = r#"[
      {"lens":"design","scenario":"s","severity":"blocker","region":"panel",
       "claim":"over-severe","evidence":"e","confidence":"low"}
    ]"#;
    let p = parse_findings(Lens::Design, "s", raw).expect("parses");
    assert_eq!(p.kept[0].severity, Severity::Major);
    assert_eq!(p.clamped, 1);
    assert_eq!(p.clamped_records.len(), 1);
    assert_eq!(p.clamped_records[0].from, Severity::Blocker);
    assert_eq!(p.clamped_records[0].finding.severity, Severity::Major);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd /d/Dev/Projects/Parallax && cargo test --workspace finding::tests`
Expected: FAIL — no field `dropped` / `clamped_records` on `ParsedFindings`.

- [ ] **Step 3: Implement**

In `finding.rs`, add above `ParsedFindings`:

```rust
/// A finding whose severity exceeded its lens's ceiling, with the
/// severity it was lowered from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ClampRecord {
    /// The finding as it survived, already clamped to the ceiling.
    pub finding: Finding,
    /// The severity the lens originally asserted.
    pub from: Severity,
}
```

Add two fields to `ParsedFindings`:

```rust
    /// Findings discarded for naming no region, retained in full so a
    /// reader can judge whether the drop was correct.
    pub dropped: Vec<Finding>,
    /// Findings whose severity was lowered to their lens's ceiling.
    pub clamped_records: Vec<ClampRecord>,
```

In `parse_findings`, populate them at the same points that currently
increment `dropped_no_region` and `clamped`: push the finding before
discarding it, and push a `ClampRecord { finding: clamped.clone(), from: original }`
before the clamped finding is pushed to `kept`.

- [ ] **Step 4: Run to verify they pass**

Run: `cd /d/Dev/Projects/Parallax && cargo test --workspace`
Expected: PASS, 264 tests (262 + 2).

- [ ] **Step 5: Verify the gates**

Run each in the foreground:
`cargo build`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add plumb/capture/src/finding.rs
git commit -m "feat(finding): retain discarded findings, not just their counts

A count says a finding was removed; only the text lets a reader judge
whether it deserved to be. Additive, so every existing caller of
dropped_no_region and clamped is untouched."
```

### Slice 1.2: The contract

#### Task 2: `evidence.rs` — layout, versioning, round-trip

**Files:**
- Create: `plumb/capture/src/evidence.rs`
- Modify: `plumb/capture/src/lib.rs` (add `pub mod evidence;`)

**Interfaces:**
- Consumes: `Finding`, `ClampRecord` (Task 1), `Lens`.
- Produces:
  - `pub const CONTRACT_VERSION: u32 = 1;`
  - `pub fn lens_dir(run_dir: &Path, lens: Lens, scenario: &str) -> PathBuf`
  - `pub fn merge_dir(run_dir: &Path) -> PathBuf`
  - `pub fn write_prompt(run_dir: &Path, lens: Lens, scenario: &str, prompt: &str) -> Result<(), EvidenceError>`
  - `pub fn write_reply(run_dir: &Path, lens: Lens, scenario: &str, attempt: u32, raw: &str) -> Result<(), EvidenceError>`
  - `pub fn write_findings(run_dir: &Path, lens: Lens, scenario: &str, parsed: &ParsedFindings) -> Result<(), EvidenceError>`
  - `pub fn write_run_json(run_dir: &Path, run_id: &str) -> Result<(), EvidenceError>`
  - `pub fn read_lens_evidence(run_dir: &Path, lens: Lens, scenario: &str) -> LensEvidence`
  - ```rust
    /// One artifact's three possible states, kept distinct because
    /// "nothing was recorded" and "something was recorded that could
    /// not be read" are different facts about a run, and an audit tool
    /// that conflates them is lying by omission.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Evidence<T> {
        /// The artifact was never written.
        Missing,
        /// The artifact exists but could not be parsed; carries its
        /// raw text so the report can show it rather than drop it.
        Unparseable(String),
        /// The artifact was read successfully.
        Present(T),
    }

    pub struct LensEvidence {
        pub prompt: Evidence<String>,
        pub replies: Vec<(u32, String)>,
        pub parsed: Evidence<Vec<Finding>>,
        pub dropped: Evidence<Vec<Finding>>,
        pub clamped: Evidence<Vec<ClampRecord>>,
    }
    ```

    **Corrected 2026-08-16, after Task 2's review.** This originally
    pinned every field as an `Option`, which cannot hold a third state
    — so a corrupt `parsed.json` became `None`, indistinguishable from
    an absent one. That is the spec's own "absence reads as success"
    failure reappearing one level down, inside the design written to
    prevent it. The spec was right; these types were wrong.
    `replies` stays a plain `Vec` because a reply is raw text decoded
    lossily and therefore always readable.
  - `pub struct RunJson { pub contract_version: u32, pub run_id: String }`
  - `pub enum EvidenceError { Io(IoFailure), Json(JsonFailure) }`

  Task 3 calls `write_prompt`. Task 4 calls `write_reply`,
  `write_findings`, `write_run_json`. Task 8 calls `read_lens_evidence`.

**Note on `read_lens_evidence` returning a bare struct rather than a
`Result`:** a missing artifact is an expected state the report must
render as a marker, not an error. `None` means "not persisted"; an
unreadable file yields `Some` containing the raw bytes as a string so
the report can show "present but unparseable" with the content.

- [ ] **Step 1: Write the failing test**

Create `plumb/capture/src/evidence.rs` with only a `//!` header and
`#[cfg(test)] mod tests` containing:

```rust
#[test]
fn a_written_evidence_directory_round_trips() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    write_prompt(run, Lens::Breakage, "omni", "PROMPT BODY").expect("prompt");
    write_reply(run, Lens::Breakage, "omni", 1, "garbled").expect("r1");
    write_reply(run, Lens::Breakage, "omni", 2, "[]").expect("r2");

    let ev = read_lens_evidence(run, Lens::Breakage, "omni");
    assert_eq!(ev.prompt.as_deref(), Some("PROMPT BODY"));
    assert_eq!(ev.replies.len(), 2);
    assert_eq!(ev.replies[0], (1, "garbled".to_string()));
    assert_eq!(ev.replies[1], (2, "[]".to_string()));
}

#[test]
fn absent_evidence_is_none_not_an_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ev = read_lens_evidence(tmp.path(), Lens::Motion, "nothing-here");
    assert!(ev.prompt.is_none());
    assert!(ev.replies.is_empty());
    assert!(ev.parsed.is_none());
}

#[test]
fn run_json_carries_the_contract_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_run_json(tmp.path(), "20260815T000000Z").expect("run.json");
    let text = std::fs::read_to_string(tmp.path().join("run.json")).expect("read");
    let parsed: RunJson = serde_json::from_str(&text).expect("parse");
    assert_eq!(parsed.contract_version, CONTRACT_VERSION);
    assert_eq!(parsed.run_id, "20260815T000000Z");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd /d/Dev/Projects/Parallax && cargo test --workspace evidence`
Expected: FAIL — module not declared / functions not found.

- [ ] **Step 3: Implement**

Add `pub mod evidence;` to `lib.rs`. Implement the interfaces listed
above. Layout rules:

- `lens_dir` = `run_dir/lenses/<lens>.<scenario>/`, where `<lens>` is
  `Lens`'s serde name (`breakage`/`intent`/`design`/`motion`).
- `write_prompt` → `prompt.txt`; `write_reply` → `reply.<attempt>.raw.txt`.
- `write_findings` → `parsed.json`, `dropped.json`, `clamped.json`,
  each an array, each written even when empty so a reader can tell
  "zero findings" from "not persisted".
- `write_run_json` → `run.json` with `contract_version` and `run_id`.
- Every error names its file via the settled `IoFailure { path, source }`
  / `JsonFailure { path, source }` wrappers, kept as single-field tuple
  variants so `Err(EvidenceError::Io(_))` matches opaquely.
- `read_lens_evidence` reads replies by globbing `reply.*.raw.txt`,
  parsing the attempt number from the filename, and sorting ascending.

- [ ] **Step 4: Run to verify it passes**

Run: `cd /d/Dev/Projects/Parallax && cargo test --workspace`
Expected: PASS, 267 tests.

- [ ] **Step 5: Verify the gates, then commit**

```bash
git add plumb/capture/src/evidence.rs plumb/capture/src/lib.rs
git commit -m "feat(evidence): define the versioned run-evidence contract

Writes and reads the prompt, raw replies, and discarded findings that a
verdict currently cannot be audited without. Absent evidence reads as
None rather than an error, because the report must render it as an
explicit marker rather than as a lens that said nothing."
```

### Slice 1.3: Wire the writers

#### Task 3: `plan` persists each dispatched prompt

**Files:**
- Modify: `plumb/capture/src/cli/plan.rs`

**Interfaces:**
- Consumes: `evidence::write_prompt` (Task 2), the existing
  `DispatchPlan`/`Dispatch` produced by `prompt::plan_dispatch`.
- Produces: `prompt.txt` per lens per scenario in the run directory.

**Blinding note.** The prompt is written *after* `build_prompt` returns.
`plan.rs` must not read anything back from the evidence directory, and
no evidence type may be passed into prompt construction.

- [ ] **Step 1: Write the failing test**

In `cli/plan.rs`'s tests:

```rust
#[test]
fn planning_persists_each_dispatched_prompt_verbatim() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    write_test_manifest(run, "omni", 6); // existing helper in this module
    run_plan(run, None, 8).expect("plan succeeds");

    let p = std::fs::read_to_string(
        run.join("lenses/breakage.omni/prompt.txt")
    ).expect("prompt persisted");
    assert!(p.contains("Sim Sup"), "prompt body was written verbatim");
}
```

If `write_test_manifest` does not exist under that name, use whatever
helper the module's existing tests use to stage a manifest, and keep
the assertion identical.

- [ ] **Step 2: Run to verify it fails**

Run: `cd /d/Dev/Projects/Parallax && cargo test --workspace plan::tests`
Expected: FAIL — `prompt.txt` not found.

- [ ] **Step 3: Implement**

In `run_plan`, after each `Dispatch` is built and before it is emitted,
call:

```rust
evidence::write_prompt(run_dir, d.lens, &d.scenario, &d.prompt)?;
```

Map `EvidenceError` into `plan`'s existing CLI error type, naming the
file, per the settled convention.

- [ ] **Step 4: Run, verify the gates, commit**

Run: `cargo test --workspace` → PASS, 268 tests.

```bash
git add plumb/capture/src/cli/plan.rs
git commit -m "feat(plan): persist each blinded prompt as it is dispatched

Written after the prompt is built and into a directory no lens agent can
read, so evidence never becomes a channel back into a prompt."
```

#### Task 4: `merge` persists replies, findings, and the chain

**Files:**
- Modify: `plumb/capture/src/cli/merge/mod.rs`
- Modify: `plumb/capture/src/main.rs` (document the new `--report` field)

**Interfaces:**
- Consumes: `evidence::{write_reply, write_findings, write_run_json}`,
  `ParsedFindings.dropped` / `.clamped_records` (Task 1).
- Produces: `reply.N.raw.txt`, `parsed.json`, `dropped.json`,
  `clamped.json`, `merge/suppressed.json`, `merge/survivors.json`,
  `run.json`. Task 8 reads all of them.
- `--report` accepts `lens:scenario:file` **or**
  `lens:scenario:file:attempt`; a missing attempt defaults to `1`.

- [ ] **Step 1: Write the failing tests**

In `cli/merge/tests.rs`:

```rust
#[test]
fn merging_persists_the_raw_reply_and_the_discarded_findings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    let body = r#"[
      {"lens":"breakage","scenario":"omni","severity":"major","region":"",
       "claim":"dropped for no region","evidence":"e","confidence":"high"}
    ]"#;
    let f = tmp.path().join("rep.json");
    std::fs::write(&f, body).expect("write report");

    run_merge(run, &[format!("breakage:omni:{}", f.display())], &[], &[], None, None)
        .expect("merge succeeds");

    let raw = std::fs::read_to_string(run.join("lenses/breakage.omni/reply.1.raw.txt"))
        .expect("reply persisted");
    assert!(raw.contains("dropped for no region"));

    let dropped = std::fs::read_to_string(run.join("lenses/breakage.omni/dropped.json"))
        .expect("dropped persisted");
    assert!(dropped.contains("dropped for no region"),
        "the discarded finding keeps its text");
}

#[test]
fn a_fourth_report_field_records_the_retry_attempt() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    let f = tmp.path().join("rep.json");
    std::fs::write(&f, "[]").expect("write report");

    run_merge(run, &[format!("breakage:omni:{}:2", f.display())], &[], &[], None, None)
        .expect("merge succeeds");

    assert!(run.join("lenses/breakage.omni/reply.2.raw.txt").exists(),
        "attempt number came from the fourth field");
}
```

Adjust `run_merge`'s argument list to match its real signature; keep the
assertions identical.

- [ ] **Step 2: Run to verify they fail**

Run: `cd /d/Dev/Projects/Parallax && cargo test --workspace merge::tests`
Expected: FAIL — files not found; four-field parse rejected.

- [ ] **Step 3: Implement**

- Extend `parse_report_arg` to accept an optional fourth
  colon-separated field parsed as `u32`, defaulting to `1`. Reject a
  non-numeric fourth field with an actionable error naming the argument.
- After reading each report file, call `evidence::write_reply` with its
  raw text and attempt.
- After `parse_findings`, call `evidence::write_findings`.
- After suppression and merge, write `merge/suppressed.json` and
  `merge/survivors.json`.
- Call `evidence::write_run_json` once, using the run directory's name
  as the run id.

- [ ] **Step 4: Run, verify the gates, commit**

Run: `cargo test --workspace` → PASS, 270 tests.

```bash
git add plumb/capture/src/cli/merge/mod.rs plumb/capture/src/cli/merge/tests.rs plumb/capture/src/main.rs
git commit -m "feat(merge): persist raw replies, discards, and the merge chain

--report gains an optional fourth field so a retried lens records both
attempts: 'the first reply was garbled and the retry was clean' is a
materially different story from 'it held'."
```

---

## Arc 2: The report

### Slice 2.1: Geometry

#### Task 5: Frame rectangles and crop extraction

**Files:**
- Modify: `plumb/capture/src/contact.rs` (expose geometry)
- Create: `plumb/capture/src/report/geometry.rs`
- Modify: `plumb/capture/src/lib.rs` (add `pub mod report;`)
- Create: `plumb/capture/src/report/mod.rs` (declaring the submodules)

**Interfaces:**
- Consumes: `contact::grid_dims` (made `pub(crate)`), `GUTTER_PX`.
- Produces:
  - `pub struct FrameRect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }`
  - `pub fn frame_rect(index: usize, frame_count: usize, sheet_w: u32, sheet_h: u32) -> Option<FrameRect>`
  - `pub fn crop_png_data_uri(sheet: &Path, rect: FrameRect) -> Result<String, ReportError>`
  - `pub fn png_data_uri(path: &Path) -> Result<String, ReportError>`

  Task 8 calls all three.

**The geometry, stated exactly.** `contact.rs` lays out a
`cols × rows` grid with a gutter on **every** edge, not only between
frames. So for a sheet of `n` frames:

```
cols, rows = grid_dims(n)
frame_w = (sheet_w - (cols + 1) * GUTTER_PX) / cols
frame_h = (sheet_h - (rows + 1) * GUTTER_PX) / rows
x = GUTTER_PX + col * (frame_w + GUTTER_PX)
y = GUTTER_PX + row * (frame_h + GUTTER_PX)
```

Verified against a real sheet: 8 frames of 1920×640 tile to 3×3 at
`3*1920 + 4*8 = 5792` wide and `3*640 + 4*8 = 1952` tall. Using `cols - 1`
gutters instead of `cols + 1` yields 5776 and silently shifts every crop.

- [ ] **Step 1: Write the failing test**

In `report/geometry.rs`:

```rust
#[test]
fn frame_rects_match_a_real_contact_sheet() {
    // 8 frames of 1920x640 tile 3x3 with an 8px gutter on every edge.
    let (w, h) = (5792u32, 1952u32);
    let r0 = frame_rect(0, 8, w, h).expect("frame 0");
    assert_eq!((r0.x, r0.y, r0.w, r0.h), (8, 8, 1920, 640));

    let r2 = frame_rect(2, 8, w, h).expect("frame 2");
    assert_eq!(r2.x, 8 + 2 * (1920 + 8), "third column starts past two gutters");
    assert_eq!(r2.y, 8);

    let r3 = frame_rect(3, 8, w, h).expect("frame 3");
    assert_eq!(r3.x, 8, "fourth frame wraps to the second row");
    assert_eq!(r3.y, 8 + (640 + 8));
}

#[test]
fn an_out_of_range_frame_has_no_rect() {
    assert!(frame_rect(8, 8, 5792, 1952).is_none());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd /d/Dev/Projects/Parallax && cargo test --workspace geometry`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

Change `fn grid_dims` in `contact.rs` to `pub(crate) fn grid_dims`, and
`const GUTTER_PX` to `pub(crate) const GUTTER_PX`. Implement
`frame_rect` with the formula above, returning `None` when
`index >= frame_count`. Implement `crop_png_data_uri` by loading the
sheet with `image::open`, cropping with `image::imageops::crop_imm`,
encoding to PNG bytes in memory, and base64-encoding into
`data:image/png;base64,…`. `png_data_uri` does the same without
cropping.

- [ ] **Step 4: Run, verify the gates, commit**

Run: `cargo test --workspace` → PASS, 272 tests.

```bash
git add plumb/capture/src/contact.rs plumb/capture/src/report/
git commit -m "feat(report): resolve contact-sheet frame rectangles

The sheet carries a gutter on every edge, not only between frames, so a
crop computed with cols-1 gutters silently shows the wrong pixels — the
one arithmetic error that would actively mislead a reader."
```

### Slice 2.2: Region resolution

#### Task 6: A conservative region-to-frame matcher

**Files:**
- Create: `plumb/capture/src/report/region.rs`

**Interfaces:**
- Produces: `pub fn resolve_frame(region: &str, frame_count: usize, cols: u32) -> Option<usize>`.
  Task 8 uses it to decide between a crop and the full sheet.

**Rule.** Resolve only when the text unambiguously names a frame:
either `frame <n>` / `frame #<n>` (1-based), or a row-and-column phrase
(`top row, third frame`) using the ordinals `first`…`ninth` and the row
words `top`/`middle`/`bottom`. Everything else returns `None`. Matching
is case-insensitive. **When in doubt, return `None`** — the full sheet
costs a moment of scanning, a wrong crop costs trust in every crop after
it.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn an_explicit_frame_number_resolves() {
    assert_eq!(resolve_frame("frame 3, upper left", 8, 3), Some(2));
    assert_eq!(resolve_frame("FRAME #1", 8, 3), Some(0));
}

#[test]
fn a_row_and_ordinal_resolves() {
    // top row, third frame -> row 0, col 2 -> index 2
    assert_eq!(resolve_frame("top row, third frame", 8, 3), Some(2));
    // bottom row, first frame -> row 2, col 0 -> index 6
    assert_eq!(resolve_frame("bottom row, first frame", 8, 3), Some(6));
}

#[test]
fn a_vague_region_resolves_to_nothing() {
    assert_eq!(resolve_frame("upper-right quadrant", 8, 3), None);
    assert_eq!(resolve_frame("the mode label row", 8, 3), None);
    assert_eq!(resolve_frame("entire frame", 8, 3), None);
}

#[test]
fn a_frame_number_beyond_the_capture_resolves_to_nothing() {
    assert_eq!(resolve_frame("frame 99", 8, 3), None);
}
```

- [ ] **Step 2: Run to verify it fails, then implement, then verify it passes**

Run: `cargo test --workspace region`
Expected: FAIL, then PASS after implementing `resolve_frame`.

Note `"entire frame"` must **not** match the `frame <n>` rule — require
a digit or `#` immediately following `frame`.

- [ ] **Step 3: Verify the gates, commit**

Run: `cargo test --workspace` → PASS, 276 tests.

```bash
git add plumb/capture/src/report/region.rs
git commit -m "feat(report): resolve a finding's region to a frame, conservatively

Resolves only an unambiguous frame reference and falls back to the full
sheet otherwise. Erring toward the sheet costs a moment of scanning;
erring toward a crop costs the reader's trust in every crop after it."
```

### Slice 2.3: Rendering

#### Task 7: HTML skeleton and self-containment

**Files:**
- Create: `plumb/capture/src/report/render.rs`
- Modify: `plumb/capture/Cargo.toml` (add `base64 = "0.22"`)

**Interfaces:**
- Produces: `pub fn render_report(run: &RunReport) -> String` and
  `pub struct RunReport { pub run_id: String, pub contract_version: Option<u32>, pub scenarios: Vec<ScenarioReport> }`.
  Task 8 fills `ScenarioReport`; Task 9 writes the string to disk.

`base64` is the one new dependency; it is required to inline images and
has no alternative already in the tree. Add exactly that, nothing else.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn the_rendered_report_references_no_external_resource() {
    let html = render_report(&RunReport {
        run_id: "r".into(), contract_version: Some(1), scenarios: vec![],
    });
    assert!(!html.contains("http://"), "no external URL");
    assert!(!html.contains("https://"), "no external URL");
    assert!(!html.contains("<link"), "no external stylesheet or font");
    assert!(html.contains("<!doctype html>") || html.contains("<!DOCTYPE html>"));
}
```

- [ ] **Step 2: Fail, implement, pass**

Implement `render_report` emitting a complete standalone document with
an inline `<style>` block. Escape all interpolated text with an
`html_escape` helper in this module (`&`, `<`, `>`, `"`), since prompts
and replies contain arbitrary model output.

- [ ] **Step 3: Verify the gates, commit**

```bash
git add plumb/capture/src/report/render.rs plumb/capture/Cargo.toml plumb/capture/Cargo.lock
git commit -m "feat(report): render a self-contained HTML document

Evidence that breaks when moved is not evidence, so the report inlines
everything and references no external resource."
```

#### Task 8: Lens sections, discards, and the attrition chain

**Files:**
- Modify: `plumb/capture/src/report/render.rs`
- Modify: `plumb/capture/src/report/mod.rs`

**Interfaces:**
- Consumes: `evidence::read_lens_evidence`, `manifest::read_manifest`,
  `geometry::{frame_rect, crop_png_data_uri, png_data_uri}`,
  `region::resolve_frame`.
- Produces: `pub fn build_run_report(run_dir: &Path) -> RunReport`, and
  `pub struct ScenarioReport { pub scenario: String, pub sheet_uri: Option<String>, pub frame_count: usize, pub lenses: Vec<LensReport> }`,
  `pub struct LensReport { pub lens: Lens, pub findings: Vec<RenderedFinding>, pub dropped: Evidence<Vec<Finding>>, pub clamped: Evidence<Vec<ClampRecord>>, pub prompt: Evidence<String>, pub replies: Vec<(u32, String)> }`,

  **Note the `Evidence<T>` fields (corrected 2026-08-16).** Task 8 must
  render all three states distinctly: `Missing` → *"not persisted"*,
  `Unparseable(raw)` → *"present but unparseable"* **with the raw text
  shown**, `Present(v)` → the findings. Collapsing `Unparseable` into
  `Missing` would drop content the run actually recorded, which is the
  one thing this report exists to stop.

  and

  ```rust
  /// A finding paired with the crop of the frame its region names, when
  /// that region resolved to one.
  pub struct RenderedFinding {
      /// The finding as it reached the verdict.
      pub finding: Finding,
      /// A `data:image/png;base64,…` crop of the named frame, or `None`
      /// when the region did not unambiguously identify one.
      pub crop_uri: Option<String>,
  }
  ```

  Task 9 calls `build_run_report`.

- [ ] **Step 1: Write the failing test — this is the load-bearing one**

```rust
#[test]
fn the_report_contains_every_discarded_findings_text() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    stage_run_with_discards(run); // helper: writes manifest, sheet, and
    // evidence containing one dropped and one clamped finding whose
    // claims are "DROPPED-CLAIM-TEXT" and "CLAMPED-CLAIM-TEXT".

    let html = render_report(&build_run_report(run));

    assert!(html.contains("DROPPED-CLAIM-TEXT"),
        "a dropped finding's text must survive into the report");
    assert!(html.contains("CLAMPED-CLAIM-TEXT"),
        "a clamped finding's text must survive into the report");
}

#[test]
fn a_lens_with_no_persisted_reply_says_so() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    stage_run_without_evidence(run); // manifest and sheet only

    let html = render_report(&build_run_report(run));
    assert!(html.contains("no reply persisted"),
        "absence is marked, never rendered as zero findings");
}
```

- [ ] **Step 2: Fail, implement, pass**

`build_run_report` walks the run directory: reads each
`*.manifest.json`, embeds the sheet via `png_data_uri`, and for each of
the four lenses calls `read_lens_evidence`. For each kept finding, call
`resolve_frame(&f.region, frame_count, cols)`; on `Some(i)`, attach
`crop_png_data_uri(sheet, frame_rect(i, …))`, otherwise attach nothing
and render against the full sheet.

`render_report` emits, per scenario: the verdict line, the sheet, then
per lens the findings (each with its crop when resolved), a visible line
per discard with the text in a collapsed `<details>`, the prompt and each
reply in collapsed `<details>`, and the attrition chain
`N raw → D dropped → C clamped → S suppressed → K survivors`.

- [ ] **Step 3: Verify the gates, commit**

```bash
git add plumb/capture/src/report/
git commit -m "feat(report): render lens findings, discards, and attrition

Asserts the discarded findings' text reaches the output, which is the
one property the tool exists to provide and the one a later tidy-up of
the report would most naturally erode."
```

#### Task 9: The `report` subcommand

**Files:**
- Create: `plumb/capture/src/cli/report.rs`
- Modify: `plumb/capture/src/cli/mod.rs`, `plumb/capture/src/main.rs`

**Interfaces:**
- Consumes: `report::{build_run_report, render_report}`.
- Produces: `plumb report <run-dir> [--out <file>]`, exit `0` on
  success, `1` on an unreadable run directory. Defaults `--out` to
  `<run-dir>/report.html`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn report_writes_a_file_and_never_touches_the_run() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    stage_run_with_discards(run);
    let before = dir_fingerprint(run); // helper: sorted (name, len) pairs

    let out = tmp.path().join("r.html");
    assert_eq!(run_report(run, Some(&out)), 0);
    assert!(out.exists());
    assert_eq!(dir_fingerprint(run), before,
        "report is read-only with respect to the run directory");
}
```

- [ ] **Step 2: Fail, implement, pass. Verify the gates, commit.**

```bash
git add plumb/capture/src/cli/report.rs plumb/capture/src/cli/mod.rs plumb/capture/src/main.rs
git commit -m "feat(cli): add plumb report

A viewer, not a stage: it runs no capture, dispatches no agents, and is
asserted not to write into the run directory it reads."
```

### Slice 2.4: Guarding the boundary

#### Task 10: Assert evidence never reaches a prompt

**Files:**
- Modify: `plumb/capture/src/prompt/tests.rs`

**Interfaces:**
- Consumes: nothing new. Extends the existing guard established in Arc 4
  for rulings.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn no_evidence_type_or_path_reaches_prompt_construction() {
    for f in ["mod.rs", "text.rs"] {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/prompt").join(f)
        ).expect("read prompt source");
        for needle in ["evidence", "Evidence", "reply.raw", "read_lens_evidence"] {
            assert!(!src.contains(needle),
                "{f} must not reference {needle}: persisting evidence must \
                 never become a channel into a prompt");
        }
    }
}
```

- [ ] **Step 2: Run — it should PASS immediately**

This test guards a property the design already holds. Confirm it is a
real guard rather than vacuous by temporarily adding
`// evidence` to `prompt/mod.rs`, re-running to watch it FAIL, then
reverting.

- [ ] **Step 3: Verify the gates, commit**

```bash
git add plumb/capture/src/prompt/tests.rs
git commit -m "test(prompt): guard evidence out of prompt construction

Same shape as Arc 4's ruling guard. Verified to fire by temporarily
adding the forbidden token and watching it fail."
```

---

## Arc 3: The check that actually matters

#### Task 11: Generate a report from a real run and read it

**Files:** none — this task produces a judgement, not a diff.

**TDD exception: real-artifact verification**, the same class as this
repository's real-TTY and visual-review carve-outs. Whether the report
is *legible* has no unit test.

- [ ] **Step 1: Run a real scenario end to end**

From the TTUI worktree
`D:/Dev/Projects/TTUI/.claude/worktrees/plumb-seed-scenario`, capture
`omnitrix-dial-rotate` into a fresh run directory, then `plan`, then
dispatch the four lenses through the harness, then `merge`.

- [ ] **Step 2: Generate the report**

```bash
D:/Dev/Projects/Parallax/target/debug/plumb.exe report .plumb/runs/<id> --out report.html
```

- [ ] **Step 3: Open it and answer these, in writing**

1. Can you see the contact sheet at full resolution?
2. For each lens, can you read the exact prompt it was given?
3. For each lens, can you read the raw text it returned?
4. Does at least one finding sit beside a crop of the frame it names,
   and is that the correct frame?
5. Are the discarded findings visible with their text?
6. Does the attrition chain account for every finding?

- [ ] **Step 4: Record the answers**

Write them into the task report. **A "no" to question 4 or 5 is a
finding, not a note** — those are the two properties the report exists
to provide.

- [ ] **Step 5: Commit the report as evidence**

```bash
git add report.html
git commit -m "docs(plumb): record the first audited Plumb run

The report generated from a real omnitrix capture, kept as the archival
unit for this run per the evidence design's durability rule."
```

---

## Spec coverage

| Spec requirement | Task |
|---|---|
| Evidence contract, versioned layout | 2 |
| Discards retain their text | 1, 4, 8 |
| Prompts persisted | 3 |
| Raw replies persisted, retries distinguished | 4 |
| Merge chain persisted | 4 |
| Report organised around the five questions | 8 |
| Region anchored to a crop | 5, 6, 8 |
| Conservative matcher, falls back to full sheet | 6 |
| Self-contained output | 7 |
| Absence marked, never omitted | 8 |
| `report` read-only | 9 |
| Multi-scenario runs | 8 |
| Blinding preserved | 3, 10 |
| Manual legibility check | 11 |

**Deferred by the spec, deliberately absent from this plan:** the TTUI
interactive view, `plumb verify`, repeatability guarantees, cross-run
comparison, and reconstruction of pre-contract runs.

## Judgment calls made while planning

1. **`ParsedFindings` grows additively** rather than replacing its
   counts, so no existing caller or test changes. The redundancy is
   deliberate and cheap.
2. **`read_lens_evidence` returns a struct, not a `Result`.** A missing
   artifact is an expected state the report must render, not an error —
   making it an error would push "absence" into a code path that
   naturally logs and continues, which is how absence starts reading as
   success.
3. **`base64` is the only new dependency.** Required to inline images;
   nothing already in the tree does it.
4. **The region matcher handles two phrasings**, numeric and
   row-plus-ordinal, drawn from the regions real lens agents actually
   produced during Arcs 2–6. It is deliberately narrow; widening it is
   cheap and safe, while a false positive is not.
5. **Task 10 is expected to pass on the first run.** It guards a
   property the design already holds, and the plan requires proving it
   fires rather than trusting a green result — a guard nobody has seen
   fail is a guard nobody should trust.

## Execution handoff

Plan complete. Arcs 1–2 are ten small TDD tasks; Arc 3 is a single
human judgement that no subagent can substitute for.
