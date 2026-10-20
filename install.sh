#!/usr/bin/env bash
# Public bootstrap: curl -fsSL https://github.com/SteamFramelyHomebrew/framely/releases/latest/download/install.sh | bash
set -euo pipefail
export FRAMELY_INSTALL_TRACE_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
repo=${FRAMELY_REPOSITORY:-SteamFramelyHomebrew/framely}
[[ $repo =~ ^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+$ ]] || { echo 'Invalid repository.' >&2; exit 1; }
command -v python3 >/dev/null || { echo 'Python 3 is required.' >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
release_path=latest/download
args=("$@")
github_proxy=${FRAMELY_GITHUB_PROXY:-}
local_bundle=''
if (( $# == 2 )) && [[ $1 == --local-bundle ]]; then
  local_bundle=$2
  [[ $local_bundle == /* && -f $local_bundle/bootstrap.py && -f $local_bundle/SHA256SUMS ]] || {
    echo 'Framely: --local-bundle needs an absolute directory containing bootstrap.py and SHA256SUMS.' >&2
    exit 1
  }
  args=()
fi
fetch_engine() {
  if [[ -n $local_bundle ]]; then
    cp -- "$local_bundle/bootstrap.py" "$work/bootstrap.py"
    return
  fi
  local bootstrap_url="https://github.com/$repo/releases/$release_path/bootstrap.py"
if [[ -n $github_proxy ]]; then
  echo "Framely[$FRAMELY_INSTALL_TRACE_ID]: Fetching installer engine through configured GitHub proxy (address hidden)."
  if status=$(curl --disable --silent --show-error --fail --location --proto '=https' --proto-redir '=https' --write-out '%{http_code}' --output "$work/bootstrap.py" "$github_proxy/$bootstrap_url"); then
    :
  else
    code=$?
    echo "Framely[$FRAMELY_INSTALL_TRACE_ID]: Failed to download the installer engine through the configured GitHub proxy (curl $code, HTTP ${status:-000}). Check the proxy connection or use another trusted proxy, then retry." >&2
    return 1
  fi
  return 0
fi
echo "Framely[$FRAMELY_INSTALL_TRACE_ID]: Fetching installer engine from GitHub."
if status=$(curl --disable --silent --show-error --fail --location --proto '=https' --proto-redir '=https' --dump-header "$work/headers" --write-out '%{http_code}' --output "$work/bootstrap.py" "$bootstrap_url"); then
  :
else
  code=$?
  retry=false
  case "$code" in 5|6|7|18|28|35|52|55|56) retry=true;; esac
  if [[ $code == 22 ]]; then
    case "$status" in 408|429|5??) retry=true;; 403)
      if grep -Eiq '^(x-ratelimit-remaining:[[:space:]]*0[[:space:]]*$|retry-after:)' "$work/headers"; then retry=true; fi;;
    esac
  fi
  if [[ ${FRAMELY_GITHUB_AUTO_FALLBACK:-1} == 0 || $retry != true ]]; then
    echo "Framely[$FRAMELY_INSTALL_TRACE_ID]: Failed to download the installer engine from GitHub (curl $code, HTTP $status). Check the network, release version and proxy settings, then retry." >&2; return 1
  fi
  echo "Framely[$FRAMELY_INSTALL_TRACE_ID]: GitHub request failed (curl $code, HTTP $status); trying gh-proxy.com once." >&2
  if ! curl --disable --silent --show-error --fail --location --proto '=https' --proto-redir '=https' --output "$work/bootstrap.py" "https://gh-proxy.com/$bootstrap_url"; then
    echo "Framely[$FRAMELY_INSTALL_TRACE_ID]: Failed to download the installer engine from GitHub (curl $code, HTTP $status) and gh-proxy.com. Check the network or specify another trusted --github-proxy, then retry." >&2; return 1
  fi
fi
}
if (( ${#args[@]} == 0 )); then
  if ! exec 3<>/dev/tty; then
    echo 'Framely: The interactive installer needs a terminal. Run it in a Frame terminal, use an explicit command such as install --yes, or add -t to SSH.' >&2
    exit 1
  fi
  state=/home/.framely/state
  [[ -L $state/current || ! -L /var/lib/framely/current ]] || state=/var/lib/framely
  if [[ -L $state/current ]]; then
    link=$(readlink "$state/current")
    if [[ ! $link =~ ^releases/[a-zA-Z0-9.+-]+$ || ! -f $state/current/VERSION || ! -f $state/current/uninstall.sh ]]; then
      echo 'Framely: Installation records are incomplete. Run /home/.framely/repair.sh, then try again.' >&3
      exit 1
    fi
    installed_version=$(<"$state/current/VERSION")
    if [[ ! $installed_version =~ ^[a-zA-Z0-9][a-zA-Z0-9.+-]*$ ]]; then
      echo 'Framely: The installed version record is invalid. Repair the installation, then try again.' >&3
      exit 1
    fi
    installed=true
  elif [[ -e $state/current || -d /home/.framely/releases && -n $(ls -A /home/.framely/releases 2>/dev/null) ]]; then
    echo 'Framely: Installation records are incomplete. Run /home/.framely/repair.sh, then try again.' >&3
    exit 1
  else
    installed=false
    installed_version='Not installed'
  fi
  echo >&3
  echo '  FRAMELY  INSTALL & MAINTAIN' >&3
  echo '  ──────────────────────────────' >&3
  echo "  Current version: $installed_version" >&3
  [[ -z $local_bundle ]] || echo '  Package source: local bundle' >&3
  echo >&3
  step=menu
  while :; do
    case $step in
      menu)
        echo "  1) $([[ $installed == true ]] && echo 'Update Framely' || echo 'Install Framely')" >&3
        [[ $installed == true ]] && echo '  2) Uninstall Framely' >&3
        echo '  q) Quit' >&3
        printf '  Select: ' >&3
        IFS= read -r choice <&3 || exit 1
        case $choice in
          1) [[ $installed == true ]] && wizard_action=update || wizard_action=install; step=channel ;;
          2) [[ $installed == true ]] && step=uninstall_confirm ;;
          q|Q) echo 'Framely: Exited.' >&3; exit 0 ;;
          *) echo '  Enter one of the listed options.' >&3 ;;
        esac ;;
      uninstall_confirm)
        echo '  Uninstall disables and removes all plugins, then removes Framely. Saved data is kept.' >&3
        printf '  1) Start   b) Back   q) Quit: ' >&3
        IFS= read -r choice <&3 || exit 1
        case $choice in
          1)
            cp -- "$state/current/uninstall.sh" "$work/uninstall.sh"
            if [[ ! -e /var/lib/framely/current && -f /home/.framely/repair.sh ]]; then
              if (( EUID == 0 )); then bash /home/.framely/repair.sh steamos; else sudo bash /home/.framely/repair.sh steamos; fi
            fi
            if (( EUID == 0 )); then bash "$work/uninstall.sh"; else sudo bash "$work/uninstall.sh"; fi
            exit ;;
          b|B) step=menu ;;
          q|Q) exit 0 ;;
        esac ;;
      channel)
        echo >&3
        echo '  Release channel' >&3
        echo '  1) Stable   2) Testing (includes stable releases)' >&3
        printf '  Select (b: back, q: quit): ' >&3
        IFS= read -r choice <&3 || exit 1
        case $choice in
          1) channel=stable; step=mirror ;;
          2) channel=testing; step=mirror ;;
          b|B) step=menu ;;
          q|Q) exit 0 ;;
          *) echo '  Enter one of the listed options.' >&3 ;;
        esac ;;
      mirror)
        echo >&3
        echo '  Download route' >&3
        echo '  1) Direct from GitHub   2) Use a mirror' >&3
        printf '  Select (b: back, q: quit): ' >&3
        IFS= read -r choice <&3 || exit 1
        case $choice in
          1) github_proxy=''; export FRAMELY_GITHUB_PROXY=''; export FRAMELY_GITHUB_AUTO_FALLBACK=0; step=lookup ;;
          2) step=mirror_address ;;
          b|B) step=channel ;;
          q|Q) exit 0 ;;
          *) echo '  Enter one of the listed options.' >&3 ;;
        esac ;;
      mirror_address)
        echo >&3
        printf '  Mirror HTTPS base URL (blank: https://gh-proxy.com; b: back): ' >&3
        IFS= read -r choice <&3 || exit 1
        case $choice in
          b|B) step=mirror ;;
          q|Q) exit 0 ;;
          *) github_proxy=${choice:-https://gh-proxy.com}; step=lookup ;;
        esac ;;
      lookup)
        if [[ -n $github_proxy ]]; then
          if [[ ! $github_proxy =~ ^https://[a-zA-Z0-9.-]+(:[0-9]{1,5})?(/[a-zA-Z0-9._~/-]*)?$ || /$github_proxy/ == *'/../'* || /$github_proxy/ == *'/./'* ]]; then
            echo '  Invalid address. Enter an HTTPS mirror base URL without query parameters.' >&3
            step=mirror_address
            continue
          fi
          github_proxy=${github_proxy%/}
          export FRAMELY_GITHUB_PROXY=$github_proxy
          unset FRAMELY_GITHUB_AUTO_FALLBACK
        fi
        echo '  Checking available releases...' >&3
        # The current stable release supplies the resolver; the chosen tag
        # supplies the actual engine used after final confirmation.
        release_path=latest/download
        if ! fetch_engine; then
          echo '  Could not download the release checker. Check your connection or mirror, then retry.' >&3
          step=lookup_error
          continue
        fi
        if ! selection=$(python3 "$work/bootstrap.py" --repo "$repo" --resolve-release "$channel" --selection-action "$wizard_action"); then
          echo '  Could not find an installable release. Check the connection, mirror, and release assets, then retry.' >&3
          step=lookup_error
          continue
        fi
        selected_tag=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["tag"])' "$selection")
        selected_size=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["size"])' "$selection")
        if [[ -n $local_bundle ]]; then
          local_archives=()
          for candidate in "$local_bundle"/framely-*-linux-arm64.tar.gz; do
            [[ -f $candidate ]] || continue
            if [[ $wizard_action == install && $candidate == *-offline-linux-arm64.tar.gz ]] ||
               [[ $wizard_action == update && $candidate != *-offline-linux-arm64.tar.gz ]]; then
              local_archives+=("$candidate")
            fi
          done
          if (( ${#local_archives[@]} != 1 )); then
            echo '  The local bundle must contain exactly one archive for this operation.' >&3
            step=lookup_error
            continue
          fi
          local_archive=${local_archives[0]}
          local_name=${local_archive##*/}
          local_suffix='-linux-arm64.tar.gz'
          [[ $wizard_action != install ]] || local_suffix='-offline-linux-arm64.tar.gz'
          if [[ ! $local_name =~ ^framely-${selected_tag#v}-[a-f0-9]{12}${local_suffix//./\.}$ ]]; then
            echo '  The local archive does not match the selected release tag.' >&3
            step=lookup_error
            continue
          fi
          selected_size=$(stat -c %s -- "$local_archive")
        fi
        step=confirm ;;
      lookup_error)
        printf '  r) Retry   b) Back to download route   q) Quit: ' >&3
        IFS= read -r choice <&3 || exit 1
        case $choice in
          r|R) step=lookup ;;
          b|B) step=mirror ;;
          q|Q) exit 0 ;;
        esac ;;
      confirm)
        echo >&3
        echo '  Ready to proceed' >&3
        echo "  Action: $([[ $wizard_action == update ]] && echo Update || echo Install)" >&3
        echo "  Current version: $installed_version" >&3
        echo "  Target release: $selected_tag" >&3
        echo "  Package size: about $(( (selected_size + 1048575) / 1048576 )) MiB" >&3
        if [[ -n $local_bundle ]]; then
          echo '  Package source: Local staged archive (SHA256 checked before installation)' >&3
          echo "  Release lookup: $([[ -n $github_proxy ]] && echo 'GitHub mirror (address hidden)' || echo 'Direct from GitHub')" >&3
        else
          echo "  Download route: $([[ -n $github_proxy ]] && echo 'GitHub mirror (address hidden)' || echo 'Direct from GitHub')" >&3
        fi
        printf '  1) Start   b) Back   q) Quit: ' >&3
        IFS= read -r choice <&3 || exit 1
        case $choice in
          1) args=("$wizard_action" --version "$selected_tag" --yes); [[ -n $github_proxy ]] && args+=(--github-proxy "$github_proxy"); [[ -z $local_bundle ]] || args+=(--archive "$local_archive" --checksums "$local_bundle/SHA256SUMS"); release_path="download/$selected_tag"; break ;;
          b|B) [[ -n $github_proxy ]] && step=mirror_address || step=mirror ;;
          q|Q) exit 0 ;;
        esac ;;
    esac
  done
