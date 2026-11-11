//! The ONE place where Java offsets (UTF-16 units) and Rust offsets (UTF-8 bytes) meet.
//! "é" = 1 unit / 2 bytes, "😀" = 2 units / 4 bytes.

/// UTF-16 index → byte index. An index inside a surrogate pair rounds down to the char start.
pub fn utf16_to_byte(s: &str, idx: usize) -> usize {
    let mut units = 0;
    for (byte, c) in s.char_indices() {
        let next = units + c.len_utf16();
        if units >= idx || next > idx {
            return byte;
        }
        units = next;
    }
    s.len()
}

/// Byte index → UTF-16 index. A byte inside a char rounds down to the char start.
pub fn byte_to_utf16(s: &str, mut byte: usize) -> usize {
    byte = byte.min(s.len());
    while !s.is_char_boundary(byte) {
        byte -= 1;
    }
    s[..byte].encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_identity() {
        assert_eq!(utf16_to_byte("hello", 3), 3);
        assert_eq!(byte_to_utf16("hello", 3), 3);
    }

    #[test]
    fn accent_is_two_bytes() {
        let s = "héllo";
        assert_eq!(utf16_to_byte(s, 2), 3); // after "hé"
        assert_eq!(byte_to_utf16(s, 3), 2);
    }

    #[test]
    fn emoji_is_two_units_four_bytes() {
        let s = "a😀b";
        assert_eq!(utf16_to_byte(s, 3), 5); // after "a😀"
        assert_eq!(byte_to_utf16(s, 5), 3);
        assert_eq!(utf16_to_byte(s, 2), 1); // inside the pair → start of 😀
        assert_eq!(byte_to_utf16(s, 3), 1); // inside 😀 → start of 😀
    }

    #[test]
    fn arabic_and_out_of_range() {
        let s = "سلام"; // 4 chars, 2 bytes each
        assert_eq!(utf16_to_byte(s, 2), 4);
        assert_eq!(utf16_to_byte(s, 99), s.len());
        assert_eq!(byte_to_utf16(s, 99), 4);
    }

    #[test]
    fn round_trip_on_char_boundaries() {
        let s = "héllo 😀 سلام";
        for (b, _) in s.char_indices() {
            assert_eq!(utf16_to_byte(s, byte_to_utf16(s, b)), b);
        }
    }
}
