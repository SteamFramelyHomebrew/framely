#!/usr/bin/env python3
"""Keep SteamVR's click and double-click actions while Framely handles hold."""

import hashlib
import json
import os
from pathlib import Path
import sys
from urllib.parse import urlencode
from urllib.request import Request, urlopen

APP = "openvr.component.vrcompositor"
CONTROLLER = "frame_controller"
BASE = "http://127.0.0.1:27062"
HEADERS = {
    "Origin": BASE,
    "Referer": BASE + "/dashboard/controllerbinding.html",
}
DIRECTORY = Path.home() / ".local/share/framely/input"
BINDING = DIRECTORY / "steam-system-menu.json"
META = DIRECTORY / "steam-system-menu-source.json"


def request(path, params=None, body=None):
    if params:
        path += "?" + urlencode(params)
    data = None if body is None else json.dumps(body).encode()
    headers = dict(HEADERS)
    if data is not None:
        headers["Content-Type"] = "application/json"
    with urlopen(Request(BASE + path, data=data, headers=headers), timeout=3) as response:
        return json.load(response)


def current_url():
    data = request("/input/getactions.json", {"app_key": APP})
    return data["current_binding_url"][CONTROLLER]


def select(url):
    result = request(
        "/input/selectconfig.action",
        body={"app_key": APP, "controller_type": CONTROLLER, "url": url},
    )
    if not result.get("success"):
        raise RuntimeError(f"SteamVR rejected controller binding: {result}")


def source_binding(url):
    result = request(
        "/input/loadbindingfromurl.json",
        {"binding_url": url, "controller_type": CONTROLLER, "app_key": APP},
    )
    if "binding_config" not in result:
        raise RuntimeError(f"SteamVR could not load controller binding: {result}")
    return result["binding_config"]


def without_recenter(binding):
    # Preserve every other compositor action, including click, double and
    # screenshot chords. A user's unrelated quick-recenter sources survive.
    result = json.loads(json.dumps(binding))
    sources = result.get("bindings", {}).get("/actions/quickrecenter", {}).get("sources", [])
    kept = [
        source for source in sources
        if source.get("path") not in (
            "/user/hand/left/input/system",
            "/user/hand/right/input/system",
        ) or "long" not in source.get("inputs", {})
    ]
    result["bindings"]["/actions/quickrecenter"]["sources"] = kept
    return result, len(kept) != len(sources)


def atomic_json(path, value):
    DIRECTORY.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".new")
    with temporary.open("w") as out:
        json.dump(value, out, ensure_ascii=False, separators=(",", ":"))
        out.flush()
        os.fsync(out.fileno())
    os.replace(temporary, path)


def main():
    active = current_url()
    ours = BINDING.resolve().as_uri()
    metadata = json.loads(META.read_text()) if META.exists() else {}
    if len(sys.argv) > 1 and sys.argv[1] == "--restore":
        if active == ours and metadata.get("source_url"):
            select(metadata["source_url"])
            return 10
        return 0
    source_url = metadata.get("source_url") if active == ours else active
    if not source_url:
        raise RuntimeError("SteamVR did not report a Frame controller binding")
    source = source_binding(source_url)
    patched, changed = without_recenter(source)
    if not changed:
        return 0
    digest = hashlib.sha256(json.dumps(source, sort_keys=True).encode()).hexdigest()
    if active == ours and metadata.get("source_hash") == digest and BINDING.exists():
        return 0
    atomic_json(BINDING, patched)
    atomic_json(META, {"source_url": source_url, "source_hash": digest})
    select(ours)
    return 10  # SteamVR caches the old long-press action until it restarts.


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(f"SteamVR system-menu binding: {error}", file=sys.stderr)
        sys.exit(1)
