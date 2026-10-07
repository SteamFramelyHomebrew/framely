#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cef=$(realpath "$1")
out=$(realpath -m "${2:-$base/target/native}")
mkdir -p "$out"
(cd "$base" && node tools/build-scroll-gestures.mjs "$out")
openvr="$base/native/vendor/openvr"
g++ -std=c++17 -O2 -I"$cef" -I"$openvr" -I"$base/native" -I"$out" "$base/native/host.cpp" -L"$cef/Release" -L"$openvr/lib/linuxarm64" -lcef -lopenvr_api -lGL -lX11 -ldl -pthread -Wl,-rpath,'$ORIGIN:$ORIGIN/../openvr' -o "$out/framely-vr"

# Independent of CEF; both binaries ship in the core/update archive.
g++ -std=c++17 -O2 -I"$openvr" "$base/native/gamepad.cpp" -L"$openvr/lib/linuxarm64" -lopenvr_api -ldl -pthread -Wl,-rpath,'$ORIGIN' -o "$out/framely-gamepad"
gcc -shared -fPIC -nostdlib -fno-stack-protector -mno-outline-atomics -Wl,--hash-style=both "$base/native/gamepad_grab.c" -o "$out/libframely-gamepad-grab.so"

# Android GLES layer uses loader-supplied symbols; no host glibc dependency.
gcc -shared -fPIC -nostdlib -fno-stack-protector -fvisibility=hidden -Wl,-z,defs,--hash-style=both,-soname,libFramelyFBOCompat.so "$base/native/fbo_compat.c" -o "$out/libFramelyFBOCompat.so"
