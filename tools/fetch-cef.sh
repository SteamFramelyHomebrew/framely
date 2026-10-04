#!/usr/bin/env bash
set -euo pipefail
# Pinned distribution used by the device probe; no latest-version lookup.
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
out=$(realpath -m "${1:-$base/target/cef}")
[[ ! -e $out ]] || { echo 'Destination already exists.' >&2; exit 1; }
archive=$(mktemp /tmp/framely-cef-XXXXXX.tar.bz2)
trap 'rm -f -- "$archive"' EXIT
url='https://cef-builds.spotifycdn.com/cef_binary_154.0.32%2Bg682c378%2Bchromium-154.0.8037.58_linuxarm64_minimal.tar.bz2'
curl --fail --location --proto '=https' --proto-redir '=https' --retry 3 --output "$archive" "$url"
printf '7139f92aac35073de63bc4731308f984d4998513  %s\n' "$archive" | sha1sum -c -
mkdir -p "$out"
tar -xjf "$archive" --strip-components=1 -C "$out"
echo "$out"
