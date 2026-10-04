#!/usr/bin/env bash
set -euo pipefail
if [[ $EUID != 0 ]]; then exec sudo -- bash "$0" "$@"; fi
[[ $# -le 1 ]] || { echo 'Usage: repair.sh [Steam user]' >&2; exit 1; }
store=/home/.framely
[[ -d $store && ! -L $store && $(stat -c %u "$store") == 0 ]] || { echo 'Invalid Framely storage.' >&2; exit 1; }
state="$store/state"
[[ -d $state && ! -L $state && $(stat -c %u "$state") == 0 ]] || { echo 'Persistent Framely state is missing or invalid.' >&2; exit 1; }
current=$(readlink "$state/current")
[[ $current =~ ^releases/[a-zA-Z0-9.+-]+$ && $current != *..* ]] || { echo 'Invalid current release.' >&2; exit 1; }
release="$store/$current"
[[ -d $release && ! -L $release && $(stat -c %u "$release") == 0 ]] || { echo 'Installed release is missing or invalid.' >&2; exit 1; }
steam_user=${1:-$(cat "$state/steam-user")}
cd "$release"
sha256sum --quiet -c SHA256SUMS
exec bash "$release/install.sh" --repair "$steam_user"
