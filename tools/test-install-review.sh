#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cef=$(realpath "$1")
cd "$base"
out=$(mktemp -d /tmp/framely-install-review.XXXXXX)
server_pid=''
cleanup() { if [[ -n $server_pid ]]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; fi; }
trap cleanup EXIT
export FRAMELY_INSTALL_REVIEW_OUTPUT="$out"
node --input-type=module - <<'JS'
import {build} from 'esbuild';
import {writeFile} from 'node:fs/promises';
const out=process.env.FRAMELY_INSTALL_REVIEW_OUTPUT;
await build({entryPoints:['tests/install_review_fixture.tsx'],bundle:true,outfile:`${out}/fixture.js`,define:{'process.env.NODE_ENV':'"production"'}});
await writeFile(`${out}/index.html`,'<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="fixture.css"><div id="root"></div><script src="fixture.js"></script></html>');
JS
python3 - "$out" > "$out/server.log" 2>&1 <<'PY' &
import functools,http.server,pathlib,sys
root=pathlib.Path(sys.argv[1])
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(http.server.SimpleHTTPRequestHandler,directory=str(root)))
(root/'port').write_text(str(server.server_port))
server.serve_forever()
PY
server_pid=$!
for _ in {1..100}; do [[ ! -f $out/port ]] || break; sleep 0.05; done
port=$(cat "$out/port")
cp -a "$cef/Resources/." "$cef/Release/"
g++ -std=c++17 -O2 -I"$cef" tests/browser_probe.cpp -L"$cef/Release" -lcef -pthread '-Wl,-rpath,$ORIGIN' -o "$cef/Release/framely-browser-probe"
for mode in desktop mobile; do
 mkdir "$out/$mode"
 export FRAMELY_CEF_ROOT="$cef" FRAMELY_INSTALL_REVIEW_TEST=1 FRAMELY_PREVIEW_DIR="$out/$mode" GSETTINGS_BACKEND=memory
 if [[ $mode == desktop ]]; then export FRAMELY_STORE_TEST=1; else unset FRAMELY_STORE_TEST; fi
 if ! "$cef/Release/framely-browser-probe" "http://127.0.0.1:$port" "$out/$mode-runtime" --ozone-platform=headless --disable-gpu > "$out/$mode.log" 2>&1; then
  cat "$out/$mode.log"; exit 1
 fi
 tail -n 1 "$out/$mode.log"
done
printf 'Install review previews: %s\n' "$out"
