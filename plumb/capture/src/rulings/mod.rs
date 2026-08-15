//! Rulings: a post-hoc suppression filter over merged findings, never
//! an input to a lens prompt. An operator overrules a finding once;
//! `suppress` hides it from later runs' verdicts (scoped to the
//! scenario by default) until `taste.md` changes, at which point the
//! ruling goes stale and the finding returns rather than staying
//! silenced under a rejected aesthetic. See the design spec's
//! "Rulings, and the calcification guard" for the full rationale.
//!
//! Structurally isolated from prompt construction: this module is
//! never imported by `prompt/mod.rs` or `prompt/text.rs`, and
//! `prompt::LensInputs` carries no ruling-shaped field for one to flow
//! through even by accident. `prompt_construction_never_references_
//! rulings_or_suppression` (below) pins that as a regression guard.

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::finding::{Lens, Severity};
use crate::merge::{normalize_claim, MergedFinding};

/// How far a ruling's suppression reaches. `Scenario` is the default;
/// `ProjectWide` is an explicit opt-in so overruling one screen's
/// density can never silently mute density everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Applies only to findings from the scenario the ruling was made
    /// against.
    Scenario,
    /// Applies to a matching finding from any scenario.
    ProjectWide,
}

/// A recorded overrule of one finding: its identity, the operator's
/// reasoning, when it was made, and what `taste.md` looked like at the
/// time. Never read by a lens agent — see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ruling {
    /// The overruled finding's fingerprint (Task 11): identifies
    /// scenario, region, and claim, but deliberately not the lens.
    pub fingerprint: String,
    /// The lens that originally raised the overruled finding. Stored
    /// for the record; not part of the fingerprint's identity, and not
    /// consulted when matching a later finding raised by a different
    /// lens.
    pub lens: Lens,
    /// The severity of the finding that was actually overruled. Acts
    /// as a ceiling: this ruling suppresses only findings whose
    /// severity is at or below it, so waiving a cosmetic complaint can
    /// never silence a blocker that happens to share the same region.
    pub severity: Severity,
    /// The scenario the ruling was made against.
    pub scenario: String,
    /// Where on screen the overruled finding lived.
    pub region: String,
    /// The overruled finding's claim, verbatim.
    pub claim: String,
    /// The operator's reasoning for overruling it.
    pub reason: String,
    /// The date the ruling was made (`YYYY-MM-DD`).
    pub date: String,
    /// `taste_hash` of `taste.md` at ruling time. A later run whose
    /// current hash differs marks this ruling stale rather than
    /// applying it forever.
    pub taste_hash: String,
    /// How far this ruling's suppression reaches.
    pub scope: Scope,
}

/// A file operation failure, together with the path that caused it —
/// mirrors `manifest::IoFailure`/`config::IoFailure` so `RulingError`
/// stays an opaque, single-wildcard match for a caller that only cares
/// that loading failed.
#[derive(Debug)]
pub struct IoFailure {
    /// The path `load_rulings` was asked to read.
    pub path: PathBuf,
    /// The underlying I/O error.
    pub source: std::io::Error,
}

/// A JSONL parse failure, together with the path that caused it.
#[derive(Debug)]
pub struct JsonFailure {
    /// The path `load_rulings` was asked to read.
    pub path: PathBuf,
    /// The underlying JSON error.
    pub source: serde_json::Error,
}

/// Failure loading rulings from `rulings.jsonl`. A missing file is not
/// one of these — it is a legitimate empty history (see `load_rulings`).
#[derive(Debug)]
pub enum RulingError {
    /// Filesystem failure reading the file, and the path that caused it.
    Io(IoFailure),
    /// A line was not valid JSON, or not this schema.
    Json(JsonFailure),
}

impl std::fmt::Display for RulingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RulingError::Io(e) => write!(f, "reading {}: {}", e.path.display(), e.source),
            RulingError::Json(e) => write!(f, "parsing {}: {}", e.path.display(), e.source),
        }
    }
}
impl std::error::Error for RulingError {}

/// The sentinel hashed in place of `taste.md`'s content when no
/// `taste.md` exists, so `taste_hash(None)` is stable and distinct
/// from hashing an empty file.
const NO_TASTE_SENTINEL: &str = "plumb:no-taste-profile";

