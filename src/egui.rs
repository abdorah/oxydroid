//! egui adapter (feature `egui`): a plain [`egui::TextEdit`] gets real keyboard input,
//! touch selection handles and the Cut/Copy/Paste toolbar on Android.
//!
//! ```ignore
//! struct App { ime: oxydroid::egui::Ime, name: String }
//!
//! impl eframe::App for App {
//!     fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
//!         self.ime.begin_frame(ctx, raw);                    // keyboard → egui events
//!     }
//!     fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
//!         self.ime.text_edit(ui, &mut self.name, FieldKind::Text, EnterAction::Next,
//!             |t| egui::TextEdit::singleline(t).hint_text("Name"));
//!         self.ime.end_frame(ui.ctx());                      // last, after all text fields
//!     }
//! }
//! ```
//!
//! How it maps the two models:
//! - **Keyboard → egui:** each snapshot is diffed against the field's text; the changed range is
//!   selected and typed over with an `Event::Text` (or Backspace), so egui applies it like a
//!   physical keyboard. The snapshot's caret/selection is applied right after.
//! - **egui → keyboard:** after the field is shown, its text and cursor are compared with what
//!   the keyboard knows. Edits made by the app or by egui itself → `set_state`; cursor moves
//!   (tap, drag, handles, arrows) → `set_selection`.
//! - egui-winit's own IME requests are suppressed (`end_frame`): on Android they hide/show the
//!   keyboard and fight with ours.
//! - Arrow keys are forwarded to egui, which moves the caret with its own (bidi-aware) layout.
//! - Composing text is applied as plain text: no underline yet.

use crate::{Config, EnterAction, Event, FieldKind, Insets, text};
use ::egui::{
    Context, Id, Key, LayerId, Modifiers, Order, Pos2, RawInput, Rect, Sense, Stroke, TextEdit, Ui,
    Vec2,
    text::CCursor,
    text_edit::{TextEditOutput, TextEditState},
    text_selection::CCursorRange,
};

/// What the keyboard currently believes about the focused field (byte offsets).
struct Synced {
    id: Id,
    text: String,
    anchor: usize,
    head: usize,
    kind: FieldKind,
    action: EnterAction,
    seen: bool,
}

#[derive(Default)]
pub struct Ime {
    focused: Option<Synced>,
    /// Snapshot caret to apply after egui processed our injected edit: (field, anchor, head).
    pending_cursor: Option<(Id, usize, usize)>,
    toolbar_shown: bool,
    /// Last insets / configuration seen (also in [`Ime::events`]).
    pub insets: Insets,
    pub config: Option<Config>,
    /// Events the adapter doesn't consume itself (`Back`, `Insets`, `Config`, `Action`, every
    /// `Text`): drained by the app with [`Ime::take_events`].
    events: Vec<Event>,
}

const SELECTION_HANDLE_RADIUS: f32 = 9.0;

impl Ime {
    /// Call once, after `oxydroid::init`, with the egui context (e.g. in eframe's app creator).
    pub fn new(ctx: &Context) -> Self {
        let c = ctx.clone();
        crate::set_waker(move || c.request_repaint());
        crate::set_arrows_to_app(true); // egui moves the caret itself
        Self::default()
    }

    /// Feed keyboard events into egui's input. Call from `eframe::App::raw_input_hook`.
    pub fn begin_frame(&mut self, ctx: &Context, raw: &mut RawInput) {
        for ev in crate::poll() {
            match &ev {
                Event::Text(s) => self.apply_snapshot(ctx, raw, s),
                Event::Action(_) => press(raw, Key::Enter, Modifiers::NONE),
                Event::Key { code, shift } => {
                    let m = if *shift {
                        Modifiers::SHIFT
                    } else {
                        Modifiers::NONE
                    };
                    if let Some(k) = egui_key(*code) {
                        press(raw, k, m);
                    }
                }
                Event::Insets(i) => self.insets = *i,
                Event::Config(c) => self.config = Some(*c),
                Event::Back => {}
            }
            self.events.push(ev);
        }
        if let Some(f) = &mut self.focused {
            f.seen = false;
        }
    }

