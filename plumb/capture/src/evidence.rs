//! The versioned on-disk evidence contract: the prompt a lens agent
//! read, the raw reply it returned, and what the pipeline did with that
//! reply, persisted so a verdict can be audited after the fact. This
//! module deliberately never reads from or writes into `prompt/` —
//! persisting evidence must not become a channel back into a prompt.

use crate::finding::{ClampRecord, Finding, Lens};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The evidence contract's schema version, written into every `run.json`.
pub const CONTRACT_VERSION: u32 = 1;

/// An I/O failure acting on a file, together with the path that caused
/// it — kept as a single field so `EvidenceError::Io(_)` stays an
/// opaque match for callers that only care *that* it failed, not why.
/// Mirrors `manifest::IoFailure`.
#[derive(Debug)]
pub struct IoFailure {
    /// The path the failing operation was acting on.
    pub path: PathBuf,
    /// The underlying I/O error.
    pub source: std::io::Error,
}

/// A JSON encoding failure, together with the path that caused it.
/// Mirrors `manifest::JsonFailure`.
#[derive(Debug)]
pub struct JsonFailure {
    /// The path the failing operation was acting on.
    pub path: PathBuf,
    /// The underlying JSON error.
    pub source: serde_json::Error,
}

/// Failure persisting a piece of run evidence.
#[derive(Debug)]
pub enum EvidenceError {
    /// Filesystem failure, and the path that caused it.
    Io(IoFailure),
    /// JSON encoding failure, and the path that caused it.
    Json(JsonFailure),
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvidenceError::Io(e) => write!(f, "writing {}: {}", e.path.display(), e.source),
            EvidenceError::Json(e) => write!(f, "encoding {}: {}", e.path.display(), e.source),
        }
    }
}
impl std::error::Error for EvidenceError {}

/// `Lens`'s serde name (`breakage`/`intent`/`design`/`motion`), used to
/// name a lens's evidence directory. Matches `finding::Lens`'s
/// `#[serde(rename_all = "lowercase")]` by construction, not by a
/// second hand-maintained mapping — see the
/// `lens_dir_names_match_lens_serde_output` test below, which guards
/// against the two drifting apart.
fn lens_name(lens: Lens) -> &'static str {
    match lens {
        Lens::Breakage => "breakage",
        Lens::Intent => "intent",
        Lens::Design => "design",
        Lens::Motion => "motion",
    }
}

/// The evidence directory for one lens dispatched against one scenario:
/// `<run_dir>/lenses/<lens>.<scenario>/`.
pub fn lens_dir(run_dir: &Path, lens: Lens, scenario: &str) -> PathBuf {
    run_dir
        .join("lenses")
        .join(format!("{}.{}", lens_name(lens), scenario))
}

/// The directory holding whole-run evidence not scoped to a single lens
/// (the merge step's own inputs/outputs, once Task 4 wires it in).
pub fn merge_dir(run_dir: &Path) -> PathBuf {
    run_dir.join("merge")
}

/// Writes `contents` to `path`, creating any missing parent directories
/// first. The one place every writer in this module funnels through,
/// so every I/O failure is reported the same way.
fn write_text(path: &Path, contents: &str) -> Result<(), EvidenceError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| {
            EvidenceError::Io(IoFailure {
                path: parent.to_path_buf(),
                source,
            })
        })?;
    }
    std::fs::write(path, contents).map_err(|source| {
        EvidenceError::Io(IoFailure {
            path: path.to_path_buf(),
            source,
        })
    })
}

/// Serializes `value` to pretty JSON and writes it via [`write_text`].
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), EvidenceError> {
    let json = serde_json::to_string_pretty(value).map_err(|source| {
        EvidenceError::Json(JsonFailure {
            path: path.to_path_buf(),
            source,
        })
    })?;
    write_text(path, &json)
}

