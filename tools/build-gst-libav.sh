#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$base"
# SteamOS Frame uses FFmpeg 7 (libavcodec.so.61). CI's FFmpeg may have
# a different ABI. Build only the matching development API for linking;
# never bundle or use these minimal libraries for runtime decoding.
if [[ $(pkg-config --modversion libavcodec | cut -d. -f1) != 61 ]]; then
  api="$base/media/source/ffmpeg-build-api"
  mkdir -p "$api"
  pushd "$api" >/dev/null
  ../ffmpeg-7.0/configure --prefix="$api/install" --disable-everything --disable-autodetect --disable-programs --disable-doc --disable-asm --disable-static --enable-shared
  make -j"${FRAMELY_BUILD_JOBS:-4}"
  make install
  popd >/dev/null
  export PKG_CONFIG_PATH="$api/install/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
fi
meson setup media/source/gst-libav-build media/source/gst-libav-1.24.13 --prefix=/usr --libdir=lib --buildtype=release -Dtests=disabled -Ddoc=disabled
ninja -C media/source/gst-libav-build -j"${FRAMELY_BUILD_JOBS:-4}"
# Installing to staging strips build-directory RPATHs from the plugin.
DESTDIR="$base/media/source/gst-libav-install" meson install -C media/source/gst-libav-build --no-rebuild
cp media/source/gst-libav-install/usr/lib/gstreamer-1.0/libgstlibav.so media/bin/
cp media/source/gst-libav-1.24.13/COPYING media/bin/LICENSE.gst-libav
