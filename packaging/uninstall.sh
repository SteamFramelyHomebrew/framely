#!/usr/bin/env bash
set -euo pipefail
if [[ $EUID != 0 ]]; then exec sudo -- bash "$0" "$@"; fi
[[ $# == 0 ]] || { echo 'Usage: uninstall.sh (uninstalls all plugins, then Framely)' >&2; exit 1; }
root=/var/lib/framely
[[ -x $root/current/bin/framely ]] || { echo 'Framely is unavailable. Repair the installation before uninstalling.' >&2; exit 1; }
# Stop the UI and its download workers first, keeping the core alive for hooks.
systemctl stop framely-session.service
restore_session() {
  local code=$?
  echo 'Plugin uninstall failed. Framely was retained; inspect the error and retry.' >&2
  systemctl start --no-block framely-session.service || true
  exit "$code"
}
trap restore_session ERR
systemctl start framely.service
ready=false
for attempt in {1..30}; do
  if "$root/current/bin/framely" status >/dev/null 2>&1; then ready=true; break; fi
  sleep 0.2
done
[[ $ready == true ]]
"$root/current/bin/framely" prepare-uninstall --approve
trap - ERR
# Only remove the manager after every plugin has been disabled and uninstalled.
systemctl disable --now framely-session.service framely.service
systemctl stop 'framely-plugin-*.service' 'framely-backend-*.service' 'framely-hook-*.service' 2>/dev/null || true
rm -f /etc/systemd/system/framely.service /etc/systemd/system/framely-session.service
systemctl daemon-reload
rm -f "$root/current" "$root/previous-release"
rm -f /home/.framely/state/current /home/.framely/state/previous-release /home/.framely/repair.sh
rm -rf /home/.framely/releases /run/framely
rm -f "$root/releases" /home/.framely/state/releases
# External plugin effects cannot be reliably erased; retain data for recovery.
echo 'All plugins and Framely were uninstalled. Saved settings and plugin data were retained.'
