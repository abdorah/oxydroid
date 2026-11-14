//! The bridge: oxydroid snapshots -> Bevy `Ime` / `KeyboardInput` messages, and Bevy's field
//! state -> the keyboard.

use crate::{AndroidPlatform, OxydroidEvent};
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput},
    },
    input_focus::InputFocus,
    prelude::*,
    text::{EditableText, TextEdit},
    window::{Ime, PrimaryWindow},
};
use oxydroid::{
    EnterAction, Event, FieldKind, TextState,
    winit_ime::{ImeEdit, Translator},
};

/// What the keyboard believes about the focused field.
#[derive(Resource, Default)]
struct Bridge {
    translator: Translator,
    /// The field the keyboard is open for.
    field: Option<Entity>,
    /// Caret (anchor, focus) in bytes last told to the keyboard.
    caret: (usize, usize),
}

pub(crate) fn build(app: &mut App) {
    match bevy::android::ANDROID_APP.get() {
        Some(a) => {
            if let Err(e) = oxydroid::init(a.vm_as_ptr(), a.activity_as_ptr()) {
                error!("oxydroid::init failed: {e:?}");
            }
        }
        None => error!("oxydroid-bevy: no AndroidApp (missing #[bevy_main]?)"),
    }
    oxydroid::set_arrows_to_app(true); // Parley moves the caret visually, bidi included
    app.init_resource::<Bridge>()
        // Messages written in First are read by Bevy's input-focus dispatch and TextInput IME
        // systems in PreUpdate of the same frame.
        .add_systems(First, keyboard_to_bevy)
        // After TextInput applied its edits (PostUpdate): open/close the keyboard, then tell it
        // about the field's own changes.
        .add_systems(Last, (open_close_keyboard, field_to_keyboard).chain());
}

/// Byte offsets of the field's selection: (anchor, focus).
fn selection(t: &EditableText) -> (usize, usize) {
    let s = t.editor().raw_selection();
    (s.anchor().index(), s.focus().index())
}

fn state(text: &str, (anchor, head): (usize, usize)) -> TextState {
    TextState {
        text: text.to_owned(),
        anchor,
        head,
        composing: None,
        last_app_seq: 0,
    }
}

/// Bevy asks for IME when an editable `TextInput` is focused: open our keyboard with its text.
fn open_close_keyboard(
    windows: Query<&Window, With<PrimaryWindow>>,
    focus: Res<InputFocus>,
    texts: Query<&EditableText>,
    mut bridge: ResMut<Bridge>,
) {
    let Ok(window) = windows.single() else { return };
    let field = focus.get().filter(|e| texts.contains(*e));
    match (window.ime_enabled, field) {
        (true, Some(e)) if bridge.field != Some(e) => {
            let t = texts.get(e).expect("checked above");
            let text = t.value().to_string();
            let caret = selection(t);
            oxydroid::show(FieldKind::Text, EnterAction::Done);
            oxydroid::set_text(&text, caret.0, caret.1);
            bridge.translator.reset(&state(&text, caret));
            bridge.field = Some(e);
            bridge.caret = caret;
        }
        (false, _) | (true, None) if bridge.field.is_some() => {
            oxydroid::hide();
            bridge.field = None;
        }
        _ => {}
    }
}

/// The field changed on Bevy's side (a tap, arrow keys, the app writing to it): tell the keyboard.
fn field_to_keyboard(texts: Query<&EditableText>, mut bridge: ResMut<Bridge>) {
    let Some(e) = bridge.field else { return };
    let Ok(t) = texts.get(e) else { return };
    if t.is_composing() {
        return; // mid-composition the keyboard is the authority
    }
    let text = t.value().to_string();
    let caret = selection(t);
    if text != bridge.translator.text() {
        oxydroid::set_text(&text, caret.0, caret.1);
        bridge.translator.reset(&state(&text, caret));
    } else if caret != bridge.caret {
        oxydroid::set_selection(&text, caret.0, caret.1);
        bridge.translator.reset(&state(&text, caret));
    } else {
        return;
    }
    bridge.caret = caret;
}

/// Keyboard -> Bevy: snapshots become `Ime` and `KeyboardInput` messages.
#[allow(clippy::too_many_arguments)]
fn keyboard_to_bevy(
    mut bridge: ResMut<Bridge>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut texts: Query<&mut EditableText>,
    mut ime: MessageWriter<Ime>,
    mut keys: MessageWriter<KeyboardInput>,
    mut out: MessageWriter<OxydroidEvent>,
    mut platform: ResMut<AndroidPlatform>,
) {
    let Ok(window) = windows.single() else { return };
    let mut press = |key: Key, code: KeyCode| {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            keys.write(KeyboardInput {
                key_code: code,
                logical_key: key.clone(),
                state,
                text: None,
                repeat: false,
                window,
            });
        }
    };
    for ev in oxydroid::poll() {
        match &ev {
            Event::Text(s) if bridge.field.is_some() => match bridge.translator.update(s) {
                Ok(edits) => {
                    for edit in edits {
                        match edit {
                            ImeEdit::Commit(value) => {
                                ime.write(Ime::Commit { window, value });
                            }
                            ImeEdit::Preedit(value, cursor) => {
                                ime.write(Ime::Preedit {
                                    window,
                                    value,
                                    cursor,
                                });
                            }
                            ImeEdit::DeleteSurrounding {
                                before_chars,
                                after_chars,
                                ..
                            } => {
                                for _ in 0..before_chars {
                                    press(Key::Backspace, KeyCode::Backspace);
                                }
                                for _ in 0..after_chars {
                                    press(Key::Delete, KeyCode::Delete);
                                }
                            }
                        }
                    }
                    bridge.caret = (bridge.translator.cursor(), bridge.translator.cursor());
                }
                Err(r) => {
                    // Not expressible as edits at the caret: replace the field's text.
                    if let Some(Ok(mut t)) = bridge.field.map(|e| texts.get_mut(e)) {
                        t.clear();
                        t.queue_edit(TextEdit::Insert(r.text.as_str().into()));
                    }
                    if let Some(ImeEdit::Preedit(value, cursor)) = r.preedit {
                        ime.write(Ime::Preedit {
                            window,
                            value,
                            cursor,
                        });
                    }
                    bridge.caret = (r.text.len(), r.text.len());
                }
            },
            Event::Action(_) => press(Key::Enter, KeyCode::Enter),
            Event::Key { code, .. } => match code {
                21 => press(Key::ArrowLeft, KeyCode::ArrowLeft),
                22 => press(Key::ArrowRight, KeyCode::ArrowRight),
                66 => press(Key::Enter, KeyCode::Enter),
                67 => press(Key::Backspace, KeyCode::Backspace),
                112 => press(Key::Delete, KeyCode::Delete),
                _ => {}
            },
            Event::Insets(i) => platform.insets = *i,
            Event::Config(c) => platform.config = Some(*c),
            _ => {}
        }
        out.write(OxydroidEvent(ev));
    }
}
