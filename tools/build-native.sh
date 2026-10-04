#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cef=$(realpath "$1")
out=$(realpath -m "${2:-$base/target/native}")
mkdir -p "$out"
openvr="$base/native/vendor/openvr"
g++ -std=c++17 -O2 -I"$cef" -I"$openvr" -I"$base/native" "$base/native/host.cpp" -L"$cef/Release" -L"$openvr/lib/linuxarm64" -lcef -lopenvr_api -lGL -lX11 -pthread -Wl,-rpath,'$ORIGIN:$ORIGIN/../openvr' -o "$out/framely-vr"
