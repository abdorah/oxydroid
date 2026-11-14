#!/usr/bin/env bash
# Android 16 emulator for automated tests (headless; KVM-accelerated in WSL).
# Usage:
#   demo/emulator.sh setup     install emulator + system image, create the AVD (once, ~2 GB)
#   demo/emulator.sh start     boot headless; waits until Android is ready; prints the adb serial
#   demo/emulator.sh window    boot with a window (WSLg), to watch it
#   demo/emulator.sh stop
# Needs: /dev/kvm readable/writable (user in the kvm group).
set -euo pipefail
SDK="$HOME/Android/Sdk"
SM="$SDK/cmdline-tools/latest/bin/sdkmanager"
AVDM="$SDK/cmdline-tools/latest/bin/avdmanager"
EMU="$SDK/emulator/emulator"
A="$SDK/platform-tools/adb"
IMAGE="system-images;android-36;google_apis_playstore;x86_64"   # Google Play image: has Gboard
AVD=oxy-pixel-36
PORT=5554                                                       # adb serial: emulator-5554
here="$(cd "$(dirname "$0")" && pwd)"
[ -f "$here/../../scripts/env.sh" ] && source "$here/../../scripts/env.sh"   # local JDK 17, optional
[ -n "${JAVA_HOME:-}" ] && export PATH="$JAVA_HOME/bin:$PATH"
export ANDROID_HOME="$SDK" ANDROID_SDK_ROOT="$SDK"

case "${1:-}" in
  setup)
    [ -r /dev/kvm ] && [ -w /dev/kvm ] || { echo "no access to /dev/kvm (kvm group?)"; exit 3; }
    yes | "$SM" --sdk_root="$SDK" --licenses >/dev/null || true
    "$SM" --sdk_root="$SDK" "emulator" "$IMAGE"
    if ! "$AVDM" list avd -c | grep -qx "$AVD"; then
      echo no | "$AVDM" create avd -n "$AVD" -k "$IMAGE" -d pixel_7 --force
    fi
    # More RAM than the default makes Gboard and the first boot much smoother.
    cfg="$HOME/.android/avd/$AVD.avd/config.ini"
    sed -i -e 's/^hw.ramSize=.*/hw.ramSize=4096/' -e 's/^hw.keyboard=.*/hw.keyboard=no/' "$cfg"
    grep -q '^hw.ramSize=' "$cfg" || echo 'hw.ramSize=4096' >> "$cfg"
    grep -q '^hw.keyboard=' "$cfg" || echo 'hw.keyboard=no' >> "$cfg"   # show the soft keyboard
    "$EMU" -accel-check
    echo "AVD $AVD ready"
    ;;
  start|window)
    if "$A" devices | grep -q "^emulator-$PORT"; then echo "already running: emulator-$PORT"; exit 0; fi
    flags=(-avd "$AVD" -port "$PORT" -no-snapshot-save -no-boot-anim -gpu swiftshader_indirect -no-audio)
    [ "$1" = start ] && flags+=(-no-window)
    nohup "$EMU" "${flags[@]}" > /tmp/oxy-emulator.log 2>&1 &
    "$A" -s "emulator-$PORT" wait-for-device
    for _ in $(seq 180); do
      [ "$("$A" -s "emulator-$PORT" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ] && break
      sleep 1
    done
    "$A" -s "emulator-$PORT" shell input keyevent KEYCODE_WAKEUP
    "$A" -s "emulator-$PORT" shell wm dismiss-keyguard
    echo "booted: emulator-$PORT (Android $("$A" -s "emulator-$PORT" shell getprop ro.build.version.release | tr -d '\r'))"
    ;;
  stop)
    "$A" -s "emulator-$PORT" emu kill || true
    ;;
  *)
    sed -n '2,8p' "$0"; exit 2 ;;
esac