/// A content hash of `taste.md` (or a stable sentinel when absent):
/// the first 16 hex characters of `sha256`, matching `merge::
/// fingerprint`'s own truncation convention. A ruling records this at
/// the time it is made so a later edit to `taste.md` can be detected.
pub fn taste_hash(taste: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(taste.unwrap_or(NO_TASTE_SENTINEL).as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Reads every ruling from `path`, one JSON object per line. A missing
/// file is not an error: it is the expected state before any ruling
/// has ever been made, and loads as an empty history.
pub fn load_rulings(path: &Path) -> Result<Vec<Ruling>, RulingError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(RulingError::Io(IoFailure {
                path: path.to_path_buf(),
                source,
            }))
        }
    };

    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|source| {
                RulingError::Json(JsonFailure {
                    path: path.to_path_buf(),
                    source,
                })
            })
        })
        .collect()
}

/// Appends one ruling to `path` as a single JSON line, creating the
/// file (and its parent directory) if this is the first ruling ever
/// recorded. Never rewrites or reorders earlier lines.
pub fn append_ruling(path: &Path, r: &Ruling) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_string(r)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{json}")
}

/// The result of running every merged finding through the ruling
/// history: what still needs a look, what a prior ruling already
/// disposed of, and which rulings need re-validation because
/// `taste.md` moved out from under them.
pub struct Suppression {
    /// Findings no ruling disposed of; these are what the verdict
    /// actually judges the run on.
    pub kept: Vec<MergedFinding>,
    /// Findings a still-fresh ruling already overruled. Never dropped
    /// outright — `verdict::render_verdict` collapses these into a
    /// `previously overruled (N)` line so a finding several
    /// independent runs keep raising stays visible.
    pub suppressed: Vec<MergedFinding>,
    /// Fingerprints of findings whose only matching ruling(s) were made
    /// under a `taste.md` hash that no longer matches: the ruling needs
    /// re-validation, and (the strict reading) the finding is kept, not
    /// suppressed.
    pub stale: Vec<String>,
}

/// Whether `r` applies to `finding` by scope alone, ignoring severity
/// and staleness. A `Scenario`-scoped ruling's stored `fingerprint`
/// already encodes its own scenario (Task 11: `sha256(scenario +
/// region + claim)`), so exact fingerprint equality is sufficient and
/// automatically confines it to that scenario. A `ProjectWide` ruling
/// must cross scenarios by design, so it instead compares the
/// normalized region and claim directly, the same normalization
/// `merge::fingerprint` itself uses, deliberately ignoring scenario.
fn scope_matches(r: &Ruling, finding: &MergedFinding) -> bool {
    match r.scope {
        Scope::Scenario => r.fingerprint == finding.fingerprint,
        Scope::ProjectWide => {
            normalize_claim(&r.region) == normalize_claim(&finding.finding.region)
                && normalize_claim(&r.claim) == normalize_claim(&finding.finding.claim)
        }
    }
}

/// Partitions `findings` against `rulings`: a finding is suppressed
/// only by a ruling that (1) matches it by scope (see `scope_matches`),
/// (2) was made under the current `taste.md` hash, and (3) covers the
/// finding's severity — a ruling suppresses only findings at or below
/// the severity of the finding it was originally made against, so
/// overruling a cosmetic complaint can never silence a blocker sharing
/// its region. A finding whose only matching ruling(s) were made under
/// a different taste hash is kept (the strict reading: a stale ruling
/// does not suppress) and its fingerprint is collected into `stale`.
pub fn suppress(
    findings: Vec<MergedFinding>,
    rulings: &[Ruling],
    current_taste_hash: &str,
) -> Suppression {
    let mut kept = Vec::new();
    let mut suppressed = Vec::new();
    let mut stale = Vec::new();

    for finding in findings {
        let relevant: Vec<&Ruling> = rulings
            .iter()
            .filter(|r| scope_matches(r, &finding))
            .collect();

        let applies = relevant
            .iter()
            .any(|r| r.taste_hash == current_taste_hash && finding.finding.severity <= r.severity);

        if applies {
            suppressed.push(finding);
            continue;
        }

        if relevant.iter().any(|r| r.taste_hash != current_taste_hash) {
            stale.push(finding.fingerprint.clone());
        }
        kept.push(finding);
    }

    Suppression {
        kept,
        suppressed,
        stale,
    }
}
