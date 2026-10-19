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
github_proxy=${FRAMELY_GITHUB_PROXY:-}
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
bootstrap_url="https://github.com/$repo/releases/$release_path/bootstrap.py"
if [[ -n $github_proxy ]]; then
  curl --disable --silent --show-error --fail --location --proto '=https' --proto-redir '=https' --output "$work/bootstrap.py" "$github_proxy/$bootstrap_url"
  python3 "$work/bootstrap.py" --repo "$repo" "$@"
  exit
fi
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
