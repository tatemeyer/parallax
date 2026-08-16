//! Tests for `cli::merge`: argument parsing, the merge/verdict
//! pipeline's exit-code contract, and the ruling suppression wiring
//! (Arc 4) — split into its own file to keep `cli/merge/mod.rs`
//! under the project's soft line-count ceiling.

use super::*;

fn write_report(dir: &Path, name: &str, json: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, json).unwrap();
    path
}

const ONE_BLOCKER: &str = r#"[{"lens":"breakage","scenario":"dial","severity":"blocker",
  "region":"upper-right","claim":"the border does not close",
  "evidence":"e","confidence":"high"}]"#;

#[test]
fn merge_rejects_a_malformed_report_spec() {
    let err = parse_report_arg("breakage-dial-file.json").unwrap_err();
    assert!(matches!(err, MergeCliError::Usage(_)));
}

#[test]
fn merge_rejects_an_unknown_lens_name() {
    let err = parse_report_arg("nonsense:dial:file.json").unwrap_err();
    assert!(matches!(err, MergeCliError::Usage(_)));
}

#[test]
fn a_clean_report_produces_a_go_and_writes_verdict_md() {
    let tmp = tempfile::tempdir().unwrap();
    let report = write_report(tmp.path(), "breakage.json", "[]");
    let spec = format!("breakage:dial:{}", report.display());

    let (verdict, path) = run_merge(tmp.path(), &[spec], &[], &[], None, None).unwrap();

    assert_eq!(verdict, Verdict::Go);
    assert!(path.is_file());
}

#[test]
fn a_blocker_report_produces_a_no_go() {
    let tmp = tempfile::tempdir().unwrap();
    let report = write_report(tmp.path(), "breakage.json", ONE_BLOCKER);
    let spec = format!("breakage:dial:{}", report.display());

    let (verdict, _) = run_merge(tmp.path(), &[spec], &[], &[], None, None).unwrap();

    assert_eq!(verdict, Verdict::NoGo);
}

#[test]
fn unparseable_report_text_holds_rather_than_erroring_the_whole_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let report = write_report(tmp.path(), "intent.json", "not json at all");
    let spec = format!("intent:dial:{}", report.display());

    let (verdict, _) = run_merge(tmp.path(), &[spec], &[], &[], None, None).unwrap();

    assert_eq!(
        verdict,
        Verdict::Hold,
        "an unparseable lens report must hold, never silently pass as GO"
    );
}

#[test]
fn a_missing_report_file_is_an_io_error() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = format!(
        "breakage:dial:{}",
        tmp.path().join("missing.json").display()
    );

    let err = run_merge(tmp.path(), &[spec], &[], &[], None, None).unwrap_err();

    assert!(matches!(err, MergeCliError::Io(_)));
}

// --- review Finding 1: a capture failure must actually be able to
// reach verdict.md from the CLI, not just from a hand-built
// VerdictInput in a unit test. ----------------------------------

#[test]
fn a_capture_failure_flag_holds_the_run_with_no_report_at_all() {
    let tmp = tempfile::tempdir().unwrap();
    let cf = "tardis-idle:unmapped glyph U+2726".to_string();

    let (verdict, path) = run_merge(tmp.path(), &[], &[], &[cf], None, None).unwrap();

    assert_eq!(
        verdict,
        Verdict::Hold,
        "a capture failure supplied via the CLI must reach the verdict, not just a hand-built VerdictInput"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("tardis-idle"));
    assert!(text.contains("U+2726"));
}

#[test]
fn a_capture_failure_flag_does_not_soften_an_existing_no_go() {
    let tmp = tempfile::tempdir().unwrap();
    let report = write_report(tmp.path(), "breakage.json", ONE_BLOCKER);
    let spec = format!("breakage:dial:{}", report.display());
    let cf = "other-scenario:boom".to_string();

    let (verdict, _) = run_merge(tmp.path(), &[spec], &[], &[cf], None, None).unwrap();

    assert_eq!(verdict, Verdict::NoGo);
}

