//! Encodes a snapshot script's named `{"key": "..."}` steps into the raw
//! byte sequences a real terminal would send for them — arrow keys as CSI
//! sequences, `Ctrl+`-combos as control bytes, everything else as a
//! literal ASCII byte. Also encodes `{"x": N, "y": N}` click steps
//! (`encode_click`) as SGR mouse-protocol press/release sequences.

/// A script step's `"key"` name that doesn't match any known encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyEncodeError {
    /// The unrecognized key name, as written in the script.
    Unknown(String),
}

/// Encodes a named key (as used in a snapshot script's `{"key": "..."}`
/// step) into the raw byte sequence a real terminal would send for it.
pub fn encode_key(name: &str) -> Result<Vec<u8>, KeyEncodeError> {
    match name {
        "Up" => Ok(b"\x1b[A".to_vec()),
        "Down" => Ok(b"\x1b[B".to_vec()),
        "Right" => Ok(b"\x1b[C".to_vec()),
        "Left" => Ok(b"\x1b[D".to_vec()),
        "Enter" => Ok(b"\r".to_vec()),
        "Esc" => Ok(b"\x1b".to_vec()),
        "Tab" => Ok(b"\t".to_vec()),
        _ => {
            if let Some(letter) = name.strip_prefix("Ctrl+") {
                let mut chars = letter.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else {
                    return Err(KeyEncodeError::Unknown(name.to_string()));
                };
                let upper = c.to_ascii_uppercase();
                if upper.is_ascii_alphabetic() {
                    return Ok(vec![(upper as u8) - b'A' + 1]);
                }
                return Err(KeyEncodeError::Unknown(name.to_string()));
            }
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii() => Ok(vec![c as u8]),
                _ => Err(KeyEncodeError::Unknown(name.to_string())),
            }
        }
    }
}

/// Encodes a left-button click at cell `(x, y)` (0-indexed) into the
/// raw SGR mouse-protocol byte sequence a real terminal would send: a
/// press immediately followed by a release, both at the same
/// position. SGR coordinates are 1-indexed.
pub fn encode_click(x: u16, y: u16) -> Vec<u8> {
    let press = format!("\x1b[<0;{};{}M", x + 1, y + 1);
    let release = format!("\x1b[<0;{};{}m", x + 1, y + 1);
    let mut bytes = press.into_bytes();
    bytes.extend(release.into_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_keys_encode_as_csi_sequences() {
        assert_eq!(encode_key("Right").unwrap(), b"\x1b[C".to_vec());
        assert_eq!(encode_key("Left").unwrap(), b"\x1b[D".to_vec());
        assert_eq!(encode_key("Up").unwrap(), b"\x1b[A".to_vec());
        assert_eq!(encode_key("Down").unwrap(), b"\x1b[B".to_vec());
    }

    #[test]
    fn enter_esc_tab_encode_correctly() {
        assert_eq!(encode_key("Enter").unwrap(), b"\r".to_vec());
        assert_eq!(encode_key("Esc").unwrap(), b"\x1b".to_vec());
        assert_eq!(encode_key("Tab").unwrap(), b"\t".to_vec());
    }

    #[test]
    fn single_char_keys_encode_as_their_own_byte() {
        assert_eq!(encode_key("a").unwrap(), b"a".to_vec());
        assert_eq!(encode_key("Q").unwrap(), b"Q".to_vec());
        assert_eq!(encode_key("5").unwrap(), b"5".to_vec());
    }

    #[test]
    fn ctrl_combos_encode_as_control_bytes() {
        // Ctrl+A..Ctrl+Z map to bytes 0x01..0x1a
        assert_eq!(encode_key("Ctrl+A").unwrap(), vec![0x01]);
        assert_eq!(encode_key("Ctrl+C").unwrap(), vec![0x03]);
        assert_eq!(encode_key("Ctrl+Z").unwrap(), vec![0x1a]);
    }

    #[test]
    fn unknown_key_name_is_an_error() {
        let err = encode_key("Nonsense").unwrap_err();
        assert_eq!(err, KeyEncodeError::Unknown("Nonsense".to_string()));
    }

    #[test]
    fn encode_click_produces_the_expected_sgr_press_and_release_sequence() {
        let bytes = encode_click(3, 7);
        // 1-indexed SGR coords: x=3->4, y=7->8. Button 0 = left.
        assert_eq!(bytes, b"\x1b[<0;4;8M\x1b[<0;4;8m".to_vec());
    }

    #[test]
    fn encode_click_at_the_origin_is_1_indexed_correctly() {
        let bytes = encode_click(0, 0);
        assert_eq!(bytes, b"\x1b[<0;1;1M\x1b[<0;1;1m".to_vec());
    }
}
