#!/usr/bin/env bash
# Public bootstrap: curl -fsSL https://github.com/SteamFramelyHomebrew/framely/releases/latest/download/install.sh | bash
set -euo pipefail
repo=${FRAMELY_REPOSITORY:-SteamFramelyHomebrew/framely}
[[ $repo =~ ^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+$ ]] || { echo 'Invalid repository.' >&2; exit 1; }
command -v python3 >/dev/null || { echo 'Python 3 is required.' >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
release_path=latest/download
args=("$@")
for ((i=0; i<${#args[@]}; i++)); do
  case ${args[i]} in
    --version)
      ((i+=1))
      version=${args[i]:-}
      ;;
    --version=*) version=${args[i]#--version=} ;;
    *) continue ;;
  esac
  [[ $version =~ ^[a-zA-Z0-9][a-zA-Z0-9.+-]*$ ]] || { echo 'Invalid release version.' >&2; exit 1; }
  release_path="download/$version"
done
bootstrap_url="https://github.com/$repo/releases/$release_path/bootstrap.py"
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
    echo "Bootstrap primary route failed (curl $code, HTTP $status); not eligible for automatic proxy." >&2; exit 1
  fi
  echo 'GitHub connection failed; trying gh-proxy.com once' >&2
  if ! curl --disable --silent --show-error --fail --location --proto '=https' --proto-redir '=https' --output "$work/bootstrap.py" "https://gh-proxy.com/$bootstrap_url"; then
    echo "Bootstrap primary route failed (curl $code, HTTP $status); gh-proxy.com fallback also failed." >&2; exit 1
  fi
fi
python3 "$work/bootstrap.py" --repo "$repo" "$@"
