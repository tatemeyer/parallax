//! Small display-formatting helpers shared by every screen. Kept
//! crate-private and separate from any one screen so `overview`,
//! `detail`, and `bell` render the three-state distinction (not
//! declared / fetch failed / fetched) identically rather than each
//! re-deriving its own glyphs.

use parallax_baseline::state::ProjectState;
use std::time::Duration;

/// Not declared: the field is absent and nothing in `degradations`
/// explains why.
pub(crate) const DASH: &str = "\u{2014}"; // —
/// Fetch failed: declared, but this cycle could not read it.
pub(crate) const ALERT: &str = "!";

/// Whether a `Degradation` belonging to `family_prefix` (Baseline names
/// sources `"<family>:<detail>"`, e.g. `work:github`) exists for this
/// project -- what distinguishes "never declared" from "declared but
/// this cycle's fetch failed" for a family whose field is absent.
pub(crate) fn family_degraded(state: &ProjectState, family_prefix: &str) -> bool {
    state
        .degradations
        .iter()
        .any(|d| d.source.starts_with(family_prefix))
}

/// Strips ANSI escape sequences (CSI colour/cursor codes, OSC sequences,
/// and lone escape+byte pairs) and replaces raw tab/CR/LF with a single
/// space, so externally-captured text (verification detail, work item
/// titles, artifact paths, session names, degradation/unavailable
/// reasons) can never write control bytes into a `Cell.symbol`, drive
/// the host terminal, or throw off column-width accounting. Every field
/// carrying data this crate did not itself produce must pass through
/// this before it reaches a `Buffer` -- see `detail.rs`'s section
/// builders for the call sites. Deliberately does not interpret the
/// escapes into colour (ttui#175, and observed data must not steer the
/// display either way): they are simply dropped.
pub(crate) fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.peek() {
                Some('[') => {
                    // CSI: ESC '[' parameter/intermediate bytes, then one
                    // final byte in 0x40..=0x7E (e.g. 'm' for colour).
                    chars.next();
                    for c2 in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c2) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: ESC ']' ... terminated by BEL or ESC '\'.
                    chars.next();
                    loop {
                        match chars.next() {
                            None | Some('\u{7}') => break,
                            Some('\x1b') => {
                                if chars.peek() == Some(&'\\') {
                                    chars.next();
                                }
                                break;
                            }
                            Some(_) => {}
                        }
                    }
                }
                Some(_) => {
                    // A lone escape plus one following byte (e.g. reset
                    // codes outside CSI/OSC) -- drop both.
                    chars.next();
                }
                None => {}
            },
            '\t' | '\r' | '\n' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// A compact age string: seconds/minutes/hours/days, whichever is the
/// coarsest unit that keeps the number small.
pub(crate) fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_picks_the_coarsest_unit_that_keeps_the_number_small() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m");
        assert_eq!(format_duration(Duration::from_secs(7200)), "2h");
        assert_eq!(format_duration(Duration::from_secs(172_800)), "2d");
    }

    #[test]
    fn sanitize_strips_ansi_colour_codes_and_keeps_the_message() {
        let raw = "\x1b[33m====== no tests ran \x1b[0min 0.01s\x1b[0m";
        let clean = sanitize(raw);
        assert!(!clean.contains('\x1b'), "{clean:?}");
        assert!(!clean.contains("[33m"), "{clean:?}");
        assert!(!clean.contains("[0m"), "{clean:?}");
        assert_eq!(clean, "====== no tests ran in 0.01s");
    }

    #[test]
    fn sanitize_leaves_plain_text_untouched() {
        assert_eq!(sanitize("2 errors"), "2 errors");
        assert_eq!(sanitize(""), "");
    }

    #[test]
    fn sanitize_strips_osc_sequences() {
        // OSC 8 hyperlink wrapper, BEL-terminated.
        let raw = "\x1b]8;;https://example.com\x07link\x1b]8;;\x07 text";
        assert_eq!(sanitize(raw), "link text");
    }

    #[test]
    fn sanitize_replaces_tab_cr_lf_with_a_space_not_a_deletion() {
        assert_eq!(sanitize("a\tb\rc\nd"), "a b c d");
    }

    #[test]
    fn sanitize_drops_other_control_bytes() {
        assert_eq!(sanitize("a\u{7}b\u{8}c"), "abc");
    }

    #[test]
    fn family_degraded_matches_on_the_source_prefix() {
        use parallax_baseline::state::Degradation;
        let mut p = ProjectState {
            name: "x".into(),
            ..Default::default()
        };
        p.degradations.push(Degradation {
            source: "work:github".into(),
            reason: "boom".into(),
        });
        assert!(family_degraded(&p, "work"));
        assert!(!family_degraded(&p, "verification"));
    }
}
