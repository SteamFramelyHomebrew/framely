#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$base"
cef=$(realpath "$1")
export CARGO_HOME=${CARGO_HOME:-/tmp/framely-cargo}
export npm_config_cache=${npm_config_cache:-/tmp/framely-npm-cache}
cargo build --release --locked
npm ci --no-audit --no-fund
npm run build
bash tools/build-native.sh "$cef"
binary=${CARGO_TARGET_DIR:-target}/release/framely
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
build_id="$version-$(find "$binary" target/native ui/dist sdk templates/plugin examples/showcase packaging tools docs README.md README.en.md LICENSE -type f ! -path '*/__pycache__/*' -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -c1-12)"
stage="$base/release/framely-$build_id"
[[ ! -e $stage ]] || { echo 'Release already exists.' >&2; exit 1; }
mkdir -p "$stage"/{bin,lib/cef,lib/openvr,share/ui,share/licenses,tools}
cp "$binary" "$stage/bin/"
cp target/native/framely-vr "$stage/lib/cef/"
cp -a "$cef/Release/." "$stage/lib/cef/"
cp -a "$cef/Resources/." "$stage/lib/cef/"
rm -f "$stage/lib/cef/framely-probe" "$stage/lib/cef/framely-browser-probe"
# Remove distribution debug information; browser functionality is unchanged.
strip --strip-debug "$stage/lib/cef/libcef.so"
cp native/vendor/openvr/lib/linuxarm64/libopenvr_api.so "$stage/lib/openvr/"
cp -a ui/dist/. "$stage/share/ui/"
cp "$cef/LICENSE.txt" "$stage/share/licenses/cef.txt"
cp native/vendor/openvr/LICENSE "$stage/share/licenses/openvr.txt"
[[ ! -f $cef/CREDITS.html ]] || cp "$cef/CREDITS.html" "$stage/share/licenses/cef-credits.html"
cp native/vendor/json.hpp "$stage/share/licenses/nlohmann-json.hpp"
cp packaging/*.sh "$stage/"
cp README.md README.en.md "$stage/"
cp LICENSE "$stage/LICENSE"
mkdir -p "$stage/tools/source/examples" "$stage/tools/source/tools" "$stage/tools/source/ui"
cp package.json package-lock.json tsconfig.json LICENSE "$stage/tools/source/"
cp -a sdk "$stage/tools/source/"
mkdir -p "$stage/tools/source/templates"
cp -a templates/plugin "$stage/tools/source/templates/"
cp -a examples/showcase "$stage/tools/source/examples/"
cp -a ui/src ui/locales ui/index.html ui/package.json "$stage/tools/source/ui/"
cp tools/build-ui.mjs tools/plugin-dev.mjs "$stage/tools/source/tools/"
cp tools/extract-release.py tools/bootstrap.py "$stage/tools/"
find "$stage/tools/source" -type d -name __pycache__ -prune -exec rm -rf -- {} +
cp -a docs "$stage/share/"
printf '%s\n' "$build_id" > "$stage/VERSION"
chmod 755 "$stage"/*.sh "$stage/bin/"* "$stage/lib/cef/framely-vr"
(cd "$stage" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)
tar -C "$base/release" -czf "$base/release/framely-$build_id-linux-arm64.tar.gz" "framely-$build_id"
(cd "$base/release" && sha256sum "framely-$build_id-linux-arm64.tar.gz" > "framely-$build_id-linux-arm64.tar.gz.sha256")
(cd "$base/release" && cp "framely-$build_id-linux-arm64.tar.gz.sha256" SHA256SUMS)
echo "Package: release/framely-$build_id-linux-arm64.tar.gz"
