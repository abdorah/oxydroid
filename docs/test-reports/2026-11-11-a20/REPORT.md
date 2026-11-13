# On-device test report: 2026-11-11 15:57

Device: SM-A205F, Android 11. Keyboard: `com.samsung.android.honeyboard/.service.HoneyBoardService`. oxydroid commit `2e46f0b` (+ working tree).
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
oxytest: PASS password field gets text: 6 chars
race test: PASS (stale dropped: 2, RACE leaked: false, echo received: true)
latency: 1000 chars: 567 us per snapshot
latency: 10000 chars: 1610 us per snapshot
latency: 100000 chars: 4324 us per snapshot
oxytest: PASS race test (Q1): see race line
oxytest: DONE passed=13 failed=0
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
