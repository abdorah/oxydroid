//! Toolkit-neutral text helpers for adapters: turning a keyboard snapshot into an edit.

/// The smallest edit turning `old` into `new`: `old[start..old_end]` is replaced by
/// `new[start..new_end]`. Offsets are bytes on char boundaries. Equal strings → empty edit.
pub fn diff(old: &str, new: &str) -> (usize, usize, usize) {
    let start = old
        .char_indices()
        .zip(new.chars())
        .take_while(|((_, a), b)| a == b)
        .last()
        .map_or(0, |((i, a), _)| i + a.len_utf8());
    let suffix: usize = old[start..]
        .chars()
        .rev()
        .zip(new[start..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    (start, old.len() - suffix, new.len() - suffix)
}

/// Byte offset → char index (what egui's `CCursor` counts).
pub fn byte_to_char(text: &str, byte: usize) -> usize {
    let mut b = byte.min(text.len());
    while !text.is_char_boundary(b) {
        b -= 1;
    }
    text[..b].chars().count()
}

/// Char index → byte offset (clamped to the end).
pub fn char_to_byte(text: &str, ch: usize) -> usize {
    text.char_indices().nth(ch).map_or(text.len(), |(b, _)| b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, new: &str) -> String {
        let (s, oe, ne) = diff(old, new);
        format!("{}{}{}", &old[..s], &new[s..ne], &old[oe..])
    }

    #[test]
    fn typing_one_char() {
        assert_eq!(diff("helo", "hello"), (3, 3, 4));
    }

    #[test]
    fn backspace() {
        assert_eq!(diff("hello", "hell"), (4, 5, 4));
    }

    #[test]
    fn autocorrect_replaces_a_word() {
        let (s, oe, ne) = diff("I teh cat", "I the cat");
        assert_eq!((&"I teh cat"[s..oe], &"I the cat"[s..ne]), ("eh", "he"));
    }

    #[test]
    fn repeated_letters_dont_overlap() {
        // "aa" → "aaa": prefix and suffix must not both claim the same 'a'.
        let (s, oe, ne) = diff("aa", "aaa");
        assert!(s <= oe && s <= ne);
        assert_eq!(apply("aa", "aaa"), "aaa");
    }

    #[test]
    fn multibyte_round_trips() {
        for (old, new) in [
            ("héllo", "hállo"),
            ("سلام", "سلامة"),
            ("a😀b", "a😃b"),
            ("", "é"),
            ("x", ""),
        ] {
            assert_eq!(apply(old, new), new, "{old} → {new}");
        }
    }

    #[test]
    fn equal_is_empty() {
        let (s, oe, ne) = diff("same", "same");
        assert_eq!((oe - s, ne - s), (0, 0));
    }

    #[test]
    fn char_byte_conversions() {
        let t = "hé😀";
        assert_eq!(byte_to_char(t, 3), 2); // after "hé"
        assert_eq!(char_to_byte(t, 2), 3);
        assert_eq!(char_to_byte(t, 9), t.len());
        assert_eq!(byte_to_char(t, 4), 2); // inside 😀 → before it
    }
}
