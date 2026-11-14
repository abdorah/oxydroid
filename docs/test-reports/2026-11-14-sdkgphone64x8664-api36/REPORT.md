# On-device test report: 2026-11-14 12:05

## Summary

**14/14 checks pass** on the Android 16 emulator (API 36, x86_64, Google Play image) with **Gboard**,
the first run outside Samsung's keyboard. New since the A20 report: `getExtractedText` returns the
text and selection.

Found by this run (not visible on the Android 11 phone):
- **Status bar icons are near-invisible** (white on the light UI): Android 16 forces edge-to-edge,
  and nothing tells the system the content behind the bars is light. Needs light system-bar icons.
- **The floating toolbar doesn't show** in `08-selection-handles-toolbar` (it does on the A20).
  To investigate: timing vs. an API 36 change.

Confirmed: typing with accents, Arabic, color emoji; selection and handles; Gboard shows its "Next"
key for `EnterAction::Next`; password field; race test; insets keep content below the status bar
under forced edge-to-edge.


Device: sdk_gphone64_x86_64, Android 16. Keyboard: `com.google.android.inputmethod.latin/com.android.inputmethod.latin.LatinIME`. oxydroid commit `39f0453` (+ working tree).
Run: `demo/autotest.sh`, which launches the demo with `--es oxytest 1`; steps in `demo/src/autotest.rs`.
Each step calls the real `InputConnection` the way a keyboard app does, then checks the egui field.

## Results
```
oxytest: PASS focus opens a keyboard session: focused=true
oxytest: PASS compose (é): "hé" (expected "hé")
oxytest: PASS commit replaces the composing word: "héllo" (expected "héllo")
oxytest: PASS Arabic + emoji: "héllo عربي 😀" (expected "héllo عربي 😀")
oxytest: PASS delete one code point (emoji = 2 UTF-16 units): "héllo عربي " (expected "héllo عربي ")
oxytest: PASS keyboard selection reaches egui: Some((0, 5))
oxytest: PASS typing replaces the selection: "HELLO عربي " (expected "HELLO عربي ")
oxytest: PASS select all + cut: "" (expected "")
oxytest: PASS paste (system clipboard): "HELLO عربي " (expected "HELLO عربي ")
oxytest: PASS app write reached the keyboard's mirror: "app wrote!" (expected "app wrote!")
oxytest: PASS DEL key event: "app wrote" (expected "app wrote")
latency: extracted=app wrote sel=9..9
oxytest: PASS getExtractedText returns the text and selection: Some("app wrote sel=9..9")
oxytest: PASS password field gets text: 6 chars
race test: PASS (stale dropped: 2, RACE leaked: false, echo received: true)
latency: 1000 chars: 179 us per snapshot
latency: 10000 chars: 231 us per snapshot
latency: 100000 chars: 3754 us per snapshot
oxytest: PASS race test (Q1): see race line
oxytest: DONE passed=14 failed=0
```

## Screenshots

### 02-composing-underlined

![02-composing-underlined](02-composing-underlined.png)

### 05-arabic-emoji

![05-arabic-emoji](05-arabic-emoji.png)

### 08-selection-handles-toolbar

![08-selection-handles-toolbar](08-selection-handles-toolbar.png)

### 17-password

![17-password](17-password.png)

### 99-results

![99-results](99-results.png)

Full log: [log.txt](log.txt)
