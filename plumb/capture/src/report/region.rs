//! Resolves a finding's free-text `region` claim to a contact-sheet
//! frame index, conservatively: only when the text unambiguously names
//! exactly one frame. Three phrasings are accepted — an explicit frame
//! number (`frame 3`, `frame #3`), a row-word-plus-ordinal phrase
//! (`top row, third frame`, tolerating a parenthetical remark between
//! the ordinal and `frame`), and an ordinal frame reference paired with
//! a compass position qualifier instead of a row word (`first frame,
//! top-left`, `first frame of the contact sheet (top-left panel)`).
//! Everything else — a numeric range, a reference to two distinct
//! frames, an ordinal with no row word and no position qualifier, or a
//! qualifier that disagrees with the ordinal's own linear position —
//! returns `None` so the report falls back to showing the full sheet
//! rather than risk cropping the wrong pixels beside a confident
//! claim. A wrong crop costs a reader's trust in every crop after it;
//! no crop only costs a moment of scanning.

const ROW_WORDS: [(&str, u32); 3] = [("top", 0), ("middle", 1), ("bottom", 2)];

const ORDINAL_WORDS: [(&str, u32); 9] = [
    ("first", 0),
    ("second", 1),
    ("third", 2),
    ("fourth", 3),
    ("fifth", 4),
    ("sixth", 5),
    ("seventh", 6),
    ("eighth", 7),
    ("ninth", 8),
];

/// Row synonyms usable in a compass-style position qualifier
/// (`top-left`, `upper right`) beside an ordinal frame reference. Kept
/// separate from [`ROW_WORDS`] because this phrasing pairs directly
/// with a column word (`left`/`right`), never with the word `row`, and
/// additionally accepts `upper`/`lower` as synonyms for `top`/`bottom`.
const POSITION_ROW_WORDS: [(&str, u32); 5] = [
    ("top", 0),
    ("upper", 0),
    ("middle", 1),
    ("bottom", 2),
    ("lower", 2),
];

/// Resolves `region`'s free text to a 0-based frame index within a
/// `frame_count`-frame contact sheet laid out `cols` columns wide, or
/// `None` when the text does not unambiguously name exactly one frame.
/// Matching is case-insensitive.
pub fn resolve_frame(region: &str, frame_count: usize, cols: u32) -> Option<usize> {
    let text = region.to_lowercase();
    let numbers = explicit_frame_numbers(&text);

    if numbers.len() == 1 {
        let n = numbers[0];
        let candidate = (n >= 1 && n - 1 < frame_count).then(|| n - 1)?;
        // The explicit number is a candidate on its own, but the same
        // text may also carry a row-and-ordinal phrase (`"frame 3, top
        // row, first frame"`). When it does and the two disagree, the
        // text is making two different claims about which frame it
        // means — resolve to neither rather than silently prefer one.
        return match resolve_row_and_ordinal(&text, frame_count, cols) {
            Some(other) if other != candidate => None,
            _ => Some(candidate),
        };
    }
    if !numbers.is_empty() {
        // Named more than one distinct frame number outright: a
        // confident half-truth either way it's resolved.
        return None;
    }

    resolve_row_and_ordinal(&text, frame_count, cols)
        .or_else(|| resolve_ordinal_with_position(&text, frame_count, cols))
}

/// Resolves an ordinal frame reference paired with a compass position
/// qualifier instead of a `row`-word phrase (`"first frame, top-left"`,
/// `"first frame of the contact sheet (top-left panel)"`): the ordinal
/// names a frame's linear index directly (`first` = index 0), and the
/// qualifier must agree with the row/column that index actually falls
/// in. No qualifier at all (`"third frame"` alone) still resolves to
/// nothing — this only widens the grammar for a reference that commits
/// to a specific position, not for a bare ordinal — and a disagreeing
/// qualifier is a confident half-truth, same as every other
/// disagreement this module already refuses to arbitrate.
fn resolve_ordinal_with_position(text: &str, frame_count: usize, cols: u32) -> Option<usize> {
    let ordinal = single_paired_value(text, &ORDINAL_WORDS, "frame")?;
    let (qual_row, qual_col) = position_qualifier(text, cols)?;
    let index = ordinal as usize;
    if index >= frame_count {
        return None;
    }
    let actual_row = ordinal / cols;
    let actual_col = ordinal % cols;
    (actual_row == qual_row && actual_col == qual_col).then_some(index)
}

