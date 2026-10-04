# Plugin development reference

[简体中文](zh-CN/plugin-development.md)

Start with the [workflow](developer-guide/README.md), [SDK](developer-guide/sdk.md), [Manifest](developer-guide/manifest.md) and [publishing](developer-guide/publishing.md).

The Dock quick menu has Favorites/Installed/Settings tabs; management opens a separate Dock window. Installation and sources are handled there. See `examples/showcase` and `sdk/src/index.tsx`. Release `tools/source` includes UI/SDK/examples/template/tools. Bundle pages into browser JS and call `registerPlugin({QuickPage, WindowPage})`. The host installs the bridge first. Ordinary bundled page iframes use `sandbox="allow-scripts"` with opaque origins and cannot call administrator HTTP APIs.

## Manifest and identities

```json
{
  "schemaVersion": 1, "apiVersion": 1,
  "id": "example.plugin", "name": "Example", "author": "Developer", "version": "1.0.0",
  "description": "Example plugin",
  "backend": {"entry": "backend.py", "runAs": "steamos", "autostart": false},
  "ui": {"quickPage": "page.js", "windows": {"main": {"entry": "page.js", "title": "Example", "dockIcon": true}}},
  "files": {}
}
```

Backend is optional. Runtime users are `steamos` (default) or `root`; autostart defaults false. Retired `permissions` is unsupported; windows, notifications and networking need no declarations. Backend networking uses the device; ordinary pages permit HTTP/HTTPS and WebSockets.

IDs permit lowercase letters, numbers, dots, hyphens, underscores. Payload paths permit ASCII letters/numbers and `/._-+`, never absolute paths, empty/dot/dot-dot segments. Packaging supplies files hashes. Entries must be hashed; executable scripts need shebangs, e.g. `/usr/bin/python3`. Framely does not install system/Python dependencies.

Published versions are immutable. User changes select `/var/lib/framely/data/<id>/<identity>`; old data remains without automatic migration, and reinstalling the old identity can access it again.

## React bridge

```tsx
import {framely, registerPlugin, Button, Section} from '@framely/sdk';
function QuickPage() {
  return <Section title="Example">
    <Button onClick={() => framely.windows.open('main')}>Open</Button>
    <Button onClick={() => framely.call('save', {text: 'hello'})}>Save</Button>
    <Button onClick={() => framely.notifications.send({id: 'download', title: 'Done', body: 'Downloaded', durationMs: 8000, actions: [{id:'open',label:'Open',icon:'↗'}]})}>Notify</Button>
  </Section>;
}
registerPlugin({QuickPage});
```

Keys must match Manifest. Closing windows does not stop backend. IDs update/withdraw notifications. Images accept PNG/JPEG data URL ≤1 MiB or HTTPS. At most three actions, 1–60 seconds and ten sends per ten seconds/plugin. `onEvent` returns unsubscribe and receives `{type,data}`. Each window has its own event cursor. Input focuses the native keyboard rather than web prompts.

## Backend protocol

Newline-delimited JSON on stdin/stdout; logs on stderr, rotating around 2 MiB. Messages ≤64 KiB; 5-second input or 15-second response timeouts stop the backend and allow later retry.

```json
{"id":1,"method":"save","params":{"text":"hello"}}
{"id":1,"result":{"saved":"hello"}}
{"id":2,"error":"Unknown method"}
{"event":"progress","data":{"percent":50}}
```

Environment includes plugin ID, data directory and the user's HOME. Notification actions invoke `notification.action` with `{id,action}` and also reach pages; UI-only plugins can listen. Backend `notification` events send notifications under the same limits.

## Packaging and catalogs

```bash
framely pack --manifest manifest.json --payload payload --output example.plugin-1.0.0.framely
framely verify example.plugin-1.0.0.framely
framely catalog --name 'My plugins' --base-url https://example.org/plugins --packages ./packages --output ./packages/catalog.json
```

