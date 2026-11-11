#!/usr/bin/env bash
# Run the demo's scripted on-device test and write a report with screenshots.
# Usage: demo/autotest.sh [ip:connect_port] [report-dir]
#   No device given: uses the single connected device (adb connect <ip:port> first). The script itself
#   launches the demo with --es oxytest 1; don't pass that.
# Needs: the phone UNLOCKED (we never touch the lock screen). Builds nothing: build the demo first.
# Side effect: the test's "cut" step puts text on the phone's clipboard.
set -euo pipefail
A="$HOME/Android/Sdk/platform-tools/adb"
DEV="${1:-$("$A" devices | awk 'NR > 1 && $2 == "device" {print $1}' | head -1)}"
if [[ -z "$DEV" || "$DEV" == -* || "$DEV" != *:* && "$DEV" != *[0-9A-Z]* ]]; then
  echo "usage: $0 [ip:connect_port] [report-dir]   (no device connected? adb connect <ip:port> first)"; exit 2
fi
echo "device: $DEV"
repo="$(cd "$(dirname "$0")/.." && pwd)"   # this repo
OUT="${2:-$repo/docs/test-reports/$(date +%F)-a20}"
APK="$repo/demo/target/debug/apk/oxydroid-demo.apk"
PKG=dev.kotbyte.oxydroid.demo
mkdir -p "$OUT"

"$A" connect "$DEV" >/dev/null
if "$A" -s "$DEV" shell dumpsys window | grep -q "isKeyguardShowing=true"; then
  echo "The phone is locked: unlock it and run again."; exit 3
fi
"$A" -s "$DEV" install -r "$APK" | tail -1
"$A" -s "$DEV" logcat -c
"$A" -s "$DEV" shell am start -S -n "$PKG/android.app.NativeActivity" --es oxytest 1 >/dev/null

# Follow the log; take a screenshot at each "oxytest: shot NAME" cue; stop at DONE (max 90 s).
declare -A taken=()
for _ in $(seq 300); do
  log=$("$A" -s "$DEV" logcat -d -v time oxydemo:I '*:S')
  for name in $(grep -oE 'oxytest: shot [A-Za-z0-9_-]+' <<<"$log" | awk '{print $3}'); do
    if [[ -z "${taken[$name]:-}" ]]; then
      "$A" -s "$DEV" exec-out screencap -p > "$OUT/$name.png"
      taken[$name]=1
      echo "screenshot $name"
    fi
  done
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
