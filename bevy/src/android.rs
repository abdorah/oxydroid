//! The bridge: oxydroid snapshots -> Bevy's `EditableText`, and Bevy's field state -> the keyboard.
//!
//! Parley's editor models a field the way Android does: the raw text (preedit included), a
//! compose range inside it, and a selection with an anchor and a focus. So each keyboard snapshot
//! is mirrored into the editor: the changed range is selected and replaced by a normal Bevy edit
//! (so the field's character filter and `max_characters` apply), then the compose range and the
//! selection are set. Keyboard-side selections (Select mode, select-all, a word re-marked for
//! correction) arrive intact.

use crate::{AndroidPlatform, OxydroidEvent};
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput},
    },
    input_focus::InputFocus,
    prelude::*,
    text::{EditableText, EditableTextSystems, FontCx, LayoutCx, TextEdit},
    ui::UiSystems,
    window::PrimaryWindow,
};
use oxydroid::{EnterAction, Event, FieldKind, TextState};

/// What the keyboard believes about the focused field.
#[derive(Resource, Default)]
struct Bridge {
    /// The field the keyboard is open for.
    field: Option<Entity>,
    /// Raw text (preedit included) last exchanged with the keyboard.
    text: String,
    /// Selection (anchor, focus) in bytes last exchanged with the keyboard.
    caret: (usize, usize),
    /// A keyboard snapshot whose text edit is queued; its selection is applied after the edit.
    pending: Option<TextState>,
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
        // Before Bevy's input dispatch (PreUpdate) and its text edits (PostUpdate).
        .add_systems(First, keyboard_to_bevy)
        .add_systems(
            PostUpdate,
            finish_snapshot
                .after(EditableTextSystems)
                .before(UiSystems::PostLayout),
        )
        // After TextInput applied its edits (PostUpdate): open/close the keyboard, then tell it
        // about the field's own changes.
        .add_systems(Last, (open_close_keyboard, field_to_keyboard).chain());
}

/// Byte offsets of the field's selection: (anchor, focus).
fn selection(t: &EditableText) -> (usize, usize) {
    let s = t.editor().raw_selection();
    (s.anchor().index(), s.focus().index())
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
            *bridge = Bridge {
                field: Some(e),
                text,
                caret,
                pending: None,
            };
        }
        (false, _) | (true, None) if bridge.field.is_some() => {
            oxydroid::hide();
            bridge.field = None;
        }
        _ => {}
    }
}

/// The field changed on Bevy's side (a tap, arrow keys, the app writing to it, a keyboard edit
/// the filter rejected): tell the keyboard.
fn field_to_keyboard(texts: Query<&EditableText>, mut bridge: ResMut<Bridge>) {
    let Some(e) = bridge.field else { return };
    let Ok(t) = texts.get(e) else { return };
    if t.is_composing() {
        return; // mid-composition the keyboard is the authority
    }
    let text = t.editor().raw_text();
    let caret = selection(t);
    if text != bridge.text {
        oxydroid::set_text(text, caret.0, caret.1);
        bridge.text = text.to_owned();
    } else if caret != bridge.caret {
        oxydroid::set_selection(text, caret.0, caret.1);
    } else {
        return;
    }
    bridge.caret = caret;
}

/// Keyboard -> Bevy, part 1 (before Bevy's edits): select the range the snapshot replaced and
/// queue the replacement as a normal Bevy edit, so the field's filter and `max_characters` apply.
fn start_snapshot(t: &mut EditableText, s: &TextState, font_cx: &mut FontCx, layout_cx: &mut LayoutCx) {
    let mut d = t
        .editor_mut()
        .driver(&mut font_cx.context, &mut layout_cx.0);
    d.finish_compose(); // keeps the text; the snapshot says where composing is now
    let old = d.editor.raw_text().to_owned();
    let (start, old_end, new_end) = oxydroid::text::diff(&old, &s.text);
    if old_end == start && new_end == start {
        return; // selection or composing change only
    }
    d.select_byte_range(start, old_end);
    t.queue_edit(if new_end > start {
        TextEdit::Insert(s.text[start..new_end].into())
    } else {
        TextEdit::Backspace // deletes the selected range
    });
}

/// Keyboard -> Bevy, part 2 (after Bevy's edits, before its layout): the compose range and the
/// selection. If the field rejected the text, leave it: `field_to_keyboard` resyncs the keyboard.
fn finish_snapshot(
    mut bridge: ResMut<Bridge>,
    mut texts: Query<&mut EditableText>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    let Some(s) = bridge.pending.take() else { return };
    let Some(Ok(mut t)) = bridge.field.map(|e| texts.get_mut(e)) else { return };
    if t.editor().raw_text() != s.text {
        return;
    }
    let mut d = t
        .editor_mut()
        .driver(&mut font_cx.context, &mut layout_cx.0);
    if let Some(c) = &s.composing {
        d.set_compose_byte_range(c.start, c.end);
    }
    d.select_byte_range(s.anchor, s.head);
}

/// Keyboard -> Bevy.
#[allow(clippy::too_many_arguments)]
fn keyboard_to_bevy(
    mut bridge: ResMut<Bridge>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut texts: Query<&mut EditableText>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
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
    let events = oxydroid::poll();
    // Every snapshot is a whole state: only the last one of this frame matters.
    let last_text = events.iter().rposition(|e| matches!(e, Event::Text(_)));
    for (i, ev) in events.into_iter().enumerate() {
        match &ev {
            Event::Text(s) if Some(i) == last_text => {
                if let Some(Ok(mut t)) = bridge.field.map(|e| texts.get_mut(e)) {
                    start_snapshot(&mut t, s, &mut font_cx, &mut layout_cx);
                    bridge.text = s.text.clone();
                    bridge.caret = (s.anchor, s.head);
                    bridge.pending = Some(s.clone());
                }
            }
            Event::Action(_) => press(Key::Enter, KeyCode::Enter),
            // Arrows, Shift = extend the selection (the keyboard's Select mode). Queued on the
            // field so Parley moves visually, right-to-left text included.
            Event::Key {
                code: code @ (21 | 22),
                shift,
            } => {
                if let Some(Ok(mut t)) = bridge.field.map(|e| texts.get_mut(e)) {
                    t.queue_edit(if *code == 21 {
                        TextEdit::Left(*shift)
                    } else {
                        TextEdit::Right(*shift)
                    });
                }
            }
            Event::Key { code: 66, .. } => press(Key::Enter, KeyCode::Enter),
            Event::Key { code: 67, .. } => press(Key::Backspace, KeyCode::Backspace),
            Event::Key { code: 112, .. } => press(Key::Delete, KeyCode::Delete),
            Event::Insets(i) => platform.insets = *i,
            Event::Config(c) => platform.config = Some(*c),
            _ => {}
        }
        out.write(OxydroidEvent(ev));
    }
}
