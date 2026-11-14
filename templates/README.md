# Starter templates

Android apps in Rust with a soft keyboard that actually works: accents, any script, emoji,
suggestions, swipe and voice typing. `NativeActivity`, no Java, no Gradle.

| Template | Toolkit | What you get |
|---|---|---|
| `egui-app` | egui / eframe 0.37 | A notes screen: one `TextEdit` (Enter = Send), dark mode and status-bar icons following the system, native selection handles + toolbar, system fonts for Arabic and color emoji. |
| `bevy-app` | Bevy 0.20 | A UI with one Bevy `TextInput`, padded below the status bar. `OxydroidPlugin` from [`oxydroid-bevy`](../bevy) does the rest. |

## Create an app

```bash
templates/new-app.sh egui my-notes          # or: bevy my-game
cd my-notes
cargo apk run --lib                          # builds, installs and starts it on the connected device
```

`new-app.sh` renames the crate, the Android package (`dev.example.<name>`: change it to your own
before publishing) and the app label, and points the oxydroid dependencies at this checkout.
Add `--git` to depend on oxydroid from GitHub instead.

Both templates build for `aarch64` (phones) and `x86_64` (the emulator).

## Prerequisites

- The Android SDK + NDK and `cargo-apk` (see the main README; the repo's
  `scripts/00-setup-android-toolchain.sh` does it all on Ubuntu/WSL).
- `rustup target add aarch64-linux-android x86_64-linux-android`.
- A device with USB/Wi-Fi debugging, or an emulator (`demo/emulator.sh setup && demo/emulator.sh start`).

## Checking the keyboard path

The Bevy template has an optional self-test (`src/selftest.rs`, delete it if you don't want it):
```bash
adb shell am start -S -n dev.example.mygame/android.app.NativeActivity --es oxytest 1
adb logcat -d | grep oxytest
```
It focuses the field and drives the real `InputConnection` the way a keyboard app does:
commit, composing text, delete, an arrow key followed by typing, a keyboard-side selection
deleted as a whole, and Shift+arrows (Samsung's Select mode). 8/8 on the Android 16 emulator
(Gboard); the first 4 steps also pass on a Galaxy A20 (Android 11, Samsung Keyboard).
