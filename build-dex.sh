#!/usr/bin/env bash
# Maintainer-only: rebuild classes.dex from java/. Users of the crate never run this;
# the result is committed and embedded with include_bytes!.
# Needs: JDK 17 (javac on PATH or JAVA_HOME), ANDROID_HOME with platforms;android-36 + build-tools.
set -euo pipefail
cd "$(dirname "$0")"
[ -f ../scripts/env.sh ] && source ../scripts/env.sh       # local dev setup (JDK 17), optional
[ -n "${JAVA_HOME:-}" ] && export PATH="$JAVA_HOME/bin:$PATH"
SDK="${ANDROID_HOME:-$HOME/Android/Sdk}"
JAR="$SDK/platforms/android-36/android.jar"
D8="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)/d8"

out=$(mktemp -d)
javac -source 8 -target 8 -bootclasspath "$JAR" -Xlint:all -Xlint:-options \
      -d "$out" $(find java -name '*.java')
"$D8" --min-api 26 --lib "$JAR" --output . $(find "$out" -name '*.class')
rm -rf "$out"
ls -l classes.dex                                          # verify: ~18 KB
