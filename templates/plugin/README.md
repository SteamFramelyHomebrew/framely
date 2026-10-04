# Framely plugin template

[简体中文](README.zh-CN.md)

This template includes a React quick page, a separate Dock window, notifications, a Python JSON RPC backend, and persistent storage. It runs as `steamos` by default and does not request root.

Generate a standalone project from the Framely repository. The scaffolder copies the SDK, licenses, and build tools:

```bash
node tools/plugin-dev.mjs init ../my-plugin yourname.my-plugin
cd ../my-plugin
npm install
npm run dev
npm run build
```

Edit the ID, name, author, description, version, tags, and update notes in `manifest.json`. Page source is in `page.tsx`, backend source in `backend.py`, and build output in `payload/`. Leave `files` as an empty object; the packager calculates it.

Browser preview is available at `http://127.0.0.1:5173` and simulates windows and notifications. Read/save buttons call the actual backend and require installation on Frame for testing; the browser reports that preview does not start a backend.

On Linux/WSL with Rust, set the Framely repository path, then package and verify (the core CLI uses Linux-specific interfaces):

```bash
FRAMELY_REPO=/absolute/path/to/framely
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- pack --manifest manifest.json --payload payload --output my-plugin-0.1.0.framely
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- verify my-plugin-0.1.0.framely
```

Upload the package to Frame using Plugins → Import plugin in the network panel. Review the runtime user and confirm. For updates, increment the version, rebuild, and package again. Store backend data in `FRAMELY_DATA_DIR`, not in the program directory.

To edit this template inside the Framely repository, run `npm install`, `npm run dev`, and `npm run build` in `templates/plugin/`. Copying only this directory elsewhere requires adjusting SDK and build-tool paths in `package.json`; the scaffolder handles this automatically.

The template and bundled SDK use `AGPL-3.0-only`. See Framely's `docs/developer-guide/README.md` for the full development and publishing workflow. The template lifecycle callbacks preserve user data.

For community GitHub registration, source may omit `downloadUrl`; default assets use `<id>-<version>.framely`. Release packages need a temporary Manifest with the derived URL. See Framely's `docs/developer-guide/publishing.md` for commands, Release digests, custom URLs and database window-dimension validation. The direct packaging command above is for core testing and does not establish community registration acceptance.
