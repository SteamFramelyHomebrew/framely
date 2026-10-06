#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cef=$(realpath "$1")
out=$(realpath -m "${2:-$base/target/native}")
mkdir -p "$out"
(cd "$base" && node tools/build-scroll-gestures.mjs "$out")
openvr="$base/native/vendor/openvr"
g++ -std=c++17 -O2 -I"$cef" -I"$openvr" -I"$base/native" -I"$out" "$base/native/host.cpp" -L"$cef/Release" -L"$openvr/lib/linuxarm64" -lcef -lopenvr_api -lGL -lX11 -pthread -Wl,-rpath,'$ORIGIN:$ORIGIN/../openvr' -o "$out/framely-vr"

# Independent of CEF; both binaries ship in the core/update archive.
g++ -std=c++17 -O2 -I"$openvr" "$base/native/gamepad.cpp" -L"$openvr/lib/linuxarm64" -lopenvr_api -pthread -Wl,-rpath,'$ORIGIN' -o "$out/framely-gamepad"
gcc -shared -fPIC -nostdlib -fno-stack-protector -Wl,--hash-style=both "$base/native/gamepad_grab.c" -o "$out/libframely-gamepad-grab.so"
