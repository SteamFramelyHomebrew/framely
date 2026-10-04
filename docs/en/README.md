# Framely documentation

[简体中文](../README.md)

Framely manages plugins on Steam Frame through device services, a SteamVR interface, a web panel, and a desktop installer.

## User guides

1. [Install, update, repair and uninstall](user-guide/installation.md): desktop installer, SSH, Preview builds, local packages and rollback.
2. [Basic usage](user-guide/basic-usage.md): Dock, manager, enabling plugins, favorites and safe mode.
3. [Install and manage plugins](user-guide/plugins.md): store, sources, subscriptions, local packages and updates.
4. [Network panel](user-guide/network-panel.md): address, first password setup, login and phone/desktop access.

## Plugin development and publishing

- [Complete workflow](developer-guide/README.md): scaffold → preview → backend → package → device testing → publish.
- [SDK API](developer-guide/sdk.md) · [Manifest configuration](developer-guide/manifest.md) · [Lifecycle](plugin-lifecycle.md)
- [Publish plugins and catalogs](developer-guide/publishing.md): default GitHub URLs, custom URLs/hashes, catalog history and database registration.
- [Plugin template](../../templates/plugin/README.en.md): React and Python, generated projects or in-repository editing.
- [Showcase plugin](../../examples/showcase/README.en.md): additional UI and device examples.

## Technical references

| Reference | Contents |
| --- | --- |
| [Plugin development reference](plugin-development.md) | Manifest, windows, notifications, JSON RPC and packaging |
| [Lifecycle](plugin-lifecycle.md) | Install, update, start, stop, uninstall and recovery hooks |
| [Dependencies and conflicts](plugin-relationships.md) | Version ranges, source selection and exclusive resources |
| [Source subscriptions](source-subscriptions.md) | JSON format, refresh, URL changes and deduplication |
| [Localization](localization.md) | Language packs and community translations |

## Maintainer documentation

- [Framely releases and updates](releases.md)
- [Desktop installer](../../installer/README.en.md)