Archives contain Manifest and hashed payload only, without keys/signatures. Host the main catalog and relative history JSON. Authors can host packages on GitHub Releases. Current first initialization includes community stable/testing sources. Origins remain explicit; same-ID plugins in other sources do not silently replace existing origins. HTTP needs explicit development permission; up to five validated redirects, without HTTPS downgrade.

## Hover feedback

Interactive controls automatically receive one hover pulse per entry, excluding disabled/inert controls and movement between their children. Supported elements include buttons, links, inputs, interactive ARIA roles/focusable controls or pointer cursors. Custom controls may mark `data-framely-interactive`. Native feedback uses the current hit controller, 12 ms/120 Hz/amplitude 0.15 with at least 80 ms spacing. Pages cannot target other windows/controllers.

## Metadata and publication

Manifest supports payload PNG icon ≤1 MiB/1024×1024, details/tags/changelog and up to eight PNG/JPEG screenshots. Icons appear in lists/Dock; missing icons use the default. Optional `authorUrl`, `documentationUrl`, `homepage` are credential-free HTTP/HTTPS links, preserved in state/catalog. Author URLs link author labels; device default browsers open external pages.

```json
{
  "downloadUrl": "https://github.com/author/plugin/releases/download/v1.0.0/plugin.framely",
  "publish": {"icon": "https://github.com/author/plugin/releases/download/v1.0.0/icon.png", "screenshots": ["https://github.com/author/plugin/releases/download/v1.0.0/screen.png"]}
}
```

The fragment above shows explicit URLs; GitHub registration source may omit `downloadUrl`, deriving repository, `v<version>` and `<id>-<version>.framely`. The packaged Manifest still requires the final URL; current pack does not derive it. See [publishing](developer-guide/publishing.md) for temporary-Manifest commands. Community icons normally use top-level `icon`, with derived and verified pinned Raw URLs, without `publish.icon`. The local catalog CLI uses external `publish` images and does not export packaged image paths; absent external icons use the plugin library default. Community registration pins submodule commits, obtains expected SHA256 from Release digests or source `downloadSha256`, then validates downloaded package hashes, declarations and payloads. Generated channels provide catalog/history JSON without mirroring packages/images. It does not execute author code or establish reproducible source/binary equivalence.

Catalog CLI only emits JSON, preferring `downloadUrl` over base URL/filename and using external publish images. It does not copy resources. The plugin library filters keywords/tags/origins/states, retains session caches on failures and installs reviewed bytes without downloading again after confirmation.

## Scaffold and preview

```bash
node tools/plugin-dev.mjs init ./my-plugin example.my-plugin
cd my-plugin
npm install
npm run dev
npm run build
```

Release tool location: `tools/source/tools/plugin-dev.mjs`. Preview binds `127.0.0.1:5173` and reloads changes, simulating windows/notifications only. Backend calls, identities, keyboards and haptics require Frame. SDK includes Section/Button/Toggle/Slider/TextField/Select/Tabs/Notice/useBackend/usePluginEvent; window maps can register different components. See [SDK](developer-guide/sdk.md).

## Windows, tags and history

Default windows are 1600×900 and 3 meters, with width 640–2560, height 360–1440, physical width 0.4–4.0. Reopen to apply changes. A `localWeb` window asks backend `window.get` for `http://localhost:<port>/framely-window/<key>`; this is a loading mode, not a permission.

Use tags instead of legacy category, accepted only for compatibility. Database `tags.json` aggregates, deduplicates and sorts current channel tags; each channel is separate, clients merge sources locally. Tags change automatically with releases.

Packages/dependencies/uploads stage in `/tmp/framely-package-*`, removed on completion/cancel/failure. Idle uploads/reviews expire after 15 minutes, checked every 30 seconds.

Main `catalog.json` has one latest entry per ID. `plugins/<ID>/versions.json` contains `{schemaVersion:1,id,versions:[...]}` without that main entry. Details load history on demand, as does resolution when latest fails a range. Unknown catalog fields are ignored while known fields are checked; changed required semantics need a schema bump. Package Manifests remain strict. See [relationships](plugin-relationships.md), [subscriptions](source-subscriptions.md) and [lifecycle](plugin-lifecycle.md).
