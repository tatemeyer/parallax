//! Spawns an arbitrary command under a real OS pseudo-console
//! (`portable-pty`, ConPTY on Windows) and captures frames of its
//! terminal output as rasterized images, either as a single snapshot
//! (`Session::capture_frame`) or driven through a scripted sequence of
//! key presses and waits (`run_script`).
//!
//! Ported from `tools/visual-snapshot/src/pty.rs`. The one intended
//! change: the source resolved its spawn target by shelling out to
//! `cargo build --example <name>` against a hardcoded TTUI manifest
//! path (`build_example`/`root_manifest_path`, plus the `[[bin]]`-target
//! counterparts `build_bin`/`bin_dir`) — that build-and-locate machinery
//! does not come across. `run_script` and `Session::spawn` take a
//! caller-supplied argv and spawn it directly, so any project can declare
//! a command with no TTUI-specific build step at all. `examples_dir` is
//! the one piece of that removed block that *does* still port: this
//! crate's own `echo_key` test fixture is a `[[example]]` target the same
//! way TTUI's fixtures are, so its integration tests need the same
//! "where did `cargo build --example` put it" lookup the source's own
//! tests used (`tools/visual-snapshot/tests/pty_roundtrip.rs`) — that
//! lookup just doesn't get threaded into `run_script`/`Session::spawn`
//! themselves anymore. Everything else — PTY setup, the DSR handshake,
//! the vt100 parsing, the frame-capture loop, and both quiescence
//! strategies — is a copy.
//!
//! **Deliberately exceeds the project's 500-line soft ceiling.** This
//! file is a faithful port of a 744-line source, and that fidelity —
//! verified hunk-by-hunk against `tools/visual-snapshot/src/pty.rs` — is
//! exactly what makes it trustworthy: splitting proven, review-verified
//! code into pieces would turn a verifiable copy into a rewrite, in
//! exchange for satisfying a soft ceiling on a file nobody is expected to
//! edit freehand. This exception is tied to the file's provenance as a
//! port, not a blanket waiver — if this module is ever substantially
//! rewritten rather than ported, the ceiling applies again.

use crate::glyph::GlyphMode;
use crate::keys;
use crate::render::{self, RenderError};
use crate::script::Step;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Errors from spawning or driving a PTY-attached child process.
///
/// Deviation from `tools/visual-snapshot`'s `pty.rs`: the source wraps
/// a bare `std::io::Error` via a blanket `From<std::io::Error>` impl.
/// Plumb's settled error convention removed blanket `From<io::Error>`
/// impls project-wide, so the `Io` variant here is constructed
/// explicitly (`.map_err(PtyError::Io)`) at every call site instead.
/// Unlike `script::ScriptError`/`encode::EncodeError`'s `IoFailure`
/// wrapper, `Io` stays a bare `std::io::Error` with no path field: every
/// site that produces one here (writing to a child's stdin, killing a
/// child) has no file path to name, so there is nothing for a wrapper
/// struct to add — `adapter::CaptureError::Spawn(std::io::Error)`, in
/// this same module's parent, is the existing precedent for a
/// path-free `Io`-style variant.
#[derive(Debug)]
pub enum PtyError {
    /// An I/O failure writing to or killing the spawned child.
    Io(std::io::Error),
    /// A `portable-pty`-reported failure (opening the pty, spawning the
    /// child) — stringified since the underlying error type is
    /// `anyhow::Error`. Also covers the one failure mode the arbitrary-
    /// command generalization adds that the source's `&Path`-typed
    /// `binary` parameter made structurally impossible: an empty argv.
    Pty(String),
    /// A failure while rasterizing the captured screen.
    Render(RenderError),
}

impl From<RenderError> for PtyError {
    fn from(e: RenderError) -> Self {
        PtyError::Render(e)
    }
}

impl std::fmt::Display for PtyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for PtyError {}