/// Persists the exact prompt text one lens read for one scenario, to
/// `<lens_dir>/prompt.txt`.
pub fn write_prompt(
    run_dir: &Path,
    lens: Lens,
    scenario: &str,
    prompt: &str,
) -> Result<(), EvidenceError> {
    write_text(
        &lens_dir(run_dir, lens, scenario).join("prompt.txt"),
        prompt,
    )
}

/// Persists one attempt's raw reply text, to
/// `<lens_dir>/reply.<attempt>.raw.txt`. A retried lens writes one file
/// per attempt; none are overwritten.
pub fn write_reply(
    run_dir: &Path,
    lens: Lens,
    scenario: &str,
    attempt: u32,
    raw: &str,
) -> Result<(), EvidenceError> {
    let path = lens_dir(run_dir, lens, scenario).join(format!("reply.{attempt}.raw.txt"));
    write_text(&path, raw)
}

/// Persists a lens's enforcement outcome as three arrays — `parsed.json`
/// (survived findings), `dropped.json` (dropped for naming no region),
/// `clamped.json` (severity-clamped) — each written even when empty, so
/// a reader can tell "recorded, and empty" from "never recorded".
pub fn write_findings(
    run_dir: &Path,
    lens: Lens,
    scenario: &str,
    parsed: &crate::finding::ParsedFindings,
) -> Result<(), EvidenceError> {
    let dir = lens_dir(run_dir, lens, scenario);
    write_json(&dir.join("parsed.json"), &parsed.kept)?;
    write_json(&dir.join("dropped.json"), &parsed.dropped)?;
    write_json(&dir.join("clamped.json"), &parsed.clamped_records)?;
    Ok(())
}

/// The run's evidence-contract version and id, written once per run to
/// `<run_dir>/run.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunJson {
    /// The evidence contract version this run was written under.
    pub contract_version: u32,
    /// The run's timestamp id, shared by every scenario in the run.
    pub run_id: String,
}

/// Writes `<run_dir>/run.json`, stamping the current [`CONTRACT_VERSION`].
pub fn write_run_json(run_dir: &Path, run_id: &str) -> Result<(), EvidenceError> {
    write_json(
        &run_dir.join("run.json"),
        &RunJson {
            contract_version: CONTRACT_VERSION,
            run_id: run_id.to_string(),
        },
    )
}

/// Everything persisted for one lens dispatched against one scenario,
/// as actually found on disk. A missing artifact is a `None`/empty
/// field, never an error — a report renders it as an explicit marker
/// ("not persisted"), not a lens that silently said nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LensEvidence {
    /// The prompt the lens read, if `prompt.txt` was persisted.
    pub prompt: Option<String>,
    /// Every raw reply the lens returned, as `(attempt, text)`, sorted
    /// ascending by attempt so a retry sequence reads in order.
    pub replies: Vec<(u32, String)>,
    /// Findings that survived enforcement, if `parsed.json` was persisted.
    pub parsed: Option<Vec<Finding>>,
    /// Findings dropped for naming no region, if `dropped.json` was
    /// persisted.
    pub dropped: Option<Vec<Finding>>,
    /// Findings whose severity was clamped, if `clamped.json` was
    /// persisted.
    pub clamped: Option<Vec<ClampRecord>>,
}

