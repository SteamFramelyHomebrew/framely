# Basic usage

[简体中文](../zh-CN/user-guide/basic-usage.md)

[Documentation](../README.md) · [Installation](installation.md)

## Quick menu and manager

Click Framely in the SteamVR Dashboard Dock. First use requires accepting the terms and privacy statement; plugins do not automatically start before consent.

- **Favorites**: open favorite plugin quick pages.
- **Installed**: browse local plugins and their pages.
- **Settings**: language, safe mode and the plugin manager entry.

The manager opens a separate large window with navigation for plugins, plugin library, sources, settings and About. Installation, updates and source management happen here. A phone or computer can also use the [network panel](network-panel.md).

## Plugins and windows

Enable, disable or favorite plugins in the list. Enabled means available for use; backends normally start on demand. Only plugins declaring `autostart` remain resident after startup. Closing a plugin window generally does not disable its backend. Dependency or conflict problems are explained, and affected plugins are shown before changes.

Dock windows use SteamVR close controls. Temporary windows may be destroyed when switching views. Individual plugin interfaces and behavior are maintained by their developers.

## Settings and diagnostics

Settings provides language, network panel and proxy configuration. HTTP and GitHub proxies have independent switches; saving an address does not automatically enable it.

Safe mode pauses third-party plugins for troubleshooting; leave it after resolving the problem. About shows the device version, updates and terms. Revoking consent disables all plugins; accepting again does not automatically re-enable them.

In About, select the Stable or Testing update channel and check for updates. Choose **Download and install**, then confirm once: Framely downloads and verifies the release and automatically starts installation. Download and verification are displayed as separate stages, with byte progress for each. Download or verification failure stops installation and displays an error. Plugins and data remain; the Framely interface briefly closes and restarts.

Log commands are in the [installation guide](installation.md). A working manager does not establish that all plugins work. Enable plugins individually and verify their sources and runtime users.
