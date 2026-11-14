# oxydroid

**Android platform services for Rust UI apps on `NativeActivity`: real keyboard input,
clipboard, insets, back and dark mode. No JDK, no Gradle.**

> Status: **experimental** (0.0.x). Tested on one device: Galaxy A20, Android 11, Samsung Keyboard.
> Automated on-device runs: [Galaxy A20, Android 11, Samsung Keyboard: 17/17](docs/test-reports/2026-11-14-sm-a205f-api30/REPORT.md) ·
> [Android 16 emulator, Gboard: 18/18](docs/test-reports/2026-11-14-sdkgphone64x8664-api36/REPORT.md).

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
| App-side edits ("clear on send") win over in-flight keyboard edits | ✅ on-device race self-test passes |
| Caret/selection from the app (tap, drag, long-press) and from the keyboard (cursor keys, Select mode) | ✅ |
| Keyboard Select all / Cut / Copy / Paste, cross-app clipboard | ✅ |
| Field kinds (password, number, email, multiline) + Enter action (Done/Send/Go/…) | ✅ |
| Floating Cut/Copy/Paste toolbar | ✅ (fixed: was invisible before 2026-11-11; screenshot in the test report) |
| Light/dark system-bar icons (`set_light_system_bars`; egui adapter follows the theme) | ✅ Android 16 emulator |
| Insets (system bars, cutout, keyboard height) + opt-in edge-to-edge | ✅ the keyboard height follows its slide animation frame by frame when edge-to-edge is on (forced on Android 15+; `set_edge_to_edge(true)` before that) |
| Dark mode / font scale / density, accent color (Material You on 12+), animations on/off | ✅ |
| `CursorAnchorInfo` / `requestCursorUpdates`: caret position on screen (Gboard floating cursor, handwriting) | 🧪 new, **not yet device-verified** |
| `getExtractedText` / `updateExtractedText`: fullscreen keyboards, extracted-text monitors | ✅ Android 16 + Gboard |
| Back via `OnBackInvokedCallback` (Android 13+) | ✅ Android 16 emulator: handled → `Event::Back`; unhandled → the system closes the app |
| **egui adapter** (feature `egui`): plain `egui::TextEdit` with real keyboard input | ✅ sign-in form + chat demo |
| Native selection handles (platform drawables) + magnifier while dragging | ✅ Android 16 emulator, dragged with real touches |

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
the text or caret, shows the platform's selection handles (with the magnifier), shows the floating toolbar over a selection, and
suppresses egui-winit's own IME requests (on Android those hide/show the keyboard). Composing text
is underlined. See [`demo/`](demo) for a sign-in form and a chat box.

**Cost per keystroke** (snapshot: Java → JNI → Rust, A20): ~0.1 ms at 1k chars, ~0.5 ms at 10k,
~3.5 ms at 100k. Mirroring the whole text is fine for normal fields and documents.

## Toolkits and starter templates

| Toolkit | Integration | Starter |
|---|---|---|
| **egui** 0.37 | `oxydroid::egui::Ime` (feature `egui`): plain `TextEdit`s | `templates/egui-app` |
| **Bevy** 0.20 | [`oxydroid-bevy`](bevy): `OxydroidPlugin`, plain `TextInput` widgets | `templates/bevy-app` |
| anything winit-style | `oxydroid::winit_ime::Translator`: snapshots → `Preedit` / `Commit` / `DeleteSurrounding` | |
| **iced** 0.14 | ❌ not possible today: `iced_winit` builds its event loop without the `AndroidApp` winit needs on Android, so iced can't start there, and upstream doesn't take mobile changes. A fork would need that, plus polling oxydroid in its event loop through `Translator`. | |

```bash
templates/new-app.sh egui my-notes   # or: bevy my-game; add --git to depend on GitHub
cd my-notes && cargo apk run --lib
```

Bevy's `TextInput` already speaks winit's IME vocabulary, so the plugin only translates: it opens
the keyboard when Bevy enables IME for a focused field, turns snapshots into `Ime` and
`KeyboardInput` messages, and tells the keyboard when Bevy's caret or text changes (taps, arrow
keys, app writes). Bevy's Parley layout moves the caret visually, right-to-left text included.
Verified on the Android 16 emulator with Gboard and on a Galaxy A20 (Android 11) with Samsung
Keyboard: commit with accents, composing, delete, and an arrow key followed by typing (4/4 each). See [`templates/README.md`](templates/README.md).

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
- Toolkit adapters: egui (`oxydroid::egui`), and `oxydroid::winit_ime::Translator` for toolkits that
  consume winit-style IME events (`Preedit` / `Commit` / `DeleteSurrounding`). Anything else can use
  `poll` / `set_text` / `set_selection` directly.

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
