# oxydroid

**Android platform services for Rust UI apps on `NativeActivity`: real keyboard input,
clipboard, insets, back and dark mode. No JDK, no Gradle.**

> Status: **experimental** (0.0.x). Tested on one device: Galaxy A20, Android 11, Samsung Keyboard.

## Why

A Rust app on Android (egui, Bevy, iced, …) can draw at 60 fps but can't type `é`.

`NativeActivity` gives native code a surface and raw input events, nothing more. When a text field
gets focus, Android looks for an `InputConnection` and finds none (`ic=null`), so the keyboard falls
back to sending fake key presses. ASCII letters survive. Accents, Arabic, emoji, suggestions, swipe
and voice typing don't. Implementing `InputConnection` needs a Java class, and that's why every
Rust GUI toolkit is stuck or carries its own private workaround.

oxydroid is that Java class, written once for everyone and shipped **prebuilt**.

## What works

| Feature | Status on the A20 (Android 11) |
|---|---|
| Typing: accents, Arabic, emoji, suggestions/autocorrect (composing text) | ✅ |
| App-side edits ("clear on send") win over in-flight keyboard edits | ✅ by design; on-device self-test in the demo |
| Caret/selection from the app (tap, drag, long-press) and from the keyboard (cursor keys, Select mode) | ✅ |
| Keyboard Select all / Cut / Copy / Paste, cross-app clipboard | ✅ |
| Field kinds (password, number, email, multiline) + Enter action (Done/Send/Go/…) | 🧪 new |
| Floating Cut/Copy/Paste toolbar | 🧪 new |
| Insets (system bars, cutout, keyboard height) + opt-in edge-to-edge | 🧪 new |
| Dark mode / font scale / density changes | 🧪 new |
| Back via `OnBackInvokedCallback` (Android 13+) | ⚠️ implemented, **untested** (needs Android 13+) |
| **egui adapter** (feature `egui`): plain `egui::TextEdit` with real keyboard input | 🧪 new |
| Draggable selection handles (drawn by the toolkit adapter) | 🧪 new, egui adapter |

## egui

```toml
oxydroid = { version = "0.0.1", features = ["egui"] }
```

```rust
struct App { ime: oxydroid::egui::Ime, name: String }   // Ime::new(&cc.egui_ctx) in the app creator

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.ime.begin_frame(ctx, raw);                        // keyboard → egui input
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.ime.text_edit(ui, &mut self.name, FieldKind::Text, EnterAction::Done,
            |t| egui::TextEdit::singleline(t).hint_text("Name"));
        self.ime.end_frame(ui.ctx());                          // after all fields
    }
}
```

The adapter diffs each keyboard snapshot against the field and types the change into egui (so
egui's own undo, filters and layout apply). It resyncs the keyboard when the app or egui changes
the text or caret, draws touch selection handles, shows the floating toolbar over a selection, and
suppresses egui-winit's own IME requests (on Android those hide/show the keyboard). Composing text
isn't underlined yet. See [`demo/`](demo) for a sign-in form and a chat box.

## How it works

- `java/` holds two small framework-only classes (no AndroidX). `build-dex.sh` compiles them **once,
  by maintainers**. The resulting `classes.dex` (~20 KB) is committed and embedded with `include_bytes!`.
- At runtime the dex is loaded from memory (`InMemoryDexClassLoader`, API 26+), native callbacks are
  bound with `RegisterNatives`, and an invisible 1×1 view is attached to your activity.
- That view keeps a **mirror** of the text, so the keyboard's synchronous questions ("text before
  the cursor?") are answered instantly on the UI thread. After every edit, Rust receives **one
  whole-state snapshot**: text, anchor, head, composing range.
- **App writes always win:** each `set_state`/`set_selection` gets a sequence number, Java echoes
  the last one it applied, and `poll()` drops snapshots that predate your latest write.
- Offsets are UTF-8 bytes in Rust and UTF-16 units in Java; the conversion lives in one tested module.

## Usage

```rust
use oxydroid::{EnterAction, Event, FieldKind};

#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    oxydroid::init(app.vm_as_ptr(), app.activity_as_ptr()).unwrap();
    // ... after creating your UI context:
    // oxydroid::set_waker(move || ctx.request_repaint());
}

// When a text field gains focus:
oxydroid::show(FieldKind::Text, EnterAction::Send);

// Every frame:
for ev in oxydroid::poll() {
    match ev {
        Event::Text(s) => { /* field.text = s.text; caret = s.head; selection = s.selection() */ }
        Event::Action(_) => { /* Enter pressed: send the message */ }
        Event::Insets(i) => { /* pad by i.top, keep the field above i.ime_bottom */ }
        Event::Config(c) => { /* c.night_mode → dark theme */ }
        Event::Back => { /* only after set_back_handled(true) */ }
        Event::Key { .. } => {}
    }
}

// App-side changes:
let seq = oxydroid::set_state("", 0..0);            // e.g. clear after sending
oxydroid::set_selection(&text, anchor, head);       // after a tap or drag
```

**Manifest:** add `uiMode|fontScale|density` (plus the usual `orientation|screenSize|keyboardHidden|keyboard`)
to your activity's `configChanges`, so theme or font changes don't recreate the activity.
With cargo-apk:

```toml
[package.metadata.android.application.activity]
config_changes = "orientation|screenSize|screenLayout|smallestScreenSize|keyboardHidden|keyboard|uiMode|fontScale|density"
```

## Things the app must do (by design)

- **Draw the text and hit-test touches.** Only your toolkit knows its layout.
- **Arrow keys are visual on Android** (in Arabic, Right moves backwards). By default the mirror moves one
  character logically. Call `set_arrows_to_app(true)` to receive arrows as `Event::Key` and move with
  your bidi-aware layout.
- **Render glyphs.** egui's default fonts have no Arabic and few emoji. Load `/system/fonts` fallbacks
  (and enable epaint's `color_fonts` for color emoji).

## Limitations

- One `NativeActivity` per process (normal for Rust apps). Re-calling `init` after the activity was
  recreated re-attaches to the new one.
- Back: Android 13+ only. Earlier versions deliver back as a key event through the input queue.
- Toolkit adapters: egui only so far. Others can use the toolkit-neutral API (`poll` /
  `set_state` / `set_selection`) and `oxydroid::text::diff`.

## Building the dex (maintainers only)

```bash
./build-dex.sh   # needs JDK 17 and $ANDROID_HOME with platforms;android-36 + build-tools
```
CI rebuilds it and fails if the committed `classes.dex` is stale (the build is reproducible).

## Prior art and credits

- [accesskit_android](https://github.com/AccessKit/accesskit): the prebuilt-dex + `RegisterNatives`
  pattern this crate follows.
- [android-activity](https://github.com/rust-mobile/android-activity), [winit](https://github.com/rust-windowing/winit):
  the ground this stands on.
- Slint has a similar, more complete helper for Slint only (GPL/commercial). oxydroid is written
  independently from the Android documentation.

## License

MIT or Apache-2.0, at your option.
