#!/usr/bin/env bash
set -euo pipefail
root=/var/lib/framely
helper=$root/current/share/input/steam-system-menu.py
[[ -f $helper && -f $root/steam-user ]] || exit 0
steam_user=$(cat "$root/steam-user")
steam_uid=$(id -u "$steam_user")
steam_home=$(getent passwd "$steam_user" | cut -d: -f6)
set +e
runuser -u "$steam_user" -- env HOME="$steam_home" XDG_RUNTIME_DIR="/run/user/$steam_uid" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$steam_uid/bus" python3 "$helper" --restore
result=$?
set -e
if [[ $result == 10 ]]; then
  systemctl --user --machine="$steam_user@" restart steamvr.service
elif [[ $result != 0 ]]; then
  echo 'Could not restore the original SteamVR system-button binding; start SteamVR and retry.' >&2
  exit "$result"
fi
