//! Toolkit-neutral adapter in **winit's IME vocabulary**: turns oxydroid's whole-state snapshots
//! into the incremental `Preedit` / `Commit` / `DeleteSurrounding` edits that winit 0.31
//! (`winit::event::Ime`) and the toolkits built on it (egui-winit, Bevy, iced, …) already consume.
//! No winit dependency: map [`ImeEdit`] onto your toolkit's own IME events.
//!
//! winit's model differs from Android's in two ways, handled here:
//! - The **preedit is not part of the text**: the toolkit's text holds committed text only, and
//!   the composing word is drawn separately. So the snapshot's composing range is cut out.
//! - Edits happen **at the cursor**: `DeleteSurrounding` counts bytes before/after the cursor and
//!   `Commit` inserts at it. A change elsewhere (rare: e.g. autocorrect two words back) can't be
//!   expressed; [`Translator::update`] then returns [`Resync`] and you replace the text wholesale.
//!
//! ```ignore
//! let mut tr = oxydroid::winit_ime::Translator::default();
//! for ev in oxydroid::poll() {
//!     if let oxydroid::Event::Text(s) = ev {
//!         match tr.update(&s) {
//!             Ok(edits) => for e in edits { /* emit as Ime::DeleteSurrounding / Commit / Preedit */ },
//!             Err(Resync { text, cursor }) => { /* set the field's text and caret directly */ }
//!         }
//!     }
//! }
//! ```

use crate::{TextState, text};

/// One incremental edit, in winit's terms. Byte offsets (UTF-8), like winit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEdit {
    /// Delete `before_bytes` before and `after_bytes` after the cursor (not counting the preedit).
    DeleteSurrounding {
        before_bytes: usize,
        after_bytes: usize,
    },
    /// Insert committed text at the cursor.
    Commit(String),
    /// Replace the preedit (composing) text. `cursor` is the caret inside it, `None` = hidden.
    /// An empty string clears the preedit.
    Preedit(String, Option<(usize, usize)>),
}

/// The change can't be expressed as edits at the cursor: replace text and caret wholesale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resync {
    /// Committed text (preedit excluded).
    pub text: String,
    /// Caret in `text`, bytes.
    pub cursor: usize,
    /// The preedit to show after resyncing, if any.
    pub preedit: Option<ImeEdit>,
}

/// Remembers the last snapshot so the next one can be expressed as a difference.
#[derive(Clone, Debug, Default)]
pub struct Translator {
    committed: String,
    cursor: usize,
    preedit: String,
}

/// Split a snapshot into (committed text, caret in it, preedit text, caret inside the preedit).
fn split(s: &TextState) -> (String, usize, String, Option<(usize, usize)>) {
    match &s.composing {
        Some(c) if c.start < c.end && c.end <= s.text.len() => {
            let committed = format!("{}{}", &s.text[..c.start], &s.text[c.end..]);
            let pre = s.text[c.clone()].to_owned();
            let inside = c.contains(&s.head) || s.head == c.end;
            let cursor = if s.head <= c.start || inside {
                c.start
            } else {
                s.head - c.len()
            };
            let pre_cursor = inside.then(|| {
                let p = s.head - c.start;
                (p, p)
            });
            (committed, cursor, pre, pre_cursor)
        }
        _ => (s.text.clone(), s.head, String::new(), None),
    }
}

impl Translator {
    /// Start from a known state (e.g. when a field gains focus).
    pub fn reset(&mut self, s: &TextState) {
        let (committed, cursor, preedit, _) = split(s);
        *self = Self {
            committed,
            cursor,
            preedit,
        };
    }