#[test]
fn merge_rejects_a_malformed_capture_failure_arg() {
    let tmp = tempfile::tempdir().unwrap();
    let err = run_merge(tmp.path(), &[], &[], &["no-colon-here".into()], None, None).unwrap_err();
    assert!(matches!(err, MergeCliError::Usage(_)));
}

// --- review Finding 2: a lens that was dispatched but never
// returned a report must hold, not silently vanish from the poll.

#[test]
fn an_expected_lens_with_no_report_holds_the_run() {
    let tmp = tempfile::tempdir().unwrap();
    let report = write_report(tmp.path(), "breakage.json", "[]");
    let spec = format!("breakage:dial:{}", report.display());

    let (verdict, path) = run_merge(
        tmp.path(),
        &[spec],
        &["motion:dial".to_string()],
        &[],
        None,
        None,
    )
    .unwrap();

    assert_eq!(
        verdict,
        Verdict::Hold,
        "a lens that was expected to report but did not must hold, not vanish"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("motion"));
}

#[test]
fn an_expected_lens_that_did_report_is_not_double_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let report = write_report(tmp.path(), "breakage.json", "[]");
    let spec = format!("breakage:dial:{}", report.display());

    let (verdict, _) = run_merge(
        tmp.path(),
        &[spec],
        &["breakage:dial".to_string()],
        &[],
        None,
        None,
    )
    .unwrap();

    assert_eq!(
        verdict,
        Verdict::Go,
        "a lens that both was expected and did report must not read as missing"
    );
}

#[test]
fn merge_rejects_a_malformed_expected_arg() {
    let tmp = tempfile::tempdir().unwrap();
    let err = run_merge(tmp.path(), &[], &["no-colon-here".into()], &[], None, None).unwrap_err();
    assert!(matches!(err, MergeCliError::Usage(_)));
}

// --- evidence persistence (Task 4) ------------------------------

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

    run_merge(
        run,
        &[format!("breakage:omni:{}", f.display())],
        &[],
        &[],
        None,
        None,
    )
    .expect("merge succeeds");

    let raw = std::fs::read_to_string(run.join("lenses/breakage.omni/reply.1.raw.txt"))
        .expect("reply persisted");
    assert!(raw.contains("dropped for no region"));

    let dropped = std::fs::read_to_string(run.join("lenses/breakage.omni/dropped.json"))
        .expect("dropped persisted");
    assert!(
        dropped.contains("dropped for no region"),
        "the discarded finding keeps its text"
    );
}

#[test]
fn a_fourth_report_field_records_the_retry_attempt() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path();
    let f = tmp.path().join("rep.json");
    std::fs::write(&f, "[]").expect("write report");

    run_merge(
        run,
        &[format!("breakage:omni:{}:2", f.display())],
        &[],
        &[],
        None,
        None,
    )
    .expect("merge succeeds");

    assert!(
        run.join("lenses/breakage.omni/reply.2.raw.txt").exists(),
        "attempt number came from the fourth field"
    );
}

/// The hazard this task was warned about: `--report` is parsed with
/// `splitn(3, ':')` so the file path is everything after the second
/// colon, which is exactly why an absolute Windows path (its own
/// drive-letter colon included) survives untouched today. A naive
/// `splitn(4, ':')` to add the attempt field would instead split the
/// drive letter off — `"C:\tmp\rep.json"` would become path `"C"` and
/// attempt `"\tmp\rep.json"`. This test pins both shapes: a bare
/// Windows-style path (no attempt suffix) must parse with the whole
/// string as the path, and the same path with a trailing `:2` must
/// parse with attempt 2 and the drive letter still attached to the path.
#[test]
fn a_windows_style_path_survives_with_and_without_a_trailing_attempt() {
    let bare = parse_report_arg("breakage:omni:C:\\tmp\\rep.json").expect("bare path parses");
    assert_eq!(bare.path, PathBuf::from("C:\\tmp\\rep.json"));
    assert_eq!(bare.attempt, 1);

    let with_attempt =
        parse_report_arg("breakage:omni:C:\\tmp\\rep.json:2").expect("path with attempt parses");
    assert_eq!(with_attempt.path, PathBuf::from("C:\\tmp\\rep.json"));
    assert_eq!(with_attempt.attempt, 2);
}