    /// Events for the app (back, insets, config, Enter actions, text snapshots), oldest first.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Show a `TextEdit` built by `build` over `text`, wired to the Android keyboard.
    /// `kind` picks the keyboard layout (use [`FieldKind::Password`] with `.password(true)`).
    pub fn text_edit(
        &mut self,
        ui: &mut Ui,
        text: &mut String,
        kind: FieldKind,
        action: EnterAction,
        build: impl for<'a> FnOnce(&'a mut String) -> TextEdit<'a>,
    ) -> TextEditOutput {
        let mut out = build(text).show(ui);
        let id = out.response.id;
        let ctx = ui.ctx().clone();

        if !out.response.has_focus() {
            if self.focused.as_ref().is_some_and(|f| f.id == id) {
                self.unfocus();
            }
            return out;
        }

        // Apply the keyboard's caret now that egui has applied the keyboard's edit.
        if let Some((_, a, h)) = self.pending_cursor.take_if(|p| p.0 == id) {
            let range = range_for(text, a, h);
            out.state.cursor.set_char_range(Some(range));
            out.state.clone().store(&ctx, id);
            out.cursor_range = Some(range);
        }
        let (anchor, head) = out
            .cursor_range
            .map(|r| {
                (
                    text::char_to_byte(text, r.secondary.index.0),
                    text::char_to_byte(text, r.primary.index.0),
                )
            })
            .unwrap_or((text.len(), text.len()));

        match &mut self.focused {
            Some(f) if f.id == id => {
                if (f.kind, f.action) != (kind, action) {
                    crate::show(kind, action);
                }
                if *text != f.text {
                    // Changed by the app or by egui (not by the keyboard): the keyboard must resync.
                    crate::set_state(text, anchor..anchor);
                    crate::set_selection(text, anchor, head);
                } else if (anchor, head) != (f.anchor, f.head) {
                    crate::set_selection(text, anchor, head);
                }
                *f = Synced {
                    id,
                    text: text.clone(),
                    anchor,
                    head,
                    kind,
                    action,
                    seen: true,
                };
            }
            _ => {
                // Newly focused field: open the keyboard with its content.
                crate::show(kind, action);
                crate::set_state(text, anchor..anchor);
                crate::set_selection(text, anchor, head);
                self.focused = Some(Synced {
                    id,
                    text: text.clone(),
                    anchor,
                    head,
                    kind,
                    action,
                    seen: true,
                });
            }
        }

        if kind != FieldKind::Password {
            self.selection_handles(ui, &mut out);
        }
        self.update_toolbar(ui, &out, text, anchor, head);
        out
    }

    /// Call at the end of `ui()`, after every text field.
    pub fn end_frame(&mut self, ctx: &Context) {
        // egui-winit maps egui's IME requests to hide/show on Android: we own the keyboard instead.
        ctx.output_mut(|o| o.ime = None);
        // The focused field wasn't drawn this frame (screen changed): close the keyboard.
        if self.focused.as_ref().is_some_and(|f| !f.seen) {
            self.unfocus();
        }
    }

    fn unfocus(&mut self) {
        self.focused = None;
        self.pending_cursor = None;
        if self.toolbar_shown {
            crate::hide_toolbar();
            self.toolbar_shown = false;
        }
        crate::hide();
    }

    /// Keyboard snapshot → egui events (see module docs).
    fn apply_snapshot(&mut self, ctx: &Context, raw: &mut RawInput, s: &crate::TextState) {
        let Some(f) = &mut self.focused else { return };
        if s.text != f.text {
            let (start, old_end, new_end) = text::diff(&f.text, &s.text);
            let inserted = &s.text[start..new_end];
            // Select the replaced range, then type over it, as a physical keyboard would.
            let mut st = TextEditState::load(ctx, f.id).unwrap_or_default();
            st.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(text::byte_to_char(&f.text, start)),
                CCursor::new(text::byte_to_char(&f.text, old_end)),
            )));
            st.store(ctx, f.id);
            if inserted.is_empty() {
                press(raw, Key::Backspace, Modifiers::NONE);
            } else if inserted == "\n" {
                press(raw, Key::Enter, Modifiers::NONE); // egui ignores "\n" text events
            } else {
                raw.events.push(::egui::Event::Text(inserted.to_owned()));
            }
            self.pending_cursor = Some((f.id, s.anchor, s.head));
        } else {
            let mut st = TextEditState::load(ctx, f.id).unwrap_or_default();
            st.cursor
                .set_char_range(Some(range_for(&s.text, s.anchor, s.head)));
            st.store(ctx, f.id);
        }
        f.text = s.text.clone();
        f.anchor = s.anchor;
        f.head = s.head;
    }

    /// D4: two draggable handles under the selection ends (touch has no Shift-click).
    /// Byte offsets are recomputed by `text_edit` on the next frame, which then tells the keyboard.
    fn selection_handles(&mut self, ui: &mut Ui, out: &mut TextEditOutput) {
        let Some(range) = out.cursor_range else {
            return;
        };
        if range.is_empty() {
            return;
        }
        let ctx = ui.ctx().clone();
        let id = out.response.id;
        let painter =
            ctx.layer_painter(LayerId::new(Order::Foreground, id.with("oxydroid-handles")));
        let color = ui.visuals().selection.stroke.color;
        let ends = [range.secondary, range.primary];
        for (i, end) in ends.iter().enumerate() {
            let caret = out
                .galley
                .pos_from_cursor(*end)
                .translate(out.galley_pos.to_vec2());
            let center = caret.center_bottom() + Vec2::new(0.0, SELECTION_HANDLE_RADIUS);
            painter.line_segment([caret.center_bottom(), center], Stroke::new(2.0, color));
            painter.circle_filled(center, SELECTION_HANDLE_RADIUS, color);
            // Generous touch target, registered after the TextEdit so it wins the hit test.
            let touch = Rect::from_center_size(center, Vec2::splat(SELECTION_HANDLE_RADIUS * 4.0));
            let r = ui.interact(touch, id.with(("oxydroid-handle", i)), Sense::drag());
            if r.dragged()
                && let Some(p) = r.interact_pointer_pos()
            {
                // The finger sits on the knob below the text: aim at the text row above it.
                let aim: Pos2 = p - Vec2::new(0.0, SELECTION_HANDLE_RADIUS + caret.height() / 2.0);
                let moved = out.galley.cursor_from_pos(aim - out.galley_pos);
                let other = ends[1 - i];
                // Dragging the start handle makes it the moving end; that's fine: head = moved.
                let new_range = CCursorRange::two(other, moved);
                out.state.cursor.set_char_range(Some(new_range));
                out.state.clone().store(&ctx, id);
                out.cursor_range = Some(new_range);
            }
        }
    }

    /// The floating Cut/Copy/Paste toolbar over a non-empty selection, once the finger is up.
    fn update_toolbar(
        &mut self,
        ui: &Ui,
        out: &TextEditOutput,
        text: &str,
        anchor: usize,
        head: usize,
    ) {
        let has_selection = anchor != head;
        let finger_down = ui.input(|i| i.pointer.any_down());
        if has_selection && !finger_down && !self.toolbar_shown {
            let (a, b) = (anchor.min(head), anchor.max(head));
            let p = |byte| {
                out.galley
                    .pos_from_cursor(CCursor::new(text::byte_to_char(text, byte)))
                    .translate(out.galley_pos.to_vec2())
            };
            let r = p(a).union(p(b));
            let ppp = ui.ctx().pixels_per_point();
            crate::show_toolbar(
                (r.left() * ppp) as i32,
                (r.top() * ppp) as i32,
                (r.right() * ppp) as i32,
                (r.bottom() * ppp) as i32,
            );
            self.toolbar_shown = true;
        } else if !has_selection && self.toolbar_shown {
            crate::hide_toolbar();
            self.toolbar_shown = false;
        }
    }
}

fn range_for(text: &str, anchor: usize, head: usize) -> CCursorRange {
    CCursorRange::two(
        CCursor::new(text::byte_to_char(text, anchor)),
        CCursor::new(text::byte_to_char(text, head)),
    )
}

fn press(raw: &mut RawInput, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        raw.events.push(::egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
    }
}

/// Android `KeyEvent.KEYCODE_*` → egui key.
fn egui_key(code: i32) -> Option<Key> {
    Some(match code {
        21 => Key::ArrowLeft,
        22 => Key::ArrowRight,
        19 => Key::ArrowUp,
        20 => Key::ArrowDown,
        61 => Key::Tab,
        66 => Key::Enter,
        67 => Key::Backspace,
        112 => Key::Delete,
        122 => Key::Home,
        123 => Key::End,
        _ => return None,
    })
}
