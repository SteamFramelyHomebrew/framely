#!/usr/bin/env bash
# Reproduce the embedded Android 11 APK-update helper. No runtime JDK needed.
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
work="$base/target/android-updates-build"
mkdir -p "$work/classes" "$work/dex"
fetch() {
    local url=$1 file=$2 digest=$3
    if [[ ! -f $file ]] || ! printf '%s  %s\n' "$digest" "$file" | sha256sum -c - >/dev/null 2>&1; then
        curl -fSL --retry 3 "$url" -o "$file"
    fi
    printf '%s  %s\n' "$digest" "$file" | sha256sum -c -
}
fetch https://dl.google.com/android/repository/platform-30_r03.zip "$work/platform.zip" f3f5b75744dbf6ee6ed3e8174a71e513bfee502d0bc3463ea97e517bff68d84e
fetch https://dl.google.com/dl/android/maven2/com/android/tools/r8/8.3.37/r8-8.3.37.jar "$work/r8.jar" 59753e70a74f918389cc87f1b7d66b5c0862932559167425708ded159e3de439
unzip -p "$work/platform.zip" android-11/android.jar > "$work/android.jar"
javac --release 8 -Xlint:-options -cp "$work/android.jar" -d "$work/classes" "$base/native/android/FramelyUpdates.java"
java -cp "$work/r8.jar" com.android.tools.r8.D8 --release --min-api 30 --lib "$work/android.jar" --output "$work/dex" "$work"/classes/FramelyUpdates*.class
cp "$work/dex/classes.dex" "$base/native/android/framely-updates.dex"