/// Resolves the directory `cargo build --example` places compiled
/// example binaries into: `$CARGO_TARGET_DIR/debug/examples` if that
/// override is set (as it would be in an environment overriding Cargo's
/// default `target/` layout), otherwise the workspace-relative
/// `target/debug/examples` this crate assumes by default. Ported
/// unchanged from the source, which shares this between `build_example`
/// and its own integration tests; `build_example` itself didn't come
/// across (see this module's header), but the lookup is still exactly
/// what this crate's own tests need to locate the `echo_key` fixture
/// `cargo test` builds as a `[[example]]` target — see
/// `tools/visual-snapshot/tests/pty_roundtrip.rs`'s `echo_key_binary`
/// helper, which this crate's `tests/pty_roundtrip.rs` mirrors.
pub fn examples_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        let mut path = PathBuf::from(dir);
        path.push("debug/examples");
        return path;
    }
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../target/debug/examples");
    path
}

/// Interval between polls of the shared output buffer while
/// `capture_frame` waits for the child's current draw to quiesce.
pub const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Upper bound on how long `capture_frame` will wait for output before
/// giving up and snapshotting whatever's there — a safety valve so a
/// session where the child has genuinely stopped producing output
/// (crashed, hung, or just idle waiting on input) can't hang
/// `capture_frame` forever. Superseded the old fixed `SETTLE_DELAY`
/// (100ms, sleep-once-then-snapshot-regardless) after that proved
/// flaky against real TUI examples: `SETTLE_DELAY` assumed a child's
/// first draw always lands within 100ms of being asked for, but a real
/// app's actual startup-to-first-draw latency varies with OS
/// scheduling and process-spawn overhead far more than a trivial test
/// fixture's does. Not derived from a formal measurement — calibrated
/// against this tool's own dev environment, where a real example's
/// first draw was directly measured taking up to ~1.9s even with no
/// competing load (see the Task 12 flakiness fix report). Should be
/// retuned if it proves insufficient — or excessive — in practice.
pub const MAX_SETTLE_WAIT: Duration = Duration::from_millis(2000);

/// Scans `chunk` (a single `read()` call's worth of bytes) for the
/// 4-byte Device Status Report cursor-position query `ESC[6n`, correctly
/// detecting it even when the OS delivers it split across two or more
/// separate `read()` calls. `carry` holds up to 3 trailing bytes left
/// over from the previous call so a query can be reassembled across
/// that boundary; callers must reuse the same `carry` across every call
/// for a given stream. Returns `true` if the query is present in
/// `carry` followed by `chunk` (i.e. spanning the join point or fully
/// contained in `chunk`).
fn dsr_query_seen(carry: &mut Vec<u8>, chunk: &[u8]) -> bool {
    carry.extend_from_slice(chunk);
    let found = carry.windows(4).any(|w| w == b"\x1b[6n");
    // Keep only the last 3 bytes (one short of the query's 4-byte
    // length) — enough to bridge into whatever arrives next, without
    // letting `carry` grow unbounded across a long-lived session.
    let keep = carry.len().min(3);
    let drop = carry.len() - keep;
    carry.drain(..drop);
    found
}

/// An active PTY-attached child process plus a background thread
/// continuously draining its output into a shared buffer.
pub struct Session {
    parser: vt100::Parser,
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    output: Arc<Mutex<Vec<u8>>>,
    // Whether `capture_frame` has completed at least once for this
    // session yet — see `capture_frame`'s doc comment for why the very
    // first call uses a deliberately different (more patient)
    // quiescence strategy than every call after it.
    first_capture_done: bool,
    // Never read directly — held purely so the pseudo-console (and the
    // reader/writer handles derived from it) stays open for as long as
    // `Session` does. Dropping it early would tear down the pty out
    // from under the background reader thread and `writer`/`parser`.
    #[allow(dead_code)]
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    // How `capture_frame`/`capture_frame_after_key` react to an
    // unmapped codepoint — `set_glyph_mode` is the only way to change
    // this away from the default `GlyphMode::Error`, so no existing
    // caller of `Session::spawn`/`capture_frame` (this module's own
    // tests, `tests/pty_roundtrip.rs`) needs to change: their behavior
    // is identical to before this field existed.
    glyph_mode: GlyphMode,
    // Every unmapped-codepoint substitution recorded across every
    // `capture_frame`/`capture_frame_after_key` call made on this
    // session so far, folded by `run_script` into `CaptureFrames::
    // substitutions`. Always empty under the default `GlyphMode::Error`.
    substitutions: std::collections::HashMap<char, usize>,
}

