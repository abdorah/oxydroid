#!/usr/bin/env bash
# Run the demo's scripted on-device test and write a report with screenshots.
# Usage: demo/autotest.sh [serial] [report-dir]
#   serial: emulator-5554, or ip:port for a phone over Wi-Fi. None given: the single connected
#   device. The script itself launches the demo with --es oxytest 1; don't pass that.
# Needs: the device UNLOCKED (we never touch a phone's lock screen). Builds nothing: build first.
# Side effect: the test's "cut" step puts text on the device's clipboard.
set -euo pipefail
A="$HOME/Android/Sdk/platform-tools/adb"
DEV="${1:-$("$A" devices | awk 'NR > 1 && $2 == "device" {print $1}' | head -1)}"
if [[ -z "$DEV" || "$DEV" == -* ]]; then
  echo "usage: $0 [serial] [report-dir]   (nothing connected? start the emulator or adb connect <ip:port>)"; exit 2
fi
[[ "$DEV" == *:* ]] && "$A" connect "$DEV" >/dev/null
echo "device: $DEV"
model=$("$A" -s "$DEV" shell getprop ro.product.model | tr -d '\r')
api=$("$A" -s "$DEV" shell getprop ro.build.version.sdk | tr -d '\r')
slug=$(tr 'A-Z ' 'a-z-' <<<"$model" | tr -cd 'a-z0-9-')-api$api   # e.g. sm-a205f-api30
repo="$(cd "$(dirname "$0")/.." && pwd)"   # this repo
OUT="${2:-$repo/docs/test-reports/$(date +%F)-$slug}"
APK="$repo/demo/target/debug/apk/oxydroid-demo.apk"
PKG=dev.kotbyte.oxydroid.demo
mkdir -p "$OUT"

if "$A" -s "$DEV" shell dumpsys window | grep -q "isKeyguardShowing=true"; then
  if [[ "$DEV" == emulator-* ]]; then
    "$A" -s "$DEV" shell wm dismiss-keyguard   # our own throwaway emulator: no PIN
  else
    echo "The phone is locked: unlock it and run again."; exit 3
  fi
fi
"$A" -s "$DEV" install -r "$APK" | tail -1
"$A" -s "$DEV" logcat -c
"$A" -s "$DEV" shell am start -S -n "$PKG/android.app.NativeActivity" --es oxytest 1 >/dev/null

# Follow the log and act on the demo's cues; stop at DONE (max ~120 s).
#   oxytest: shot NAME           -> screenshot
#   oxytest: input#N ARGS...     -> `adb shell input ARGS` once (real key presses and touches must
#                                   come from outside the app). ARGS is restricted to safe tokens.
declare -A taken=() done_input=()
for _ in $(seq 400); do
  log=$("$A" -s "$DEV" logcat -d -v time oxydemo:I '*:S')
  for name in $(grep -oE 'oxytest: shot [A-Za-z0-9_-]+' <<<"$log" | awk '{print $3}'); do
    if [[ -z "${taken[$name]:-}" ]]; then
      "$A" -s "$DEV" exec-out screencap -p > "$OUT/$name.png"
      taken[$name]=1
      echo "screenshot $name"
    fi
  done
  while IFS= read -r cue; do
    n=$(sed -E 's/^input#([0-9]+) .*/\1/' <<<"$cue")
    args=$(sed -E 's/^input#[0-9]+ //' <<<"$cue")
    if [[ -z "${done_input[$n]:-}" && "$args" =~ ^(keyevent|tap|swipe|motionevent)(\ [A-Z0-9_]+)+$ ]]; then
      done_input[$n]=1
      # shellcheck disable=SC2086  # deliberate word splitting of validated tokens
      "$A" -s "$DEV" shell input $args
      echo "input $args"
    fi
  done < <(grep -oE 'oxytest: input#[0-9]+ [A-Za-z0-9_ ]+' <<<"$log" | sed 's/^oxytest: //')
  grep -q 'oxytest: DONE' <<<"$log" && break
  sleep 0.3
done

"$A" -s "$DEV" logcat -d -v time oxydemo:D AndroidRuntime:E '*:S' > "$OUT/log.txt"
grep -E 'oxytest: (PASS|FAIL|DONE)|race test|latency' "$OUT/log.txt" | sed -E 's/^.*oxydroid_demo(::autotest)?: //' > "$OUT/results.txt"
cat "$OUT/results.txt"
model=$("$A" -s "$DEV" shell getprop ro.product.model | tr -d '\r')
android=$("$A" -s "$DEV" shell getprop ro.build.version.release | tr -d '\r')
ime=$("$A" -s "$DEV" shell settings get secure default_input_method | tr -d '\r')
commit=$(git -C "$repo" rev-parse --short HEAD)
{
  echo "# On-device test report: $(date '+%F %H:%M')"
  echo
  echo "Device: $model, Android $android. Keyboard: \`$ime\`. oxydroid commit \`$commit\` (+ working tree)."
  echo "Run: \`demo/autotest.sh\`, which launches the demo with \`--es oxytest 1\`; steps in \`demo/src/autotest.rs\`."
  echo "Each step calls the real \`InputConnection\` the way a keyboard app does, then checks the egui field."
  echo
  echo "## Results"
  echo '```'
  cat "$OUT/results.txt"
  echo '```'
  echo
  echo "## Screenshots"
  for f in "$OUT"/*.png; do n=$(basename "$f" .png); echo; echo "### $n"; echo; echo "![$n]($n.png)"; done
  echo
  echo "Full log: [log.txt](log.txt)"
} > "$OUT/REPORT.md"
echo "Report: $OUT/REPORT.md"
