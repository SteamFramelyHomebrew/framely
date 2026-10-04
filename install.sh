#!/usr/bin/env bash
# Public bootstrap: curl -fsSL https://raw.githubusercontent.com/SteamFramelyHomebrew/framely/main/install.sh | bash
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
curl --fail --location --proto '=https' --proto-redir '=https' --retry 3 --output "$work/bootstrap.py" "https://github.com/$repo/releases/$release_path/bootstrap.py"
python3 "$work/bootstrap.py" --repo "$repo" "$@"