impl Session {
    /// Spawns `command` (argv[0] is the program, the rest are its
    /// arguments) under a new pseudo-console of the given size and
    /// starts a background reader thread.
    ///
    /// Generalization from the source's `spawn`/`spawn_with_args` pair:
    /// those took a `&Path` binary plus a separate `args: &[&str]`
    /// slice, resolved via `build_example`/`build_bin`'s TTUI-specific
    /// `cargo build` machinery. That machinery is gone; `command` is
    /// spawned exactly as given, so a single `spawn` replaces both.
    pub fn spawn(command: &[String], rows: u16, cols: u16) -> Result<Session, PtyError> {
        let (program, args) = command.split_first().ok_or_else(|| {
            PtyError::Pty(
                "run_script command must have at least one element (the program to run)"
                    .to_string(),
            )
        })?;

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Pty(e.to_string()))?;

        let mut cmd = CommandBuilder::new(program);
        for a in args {
            cmd.arg(a);
        }
        // Without this the child starts wherever the pty crate decides,
        // so a scenario's relative paths resolve against a directory
        // nobody chose. Every other path in a `.plumb/config.yaml` is
        // relative to the project root, and the operator ran `plumb`
        // from it — the captured program should see the same tree.
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::Pty(e.to_string()))?;
        // Drop the slave handle once the child is spawned so the master
        // side can observe EOF when the child exits.
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Pty(e.to_string()))?;
        let writer = Arc::new(Mutex::new(writer));
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Pty(e.to_string()))?;

        let output = Arc::new(Mutex::new(Vec::new()));
        let output_writer = Arc::clone(&output);
        let dsr_writer = Arc::clone(&writer);
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            // Bridges `ESC[6n` detection across `read()` call boundaries —
            // see `dsr_query_seen`.
            let mut dsr_carry: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        // Windows' ConPTY (conhost) issues a Device Status
                        // Report cursor-position query (`ESC[6n`) to the
                        // outer terminal shortly after attaching, as part
                        // of its own startup handshake — this is separate
                        // from anything the spawned binary itself writes.
                        // (Assumption: a child's *own* legitimate `ESC[6n`
                        // emission, if it ever made one, is answered by
                        // conhost internally and never leaks out onto this
                        // stream — conhost owns the real console buffer
                        // the child is drawing into, so it can satisfy
                        // that query itself without forwarding it. Only
                        // conhost's own handshake query, sent before it
                        // has a downstream terminal to ask, is observed
                        // here in practice.) Until something answers with
                        // a Cursor Position Report (`ESC[row;colR`) on the
                        // input side, conhost stalls: it neither forwards
                        // the child's real output nor delivers further
                        // input to the child. Confirmed empirically while
                        // building `run_script` — without this reply, not
                        // even a single raw byte written via `send` ever
                        // reached a spawned child's stdin, regardless of
                        // encoding or wait time. Answering with an
                        // arbitrary position (`1;1`) is sufficient;
                        // nothing in this tool's rendering path depends on
                        // cursor state being accurate. Unix PTYs have no
                        // such handshake, so this is a no-op there — the
                        // query byte sequence never appears in that path.
                        if dsr_query_seen(&mut dsr_carry, &buf[..n]) {
                            let mut w = dsr_writer.lock().unwrap();
                            let _ = w.write_all(b"\x1b[1;1R");
                            let _ = w.flush();
                        }
                        output_writer.lock().unwrap().extend_from_slice(&buf[..n])
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Session {
            parser: vt100::Parser::new(rows, cols, 0),
            writer,
            output,
            first_capture_done: false,
            master: pair.master,
            child,
            glyph_mode: GlyphMode::default(),
            substitutions: std::collections::HashMap::new(),
        })
    }

    /// Sets the mode used by every subsequent `capture_frame`/
    /// `capture_frame_after_key` call on this session. `run_script`
    /// calls this once, immediately after spawning, so a scenario's
    /// declared `on_unmapped_glyph` reaches every frame it captures.
    pub fn set_glyph_mode(&mut self, mode: GlyphMode) {
        self.glyph_mode = mode;
    }

    /// Every unmapped-codepoint substitution recorded across every
    /// `capture_frame`/`capture_frame_after_key` call made on this
    /// session so far, as `(codepoint, total cells substituted)` pairs —
    /// the shape `CaptureFrames::substitutions` and `manifest::Caveat::
    /// UnmappedGlyphSubstituted` both consume. Empty under
    /// `GlyphMode::Error` (which never substitutes), and for any
    /// `GlyphMode::Substitute` session that never actually hit an
    /// unmapped codepoint.
    pub fn substitutions(&self) -> Vec<(char, usize)> {
        self.substitutions.iter().map(|(&c, &n)| (c, n)).collect()
    }

    /// Folds one capture's per-codepoint substitution counts into this
    /// session's running total. Shared by `capture_frame` and
    /// `capture_frame_after_key`.
    fn record_substitutions(&mut self, counts: &std::collections::HashMap<char, usize>) {
        for (&ch, &n) in counts {
            *self.substitutions.entry(ch).or_insert(0) += n;
        }
    }

    /// Waits for the child's current draw to quiesce, then rasterizes
    /// the current screen state.
    ///
    /// A session's very first capture uses a different (more patient)
    /// quiescence strategy than every capture after it —
    /// `wait_for_first_output` vs `wait_for_further_output` — because
    /// the two calls face genuinely different, *irreconcilable*
    /// uncertainty:
    ///
    /// - On the first capture, the screen is typically still blank (or
    ///   whatever the child wrote in its startup handshake — see
    ///   `Session::spawn`'s DSR handling), and there is no way to tell,
    ///   from the byte stream alone, "this app never draws anything
    ///   without input" apart from "this app is still cold-starting and
    ///   will draw something real in a moment". Both scenarios produce
    ///   the *exact same observable byte sequence* — silence — for as
    ///   long as the silence lasts; no quiescence heuristic can
    ///   distinguish "silent forever" from "silent for another 400ms"
    ///   without either waiting it out or guessing wrong. Given that,
    ///   the first capture stays patient up to the full
    ///   `MAX_SETTLE_WAIT`, because guessing wrong here means silently
    ///   returning a blank frame for a real app that *was* about to
    ///   draw. This does mean a session whose child never draws without
    ///   input pays the full `MAX_SETTLE_WAIT` on its first capture;
    ///   that cost is deliberate, not an oversight.
    /// - Every capture after the first is on an already-running,
    ///   already-warmed-up process — cold-start variance no longer
    ///   applies. Here, a screen that isn't currently changing is far
    ///   more likely to be genuinely done (an idle wait step, a key
    ///   with no visible effect, a static screen captured twice) than
    ///   mid-cold-start, so this path returns fast once two consecutive
    ///   polls agree, same as a screen that's still actively changing
    ///   returns once it stops.
    pub fn capture_frame(&mut self) -> Result<image::RgbaImage, PtyError> {
        let deadline = Instant::now() + MAX_SETTLE_WAIT;
        if self.first_capture_done {
            self.wait_for_further_output(deadline);
        } else {
            self.wait_for_first_output(deadline);
            self.first_capture_done = true;
        }
        let rendered = render::render_screen(self.parser.screen(), self.glyph_mode)?;
        self.record_substitutions(&rendered.substitutions);
        Ok(rendered.image)
    }

    /// Captures a frame using the same patient quiescence contract as a
    /// session's very first capture (`wait_for_first_output`) — requires
    /// observing at least one real screen-content change before declaring
    /// the draw complete — rather than `wait_for_further_output`'s fast
    /// consecutive-poll-match path.
    ///
    /// Used specifically for the capture immediately following a `Key`
    /// or `Click` step in `run_script`. `wait_for_further_output`'s fast
    /// path starts comparing polls with no baseline from before the key
    /// or click was sent, so it can declare "quiescent" after just two
    /// poll intervals purely because the screen hasn't changed *yet* —
    /// not because the app has finished reacting to the input. A `Wait`
    /// step and a session's true first capture don't have this problem
    /// (nothing was just sent that the screen is expected to react to),
    /// so they keep using `capture_frame`'s existing behavior; this
    /// method exists only for the "just sent a key or click, must
    /// observe the reaction" case.
    pub fn capture_frame_after_key(&mut self) -> Result<image::RgbaImage, PtyError> {
        let deadline = Instant::now() + MAX_SETTLE_WAIT;
        self.wait_for_first_output(deadline);
        self.first_capture_done = true;
        let rendered = render::render_screen(self.parser.screen(), self.glyph_mode)?;
        self.record_substitutions(&rendered.substitutions);
        Ok(rendered.image)
    }

    /// Quiescence strategy for every capture after a session's first —
    /// see `capture_frame`'s doc comment for why this differs from
    /// `wait_for_first_output`.
    ///
    /// Polls every `POLL_INTERVAL`, feeding any newly arrived bytes
    /// into the parser and comparing the parser's rendered *screen
    /// contents* (`vt100::Screen::contents`, plain text only — see the
    /// color/attribute caveat below) between two consecutive polls
    /// *taken during this call*. Once two consecutive polls agree, the
    /// draw is treated as complete. If polls never agree within
    /// `MAX_SETTLE_WAIT` (a draw that's still actively changing right
    /// up to the deadline), it gives up and returns whatever's there as
    /// a bounded fallback.
    ///
    /// Comparing rendered text instead of raw bytes matters: a real
    /// terminal app's startup emits several escape sequences (input-mode
    /// negotiation, focus-event enabling, window title, cursor-visibility
    /// toggles, on top of the DSR handshake `Session::spawn` already
    /// answers) that are genuine output but never change what's visibly
    /// on screen. A byte-count or raw-buffer-length signal treats that
    /// startup burst as "activity", then sees silence while the app does
    /// its actual first draw, and quiesces on exactly the wrong moment.
    /// Comparing rendered contents means only changes that
    /// `render_screen` would actually show ever count as "still
    /// drawing", so non-visual setup noise can't fool quiescence no
    /// matter what escape sequences a given app's terminal library
    /// happens to emit.
    ///
    /// Caveat: this comparison is plain text only, so a redraw that
    /// changes *only* color/attributes — e.g. a color transition or
    /// cursor blink with no text change — won't register as "changed"
    /// and would hit the full `MAX_SETTLE_WAIT` rather than being
    /// detected as quiescent early. The final rasterized frame is still
    /// correct either way, since `render_screen` reads the full
    /// color/attribute state regardless of what quiescence-detection
    /// compared — this only affects how quickly a *purely stylistic*
    /// redraw is recognized as finished, not what gets rendered.
    fn wait_for_further_output(&mut self, deadline: Instant) {
        let mut previous_poll_contents: Option<String> = None;
        loop {
            thread::sleep(POLL_INTERVAL);
            self.drain_pending_into_parser();
            let current_contents = self.parser.screen().contents();
            if previous_poll_contents.as_ref() == Some(&current_contents) {
                // This poll matches the immediately preceding one
                // (taken during this same call): the screen has held
                // steady for a full poll interval, so the current draw
                // is very likely finished.
                break;
            }
            previous_poll_contents = Some(current_contents);
            if Instant::now() >= deadline {
                break;
            }
        }
    }

    /// Quiescence strategy for a session's very first capture, and for
    /// every capture immediately following a `Key` step
    /// (`capture_frame_after_key`) — see `capture_frame`'s doc comment for
    /// why the first capture differs from `wait_for_further_output`, and
    /// `capture_frame_after_key`'s doc comment for why a post-`Key`
    /// capture needs the same patient discipline even though it isn't
    /// literally the session's first capture.
    ///
    /// Only ever declares the draw complete after actually observing
    /// the rendered screen contents change at least once *during this
    /// call* (relative to whatever the screen looked like at call
    /// entry) and then hold steady for a full poll interval. If the
    /// screen never changes at all, it waits out the entire
    /// `MAX_SETTLE_WAIT` rather than risk mistaking "still starting up"
    /// for "done" — seeing zero difference between those two states is
    /// exactly the ambiguity this method exists to resolve safely, in
    /// exchange for potentially waiting the full bound when a child
    /// genuinely never draws without input.
    fn wait_for_first_output(&mut self, deadline: Instant) {
        let mut last_contents = self.parser.screen().contents();
        let mut changed_at_least_once = false;
        loop {
            thread::sleep(POLL_INTERVAL);
            self.drain_pending_into_parser();
            let current_contents = self.parser.screen().contents();
            if current_contents != last_contents {
                last_contents = current_contents;
                changed_at_least_once = true;
            } else if changed_at_least_once {
                // The screen changed earlier in this call and hasn't
                // changed for a full poll interval: the child's first
                // draw is very likely finished.
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
        }
    }

    /// Drains whatever's arrived in the shared output buffer since the
    /// last drain and feeds it into the parser. Shared by both
    /// `capture_frame` quiescence strategies.
    fn drain_pending_into_parser(&mut self) {
        let pending: Vec<u8> = {
            let mut buf = self.output.lock().unwrap();
            std::mem::take(&mut *buf)
        };
        if !pending.is_empty() {
            self.parser.process(&pending);
        }
    }

    /// Writes raw bytes into the pseudo-console's input handle.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), PtyError> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes).map_err(PtyError::Io)?;
        writer.flush().map_err(PtyError::Io)?;
        Ok(())
    }

    /// Terminates the child process. Safe to call even after the child
    /// has already exited on its own: verified from the 0.9.0
    /// `portable-pty` source (`src/win/mod.rs`, `WinChild::kill`) that
    /// this project's Windows ConPTY backend already collapses a failed
    /// `TerminateProcess` call to `Ok(())` internally, so there is no
    /// "already exited" failure case for this method to special-case.
    /// The underlying `Result` is still propagated (rather than
    /// blanket-ignored) so a future backend change that *can* report a
    /// genuine termination failure isn't silently discarded here.
    pub fn kill(&mut self) -> Result<(), PtyError> {
        self.child.kill().map_err(PtyError::Io)?;
        Ok(())
    }

    /// Polls `self.child.try_wait()` until it reports the process
    /// gone or `timeout` elapses. Returns `Some(status)` if the child
    /// exited within `timeout`, `None` if the timeout elapsed first.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> Option<portable_pty::ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