    /// Edits that take the toolkit from the previous snapshot to `s`.
    pub fn update(&mut self, s: &TextState) -> Result<Vec<ImeEdit>, Resync> {
        let (committed, cursor, preedit, pre_cursor) = split(s);
        let mut edits = Vec::new();
        let preedit_changed = preedit != self.preedit;
        // winit clears the preedit before committing; do the same when anything changes.
        if preedit_changed && !self.preedit.is_empty() {
            edits.push(ImeEdit::Preedit(String::new(), None));
        }
        if committed != self.committed {
            let (start, old_end, new_end) = text::diff(&self.committed, &committed);
            let c = self.cursor;
            // Expressible only if the old caret was inside the replaced range and the new caret
            // sits right after the inserted text (where winit's Commit leaves it).
            if !(start <= c && c <= old_end && cursor == new_end) {
                return Err(self.resync(committed, cursor, preedit, pre_cursor));
            }
            let (before, after) = (c - start, old_end - c);
            if before + after > 0 {
                edits.push(ImeEdit::DeleteSurrounding {
                    before_bytes: before,
                    after_bytes: after,
                });
            }
            if new_end > start {
                edits.push(ImeEdit::Commit(committed[start..new_end].to_owned()));
            }
        } else if cursor != self.cursor {
            // A pure caret move: winit has no event for it, so the toolkit must be told directly.
            return Err(self.resync(committed, cursor, preedit, pre_cursor));
        }
        if preedit_changed && !preedit.is_empty() {
            edits.push(ImeEdit::Preedit(preedit.clone(), pre_cursor));
        }
        *self = Self {
            committed,
            cursor,
            preedit,
        };
        Ok(edits)
    }

    /// Remember the new state and describe it as a wholesale replacement.
    fn resync(
        &mut self,
        committed: String,
        cursor: usize,
        preedit: String,
        pre_cursor: Option<(usize, usize)>,
    ) -> Resync {
        let edit = (!preedit.is_empty()).then(|| ImeEdit::Preedit(preedit.clone(), pre_cursor));
        let r = Resync {
            text: committed.clone(),
            cursor,
            preedit: edit,
        };
        *self = Self {
            committed,
            cursor,
            preedit,
        };
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text: &str, head: usize, composing: Option<std::ops::Range<usize>>) -> TextState {
        TextState {
            text: text.into(),
            anchor: head,
            head,
            composing,
            last_app_seq: 0,
        }
    }

    #[test]
    fn composing_then_commit() {
        let mut t = Translator::default();
        // Keyboard composes "he" (underlined), then "hel".
        assert_eq!(
            t.update(&snap("he", 2, Some(0..2))),
            Ok(vec![ImeEdit::Preedit("he".into(), Some((2, 2)))])
        );
        assert_eq!(
            t.update(&snap("hel", 3, Some(0..3))),
            Ok(vec![
                ImeEdit::Preedit(String::new(), None),
                ImeEdit::Preedit("hel".into(), Some((3, 3)))
            ])
        );
        // User picks the suggestion "hello ": composing ends, text committed.
        assert_eq!(
            t.update(&snap("hello ", 6, None)),
            Ok(vec![
                ImeEdit::Preedit(String::new(), None),
                ImeEdit::Commit("hello ".into())
            ])
        );
    }

    #[test]
    fn backspace_is_delete_surrounding() {
        let mut t = Translator::default();
        t.reset(&snap("héllo", 6, None));
        assert_eq!(
            t.update(&snap("héll", 5, None)),
            Ok(vec![ImeEdit::DeleteSurrounding {
                before_bytes: 1,
                after_bytes: 0
            }])
        );
    }

    #[test]
    fn autocorrect_replaces_word_before_cursor() {
        let mut t = Translator::default();
        t.reset(&snap("I teh", 5, None));
        assert_eq!(
            t.update(&snap("I the", 5, None)),
            Ok(vec![
                ImeEdit::DeleteSurrounding {
                    before_bytes: 2,
                    after_bytes: 0
                },
                ImeEdit::Commit("he".into())
            ])
        );
    }

    #[test]
    fn arabic_and_emoji_commit() {
        let mut t = Translator::default();
        t.reset(&snap("", 0, None));
        let s = "سلام 😀";
        assert_eq!(
            t.update(&snap(s, s.len(), None)),
            Ok(vec![ImeEdit::Commit(s.into())])
        );
    }

    #[test]
    fn caret_move_needs_resync() {
        let mut t = Translator::default();
        t.reset(&snap("hello", 5, None));
        let r = t.update(&snap("hello", 1, None)).unwrap_err();
        assert_eq!((r.text.as_str(), r.cursor), ("hello", 1));
    }

    #[test]
    fn edit_away_from_cursor_needs_resync() {
        let mut t = Translator::default();
        t.reset(&snap("one two three", 13, None));
        let r = t.update(&snap("One two three", 13, None)).unwrap_err();
        assert_eq!(r.text, "One two three");
    }
}
