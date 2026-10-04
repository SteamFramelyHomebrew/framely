# Publish plugins and catalogs

[简体中文](../zh-CN/developer-guide/publishing.md)

[Documentation](../README.md) · [Development workflow](README.md)

## Default GitHub Release URL

A public GitHub plugin repository's **source `manifest.json` may omit `downloadUrl`**. The community database derives it from the registered repository, version and plugin ID:

```text
https://github.com/<owner>/<repo>/releases/download/v<version>/<id>-<version>.framely
```

Repository `yourname/my-plugin`, ID `yourname.my-plugin`, version `0.1.0` means tag `v0.1.0` and asset `yourname.my-plugin-0.1.0.framely`. Repository names need not match IDs. Omit the field entirely; empty strings or null do not select automatic generation. Avoid `latest` and never replace same-version assets.

Source registration and packaging differ: the database normalizes the source URL and requires **the same final `downloadUrl` in the packaged Manifest**. Current `framely pack` computes payload hashes but does not inspect Git remotes or derive Release URLs. If your source omits the field, prepare a temporary Manifest before packaging; you do not need to write the URL back into source.

## Prepare the release package

Follow the [development workflow](README.md), run `npm run build`, and configure the plugin repository's GitHub `origin`. Run this inside your plugin to create `.framely-build/manifest.json`. It supports common HTTPS/SSH remotes, preserves explicit `downloadUrl`, and leaves source unchanged:

```bash
python3 - <<'PY'
import json, re, subprocess
from pathlib import Path

manifest = json.loads(Path('manifest.json').read_text(encoding='utf-8'))
manifest.pop('downloadSha256', None)  # Source registration only; never package it.
if 'downloadUrl' not in manifest:
    remote = subprocess.check_output(
        ['git', 'remote', 'get-url', 'origin'], text=True
    ).strip()
    match = re.fullmatch(
        r'(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)'
        r'([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+?)(?:\.git)?/?', remote
    )
    if not match:
        raise SystemExit('Use a GitHub origin or set downloadUrl explicitly.')
    manifest['downloadUrl'] = (
        f"https://github.com/{match[1]}/releases/download/v{manifest['version']}/"
        f"{manifest['id']}-{manifest['version']}.framely"
    )
output = Path('.framely-build/manifest.json')
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(manifest['downloadUrl'])
PY
```

Ignore temporary `.framely-build/` in your plugin Git repository; the template already does so. Asset filenames must match the default rule. Continue inside your plugin:

```bash
FRAMELY_REPO=/absolute/path/to/framely
PLUGIN_ID=$(python3 -c 'import json; print(json.load(open("manifest.json"))["id"])')
PLUGIN_VERSION=$(python3 -c 'import json; print(json.load(open("manifest.json"))["version"])')
mkdir -p dist
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- pack --manifest .framely-build/manifest.json --payload payload --output "dist/$PLUGIN_ID-$PLUGIN_VERSION.framely"
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- verify "dist/$PLUGIN_ID-$PLUGIN_VERSION.framely"
```

Commit source, SDK, lockfile and license; tag that commit `v<version>`, create its GitHub Release, upload the `.framely` from `dist/`, and publish. Source and packaged declarations must match after normalizing the default URL and removing registration-only fields. Pin database submodules to the actual released source commit.

Release notes describe usage, runtime user and reasons for root, dependencies, changes, actual Frame tests and limitations. Provide matching source/build scripts and comply with template, SDK and dependency licenses.

### Whole-package hashes and custom URLs

| Method | Source registration | Expected whole-package hash |
| --- | --- | --- |
| Default GitHub Release | Omit `downloadUrl`; use the tag/asset naming above | Read SHA256 from the Release asset `digest`; no handwritten hash needed |
| Explicit fixed GitHub Release | Set `downloadUrl` to choose a different tag/asset name | Read Release digest if `downloadSha256` is absent; otherwise validate against the declared hash |
| Other HTTPS download | Set `downloadUrl` and `downloadSha256` | Use the declared 64-character whole-package SHA256 |

Automatic URLs always require a Release digest, even when a hash is supplied, and both must match. Missing digests, unpublished/missing assets, download failures or hash mismatches block registration; there is no fallback to trusting the downloaded bytes.

`downloadSha256` belongs only to database source registration, covers the complete `.framely`, and differs from payload `files` hashes. It is not a Framely package field. Build first, run `sha256sum dist/<id>-<version>.framely`, then add it to source registration. Remove it before packaging, as the temporary-Manifest command does. Do not embed a package's own hash inside it. Passing source containing this field directly to `framely pack` fails unknown-field validation.

### Icons and window compatibility

