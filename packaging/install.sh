#!/usr/bin/env bash
set -euo pipefail
if [[ $EUID != 0 ]]; then exec sudo -- bash "$0" "$@"; fi
base=$(cd -- "$(dirname -- "$0")" && pwd)
repair=false
if [[ ${1:-} == --repair ]]; then repair=true; shift; fi
[[ $# -le 1 ]] || { echo 'Usage: install.sh [--repair] [Steam user]' >&2; exit 1; }
steam_user=${1:-${SUDO_USER:-steamos}}
[[ $steam_user != root ]] || { echo 'Specify the Steam session user (usually steamos).' >&2; exit 1; }
[[ $(uname -m) == aarch64 ]] || { echo 'This package requires Linux ARM64.' >&2; exit 1; }
for command in systemctl systemd-run useradd getent python3 sha256sum ldd; do command -v "$command" >/dev/null || { echo "Missing dependency: $command" >&2; exit 1; }; done
steam_uid=$(id -u "$steam_user")
steam_home=$(getent passwd "$steam_user" | cut -d: -f6)
[[ $steam_uid -ge 1000 && $steam_home == /* && $steam_home != *' '* ]] || { echo 'Invalid Steam user/home.' >&2; exit 1; }
[[ -w /etc/systemd/system && -w /etc && -w /var/lib ]] || { echo 'System configuration is not writable. Framely did not change the read-only setting; installation stopped.' >&2; exit 1; }
cd "$base"
sha256sum --quiet -c SHA256SUMS
for binary in bin/framely; do
  dependencies=$(ldd "$binary" 2>&1)
  [[ $dependencies != *'not found'* ]] || { echo "$dependencies" >&2; exit 1; }
done
version=$(cat VERSION)
[[ $version =~ ^[a-zA-Z0-9.+-]+$ ]] || exit 1
root=/var/lib/framely
release="$root/releases/$version"
store=/home/.framely
[[ ! -L $store ]] || { echo 'Storage path must not be a symlink.' >&2; exit 1; }
mkdir -p "$store"
[[ $(stat -c %u "$store") == 0 ]] || { echo 'Storage must be owned by root.' >&2; exit 1; }
chmod 755 "$store"
persistent="$store/state"
[[ ! -L $persistent ]] || { echo 'Persistent state must not be a symlink.' >&2; exit 1; }
if [[ -L $root ]]; then
  [[ $(readlink "$root") == "$persistent" ]] || { echo 'Unexpected state link.' >&2; exit 1; }
elif [[ -e $root && -e $persistent ]]; then
  echo 'Both legacy and persistent state exist; refusing to merge them.' >&2; exit 1
fi
[[ ! -e $persistent || $(stat -c %u "$persistent") == 0 ]] || { echo 'Storage state must be owned by root.' >&2; exit 1; }
declare -A previous_active previous_enabled
for unit in framely.service framely-session.service; do
  previous_active[$unit]=$(systemctl is-active "$unit" || true)
  previous_enabled[$unit]=$(systemctl is-enabled "$unit" 2>/dev/null || true)
done
if [[ -d $root && ! -L $root ]]; then
  # Stop writers before moving state across filesystems. Restore running services
  # if migration or subsequent preflight fails; the compatibility path survives.
  restart_migrated_services() {
    local code=${1:-$?}
    if [[ ! -e $root && -d $persistent ]]; then ln -s "$persistent" "$root"; fi
    for unit in framely.service framely-session.service; do
      if [[ ${previous_active[$unit]} == active ]]; then systemctl start "$unit" || true; fi
    done
    exit "$code"
  }
  trap restart_migrated_services EXIT
  systemctl stop framely-session.service framely.service
  mv "$root" "$persistent"
fi
mkdir -p "$persistent"
[[ -L $root ]] || ln -s "$persistent" "$root"
for dir in releases plugins data; do
  if [[ -L $root/$dir ]]; then
    [[ $(readlink "$root/$dir") == "$store/$dir" ]] || { echo "Unexpected storage link: $dir" >&2; exit 1; }
  elif [[ -d $root/$dir ]]; then
    [[ ! -e $store/$dir ]] || { echo "Conflicting storage: $dir" >&2; exit 1; }
    mv "$root/$dir" "$store/$dir"
    ln -s "$store/$dir" "$root/$dir"
  else
    mkdir -p "$store/$dir"
    ln -s "$store/$dir" "$root/$dir"
  fi
done
mkdir -p "$root/logs"
chmod 755 "$root" "$root/releases" "$root/plugins" "$root/data"
chmod 700 "$root/logs"
old=$(readlink "$root/current" || true)
if ! $repair && [[ -z $old && -f CEF_RUNTIME.json && ! -f lib/cef/libcef.so ]]; then
  echo 'First installation requires the complete offline package with CEF.' >&2; exit 1
fi
if $repair; then
  [[ $old == "releases/$version" && -d $release && ! -L $release ]] || { echo 'Repair requires the currently installed release.' >&2; exit 1; }
  (cd "$release" && sha256sum --quiet -c SHA256SUMS)
elif [[ -e $release ]]; then
  echo "Release $version already exists; use its install.sh --repair to restore services." >&2; exit 1
fi
stage="$root/releases/.install-$$"
mkdir "$stage"
release_created=false
installed=false
cleanup_install() {
  local code=$?
  rm -rf -- "$stage"
  if $release_created && ! $installed && [[ $(readlink "$root/current" || true) != "releases/$version" ]]; then
    rm -rf -- "$release"
  fi
  if ! $installed && declare -F restart_migrated_services >/dev/null; then restart_migrated_services "$code"; fi
  return "$code"
}
trap cleanup_install EXIT
if ! $repair; then
  # Preserve every checksummed file, including documentation and runtime metadata.
  cp -a "$base/." "$stage/"
  chown -R root:root "$stage"
  find "$stage" -type d -exec chmod 755 {} +
fi
if [[ -f CEF_RUNTIME.json ]]; then
  # Cache exact runtimes outside releases so upgrades and rollback share files.
  cef_runtime=$(python3 "$base/tools/cef-runtime.py" prepare "$base" "$store")
  if $repair; then cef_release="$release"; else cef_release="$stage"; fi
  python3 "$base/tools/cef-runtime.py" attach "$cef_release" "$cef_runtime"
  (cd "$cef_release" && sha256sum --quiet -c SHA256SUMS)
  dependencies=$(ldd "$cef_release/lib/cef/framely-vr" 2>&1)
else
  dependencies=$(ldd "$base/lib/cef/framely-vr" 2>&1)
fi
[[ $dependencies != *'not found'* ]] || { echo "$dependencies" >&2; exit 1; }
if ! $repair; then
  mv "$stage" "$release"
  release_created=true
fi
mkdir -p "$root/install-backup"
chmod 700 "$root/install-backup"
rm -f "$root/install-backup/state.json"
[[ ! -f $root/state.json ]] || cp -a "$root/state.json" "$root/install-backup/state.json"
rm -f "$root/install-backup/framely.service" "$root/install-backup/framely-session.service"
for unit in framely.service framely-session.service; do
  if [[ -f /etc/systemd/system/$unit ]]; then cp -a "/etc/systemd/system/$unit" "$root/install-backup/$unit"; fi
done
rollback_on_failure() {
  local code=$?
  trap - ERR
  echo 'Installation failed; restoring previous services and release.' >&2
  systemctl stop framely-session.service framely.service 2>/dev/null || true
  rm -f "$root/current.rollback"
  if [[ -n $old ]]; then ln -s "$old" "$root/current.rollback"; mv -Tf "$root/current.rollback" "$root/current"; else rm -f "$root/current"; fi
  for unit in framely.service framely-session.service; do
    if [[ -f $root/install-backup/$unit ]]; then cp -a "$root/install-backup/$unit" "/etc/systemd/system/$unit"; else rm -f "/etc/systemd/system/$unit"; fi
  done
  [[ ! -f $root/install-backup/state.json ]] || cp -a "$root/install-backup/state.json" "$root/state.json"
  systemctl daemon-reload
  for unit in framely.service framely-session.service; do
    if [[ ${previous_enabled[$unit]} == enabled ]]; then systemctl enable "$unit"; else systemctl disable "$unit" 2>/dev/null || true; fi
    if [[ -n $old && ${previous_active[$unit]} == active ]]; then systemctl start "$unit"; fi
  done
  exit "$code"
}
trap rollback_on_failure ERR
systemctl stop framely-session.service framely.service 2>/dev/null || true
# Clear the uninstall guard retained by older releases only for a clean reinstall.
# Existing installations, repairs and incomplete plugin cleanup retain safe mode.
if ! $repair && [[ -z $old && -f $root/state.json ]]; then
  python3 - "$root/state.json" <<'PY_REINSTALL'
import json, os, pathlib, sys
path = pathlib.Path(sys.argv[1])
state = json.loads(path.read_text())
if state.get('safeMode') is True and state.get('plugins') == {}:
    state['safeMode'] = False
    temporary = path.with_name('state.json.reinstall')
    with temporary.open('w') as output:
        os.chmod(temporary, 0o600)
        json.dump(state, output, ensure_ascii=False)
        output.flush()
        os.fsync(output.fileno())
    os.replace(temporary, path)
PY_REINSTALL
fi
ln -s "releases/$version" "$root/current.new"
mv -Tf "$root/current.new" "$root/current"
cat > /etc/systemd/system/framely.service <<EOF
[Unit]
Description=Framely plugin management service
After=local-fs.target
[Service]
Type=simple
ExecStart=$root/current/bin/framely daemon --state $root --manager-uid $steam_uid
ExecStopPost=/usr/bin/systemctl stop framely-plugin-*.service framely-backend-*.service framely-hook-*.service
Restart=on-failure
RestartSec=3
RuntimeDirectory=framely
RuntimeDirectoryMode=0755
UMask=0022
TimeoutStopSec=120
[Install]
WantedBy=multi-user.target
EOF
# Keep the legacy --mailbox argument so rollback to older releases can reuse
# this service unit. The current CLI accepts and ignores it; no helper is shipped.
cat > /etc/systemd/system/framely-session.service <<EOF
[Unit]
Description=Framely SteamVR UI session
After=framely.service user@$steam_uid.service
Requires=framely.service
[Service]
User=$steam_user
Environment=HOME=$steam_home
Environment=XDG_RUNTIME_DIR=/run/user/$steam_uid
Environment=DISPLAY=:0
Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$steam_uid/bus
ExecStart=$root/current/bin/framely session --state $root --assets $root/current/share/ui --native $root/current/lib/cef/framely-vr --mailbox $root/current/bin/framely-mailbox
Restart=on-failure
RestartSec=5
KillMode=control-group
TimeoutStopSec=10
[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable framely.service framely-session.service
systemctl start framely.service
ready=false
for attempt in {1..30}; do
  if "$release/bin/framely" status > "$root/install-status.json" 2>/dev/null; then ready=true; break; fi
  sleep 0.2
done
[[ $ready == true ]]
rm -f "$root/install-status.json"
systemctl start --no-block framely-session.service
sleep 1
systemctl is-active --quiet framely.service
if ! $repair; then printf '%s\n' "$old" > "$root/previous-release"; fi
printf '%s\n' "$steam_user" > "$root/steam-user"
cp "$release/repair.sh" "$store/repair.sh"
chown root:root "$store/repair.sh"
chmod 755 "$store/repair.sh"
installed=true
trap - ERR
echo "Installed Framely $version. Open SteamVR Dashboard and look for the Framely icon beside the Dock."
echo 'Diagnostics: sudo journalctl -u framely -u framely-session --no-pager -n 100'
echo 'After a SteamOS update, restore services with: sudo bash /home/.framely/repair.sh'
