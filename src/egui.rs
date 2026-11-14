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

use crate::{
    Caret, Config, EnterAction, Event, FieldKind, HandleKind, HandlePhase, Handles, Insets, text,
};
use ::egui::{
    Context, Id, Key, Modifiers, Pos2, RawInput, Stroke, TextEdit, Ui,
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
    /// The keyboard's in-progress word (bytes), drawn underlined.
    composing: Option<std::ops::Range<usize>>,
}

#[derive(Default)]
pub struct Ime {
    focused: Option<Synced>,
    /// Snapshot caret to apply after egui processed our injected edit: (field, anchor, head).
    pending_cursor: Option<(Id, usize, usize)>,
    toolbar_shown: bool,
    caret: Option<Caret>,
    /// Native handles last sent, the one being dragged, and whether the caret handle is wanted
    /// (after a tap in the field, until the next edit), as in Android's own text fields.
    handles: Handles,
    handle_drag: Option<HandleKind>,
    caret_handle: bool,
    /// The focused field's last layout, to hit-test handle drags: (id, galley, origin, line height).
    layout: Option<(Id, std::sync::Arc<::egui::Galley>, Pos2, f32)>,
    /// Theme last reported to the system bars.
    dark_ui: Option<bool>,
    window_focused: bool,
    /// The text Java's mirror holds, as far as we know: focusing a field with the same text
    /// then needs no keyboard restart.
    mirror: Option<String>,
    /// Last insets / configuration seen (also in [`Ime::events`]).
    pub insets: Insets,
    pub config: Option<Config>,
    /// Events the adapter doesn't consume itself (`Back`, `Insets`, `Config`, `Action`, every
    /// `Text`): drained by the app with [`Ime::take_events`].
    events: Vec<Event>,
}

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
        if raw.focused != self.window_focused {
            log::debug!(
                "oxydroid-egui: window focused {} -> {}",
                self.window_focused,
                raw.focused
            );
            self.window_focused = raw.focused;
        }
        let events = crate::poll();
        // Snapshots are whole states, so only the last one of the frame matters. Applying each
        // in turn is wrong: a second snapshot (e.g. the real keyboard reacting to the first)
        // would overwrite the replace-range before egui handles the first edit.
        let last_text = events.iter().rposition(|e| matches!(e, Event::Text(_)));
        for (i, ev) in events.into_iter().enumerate() {
            match &ev {
                Event::Text(s) if Some(i) == last_text => self.apply_snapshot(ctx, raw, s),
                Event::Text(_) => {}
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
                Event::Handle { which, x, y, phase } => {
                    self.drag_handle(ctx, *which, *x, *y, *phase)
                }
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
        // Widget focus, not `Response::has_focus`: that also requires *window* focus, and on
        // Android egui-winit can miss the window-focus event at startup (then it would never open
        // the keyboard). TextEdit itself handles keys based on widget focus too.
        let focused = ctx.memory(|m| m.has_focus(id));

        if !focused {
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
                    log::debug!(
                        "oxydroid-egui: resync text {:?} -> {text:?} ({anchor},{head})",
                        f.text
                    );
                    crate::set_text(text, anchor, head);
                    self.mirror = Some(text.clone());
                    f.composing = None;
                } else if (anchor, head) != (f.anchor, f.head) {
                    log::debug!(
                        "oxydroid-egui: resync caret ({},{}) -> ({anchor},{head}) in {text:?}",
                        f.anchor,
                        f.head
                    );
                    crate::set_selection(text, anchor, head);
                }
                f.text = text.clone();
                (f.anchor, f.head, f.kind, f.action, f.seen) = (anchor, head, kind, action, true);
            }
            _ => {
                // Newly focused field: open the keyboard with its content. If Java already
                // holds this text (e.g. refocusing the same field), skip the restart.
                crate::show(kind, action);
                if self.mirror.as_deref() == Some(text.as_str()) {
                    crate::set_selection(text, anchor, head);
                } else {
                    crate::set_text(text, anchor, head);
                    self.mirror = Some(text.clone());
                }
                self.focused = Some(Synced {
                    id,
                    text: text.clone(),
                    anchor,
                    head,
                    kind,
                    action,
                    seen: true,
                    composing: None,
                });
            }
        }

        if let Some(c) = self.focused.as_ref().and_then(|f| f.composing.clone()) {
            underline(ui, &out, text, c);
        }

        self.update_caret(ui, &out, text, head);
        if out.response.clicked() {
            self.caret_handle = true;
        }
        let line_h = out.galley.pos_from_cursor(CCursor::new(0)).height();
        self.layout = Some((id, out.galley.clone(), out.galley_pos, line_h));
        self.update_handles(ui, &out, text, anchor, head, kind);
        self.update_toolbar(ui, &out, text, anchor, head);
        out
    }

    /// The native handles last sent (window pixels): what a test needs to drag them.
    pub fn handles(&self) -> Handles {
        self.handles
    }

    /// Show the platform's selection handles: two under a selection, one under a caret after a
    /// tap. Only sent when they move.
    fn update_handles(
        &mut self,
        ui: &Ui,
        out: &TextEditOutput,
        text: &str,
        anchor: usize,
        head: usize,
        kind: FieldKind,
    ) {
        let ppp = ui.ctx().pixels_per_point();
        let tip = |byte: usize| {
            let r = out
                .galley
                .pos_from_cursor(CCursor::new(text::byte_to_char(text, byte)))
                .translate(out.galley_pos.to_vec2());
            ((r.left() * ppp).round(), (r.bottom() * ppp).round())
        };
        let handles = if kind == FieldKind::Password {
            Handles::None
        } else if anchor != head {
            Handles::Selection {
                start: tip(anchor.min(head)),
                end: tip(anchor.max(head)),
            }
        } else if self.caret_handle {
            let (x, y) = tip(head);
            Handles::Caret { x, y }
        } else {
            Handles::None
        };
        if handles != self.handles {
            self.handles = handles;
            let line_h = out.galley.pos_from_cursor(CCursor::new(0)).height() * ppp;
            crate::set_handles(handles, line_h);
        }
    }

    /// A native handle moved: hit-test the field's layout just above its tip and move that end.
    fn drag_handle(
        &mut self,
        ctx: &Context,
        which: HandleKind,
        x: f32,
        y: f32,
        phase: HandlePhase,
    ) {
        self.handle_drag = match phase {
            HandlePhase::Up => None,
            _ => Some(which),
        };
        if self.toolbar_shown && phase == HandlePhase::Down {
            self.toolbar_shown = false; // Java hid it; show it again once the handle is released
        }
        let Some((id, galley, origin, line_h)) = self.layout.clone() else {
            return;
        };
        let ppp = ctx.pixels_per_point();
        let aim = Pos2::new(x / ppp, y / ppp - line_h / 2.0);
        let moved = galley.cursor_from_pos(aim - origin);
        let mut st = TextEditState::load(ctx, id).unwrap_or_default();
        let cur = st
            .cursor
            .char_range()
            .unwrap_or_else(|| CCursorRange::one(moved));
        let (lo, hi) = if cur.secondary.index <= cur.primary.index {
            (cur.secondary, cur.primary)
        } else {
            (cur.primary, cur.secondary)
        };
        // The dragged end moves (primary); the other stays put (secondary), as in Android.
        let range = match which {
            HandleKind::Caret => CCursorRange::one(moved),
            HandleKind::Start => CCursorRange::two(hi, moved),
            HandleKind::End => CCursorRange::two(lo, moved),
        };
        st.cursor.set_char_range(Some(range));
        st.store(ctx, id);
        ctx.request_repaint();
    }

    /// Report the caret's position so keyboards can place their own UI next to it (Gboard's
    /// floating cursor and handwriting, OEM prediction popups). Only sent when it moves.
    fn update_caret(&mut self, ui: &Ui, out: &TextEditOutput, text: &str, head: usize) {
        let cursor = CCursor::new(text::byte_to_char(text, head));
        let r = out
            .galley
            .pos_from_cursor(cursor)
            .translate(out.galley_pos.to_vec2());
        let ppp = ui.ctx().pixels_per_point();
        let caret = Caret {
            x: r.left() * ppp,
            top: r.top() * ppp,
            bottom: r.bottom() * ppp,
            // egui 0.37 lays out every run left-to-right and doesn't expose per-run direction,
            // so there is nothing truthful to report yet. Revisit with egui's bidi work (#1016).
            rtl: false,
        };
        if self.caret != Some(caret) {
            self.caret = Some(caret);
            crate::set_caret(Some(caret));
        }
    }

    /// Call at the end of `ui()`, after every text field.
    pub fn end_frame(&mut self, ctx: &Context) {
        // Status bar icons must contrast with the UI behind them (edge-to-edge). Follow the theme.
        let dark = ctx.global_style().visuals.dark_mode;
        if self.dark_ui != Some(dark) {
            self.dark_ui = Some(dark);
            crate::set_light_system_bars(!dark);
        }
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
        self.caret_handle = false;
        self.handle_drag = None;
        self.layout = None;
        if self.handles != Handles::None {
            self.handles = Handles::None;
            crate::set_handles(Handles::None, 0.0);
        }
        if self.caret.take().is_some() {
            crate::set_caret(None);
        }
        if self.toolbar_shown {
            crate::hide_toolbar();
            self.toolbar_shown = false;
        }
        crate::hide();
    }

    /// Keyboard snapshot → egui events (see module docs).
    fn apply_snapshot(&mut self, ctx: &Context, raw: &mut RawInput, s: &crate::TextState) {
        self.mirror = Some(s.text.clone());
        let Some(f) = &mut self.focused else { return };
        f.composing = s.composing.clone();
        if s.text != f.text {
            self.caret_handle = false;
            let (start, old_end, new_end) = text::diff(&f.text, &s.text);
            let inserted = &s.text[start..new_end];
            log::debug!(
                "oxydroid-egui: snapshot {:?} -> {:?}: replace {start}..{old_end} with {inserted:?}, caret ({},{}), comp {:?}",
                f.text,
                s.text,
                s.anchor,
                s.head,
                s.composing
            );
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
        // A native handle being dragged counts as a finger down: its touches don't reach egui.
        let finger_down = ui.input(|i| i.pointer.any_down()) || self.handle_drag.is_some();
        if has_selection && !finger_down && !self.toolbar_shown {
            let (a, b) = (anchor.min(head), anchor.max(head));
            let p = |byte| {
                out.galley
                    .pos_from_cursor(CCursor::new(text::byte_to_char(text, byte)))
                    .translate(out.galley_pos.to_vec2())
            };
            let r = p(a).union(p(b));
            let ppp = ui.ctx().pixels_per_point();
            log::debug!("oxydroid-egui: show_toolbar over {r:?} (points) x{ppp}");
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

/// Underline the keyboard's composing range, like a native text field does.
fn underline(ui: &Ui, out: &TextEditOutput, text: &str, c: std::ops::Range<usize>) {
    let (start, end) = (
        text::byte_to_char(text, c.start),
        text::byte_to_char(text, c.end),
    );
    if start >= end {
        return;
    }
    let painter = ui.painter_at(out.text_clip_rect);
    let stroke = Stroke::new(1.5, ui.visuals().text_color());
    let at = |i: usize| {
        out.galley
            .pos_from_cursor(CCursor::new(i))
            .translate(out.galley_pos.to_vec2())
    };
    // Per character, so it follows wrapped rows and right-to-left runs.
    for i in start..end {
        let (a, b) = (at(i), at(i + 1));
        if (a.bottom() - b.bottom()).abs() < 1.0 {
            painter.hline(
                a.left().min(b.left())..=a.left().max(b.left()),
                a.bottom() - 1.0,
                stroke,
            );
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
