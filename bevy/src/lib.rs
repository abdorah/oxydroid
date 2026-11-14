//! **oxydroid-bevy**: Android soft-keyboard input for Bevy's [`TextInput`] widgets.
//!
//! On Android, Bevy apps get no text from the soft keyboard (bevy#13822): `NativeActivity` offers
//! the keyboard no `InputConnection`. Add [`OxydroidPlugin`] and an ordinary Bevy `TextInput`
//! works with the real keyboard: accents, any script, emoji, suggestions, swipe and voice typing.
//!
//! ```ignore
//! App::new()
//!     .add_plugins((DefaultPlugins, OxydroidPlugin))
//!     .run();
//! ```
//!
//! How: Bevy's `TextInput` already speaks winit's IME vocabulary (`Ime::Preedit` / `Ime::Commit`)
//! plus key presses. The plugin opens oxydroid's keyboard when Bevy enables IME for a focused
//! field, turns oxydroid's whole-state snapshots into exactly those messages with
//! [`oxydroid::winit_ime::Translator`], and tells the keyboard when the field's own text or caret
//! changes (taps, arrow keys, app writes). Arrow keys go to Bevy, whose Parley layout moves the
//! caret visually, right-to-left text included.
//!
//! Other platform events (insets, Back, dark mode…) are re-sent as [`OxydroidEvent`] messages,
//! and the latest insets and configuration are kept in the [`AndroidPlatform`] resource.
//!
//! [`TextInput`]: https://docs.rs/bevy/latest/bevy/ui_widgets/struct.TextInput.html

use bevy::prelude::*;

#[cfg(target_os = "android")]
mod android;

pub use oxydroid;

/// Android soft-keyboard support for Bevy text inputs. Does nothing on other platforms.
pub struct OxydroidPlugin;

impl Plugin for OxydroidPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AndroidPlatform>()
            .add_message::<OxydroidEvent>();
        #[cfg(target_os = "android")]
        android::build(app);
    }
}

/// Every oxydroid event (Back, insets, configuration, text snapshots, …), re-sent as a message.
#[derive(Message, Clone, Debug)]
pub struct OxydroidEvent(pub oxydroid::Event);

/// The latest platform state from oxydroid.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct AndroidPlatform {
    /// System bars, display cutout and keyboard height, in physical pixels.
    pub insets: oxydroid::Insets,
    pub config: Option<oxydroid::Config>,
}
