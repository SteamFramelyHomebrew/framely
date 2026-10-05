# Manifest configuration

[简体中文](../zh-CN/developer-guide/manifest.md)

[Development](README.md) · [SDK](sdk.md) · [Lifecycle](../plugin-lifecycle.md)

The repository-root `manifest.json` describes a plugin and is included in its archive. Fields use camelCase. Package Manifests reject unknown fields; do not add retired `permissions`. Start from the [template](../../templates/plugin/manifest.json).

## Minimal example

```json
{
  "schemaVersion": 1,
  "apiVersion": 1,
  "id": "yourname.my-plugin",
  "name": "My plugin",
  "version": "0.1.0",
  "author": "Your name",
  "backend": {"entry": "backend.py", "runAs": "steamos"},
  "ui": {"quickPage": "page.js"},
  "files": {}
}
```

Leave source `files` empty. `framely pack` scans payload and supplies hashes. At least one payload file is required; all declared entries must be included. See the [complete template](../../templates/plugin/manifest.json).

## Top-level fields

| Field | Requirements/default |
| --- | --- |
| `schemaVersion` / `apiVersion` | Required, currently both 1 |
| `id` | Required, max 80 characters; lowercase letters/digits/dots/hyphens/underscores; no leading dot or `..`; the scaffold requires a letter/digit first; prefer `namespace.name` |
| `name` / `author` | Required, nonempty, max 120 bytes each |
| `version` | Required, max 64 bytes, ASCII letters/digits and `.-+` only; full SemVer recommended and required when declaring relationships |
| `description` | Short summary, default empty; database registration limits it to 4096 bytes |
| `details` / `changelog` | Full description/version changes, default empty; max 32768 / 16384 bytes respectively |
| `tags` | String array, default empty; max 12 nonempty entries of at most 80 bytes each; search/filter tags |
| `icon` | Optional payload PNG path, max 1 MiB and 1024×1024 |
| `screenshots` | Default empty; up to eight payload PNG/JPEG paths |
| `authorUrl` / `documentationUrl` / `homepage` | Optional HTTP/HTTPS links, no embedded credentials |
| `downloadUrl` | Optional fixed-version HTTPS package URL; GitHub database source may omit it for repository/version/ID derivation, but community packages need the resolved URL; see publishing |
| `publish` | Optional external plugin library image configuration |
| `backend` / `lifecycle` / `ui` | Optional backend/hooks/pages |
| `dependencies` / `optionalDependencies` / `conflicts` | Default empty maps |
| `exclusiveResources` | Default empty array; matching names prevent simultaneous enabling |
| `files` | Required path-to-SHA256 map, generated during packaging, max 2048 files |

Legacy `category` is accepted only for compatibility and no longer exported; use tags. `downloadSha256` is a database source-registration field for whole-package SHA256, not a package field. Default GitHub Releases use asset digests; custom HTTPS URLs require a declared source hash, removed before packaging. The database resolves omitted source `downloadUrl`, but current `framely pack` does not. See [publishing](publishing.md) for a temporary-Manifest command. Database IDs and description lengths have stricter constraints than core validation; passing the core does not guarantee registration.

## Backend

| Field | Meaning |
| --- | --- |
| `entry` | Required executable payload entry; scripts need a valid shebang |
| `args` | Default empty; at most 64 independent args, each ≤4096 bytes without NUL; not a shell command |
| `runAs` | `steamos` (default) or `root` |
| `autostart` | false by default for on-demand startup; true for resident startup |
| `restart` | `on-failure` (default) or `never` |
| `restartLimit` | Consecutive failure threshold 1–10, default 3 |
| `memoryLimitMiB` | Memory limit, default 512 MiB; integer 1–4294967295, no 0/null for unlimited memory |

Omit backend for UI-only plugins. System/third-party Python dependencies are not installed automatically. Runtime-user changes require confirmation and use separate data directories without automatic migration.

`backend.memoryLimitMiB` maps to systemd `MemoryMax` for the combined memory of the backend and all its child processes. It is a ceiling, not a memory reservation. Independent lifecycle hooks inherit this limit; hooks without a backend use 512 MiB. The install plan, CLI and installed-plugin management view display the limit. Omitted or explicit 512 values are omitted when packing to preserve compatibility with older hosts; other values require a host and plugin database supporting this field.

For example, declare 2 GiB for a longer replay buffer:

```json
{"backend": {"entry": "backend", "memoryLimitMiB": 2048}}
```

## UI

Optional `quickPage` points to a payload bundle. `windows` defaults to an empty map with at most eight windows; keys must be valid IDs:

| Field | Meaning |
| --- | --- |
| `entry` / `title` | Required payload entry and nonempty title ≤120 bytes |
| `dockIcon` | false by default; true creates a Dock window |
| `width` / `height` | Default 1600×900; width 640–2560, height 360–1440 |
| `widthMeters` | Default 3.0; finite 0.4–4.0 meters. Source null is accepted but omitted during packaging, then reads back as default 3.0 |
| `localWeb` | false by default; specialized localhost windows obtain a URL from backend `window.get` |

`localWeb` opens on Frame only: backend `window.get` receives `{window: key}` and returns `{url: "http://localhost:<port>/framely-window/<key>"}` without query/fragment or the management port. It uses different sandbox flags for local web apps. Ordinary React plugins do not need `localWeb`. Match window keys to `framely.windows.open(key)` and component registration. Size changes apply when reopening.

## Lifecycle

`onInstall`, `onUpdate`, `onUninstall`, `onCrashCleanup` are optional independent `{entry, args?}` commands. `onStart` / `onStop` are booleans defaulting to false and require a backend. `timeoutSeconds` is 1–15, default 10. Optional `runAs` must match the backend when present and can specify identity for UI-only standalone hooks. Standalone command `args` have the same limits as backend args. See [lifecycle](../plugin-lifecycle.md) for timing, errors and context.

## Publication and relationships

`publish.icon` is an external HTTPS icon, `publish.screenshots` contains up to eight external HTTPS images. Package `icon` and `screenshots` are paths; do not confuse them. Download URLs prefer `downloadUrl`; published versions must remain immutable. See [publishing](publishing.md).

Dependencies use ranges or `{version, source}`, with source pointing to an HTTPS catalog. Conflicts use ranges. Missing optional dependencies do not prevent installation; exclusive resources rely on author declarations. See [relationships](../plugin-relationships.md).

Manifest JSON is limited to 256 KiB and payload paths to 512 bytes. Payload paths allow ASCII letters/digits and `/._-+` only. Absolute paths, empty segments, `.`/`..`, symlinks and undeclared archive files are forbidden. Successful validation is not a safety review; users still assess runtime privileges.

`backend.uiVisibilityEvents` (boolean, default false) subscribes to manager-only `framely.ui.visibility` RPCs. Reply promptly. Delivery does not depend on mounted pages and requires a host supporting visibility events.

## Launcher actions and host compatibility

[Launcher actions and host compatibility](launcher.md)