const KEY_STEP_DISPLAY_DURATION: Duration = Duration::from_millis(150);

/// One `run_script` invocation's captured frames, and the unmapped-glyph
/// substitutions recorded while rendering them.
#[derive(Debug)]
pub struct CaptureFrames {
    /// One rendered frame per script step, plus an initial frame
    /// captured before any step runs — each paired with the wall-clock
    /// duration it should be displayed for.
    pub frames: Vec<(image::RgbaImage, Duration)>,
    /// `(codepoint, occurrence count)` pairs recorded when
    /// `GlyphMode::Substitute` stood in for an unmapped glyph rather
    /// than erroring, aggregated across every frame this run captured.
    /// Always empty under `GlyphMode::Error` (which never substitutes —
    /// an unmapped codepoint fails the whole capture via `?` instead).
    /// The caller (the future `adapter::capture` pty wiring, Task 21)
    /// folds each entry into a `manifest::Caveat::
    /// UnmappedGlyphSubstituted { codepoint, count }`.
    pub substitutions: Vec<(char, usize)>,
}

/// Spawns `command` (argv[0] is the program, the rest are its
/// arguments), drives it through `steps` (real key bytes / real
/// wall-clock waits / real click byte sequences), and returns one
/// rendered frame per step plus an initial frame captured before any
/// step runs.
///
/// `glyph_mode` governs every frame this call captures: `GlyphMode::
/// Error` (the default) hard-errors via `?` on the first unmapped
/// codepoint, same as before this parameter existed; `GlyphMode::
/// Substitute` draws a placeholder in its place instead and returns the
/// per-codepoint counts in `CaptureFrames::substitutions`.
pub fn run_script(
    command: &[String],
    rows: u16,
    cols: u16,
    steps: &[Step],
    glyph_mode: GlyphMode,
) -> Result<CaptureFrames, PtyError> {
    let mut session = Session::spawn(command, rows, cols)?;
    session.set_glyph_mode(glyph_mode);
    let mut frames = Vec::with_capacity(steps.len() + 1);

    frames.push((session.capture_frame()?, Duration::from_millis(0)));

    for step in steps {
        match step {
            Step::Wait { wait_ms } => {
                std::thread::sleep(Duration::from_millis(*wait_ms));
                frames.push((session.capture_frame()?, Duration::from_millis(*wait_ms)));
            }
            Step::Key { key } => {
                let bytes = keys::encode_key(key).map_err(|keys::KeyEncodeError::Unknown(k)| {
                    PtyError::Pty(format!("unknown key name: {k}"))
                })?;
                session.send(&bytes)?;
                // Must observe the child's actual reaction, not just two
                // stable polls — see `capture_frame_after_key`'s doc
                // comment.
                frames.push((
                    session.capture_frame_after_key()?,
                    KEY_STEP_DISPLAY_DURATION,
                ));
            }
            Step::Click { x, y } => {
                session.send(&keys::encode_click(*x, *y))?;
                // Same "wait for the child's actual reaction" quiescence
                // strategy Key steps already use — a click should also
                // produce an observable reaction.
                frames.push((
                    session.capture_frame_after_key()?,
                    KEY_STEP_DISPLAY_DURATION,
                ));
            }
        }
    }

    session.kill()?;
    let substitutions = session.substitutions();
    Ok(CaptureFrames {
        frames,
        substitutions,
    })
}

