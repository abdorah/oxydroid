# Changelog

## Unreleased

- Keyboard input through a real `InputConnection` with a Java-side text mirror and whole-state
  snapshots (`Event::Text`). App writes win, via sequence numbers.
- Selection keeps its direction (`anchor` / `head`).
- Keyboard cursor keys, Select mode (Shift), Select all / Cut / Copy / Paste.
- Field kinds and Enter actions (`show(FieldKind, EnterAction)`).
- Floating Cut/Copy/Paste toolbar (`show_toolbar`).
- Insets with keyboard height, opt-in edge-to-edge.
- Back via `OnBackInvokedCallback` on Android 13+ (`set_back_handled`).
- Configuration events: dark mode, font scale, density.
- Re-`init` after activity recreation re-attaches. Callbacks never unwind into Java.
