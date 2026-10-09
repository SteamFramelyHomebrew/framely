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
bash tools/build-media.sh
binary=${CARGO_TARGET_DIR:-target}/release/framely
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
build_id="$version-$(find "$binary" target/native media/bin ui/dist sdk templates/plugin examples/showcase packaging tools docs assets/branding assets/search README.md README.zh-CN.md LICENSE -type f ! -path '*/__pycache__/*' -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -c1-12)"
stage="$base/release/framely-$build_id"
[[ ! -e $stage ]] || { echo 'Release already exists.' >&2; exit 1; }
mkdir -p "$stage"/{bin,lib/cef,lib/openvr,lib/media,share/ui,share/licenses,tools}
cp "$binary" "$stage/bin/"
cp media/bin/framely-panel-grab "$stage/bin/"
cp media/bin/framely-capture media/bin/mediamtx media/bin/uxplay media/bin/framely-receiver media/bin/libgstframely.so "$stage/lib/media/"
cp -a media/bin/upnp "$stage/lib/media/"
cp media/bin/LICENSE "$stage/share/licenses/mediamtx.txt"
cp media/bin/LICENSE.uxplay "$stage/share/licenses/uxplay.txt"
cp media/bin/LICENSE.llhttp "$stage/share/licenses/uxplay-llhttp.txt"
cp media/capture/LICENSE.framecorder "$stage/share/licenses/panel-capture.txt"
cp media/THIRD_PARTY_NOTICES.md "$stage/share/licenses/media-notices.md"
mkdir -p "$stage/share/source/media/capture" "$stage/share/source/tools" "$stage/share/source/native/vendor"
cp -a media/native media/upnp media/patch-uxplay.py media/dependencies.json "$stage/share/source/media/"
cp -a media/capture/src media/capture/shaders media/capture/Cargo.toml media/capture/Cargo.lock media/capture/build.rs media/capture/LICENSE.framecorder "$stage/share/source/media/capture/"
cp tools/build-media.sh tools/fetch-media.py "$stage/share/source/tools/"
cp native/vendor/json.hpp "$stage/share/source/native/vendor/"
tar -czf "$stage/share/source/media/uxplay.tar.gz" -C media/source UxPlay-1.73.7

cp target/native/framely-vr "$stage/lib/cef/"
cp -a "$cef/Release/." "$stage/lib/cef/"
cp -a "$cef/Resources/." "$stage/lib/cef/"
rm -f "$stage/lib/cef/framely-probe" "$stage/lib/cef/framely-browser-probe"
# Remove distribution debug information; browser functionality is unchanged.
strip --strip-debug "$stage/lib/cef/libcef.so"
cp native/vendor/openvr/lib/linuxarm64/libopenvr_api.so "$stage/lib/openvr/"
cp target/native/framely-gamepad target/native/libframely-gamepad-grab.so target/native/libFramelyFBOCompat.so "$stage/lib/openvr/"
cp -a ui/dist/. "$stage/share/ui/"
cp -a assets/search "$stage/share/"
cp assets/search/COPYING "$stage/share/licenses/ipadic.txt"
cp assets/search/NOTICE "$stage/share/licenses/ipadic-notice.txt"
cp -a native/input "$stage/share/"
cp "$cef/LICENSE.txt" "$stage/share/licenses/cef.txt"
cp native/vendor/openvr/LICENSE "$stage/share/licenses/openvr.txt"
cp node_modules/@tabler/icons-react/LICENSE "$stage/share/licenses/tabler-icons.txt"
[[ ! -f $cef/CREDITS.html ]] || cp "$cef/CREDITS.html" "$stage/share/licenses/cef-credits.html"
cp native/vendor/json.hpp "$stage/share/licenses/nlohmann-json.hpp"
cp packaging/*.sh "$stage/"
cp README.md README.zh-CN.md "$stage/"
mkdir -p "$stage/assets"
cp -a assets/branding "$stage/assets/"
cp LICENSE "$stage/LICENSE"
mkdir -p "$stage/tools/source/examples" "$stage/tools/source/tools" "$stage/tools/source/ui"
cp package.json package-lock.json tsconfig.json LICENSE "$stage/tools/source/"
cp -a sdk "$stage/tools/source/"
mkdir -p "$stage/tools/source/templates"
cp -a templates/plugin "$stage/tools/source/templates/"
mkdir -p "$stage/tools/source/assets"
cp -a assets/branding "$stage/tools/source/assets/"
cp -a examples/showcase "$stage/tools/source/examples/"
cp -a ui/src ui/locales ui/index.html ui/package.json "$stage/tools/source/ui/"
cp tools/build-ui.mjs tools/build-branding.mjs tools/plugin-dev.mjs "$stage/tools/source/tools/"
cp tools/extract-release.py tools/bootstrap.py tools/cef-runtime.py tools/export-diagnostics.py "$stage/tools/"
find "$stage/tools/source" -type d -name __pycache__ -prune -exec rm -rf -- {} +
cp -a docs "$stage/share/"
printf '%s\n' "$build_id" > "$stage/VERSION"
chmod 755 "$stage"/*.sh "$stage/bin/"* "$stage/lib/cef/framely-vr"
python3 tools/package-runtime.py "$stage" "$cef" "${RELEASE_REPO:-SteamFramelyHomebrew/framely}" "${RELEASE_TAG:-v$version}"
echo "Package: release/framely-$build_id-linux-arm64.tar.gz"
