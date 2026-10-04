#!/usr/bin/env bash
set -euo pipefail
if [[ $EUID != 0 ]]; then exec sudo -- bash "$0" "$@"; fi
root=/var/lib/framely
previous=$(cat "$root/previous-release")
[[ $previous == releases/* && $previous != *..* && -d $root/$previous ]] || { echo 'No previous release.' >&2; exit 1; }
current=$(readlink "$root/current")
systemctl stop framely-session.service framely.service
ln -s "$previous" "$root/current.rollback"
mv -Tf "$root/current.rollback" "$root/current"
printf '%s\n' "$current" > "$root/previous-release"
systemctl start framely.service
systemctl start --no-block framely-session.service
echo "Restored $previous"