For the community database, set top-level `icon: "icon.png"` and include the same PNG path in source and payload. The database derives a GitHub Raw URL from the pinned repository/commit/path and verifies it against the packaged icon; `publish.icon` is unnecessary. Self-hosted catalogs use fixed HTTPS `publish.icon` / `publish.screenshots`. See [store metadata](../plugin-development.md).

The community database supports window `entry`, `title`, `dockIcon`, `localWeb`, `width`, `height`, `widthMeters`. Pixel width is an integer 640–2560, height an integer 360–1440, and physical width follows the core's float32 validation for 0.4–4.0 meters. Omitted dimensions normalize to 1600×900 and 3 meters, matching pack output. Custom dimensions participate in source/package comparison; undeclared changes are rejected. The template's default standalone window passed actual packaging and database validation. Registration still requires whole-package hash, ownership and other field checks.

## Option 1: host your own catalog

Self-hosting requires neither community registration nor GitHub Release digests. You may omit packaged `downloadUrl`: `framely catalog` joins `--base-url` with the actual archive filename. Explicit package URLs take precedence. For default GitHub names, use the workflow above.

Put all versions to retain in `packages/`. From the Framely repository:

```bash
mkdir -p target/plugin-catalog
cargo run --locked -- catalog --name 'My source' --base-url https://example.org/packages --packages ./packages --output ./target/plugin-catalog/catalog.json
```

Replace `example.org` with your HTTPS address. The CLI computes whole-package SHA256 and generates JSON; it does not upload packages or copy images. Without external `publish.icon`, the local CLI does not convert a packaged icon into a store URL.

**Upload the entire generated directory, not only `catalog.json`:**

```text
catalog.json
plugins/
  yourname.my-plugin/
    versions.json
```

The main catalog has one recommended entry per ID. Relative `plugins/<ID>/versions.json` contains historical entries, excluding the current main entry. Supply every retained version each time; the CLI does not merge prior history. The community database does preserve previous history, currently up to 19 historical entries per plugin. These retention rules differ.

Host the catalog over HTTPS and share its full `catalog.json` URL. Package/image URLs must also be reachable. Multiple channels may use a [source subscription](../source-subscriptions.md).

## Option 2: register in the community database

Use [framely-plugin-database](https://github.com/SteamFramelyHomebrew/framely-plugin-database) and its current [CONTRIBUTING.md](https://github.com/SteamFramelyHomebrew/framely-plugin-database/blob/testing/CONTRIBUTING.md)/validator. Plugin roots require `manifest.json`. Publish the author's Release/assets first, then register pinned Git submodule commits. The database does not mirror packages or establish reproducible source/binary equivalence.

Fork the database and use your fork's matching `testing` branch:

```bash
git clone --branch testing https://github.com/yourname/framely-plugin-database.git
cd framely-plugin-database
git submodule add https://github.com/yourname/my-plugin.git plugins/yourname.my-plugin
git -C plugins/yourname.my-plugin checkout v0.1.0
git add .gitmodules plugins/yourname.my-plugin
git commit -m 'Add yourname.my-plugin 0.1.0'
git push origin testing
```

Open a PR to upstream `testing` with actual Frame test records. Change only `.gitmodules` and `plugins/<ID>` submodule pins; ordinary file changes do not qualify for registration auto-merge.

Database IDs use `namespace.name`, lowercase letters/digits and dots/hyphens in the name, stricter than core IDs. Paths must be `plugins/<ID>` and source repositories public GitHub repositories. Initial registration verifies ownership and maintenance permission. Namespaces bind to GitHub owner numeric IDs; each owner may reserve at most five historical prefixes, and removal does not release ownership. Registered IDs cannot change source repositories. Collaborators require verifiable write/maintain/admin permission; Manifest authors or Git commit authors do not establish permission. See [full ownership rules](https://github.com/SteamFramelyHomebrew/framely-plugin-database/blob/testing/CONTRIBUTING.md#插件归属与自动合并).

Update registration:

```bash
git -C plugins/yourname.my-plugin fetch --tags
git -C plugins/yourname.my-plugin checkout v0.2.0
git add plugins/yourname.my-plugin
git commit -m 'Update yourname.my-plugin to 0.2.0'
git push origin testing
```

Stable corresponds to `main`, testing to `testing`. Auto-merge requires `testing → testing` or `main → main`; cross-channel PRs require manual handling. Promote after tests under database rules. `publish` is generated, not a target for plugin changes.

## Verify publication

Check public packages/catalogs/icons/history, installation confirmation, updates and retained data. Core `verify` alone is insufficient: registration must pass database source, whole-package hash and field checks. For updates: bump version → regenerate temporary Manifest → build/package → device test → new Release → update the source pin. Never overwrite old assets.
