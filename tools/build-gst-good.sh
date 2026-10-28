#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$base"
# SteamOS does not provide these Good plugins. Build only the elements used
# by our receive pipelines; keep GStreamer, PulseAudio and Soup system libraries.
meson setup media/source/gst-good-build media/source/gst-plugins-good-1.24.13 \
  --prefix=/usr --libdir=lib --buildtype=release --wrap-mode=nofallback \
  -Dauto_features=disabled -Dlevel=enabled -Dpulse=enabled \
  -Dvideofilter=enabled -Disomp4=enabled -Dsoup=enabled \
  -Dadaptivedemux2=enabled -Daudioparsers=enabled -Dhls-crypto=openssl \
  -Dtests=disabled -Dexamples=disabled -Ddoc=disabled
ninja -C media/source/gst-good-build -j"${FRAMELY_BUILD_JOBS:-4}"
DESTDIR="$base/media/source/gst-good-install" meson install -C media/source/gst-good-build --no-rebuild
for plugin in level pulseaudio videofilter isomp4 soup adaptivedemux2 audioparsers; do
  cp "media/source/gst-good-install/usr/lib/gstreamer-1.0/libgst${plugin}.so" media/bin/
done
cp media/source/gst-plugins-good-1.24.13/COPYING media/bin/LICENSE.gst-good