/// Resolves a compass-style position qualifier (`top-left`, `bottom
/// right`) to a `(row, col)` pair, treating `left` as the first column
/// and `right` as the last column of a `cols`-wide grid. `None` when no
/// such qualifier is present, or when more than one disagreeing
/// qualifier is named.
fn position_qualifier(text: &str, cols: u32) -> Option<(u32, u32)> {
    let mut found: Option<(u32, u32)> = None;
    for (row_word, row) in POSITION_ROW_WORDS {
        for start in whole_word_positions(text, row_word) {
            let after = skip_hyphen_or_ws(text, start + row_word.len());
            let col = if matches_whole_word_at(text, after, "left") {
                Some(0)
            } else if matches_whole_word_at(text, after, "right") {
                Some(cols.saturating_sub(1))
            } else {
                None
            };
            let Some(col) = col else { continue };
            match found {
                None => found = Some((row, col)),
                Some(existing) if existing == (row, col) => {}
                Some(_) => return None,
            }
        }
    }
    found
}

/// Advances past a single optional hyphen, or any run of whitespace, at
/// `start` — the tight adjacency a compass qualifier uses (`top-left`,
/// `top left`), stricter than the ordinal/row phrasing's looser
/// "whitespace plus optional parenthetical" gap.
fn skip_hyphen_or_ws(text: &str, start: usize) -> usize {
    if let Some(rest) = text[start..].strip_prefix('-') {
        return text.len() - rest.len();
    }
    skip_ws(text, start)
}

/// Every distinct 1-based frame number named by an explicit `frame <n>`
/// / `frame #<n>` reference in `text`, deduplicated. A `frame` not
/// immediately (allowing whitespace and an optional `#`) followed by a
/// digit is not a reference at all (`entire frame`), and `frame` only
/// counts as a whole word — never as part of `frames` or `full-frame`.
fn explicit_frame_numbers(text: &str) -> Vec<usize> {
    let mut numbers = Vec::new();
    for start in whole_word_positions(text, "frame") {
        let after = start + "frame".len();
        if let Some(n) = number_immediately_after(&text[after..]) {
            if !numbers.contains(&n) {
                numbers.push(n);
            }
        }
    }
    numbers
}

/// Parses a decimal number immediately after a matched `frame` word,
/// allowing whitespace and an optional `#` in between. `None` when
/// nothing digit-shaped follows, so `entire frame` does not match.
fn number_immediately_after(rest: &str) -> Option<usize> {
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('#').unwrap_or(rest).trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// Resolves the row-word-plus-ordinal phrasing (`top row, third
/// frame`): both a row word unambiguously paired with `row`, and an
/// ordinal unambiguously paired with `frame` (a parenthetical remark
/// may sit between the ordinal and `frame`), must each resolve to
/// exactly one value, and the resulting column must fit `cols`, or
/// this returns `None`.
fn resolve_row_and_ordinal(text: &str, frame_count: usize, cols: u32) -> Option<usize> {
    let row = single_paired_value(text, &ROW_WORDS, "row")?;
    let col = single_paired_value(text, &ORDINAL_WORDS, "frame")?;
    if col >= cols {
        return None;
    }
    let index = (row * cols + col) as usize;
    (index < frame_count).then_some(index)
}

/// Finds every whole-word occurrence of a word from `candidates` that
/// is followed (after whitespace, and optionally a parenthetical
/// remark plus more whitespace) by the whole word `partner`, and
/// returns its paired value — but only when every such occurrence
/// agrees on the same value. A text naming two different rows, or two
/// different ordinals paired with `frame`, is ambiguous and resolves
/// to nothing.
fn single_paired_value(text: &str, candidates: &[(&str, u32)], partner: &str) -> Option<u32> {
    let mut found: Option<u32> = None;
    for (word, value) in candidates {
        for start in whole_word_positions(text, word) {
            let after = skip_ws_and_optional_parenthetical(text, start + word.len());
            if matches_whole_word_at(text, after, partner) {
                match found {
                    None => found = Some(*value),
                    Some(existing) if existing == *value => {}
                    Some(_) => return None,
                }
            }
        }
    }
    found
}

/// Every byte offset in `text` where `word` occurs as a whole word —
/// not immediately preceded or followed by another alphanumeric
/// character, so `frame` never matches inside `frames` or
/// `full-frame`.
fn whole_word_positions(text: &str, word: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(start, _)| start)
        .filter(|&start| matches_whole_word_at(text, start, word))
        .collect()
}

