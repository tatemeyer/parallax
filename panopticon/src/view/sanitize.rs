//! Turning observed text into something that can only ever be *shown*.
//!
//! **A monitoring screen must never let the thing it is watching decide
//! how it renders.** Almost every string this cockpit displays was
//! captured from somewhere else — a verification command's output, a
//! pull request title, a directory name, an adapter's error — and any of
//! them can contain ANSI escape sequences. `pytest`, `cargo`, and `npm`
//! all emit colour by default, so this is the ordinary case rather than
//! an attack.
//!
//! Written into a cell grid unchanged, those bytes reach the terminal
//! and steer it: colour, cursor moves, and worse. They also break the
//! grid arithmetic, because an escape is several `char`s and zero
//! display columns, so a line is truncated against a length that has
//! nothing to do with what is visible.
//!
//! **The escapes are stripped, never interpreted.** Turning them into
//! real colours would be the same "observed data steers the display"
//! problem in a nicer suit — and `ttui` has no per-cell foreground
//! colour to turn them into.
//!
//! **This does not belong in `parallax-baseline`.** A library that
//! silently rewrote what it observed would be worse than one that
//! reports it faithfully, and a non-terminal consumer may well want the
//! escapes. "It has to survive becoming literal grid cells" is this
//! crate's constraint, so the stripping is this crate's job.

/// The `ESC` that begins every escape sequence.
const ESC: char = '\u{1b}';
/// `BEL`, one of the two ways a string-type sequence can end.
const BEL: char = '\u{7}';

/// What a control character becomes.
///
/// A space rather than nothing: a tab or a newline was separating two
/// things, and deleting it would run them together into a word that was
/// never in the output.
const REPLACEMENT: char = ' ';

/// Strips escape sequences and neutralises control characters.
///
/// Idempotent, and total — there is no input for which this returns
/// something a terminal would act on.
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != ESC {
            // Includes DEL and the C1 range, which `char::is_control`
            // covers and which a terminal acts on just as readily.
            out.push(if ch.is_control() { REPLACEMENT } else { ch });
            continue;
        }
        match chars.next() {
            // A trailing `ESC` introduced nothing. Dropping it is the
            // whole of what is needed.
            None => break,
            // CSI — the common one: colour, cursor movement, erasure.
            // Parameters and intermediates run until a final byte.
            Some('[') => {
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            // The string-argument sequences: OSC sets the window title,
            // and the others carry payloads a terminal may act on. All
            // four run until `ST` or `BEL`.
            Some(']') | Some('P') | Some('X') | Some('^') | Some('_') => {
                while let Some(next) = chars.next() {
                    if next == BEL {
                        break;
                    }
                    if next == ESC {
                        // `ESC \` is `ST`. Anything else after `ESC`
                        // inside a string is not a terminator, and is
                        // consumed like the rest of the payload.
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
            }
            // Charset designation: `ESC ( B` and friends, three bytes.
            Some('(') | Some(')') | Some('*') | Some('+') => {
                chars.next();
            }
            // Every other two-byte escape — `ESC c` resets the terminal,
            // `ESC 7` saves the cursor. Dropping both bytes is enough.
            Some(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line from the issue, as the cockpit actually rendered it.
    #[test]
    fn the_pytest_line_that_started_this_becomes_readable() {
        let raw = "\u{1b}[33m============================ \u{1b}[33mno tests ran\u{1b}[0m\u{1b}[33m in 0.01s\u{1b}[0m\u{1b}[33m ====";
        assert_eq!(
            sanitize(raw),
            "============================ no tests ran in 0.01s ===="
        );
    }

    #[test]
    fn ordinary_text_is_untouched() {
        for text in ["tests: 12 passed", "a title with — an em dash and 中文", ""] {
            assert_eq!(sanitize(text), text);
        }
    }

    #[test]
    fn colour_codes_go_and_the_words_between_them_stay() {
        assert_eq!(
            sanitize("\u{1b}[31mred\u{1b}[0m and \u{1b}[1mbold\u{1b}[m"),
            "red and bold"
        );
    }

    /// Cursor movement is the one that would let observed output write
    /// outside its own row.
    #[test]
    fn cursor_movement_is_stripped() {
        assert_eq!(sanitize("a\u{1b}[2Ab\u{1b}[1;1Hc\u{1b}[2Jd"), "abcd");
    }

    #[test]
    fn an_osc_ends_at_either_terminator() {
        assert_eq!(sanitize("a\u{1b}]0;a new window title\u{7}b"), "ab");
        assert_eq!(sanitize("a\u{1b}]0;a new window title\u{1b}\\b"), "ab");
    }

    #[test]
    fn the_other_string_sequences_are_stripped_too() {
        assert_eq!(sanitize("a\u{1b}Pq payload \u{1b}\\b"), "ab");
        assert_eq!(sanitize("a\u{1b}_application \u{7}b"), "ab");
    }

    #[test]
    fn a_two_byte_escape_takes_both_bytes() {
        // `ESC c` is a full terminal reset.
        assert_eq!(sanitize("a\u{1b}cb"), "ab");
        assert_eq!(sanitize("a\u{1b}(Bb"), "ab");
    }

    /// A tab or a newline inside a one-line field corrupts a cell grid
    /// exactly the way an escape does.
    #[test]
    fn control_characters_become_spaces_rather_than_vanishing() {
        assert_eq!(sanitize("a\tb\rc\nd"), "a b c d");
        assert_eq!(sanitize("a\u{7f}b"), "a b");
    }

    /// An escape that never ends really does run to the end of the
    /// text. That is what the sequence *says*, and inventing a
    /// terminator would put bytes back on the screen.
    #[test]
    fn an_unterminated_sequence_consumes_the_rest() {
        assert_eq!(sanitize("a\u{1b}[31"), "a");
        assert_eq!(sanitize("a\u{1b}"), "a");
    }

    #[test]
    fn sanitizing_twice_changes_nothing_the_second_time() {
        for raw in [
            "\u{1b}[33mno tests ran\u{1b}[0m",
            "a\tb",
            "\u{1b}]0;title\u{7}plain",
        ] {
            let once = sanitize(raw);
            assert_eq!(sanitize(&once), once);
        }
    }

    /// The property the whole module exists for, over every case above
    /// at once: nothing a terminal acts on survives.
    #[test]
    fn no_control_character_survives_anything() {
        let hostile = [
            "\u{1b}[31mred",
            "\u{1b}]0;title\u{7}",
            "tabs\there",
            "\u{1b}[2J\u{1b}[1;1H",
            "\u{1b}Pq\u{1b}\\",
            "\r\n\u{0}\u{8}",
            "\u{1b}",
            "\u{9b}[31m",
        ];
        for raw in hostile {
            let clean = sanitize(raw);
            assert!(
                !clean.chars().any(char::is_control),
                "{raw:?} left {clean:?}"
            );
        }
    }
}

// TEMP: deliberate fmt violation to prove the gate fails red.
pub fn   temp_gate_probe( ) -> u8 {   7   }