/// Reads a text file as UTF-8, substituting the replacement character
/// for any invalid byte rather than failing — so a present-but-oddly-
/// encoded file still surfaces its content instead of reading as
/// "not persisted". Returns `None` only when the file itself could not
/// be read (most commonly: it does not exist).
fn read_text_lossy(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads and parses a JSON file. Returns `None` when the file is
/// missing, unreadable, or not valid JSON for `T` — this module's
/// pinned struct shape (`Option<Vec<Finding>>` / `Option<Vec<ClampRecord>>`)
/// has no variant that can carry raw bytes instead, so an unparseable
/// file collapses into the same "not persisted" marker as an absent one.
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Parses the attempt number out of a `reply.<attempt>.raw.txt`
/// filename; `None` for anything else in the directory.
fn parse_reply_attempt(file_name: &str) -> Option<u32> {
    file_name
        .strip_prefix("reply.")
        .and_then(|rest| rest.strip_suffix(".raw.txt"))
        .and_then(|n| n.parse().ok())
}

/// Reads back everything persisted for one lens dispatched against one
/// scenario. Never fails: a directory that was never written yields a
/// [`LensEvidence`] of all-`None`/empty fields, because absence here is
/// an expected state the caller must render explicitly, not an error
/// path that would let a missing artifact quietly read as a clean pass.
pub fn read_lens_evidence(run_dir: &Path, lens: Lens, scenario: &str) -> LensEvidence {
    let dir = lens_dir(run_dir, lens, scenario);

    let prompt = read_text_lossy(&dir.join("prompt.txt"));

    let mut replies: Vec<(u32, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(attempt) = parse_reply_attempt(&name.to_string_lossy()) else {
                continue;
            };
            if let Some(text) = read_text_lossy(&entry.path()) {
                replies.push((attempt, text));
            }
        }
    }
    replies.sort_by_key(|(attempt, _)| *attempt);

    LensEvidence {
        prompt,
        replies,
        parsed: read_json(&dir.join("parsed.json")),
        dropped: read_json(&dir.join("dropped.json")),
        clamped: read_json(&dir.join("clamped.json")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Guards `lens_name` (and therefore `lens_dir`) against drifting
    /// from `Lens`'s own serde output — the brief pins the directory
    /// name to "`Lens`'s serde name" specifically so a reader can find
    /// a lens's evidence directory from a manifest's own `lens` field
    /// without a second lookup table.
    #[test]
    fn lens_dir_names_match_lens_serde_output() {
        for (lens, expected) in [
            (Lens::Breakage, "breakage"),
            (Lens::Intent, "intent"),
            (Lens::Design, "design"),
            (Lens::Motion, "motion"),
        ] {
            let serde_name = serde_json::to_string(&lens).unwrap();
            assert_eq!(serde_name, format!("\"{expected}\""));
            assert_eq!(lens_name(lens), expected);
        }
    }

    /// `write_findings` must persist all three arrays even when every
    /// one of them is empty, so an absent file (never run) and an
    /// empty one (ran, found/dropped/clamped nothing) read differently.
    #[test]
    fn write_findings_persists_empty_arrays_not_absence() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let run = tmp.path();
        let parsed = crate::finding::ParsedFindings {
            kept: Vec::new(),
            dropped_no_region: 0,
            clamped: 0,
            dropped: Vec::new(),
            clamped_records: Vec::new(),
        };
        write_findings(run, Lens::Design, "omni", &parsed).expect("write_findings");

        let ev = read_lens_evidence(run, Lens::Design, "omni");
        assert_eq!(ev.parsed, Some(Vec::new()));
        assert_eq!(ev.dropped, Some(Vec::new()));
        assert_eq!(ev.clamped, Some(Vec::new()));
    }

    /// A retried lens's replies must come back oldest-attempt-first
    /// regardless of filesystem enumeration order, since attempt order
    /// is audit-relevant ("garbled then clean" vs. "clean then garbled"
    /// are different stories).
    #[test]
    fn replies_sort_ascending_by_attempt_even_when_written_out_of_order() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let run = tmp.path();
        write_reply(run, Lens::Intent, "omni", 3, "third").expect("r3");
        write_reply(run, Lens::Intent, "omni", 1, "first").expect("r1");
        write_reply(run, Lens::Intent, "omni", 2, "second").expect("r2");

        let ev = read_lens_evidence(run, Lens::Intent, "omni");
        assert_eq!(
            ev.replies,
            vec![
                (1, "first".to_string()),
                (2, "second".to_string()),
                (3, "third".to_string()),
            ]
        );
    }
}