/// Whether `word` occurs at byte offset `idx` in `text` as a whole
/// word.
fn matches_whole_word_at(text: &str, idx: usize, word: &str) -> bool {
    if idx > text.len() || !text[idx..].starts_with(word) {
        return false;
    }
    let end = idx + word.len();
    let before_ok = text[..idx]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric());
    let after_ok = text[end..]
        .chars()
        .next()
        .is_none_or(|c| !c.is_alphanumeric());
    before_ok && after_ok
}

/// Advances past whitespace, then an optional `(...)` parenthetical
/// remark and any whitespace after it — the gap real findings put
/// between an ordinal and `frame` (`third (rightmost) frame`).
fn skip_ws_and_optional_parenthetical(text: &str, start: usize) -> usize {
    let mut idx = skip_ws(text, start);
    if text[idx..].starts_with('(') {
        if let Some(rel) = text[idx..].find(')') {
            idx += rel + 1;
            idx = skip_ws(text, idx);
        }
    }
    idx
}

/// Advances past leading whitespace in `text[start..]`.
fn skip_ws(text: &str, start: usize) -> usize {
    let mut idx = start;
    for ch in text[start..].chars() {
        if ch.is_whitespace() {
            idx += ch.len_utf8();
        } else {
            break;
        }
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // --- Real region strings from real lens-agent findings against a
    // real capture (8 frames, 3 cols). Independently verified: frame
    // index 2 is a solid green fill, index 6 a yellow flood — exactly
    // what these two findings claimed. ---

    #[test]
    fn real_finding_top_row_third_frame_with_parenthetical_resolves() {
        assert_eq!(
            resolve_frame(
                "top row, third (rightmost) frame of the contact sheet",
                8,
                3
            ),
            Some(2)
        );
    }

    #[test]
    fn real_finding_bottom_row_first_frame_with_parenthetical_resolves() {
        assert_eq!(
            resolve_frame(
                "bottom row, first (leftmost) frame of the contact sheet",
                8,
                3
            ),
            Some(6)
        );
    }

    #[test]
    fn real_finding_entire_frame_does_not_resolve() {
        // Contains the literal word "frame" with no digit/# after it.
        assert_eq!(
            resolve_frame(
                "entire frame, with a small icon-sized mark near center",
                8,
                3
            ),
            None
        );
    }

    #[test]
    fn real_finding_mode_label_row_does_not_resolve() {
        assert_eq!(
            resolve_frame("the mode-label row, upper-right quadrant", 8, 3),
            None
        );
    }

    #[test]
    fn real_finding_frame_range_does_not_resolve() {
        // Names a range ("frames 4-6", "frames 7-8"), not one frame.
        assert_eq!(
            resolve_frame(
                "the Omnitrix bordered box, comparing frames 4-6 (row 2) to frames 7-8 (row 3)",
                8,
                3
            ),
            None
        );
    }

    #[test]
    fn real_finding_two_distinct_frames_does_not_resolve() {
        // Names two frames (3 and 7); resolving to either is a
        // confident half-truth.
        assert_eq!(
            resolve_frame(
                "the full-frame colour fills at frame 3 (solid green) and frame 7 (solid yellow)",
                8,
                3
            ),
            None
        );
    }

    // --- Additional conservatism checks beyond the brief's examples ---

    #[test]
    fn repeating_the_same_frame_number_is_not_ambiguous() {
        assert_eq!(
            resolve_frame("frame 3 shows a fill; see frame 3 again", 8, 3),
            Some(2)
        );
    }

    #[test]
    fn an_ordinal_with_no_row_word_does_not_resolve() {
        assert_eq!(resolve_frame("third frame", 8, 3), None);
    }

    #[test]
    fn a_row_word_with_no_ordinal_does_not_resolve() {
        assert_eq!(resolve_frame("top row", 8, 3), None);
    }

    #[test]
    fn frame_as_part_of_a_longer_word_does_not_match() {
        assert_eq!(resolve_frame("the aframe widget, frameset 3", 8, 3), None);
    }

    #[test]
    fn a_column_beyond_the_grid_width_does_not_resolve() {
        // "ninth" -> col 8, but cols is only 3.
        assert_eq!(resolve_frame("top row, ninth frame", 8, 3), None);
    }

    #[test]
    fn row_and_ordinal_matching_is_case_insensitive() {
        assert_eq!(resolve_frame("TOP ROW, THIRD FRAME", 8, 3), Some(2));
    }

    #[test]
    fn disagreeing_explicit_number_and_row_ordinal_do_not_resolve() {
        // "frame 3" says index 2; "top row, first frame" says index 0.
        // Two phrasings, two different answers: a confident half-truth
        // either way it's resolved.
        assert_eq!(resolve_frame("frame 3, top row, first frame", 8, 3), None);
    }

    #[test]
    fn agreeing_explicit_number_and_row_ordinal_resolve() {
        // "frame 3" and "top row, third frame" both say index 2.
        assert_eq!(
            resolve_frame("frame 3, top row, third frame", 8, 3),
            Some(2)
        );
    }

    // --- Priority 4: widen the grammar for an ordinal frame reference
    // paired with a compass position qualifier instead of a row word.
    // Both real strings below are ones a human resolves instantly but
    // the pre-widening grammar declined (the sole load-bearing token
    // used to be the literal word `row`). ---

    #[test]
    fn real_finding_first_frame_top_left_of_the_contact_sheet_resolves() {
        assert_eq!(
            resolve_frame("first frame, top-left of the contact sheet", 8, 3),
            Some(0)
        );
    }

    #[test]
    fn real_finding_first_frame_of_the_contact_sheet_top_left_panel_resolves() {
        assert_eq!(
            resolve_frame("first frame of the contact sheet (top-left panel)", 8, 3),
            Some(0)
        );
    }

    #[test]
    fn an_ordinal_frame_with_a_disagreeing_position_qualifier_does_not_resolve() {
        // "third frame" is index 2 (top row, rightmost column in a
        // 3-wide grid); "top-left" claims column 0. Two different
        // claims about the same reference — a confident half-truth
        // either way it's resolved.
        assert_eq!(resolve_frame("third frame, top-left", 8, 3), None);
    }

    #[test]
    fn a_positional_qualifier_naming_two_different_positions_does_not_resolve() {
        assert_eq!(
            resolve_frame("first frame, top-left and also bottom-right", 8, 3),
            None
        );
    }

    #[test]
    fn upper_and_lower_are_accepted_synonyms_for_top_and_bottom() {
        // "fourth" -> index 3 -> row 1, col 0 in a 3-wide grid; not
        // top or bottom, so this only guards that the synonym list
        // itself resolves correctly for a case it does cover.
        assert_eq!(
            resolve_frame("seventh frame, lower-left of the sheet", 9, 3),
            Some(6)
        );
    }

    #[test]
    fn a_bare_ordinal_frame_with_no_position_qualifier_still_does_not_resolve() {
        // Unchanged from before this widening: an ordinal alone,
        // paired with neither a row word nor a position qualifier,
        // stays ambiguous.
        assert_eq!(resolve_frame("third frame", 8, 3), None);
    }

    #[test]
    fn a_position_qualifier_beyond_the_capture_does_not_resolve() {
        assert_eq!(
            resolve_frame("ninth frame, top-left of the sheet", 8, 3),
            None
        );
    }
}
