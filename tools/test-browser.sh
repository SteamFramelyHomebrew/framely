#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cef=$(realpath "$1")
cd "$base"
export CARGO_HOME=${CARGO_HOME:-/tmp/framely-cargo}
export npm_config_cache=${npm_config_cache:-/tmp/framely-npm-cache}
bash tools/test-native.sh
npm run build
node --input-type=module -e "import {build} from 'esbuild';await build({entryPoints:['tests/keyboard_fixture.tsx'],bundle:true,outfile:'target/keyboard-fixture.js',define:{'process.env.NODE_ENV':'\"production\"'}});"
# ICU must be adjacent to the executable before Chromium process startup.
cp -a "$cef/Resources/." "$cef/Release/"
node tools/build-scroll-gestures.mjs "target/native-scroll-probe"
g++ -std=c++17 -O2 -I"target/native-scroll-probe" -I"$cef" tests/browser_probe.cpp -L"$cef/Release" -lcef -pthread -Wl,-rpath,'$ORIGIN' -o "$cef/Release/framely-browser-probe"
FRAMELY_CEF_ROOT="$cef" FRAMELY_BROWSER_PROBE="$cef/Release/framely-browser-probe" cargo test --locked cef_manager_plugin_bridge_integration -- --ignored --nocapture
