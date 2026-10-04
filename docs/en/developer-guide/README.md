# Plugin development to publication

[简体中文](../../developer-guide/README.md)

[Documentation](../README.md) · [Publishing](publishing.md)

## 1. Prepare the development environment

Install Git, Node.js 22 and npm. The publishing Manifest script also needs Python 3. The core CLI uses Linux-specific interfaces, so source packaging requires Rust on Linux/WSL, not native Windows/macOS compilation. Alternatively copy Manifest/payload to Frame and use its installed CLI. Device users do not need Node.js/Rust; Python backends use device Python 3. Preview and packaging work on the development computer; VR interaction requires Frame.

```bash
git clone https://github.com/SteamFramelyHomebrew/framely.git
cd framely
node tools/plugin-dev.mjs init ../my-plugin yourname.my-plugin
cd ../my-plugin
npm install
npm run dev
```

Replace the example ID with your own stable namespace. The scaffold refuses to overwrite directories and copies the SDK and tools, so the resulting project does not depend on the original repository location. Create your plugin Git repository and commit `package-lock.json` and the vendored SDK. Do not commit `node_modules/`, `payload/`, archives or passwords.

## 2. Understand the template

| File | Purpose |
| --- | --- |
| `manifest.json` | Identity, version, runtime user, pages, lifecycle and dependencies |
| `page.tsx` | React quick page and independent window |
| `backend.py` | Python backend and persistent settings |
| `vendor/framely-sdk/` | UI, bridge API and Python protocol helper |
| `dev.mjs` | Build and local preview |
| `LICENSE` | Full AGPLv3 text |

Read/save uses `settings.get` / `settings.set` with `FRAMELY_DATA_DIR/settings.json`. The template opens the `main` window, sends notifications and demonstrates install/update/start/stop/uninstall callbacks. It defaults to `steamos` and on-demand startup.

## 3. Edit Manifest and UI

Set the ID, author, name, description, version, tags and changelog. Use full SemVer such as `0.1.0` or `0.2.0-preview.1`; never overwrite a published version.

`ui.quickPage` and `ui.windows.main.entry` refer to built `page.js`. Register components with `registerPlugin({QuickPage, WindowPage})`, matching declared keys. Leave `files` empty; packaging computes hashes.

Preview at `http://127.0.0.1:5173`, with automatic reload. Preview simulates windows and notifications, without Python backends or device privileges. Read/save therefore reports backend unavailability; this is expected and is not device validation.

See [SDK API](sdk.md), [Manifest configuration](manifest.md) and the [development reference](../plugin-development.md).

## 4. Implement backend and lifecycle

Backend stdin/stdout use newline-delimited JSON RPC. Reserve stdout for protocol messages and log to stderr. Python `framely.serve` handles requests, exceptions and hooks. Validate parameter types and lengths and return JSON-serializable values.

Write to `FRAMELY_DATA_DIR`, not plugin payload or Framely binaries. Design migrations for updates, older versions and runtime-user changes. `steamos` and `root` have separate data directories. Prefer `steamos`; explain any required root privileges.

Make install/update hooks repeatable and release owned external resources on stop/uninstall. Closing UI does not stop the backend; React effect cleanup cannot replace backend hooks. System dependencies are not installed automatically. See [lifecycle](../plugin-lifecycle.md).

## 5. Build, package and verify

Inside your plugin:

```bash
npm run build
FRAMELY_REPO=/absolute/path/to/framely
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- pack --manifest manifest.json --payload payload --output yourname.my-plugin-0.1.0.framely
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- verify yourname.my-plugin-0.1.0.framely
```

Set the repository's actual absolute path. Alternatively use installed `/var/lib/framely/current/bin/framely pack` / `verify` on Frame; these offline commands do not need sudo.

Include only distributable pages, backend, Python helper and resources in `payload/`. Build copies `backend.py` and `framely.py`; add icons and other assets yourself. Packaging checks entries, paths, Manifest and hashes, rejecting symlinks and undeclared extra archive files.

## 6. Test on Frame

Import the package through the web panel and review confirmation, or transfer it and run on Frame:

```bash
sudo /var/lib/framely/current/bin/framely install ./yourname.my-plugin-0.1.0.framely --approve
```

Accept the terms before management. Test read/save, windows and notifications, disabling/re-enabling, persistent data after reboot, updates after a version bump, and uninstall cleanup. Record actual device checks for VR input, controllers and hardware.

Inspect Framely status and systemd logs. Do not mix debug messages into backend stdout. See [installation](../user-guide/plugins.md).

## 7. Publish

Default GitHub Releases use tag `v<version>` and asset `<id>-<version>.framely`; source may omit `downloadUrl`. Community registration derives it automatically. Prepare a temporary packaged Manifest containing the final URL as shown in [publishing](publishing.md), because current `framely pack` does not read Git remotes. Set an explicit URL for custom downloads. Publish the Release/assets with usage, runtime-user details, changes and actual test coverage, then host a catalog or register a fixed source commit. Database registration also checks expected whole-package hashes and field compatibility.

For updates: bump version → regenerate the release Manifest (update explicit custom URLs if used) → rebuild/verify → device test → new Release → update the source pin. Never replace old assets.
