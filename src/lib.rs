//! **oxydroid**: the Android platform pieces a Rust UI app needs on `NativeActivity`, with
//! no JDK and no Gradle for users.
//!
//! - **Keyboard (IME):** real text input through an `InputConnection`: accents, any script,
//!   emoji, suggestions, swipe and voice typing. Plus field kinds (password, number, …) and the Enter action.
//! - **Clipboard:** the keyboard's and the floating toolbar's Cut/Copy/Paste/Select all.
//! - **Insets:** system bars, display cutout and keyboard height, plus opt-in edge-to-edge.
//! - **Back:** `OnBackInvokedCallback` on Android 13+ (required when targeting API 36).
//! - **Configuration:** dark mode, font scale, density.
//!
//! A small Java helper ships as a prebuilt `classes.dex`, embedded in this crate and loaded from
//! memory at runtime. All logic stays in Rust. Model: the Java side keeps a mirror of the text and
//! answers the keyboard instantly; after every edit Rust gets one whole-state snapshot, and app
//! writes always win (see `notes/02`).
//!
//! ```ignore
//! // in android_main, before the event loop:
//! oxydroid::init(app.vm_as_ptr(), app.activity_as_ptr())?;
//! oxydroid::set_waker(move || ctx.request_repaint());
//! // when a text field gains focus:
//! oxydroid::show(FieldKind::Text, EnterAction::Done);
//! // every frame:
//! for ev in oxydroid::poll() {
//!     match ev {
//!         Event::Text(state) => { /* replace the field's text + selection */ }
//!         Event::Insets(i) => { /* pad the UI */ }
//!         _ => {}
//!     }
//! }
//! ```
//!
//! **Manifest:** add `uiMode|fontScale|density` to the activity's `configChanges`, so that dark
//! mode or font changes don't recreate the activity.

pub mod offsets;
pub mod text;

/// egui adapter: real keyboard input for a plain `egui::TextEdit`.
#[cfg(all(target_os = "android", feature = "egui"))]
pub mod egui;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::*;

use std::ops::Range;

/// Whole editing state, in UTF-8 byte offsets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextState {
    pub text: String,
    /// Where the selection started (stays put while extending). Same as Android's selection start.
    pub anchor: usize,
    /// The moving end, where the caret is drawn. May be < `anchor` (backwards selection).
    pub head: usize,
    /// The underlined "in progress" part, if the keyboard is composing.
    pub composing: Option<Range<usize>>,
    /// The last app write ([`set_state`]/[`set_selection`]) this snapshot already includes.
    pub last_app_seq: u64,
}

impl TextState {
    /// The selected range, sorted. Empty = just a caret.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

/// What kind of text the focused field holds. Picks the keyboard layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FieldKind {
    #[default]
    Text = 0,
    /// Hidden input, no suggestions, no learning.
    Password = 1,
    Number = 2,
    Email = 3,
    /// Enter inserts a newline instead of an action.
    Multiline = 4,
}

/// The label/behaviour of the keyboard's Enter key. Pressing it sends [`Event::Action`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnterAction {
    None = 0,
    #[default]
    Done = 1,
    Go = 2,
    Send = 3,
    Next = 4,
    Search = 5,
}

/// Areas covered by system UI, in physical pixels of the window surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insets {
    /// System bars + display cutout.
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    /// Height of the on-screen keyboard (0 when hidden). Keep the focused field above it.
    pub ime_bottom: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub night_mode: bool,
    /// The user's font size setting (1.0 = default).
    pub font_scale: f32,
    pub density_dpi: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The keyboard edited the text or moved the selection.
    Text(TextState),
    /// Enter/action key: an `EditorInfo.IME_ACTION_*` value (2 go, 3 search, 4 send, 5 next, 6 done).
    Action(i32),
    /// A key the text mirror couldn't apply (`KeyEvent.KEYCODE_*`): DEL at the start (67),
    /// Enter (66), and arrows (21/22) when [`set_arrows_to_app`] is on. `shift` for selection.
    Key {
        code: i32,
        shift: bool,
    },
    Insets(Insets),
    /// Back gesture/button, delivered only while [`set_back_handled`]`(true)` (Android 13+).
    Back,
    Config(Config),
}

#[derive(Debug)]
pub enum Error {
    #[cfg(target_os = "android")]
    Jni(jni::errors::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_sorted_whatever_the_direction() {
        let s = TextState {
            text: "hello".into(),
            anchor: 4,
            head: 1,
            ..Default::default()
        };
        assert_eq!(s.selection(), 1..4);
        let s = TextState {
            anchor: 1,
            head: 4,
            ..s
        };
        assert_eq!(s.selection(), 1..4);
    }

    #[test]
    fn enum_values_match_the_java_side() {
        // Platform.show(kind, action) indexes by these numbers.
        assert_eq!(FieldKind::Multiline as i32, 4);
        assert_eq!(EnterAction::Search as i32, 5);
    }
}