// --- ruling round trip (Task 17 / Arc 4) -----------------------
//
// The brief's own Step 4 asks for this verified by hand against a
// real TTUI run of `/plumb:review`. This session was explicitly
// instructed not to modify anything under TTUI, and driving a real
// run there would write run artifacts into that repo. This test is
// the substitute: the same three-run shape (raise, overrule,
// re-run suppressed, edit taste.md, re-run stale) end to end
// through the real `run_merge` + `rule::run_rule` this task wired
// together, rather than a hand-built `Suppression` value.

#[test]
fn a_ruling_suppresses_on_the_next_run_and_goes_stale_when_taste_md_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let report = write_report(tmp.path(), "breakage.json", ONE_BLOCKER);
    let spec = format!("breakage:dial:{}", report.display());
    let rulings_path = tmp.path().join("rulings.jsonl");

    // Run 1: no ruling on file yet, so the blocker holds the run.
    let run1 = tmp.path().join("run1");
    std::fs::create_dir_all(&run1).unwrap();
    let (verdict, path) = run_merge(
        &run1,
        std::slice::from_ref(&spec),
        &[],
        &[],
        Some(&rulings_path),
        None,
    )
    .unwrap();
    assert_eq!(verdict, Verdict::NoGo, "the fresh finding is a NO-GO");
    let verdict_text = std::fs::read_to_string(&path).unwrap();
    assert!(
        verdict_text.contains("the border does not close"),
        "{verdict_text}"
    );

    // Overrule it: no --taste given, so the ruling's taste_hash is
    // the stable no-profile sentinel.
    let fp = parallax_plumb::merge::fingerprint("dial", "upper-right", "the border does not close");
    crate::cli::rule::run_rule(
        &run1,
        &fp,
        "the gap is this scenario's whole point",
        "scenario",
        None,
        &rulings_path,
    )
    .unwrap();

    // Run 2: same rulings.jsonl, same (absent) taste.md — the
    // ruling is fresh and suppresses the finding. GO, and the
    // finding is named in the collapsed accounting line, not
    // silently vanished.
    let run2 = tmp.path().join("run2");
    std::fs::create_dir_all(&run2).unwrap();
    let (verdict, path) = run_merge(
        &run2,
        std::slice::from_ref(&spec),
        &[],
        &[],
        Some(&rulings_path),
        None,
    )
    .unwrap();
    assert_eq!(
        verdict,
        Verdict::Go,
        "a fresh ruling must suppress the matching finding"
    );
    let verdict_text = std::fs::read_to_string(&path).unwrap();
    assert!(
        verdict_text.contains("previously overruled (1)"),
        "{verdict_text}"
    );
    assert!(
        !verdict_text.contains("the border does not close"),
        "a suppressed finding must not still appear as a live finding: {verdict_text}"
    );

    // taste.md moves. Run 3: the same rulings.jsonl is now stale
    // against the new hash — the strict reading — so the finding
    // returns and the ruling is surfaced for re-validation instead
    // of applying forever.
    let taste_path = tmp.path().join("taste.md");
    std::fs::write(&taste_path, "Prefer dense, busy layouts now.").unwrap();
    let run3 = tmp.path().join("run3");
    std::fs::create_dir_all(&run3).unwrap();
    let (verdict, path) = run_merge(
        &run3,
        &[spec],
        &[],
        &[],
        Some(&rulings_path),
        Some(&taste_path),
    )
    .unwrap();
    assert_eq!(
        verdict,
        Verdict::NoGo,
        "a stale ruling must not suppress — the finding reappears"
    );
    let verdict_text = std::fs::read_to_string(&path).unwrap();
    assert!(
        verdict_text.contains("the border does not close"),
        "{verdict_text}"
    );
    assert!(
        verdict_text.contains(&format!("stale ruling(s) needing re-validation: {fp}")),
        "{verdict_text}"
    );
}
