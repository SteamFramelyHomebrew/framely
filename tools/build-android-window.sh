#!/usr/bin/env bash
# Reproduce the checked-in tiny DEX helper; normal builds need no JDK/download.
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
work="$base/target/android-window-build"
mkdir -p "$work"
url=https://dl.google.com/dl/android/maven2/com/android/tools/r8/8.3.37/r8-8.3.37.jar
curl -fsSL "$url" -o "$work/r8.jar"
printf '%s  %s\n' 59753e70a74f918389cc87f1b7d66b5c0862932559167425708ded159e3de439 "$work/r8.jar" | sha256sum -c -
javac --release 8 -Xlint:-options -d "$work" "$base/native/android/FramelyWindow.java"
java -cp "$work/r8.jar" com.android.tools.r8.D8 --release --min-api 30 --output "$work" "$work/FramelyWindow.class"
cp "$work/classes.dex" "$base/native/android/framely-window.dex"
