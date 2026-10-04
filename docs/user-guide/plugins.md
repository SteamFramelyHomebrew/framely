# Install and manage plugins

[简体中文](../zh-CN/user-guide/plugins.md)

[Documentation](../README.md) · [Network panel](network-panel.md)

## Install from the plugin library

1. Open **Plugin library** in the manager. Current code initializes community stable/testing sources; availability depends on the actual service and device configuration. If no catalog is available, add a developer's complete HTTPS `catalog.json` URL in Sources.
2. Filter by keywords, tags, source or installation state.
3. Open details and review author, description, changes, runtime user and dependencies. The recommended version is shown first; history loads on demand.
4. Install or inspect the selected version. Read the confirmation after download, verification and dependency resolution.
5. Check actual package runtime users, including any root dependencies, then confirm. Enable or open the plugin from the installed list.

Catalog runtime users are informational; final confirmation uses the downloaded package. `steamos` runs as the Steam session user; `root` can change system and device settings. Hashes check content, not developer identity or safety.

## Sources and subscriptions

For one catalog, add a name and `https://.../catalog.json` URL in Sources, test the connection, then save.

For multiple catalogs, add a subscription JSON URL in Source subscriptions, preview its sources, then confirm. A subscription is different from a catalog. Refreshing it changes source lists and metadata, not installed plugins.

Disabling or deleting a source does not uninstall its plugins. Subscription URL changes require confirmation. See the [subscription reference](../source-subscriptions.md).

## Import a file or package URL

Packages use the `.framely` extension and need no manual extraction. Open import in the manager's Plugins page:

- **Local file**: select a downloaded `.framely` file on your phone/computer in the web panel; it uploads and is verified.
- **Package URL**: paste a fixed-version HTTPS download URL from the author and wait for verification.

Both require reviewing confirmation, with an additional confirmation for changed runtime users. Device tar.gz archives and ordinary ZIP files are not plugin packages.

You can also install from Frame's SSH terminal:

```bash
sudo /var/lib/framely/current/bin/framely install ./plugin.framely --approve
```

Add `--approve-run-as` only when you have verified that changing the runtime user is intended. CLI approval does not assess the source's trustworthiness.

## Updates, older versions and removal

Open details for plugins marked updatable, review changes and confirm. Select history to inspect an older version. Switching sources is explicitly disclosed. Settings and favorites are generally retained; a different runtime user uses a different data directory and does not automatically migrate the old one.

Uninstall through the management dialog. Hooks handle external cleanup and saved data is retained. Do not remove system plugin directories manually. Disabling stops use without removing programs or data.

## Troubleshooting

| Symptom | Action |
| --- | --- |
| Empty plugin library | Check enabled sources, connectivity and filters |
| Download/hash failure | Check network and URL; download again without bypassing verification |
| Missing history | The source may lack its history file; use latest or a trusted local old package |
| Cannot enable | Read dependency/conflict errors and install required plugins |
| Repeated crashes | Disable, inspect logs, use safe mode if needed, and report to the author |
