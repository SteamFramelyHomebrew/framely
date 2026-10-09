#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$base"
export CARGO_HOME=${CARGO_HOME:-/tmp/framely-cast-cargo}
(
if [[ -n ${FRAMELY_SYSROOT:-} ]]; then
 export PATH="$FRAMELY_SYSROOT/usr/bin:$PATH"
 export PKG_CONFIG_PATH="$FRAMELY_SYSROOT/usr/lib/aarch64-linux-gnu/pkgconfig:$FRAMELY_SYSROOT/usr/share/pkgconfig"
 export PKG_CONFIG_SYSROOT_DIR="$FRAMELY_SYSROOT"
 export LIBCLANG_PATH="$FRAMELY_SYSROOT/usr/lib/llvm-19/lib"
 export LD_LIBRARY_PATH="$FRAMELY_SYSROOT/usr/lib/aarch64-linux-gnu:$FRAMELY_SYSROOT/usr/lib/llvm-19/lib:${LD_LIBRARY_PATH:-}"
 export BINDGEN_EXTRA_CLANG_ARGS="-I$FRAMELY_SYSROOT/usr/lib/llvm-19/lib/clang/19/include -I$FRAMELY_SYSROOT/usr/include -I$FRAMELY_SYSROOT/usr/include/aarch64-linux-gnu"
 export RUSTFLAGS="${RUSTFLAGS:-} -L native=$FRAMELY_SYSROOT/usr/lib/aarch64-linux-gnu"
fi
cargo build --release --locked --manifest-path media/capture/Cargo.toml
)
mkdir -p media/bin
cp media/capture/target/release/framely-capture media/capture/target/release/framely-panel-grab media/bin/
# Native dependencies use the target system's GStreamer and GUPnP libraries.
python3 tools/fetch-media.py
bash tools/build-gst-libav.sh
python3 media/patch-uxplay.py media/source/UxPlay-1.73.7
cmake -S media/source/UxPlay-1.73.7 -B media/source/uxplay-build -DNO_X11_DEPS=ON -DCMAKE_BUILD_TYPE=Release
cmake --build media/source/uxplay-build -j"${FRAMELY_BUILD_JOBS:-4}"
cp media/source/uxplay-build/uxplay media/bin/
g++ -std=c++17 -O2 -shared -fPIC media/native/airplay_sink.cpp -o media/bin/libgstframely.so $(pkg-config --cflags --libs gstreamer-app-1.0 gstreamer-video-1.0)
g++ -std=c++17 -O2 -Inative/vendor media/native/receiver.cpp -o media/bin/framely-receiver $(pkg-config --cflags --libs gupnp-1.6 gstreamer-app-1.0 gstreamer-video-1.0)
mkdir -p media/bin/upnp
cp media/upnp/*.xml media/bin/upnp/
cp media/source/UxPlay-1.73.7/LICENSE media/bin/LICENSE.uxplay
cp media/source/UxPlay-1.73.7/lib/llhttp/LICENSE-MIT media/bin/LICENSE.llhttp
