# Changelog

## Unreleased

- Native selection handles: `set_handles(Handles, line_height)` shows the platform's own caret and
  selection handles (theme drawables, in popup windows above the native surface);
  `Event::Handle { which, x, y, phase }` reports drags. Magnifier (Android 10+) while dragging,
  kept on the text. `set_handle_color`. The egui adapter uses them instead of drawing its own.
- The floating toolbar's target rect includes the handles, so it never covers them.
- Autotest drives the handles with real touches (`input motionevent`): 18/18 on Android 16.

- Insets follow the keyboard's slide animation frame by frame (`WindowInsetsAnimation`, API 30+):
  39 intermediate heights in the emulator test instead of one jump.
- `Config` gains `accent_color` (ARGB; the Material You accent on Android 12+) and
  `animations_enabled` (false when the user turned animations off).
- Back verified on Android 16: handled -> `Event::Back`; unhandled -> the system closes the app.

- `set_light_system_bars(bool)`: dark icons for a light UI under edge-to-edge (forced on Android 16,
  where white icons vanished on light themes). The egui adapter follows the theme automatically.
- Fixed: the floating toolbar never stayed visible with Gboard (Android 16). Keyboards' no-op calls
  dismissed it; now only a real text change does.
- Edge-to-edge below API 30 keeps other system UI flags.
- Demo builds for x86_64 too; `demo/emulator.sh` runs an Android 16 emulator for the autotest.

- `set_caret(Option<Caret>)` -> `CursorAnchorInfo` + `requestCursorUpdates`: the caret's screen
  position, selection and composing region. Keyboards need it for the floating cursor, handwriting
  and prediction popups. The egui adapter sends it when the caret moves.
- `getExtractedText` / `updateExtractedText`: some keyboards disable suggestions and gesture typing
  when an editor returns null here (`BaseInputConnection`'s default).
- `closeConnection` clears both monitors.

Fixed (found by the on-device test, docs/test-reports/2026-11-11-a20):
- egui adapter: apply only the last keyboard snapshot per frame (two snapshots in one frame
  corrupted edits).
- The input view covers the whole window: the floating toolbar was never visible.
- egui adapter: use widget focus; `Response::has_focus` needs window focus, which egui-winit can
  miss at startup on Android.

- Keyboard input through a real `InputConnection` with a Java-side text mirror and whole-state
  snapshots (`Event::Text`). App writes win, via sequence numbers.
- Selection keeps its direction (`anchor` / `head`).
- Keyboard cursor keys, Select mode (Shift), Select all / Cut / Copy / Paste.
- Field kinds and Enter actions (`show(FieldKind, EnterAction)`).
- Floating Cut/Copy/Paste toolbar (`show_toolbar`).
- Insets with keyboard height, opt-in edge-to-edge.
- Back via `OnBackInvokedCallback` on Android 13+ (`set_back_handled`).
- Configuration events: dark mode, font scale, density.
- egui adapter (feature `egui`): `oxydroid::egui::Ime` wires plain `TextEdit`s to the keyboard,
  with touch selection handles and the floating toolbar. Demo app in `demo/`.
- `winit_ime::Translator`: toolkit-neutral adapter producing winit-style `Preedit` / `Commit` /
  `DeleteSurrounding` edits (for egui-winit, Bevy, iced-style consumers), with `Resync` fallback.
- egui adapter: composing text is underlined; refocusing a field Java already mirrors skips the
  keyboard restart; one `set_text` call (keeps selection direction) instead of two.
- `set_text(text, anchor, head)`, `intent_extra(activity, key)`.
- Demo: scripted on-device test (`--es oxytest 1`) driving the real InputConnection.
- `text::diff` and char/byte helpers for toolkit adapters.
- On-device self-test hooks (hidden): simulated keyboard commit, snapshot timing.
- Re-`init` after activity recreation re-attaches. Callbacks never unwind into Java.