impl Drop for Session {
    /// Ensures the child process doesn't outlive its `Session` even if
    /// the caller never calls `kill()` explicitly (as in ordinary
    /// scope-exit cleanup). Delegates to `kill()`, which is documented
    /// safe to call on an already-exited child; the `Result` can't be
    /// propagated out of `drop`, so it's discarded here specifically
    /// (not the blanket-ignore pattern flagged for the old `kill()`
    /// body — this is the one place ignoring it is unavoidable).
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_key_binary() -> PathBuf {
        let mut path = examples_dir();
        path.push("echo_key");
        if cfg!(windows) {
            path.set_extension("exe");
        }
        path
    }

    fn echo_key_command() -> Vec<String> {
        vec![echo_key_binary().to_string_lossy().into_owned()]
    }

    fn echo_unmapped_glyph_binary() -> PathBuf {
        let mut path = examples_dir();
        path.push("echo_unmapped_glyph");
        if cfg!(windows) {
            path.set_extension("exe");
        }
        path
    }

    fn echo_unmapped_glyph_command() -> Vec<String> {
        vec![echo_unmapped_glyph_binary().to_string_lossy().into_owned()]
    }

    /// Guards finding #1 from the source's Task 8 review: cleanup must
    /// be a real, verified effect, not an assumed side effect of
    /// `master`'s Drop impl. Spawns a real child, confirms it's alive,
    /// kills it, then polls `Child::try_wait` (the crate's own liveness
    /// check) until it reports the process gone, bounded so a
    /// regression fails fast instead of hanging.
    #[test]
    fn kill_terminates_a_still_running_child_within_a_bounded_time() {
        let mut session = Session::spawn(&echo_key_command(), 5, 40).unwrap();
        // The fixture blocks on event::read() until killed or sent
        // input, so it should still be alive immediately after spawn.
        assert!(
            session.child.try_wait().unwrap().is_none(),
            "expected the fixture to still be running before kill()"
        );

        session.kill().unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if session.child.try_wait().unwrap().is_some() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "child was not reported terminated within 5s of kill()"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Guards a Task 9 review finding: the original detection scanned
    /// only within a single `read()` call's bytes, so a `\x1b[6n` query
    /// split across two `read()`s (e.g. the OS delivering `\x1b[6` in
    /// one call and `n` in the next) would never be reassembled and go
    /// permanently undetected — silently hanging the session, since
    /// conhost stalls until the query is answered. This drives
    /// `dsr_query_seen` directly with two separate calls, bypassing the
    /// real PTY (whose actual `read()` chunking isn't something a test
    /// can force), to prove the carry-over reassembly works.
    #[test]
    fn dsr_query_split_across_two_reads_is_still_detected() {
        let mut carry = Vec::new();
        assert!(
            !dsr_query_seen(&mut carry, b"\x1b[6"),
            "a partial query alone must not be reported as seen"
        );
        assert!(
            dsr_query_seen(&mut carry, b"n"),
            "the query must be detected once the remaining byte arrives"
        );
    }

    #[test]
    fn dsr_query_split_one_byte_at_a_time_is_still_detected() {
        let mut carry = Vec::new();
        assert!(!dsr_query_seen(&mut carry, b"\x1b"));
        assert!(!dsr_query_seen(&mut carry, b"["));
        assert!(!dsr_query_seen(&mut carry, b"6"));
        assert!(dsr_query_seen(&mut carry, b"n"));
    }

    #[test]
    fn dsr_query_within_a_single_read_is_detected() {
        let mut carry = Vec::new();
        assert!(dsr_query_seen(&mut carry, b"\x1b[6n"));
    }

    #[test]
    fn unrelated_bytes_do_not_false_positive_as_a_dsr_query() {
        let mut carry = Vec::new();
        assert!(!dsr_query_seen(&mut carry, b"hello world"));
        assert!(!dsr_query_seen(&mut carry, b"more unrelated output"));
    }

    /// New test (not in the source): the source's `binary: &Path`
    /// parameter made an empty command structurally impossible, so
    /// there was nothing to guard here. The arbitrary-command
    /// generalization introduces `&[String]`, which *can* be empty —
    /// this proves that case fails with a named `PtyError::Pty` rather
    /// than panicking on `command[0]`.
    #[test]
    fn spawning_an_empty_command_is_a_named_error_not_a_panic() {
        // `Session` intentionally isn't `Debug` (it holds live process/pty
        // handles), so `unwrap_err()` isn't available here — match instead.
        let err = match Session::spawn(&[], 5, 40) {
            Ok(_) => panic!("expected an error for an empty command"),
            Err(e) => e,
        };
        assert!(matches!(err, PtyError::Pty(_)));
    }

    /// Real pixel data through a real spawned child, not a mocked mode
    /// flag: `echo_unmapped_glyph` writes a real unmapped codepoint,
    /// `run_script` in `GlyphMode::Substitute` must render a visible
    /// placeholder (checked as an actual non-background pixel, not just
    /// that capture succeeded) and report the substitution count.
    #[test]
    fn run_script_in_substitute_mode_renders_a_real_placeholder_and_records_the_count() {
        let out = run_script(
            &echo_unmapped_glyph_command(),
            5,
            40,
            &[],
            GlyphMode::Substitute,
        )
        .unwrap();

        assert_eq!(out.frames.len(), 1, "no steps: just the initial frame");
        let img = &out.frames[0].0;
        let top_left = img.get_pixel(0, 0);
        assert_ne!(
            *top_left,
            image::Rgba([0, 0, 0, 255]),
            "expected the placeholder box to actually draw at (0, 0)"
        );
        assert_eq!(out.substitutions, vec![('\u{2726}', 1)]);
    }

    /// Companion to the above: the same real unmapped glyph, but under
    /// `GlyphMode::Error` (the default), must still hard-error rather
    /// than silently substitute — proves `run_script` didn't quietly
    /// start substituting unconditionally.
    #[test]
    fn run_script_in_error_mode_still_hard_errors_on_the_same_real_unmapped_glyph() {
        let err =
            run_script(&echo_unmapped_glyph_command(), 5, 40, &[], GlyphMode::Error).unwrap_err();
        assert!(
            matches!(
                err,
                PtyError::Render(RenderError::Glyph(
                    crate::glyph::GlyphError::Unmapped('\u{2726}'),
                    0,
                    0
                ))
            ),
            "expected a named Glyph render error at (0, 0), got {err:?}"
        );
    }

    #[test]
    fn wait_for_exit_returns_none_when_the_timeout_elapses_first() {
        // echo_key blocks on event::read() until killed or sent input,
        // so it never exits on its own — a short timeout should elapse.
        let mut session = Session::spawn(&echo_key_command(), 5, 40).unwrap();
        let status = session.wait_for_exit(Duration::from_millis(200));
        assert!(
            status.is_none(),
            "expected the timeout to elapse since echo_key never exits on its own"
        );
    }
}
