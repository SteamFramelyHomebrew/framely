# Media components

- Panel capture code in `capture/src/{encoder,gpu,gpu_copy,kms,lut,grab,openvr}.rs`
  and `capture/shaders/convert.comp` derives from [coah80/framecorder](https://github.com/coah80/framecorder), MIT.
  The original license is preserved in `capture/LICENSE.framecorder`.
  Framely adds streaming output, field-of-view offsets, browser-compatible H.264
  encoding and a core-managed framebuffer helper. It does not invoke or require
  the Framecorder application or plugin.
- [MediaMTX](https://github.com/bluenviron/mediamtx), MIT, supplies RTSP, WHEP/WebRTC
  and HLS transport. Its version, source URL and archive checksum are pinned in
  `dependencies.json`; the release includes its license.
- [UxPlay](https://github.com/FDH2/UxPlay), GPL-3.0-or-later, supplies AirPlay/RAOP
  reception. Its version and source checksum are pinned in `dependencies.json`.
  `patch-uxplay.py` adds session-end events and enforces one sending device.
  Releases include the patched source archive, build recipe and UxPlay/llhttp licenses.
- [gst-libav](https://gstreamer.freedesktop.org/modules/gst-libav.html), LGPL-2.1-or-later,
  is bundled as a GStreamer plugin linked to the installed system FFmpeg libraries.
  Its version and checksum are pinned; releases include its source and license.
  CI links against the FFmpeg 7 development API to match SteamOS; that build
  dependency is pinned but its FFmpeg libraries are not distributed.
- GStreamer and GUPnP are linked as system libraries (LGPL). GStreamer performs
  decoding, audio playback, rotation and pixel aspect correction; GUPnP provides
  UPnP discovery, SOAP and event subscriptions. FFmpeg is invoked as the installed
  system executable for SteamVR capture, software encoding and audio muxing.
  These system components are not copied into Framely packages.
- `native/json.hpp` is the existing nlohmann/json dependency (MIT), included with
  the main Framely release notices.

Framely's added code uses the repository's AGPL-3.0-only license. See
`docs/casting.md` for build dependencies and verification commands. Source and
licenses for the bundled media additions ship under `share/source/media` and
`share/licenses` in a release.