fi
for ((i=0; i<${#args[@]}; i++)); do
  case ${args[i]} in
    --version)
      ((i+=1))
      version=${args[i]:-}
      [[ $version =~ ^[a-zA-Z0-9][a-zA-Z0-9.+-]*$ ]] || { echo 'Invalid release version.' >&2; exit 1; }
      release_path="download/$version"
      ;;
    --version=*)
      version=${args[i]#--version=}
      [[ $version =~ ^[a-zA-Z0-9][a-zA-Z0-9.+-]*$ ]] || { echo 'Invalid release version.' >&2; exit 1; }
      release_path="download/$version"
      ;;
    --github-proxy)
      ((i+=1))
      github_proxy=${args[i]:-}
      [[ -n $github_proxy ]] || { echo 'Missing GitHub proxy URL.' >&2; exit 1; }
      ;;
    --github-proxy=*)
      github_proxy=${args[i]#--github-proxy=}
      [[ -n $github_proxy ]] || { echo 'Missing GitHub proxy URL.' >&2; exit 1; }
      ;;
    *) continue ;;
  esac
done
if [[ -n $github_proxy ]]; then
  [[ $github_proxy =~ ^https://[a-zA-Z0-9.-]+(:[0-9]{1,5})?(/[a-zA-Z0-9._~/-]*)?$ && /$github_proxy/ != *'/../'* && /$github_proxy/ != *'/./'* ]] || { echo 'Invalid GitHub proxy URL (HTTPS base URL required).' >&2; exit 1; }
  github_proxy=${github_proxy%/}
  export FRAMELY_GITHUB_PROXY=$github_proxy
fi
fetch_engine
python3 "$work/bootstrap.py" --repo "$repo" "${args[@]}"
