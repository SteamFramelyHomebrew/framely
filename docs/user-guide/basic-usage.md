# Basic usage

[简体中文](../zh-CN/user-guide/basic-usage.md)

[Documentation](../README.md) · [Installation](installation.md)

## Quick menu and manager

Click Framely in the SteamVR Dashboard Dock. First use requires accepting the terms and privacy statement; plugins do not automatically start before consent.

- **Favorites**: open favorite plugin quick pages.
- **Installed**: browse enabled local plugins and their pages.
- **Notifications**: view saved notifications and update reminders, and toggle floating notifications.
- **Settings**: choose an installed language, toggle safe mode, open the plugin manager or **Check for updates**. The update entry opens the manager’s About/update page. Install language files and download the language template from the manager’s Settings page.

The manager opens a separate large window with navigation for plugins, plugin library, sources, settings, notification settings and About. Installation, updates and source management happen here. A phone or computer can also use the [network panel](network-panel.md).

## Plugins and windows

Enable, disable or favorite plugins in the list. Enabled means available for use; backends normally start on demand. Only plugins declaring `autostart` remain resident after startup. Closing a plugin window generally does not disable its backend. Dependency or conflict problems are explained, and affected plugins are shown before changes.

Dock windows use SteamVR close controls. Temporary windows may be destroyed when switching views. Individual plugin interfaces and behavior are maintained by their developers.

## Settings and diagnostics

Settings provides language, network panel and proxy configuration. HTTP and GitHub proxies have independent switches; saving an address does not automatically enable it.

Safe mode pauses third-party plugins for troubleshooting; leave it after resolving the problem. About shows the device version, updates and terms. Revoking consent disables all plugins; accepting again does not automatically re-enable them.

In About, select the Stable or Testing update channel and check for updates. Choose **Download and install**, then confirm once: Framely downloads and verifies the release and automatically starts installation. Download and verification are displayed as separate stages, with byte progress for each. Download or verification failure stops installation and displays an error. Plugins and data remain; the Framely interface briefly closes and restarts.

Automatic update checks are enabled by default. Framely checks about 30 seconds after starting (once terms are accepted), then every 6 hours. In About, turn **Check for updates automatically** off or change the interval. Checks use the selected Stable/Testing channel and configured proxy, continue with the panel closed, and never download or install automatically. A newer version saves a notification to the inbox and shows a popup when notification settings allow it. **Ignore** dismisses that version's reminder; **Open updates** opens About's update page. The same version is not repeatedly announced, including after restart. Switching the source or channel clears the old reminder.

Floating notifications follow the headset near the upper-right of the view. Their input mask matches the visible card; short button clicks tolerate small tracking movements.

Notification popups display an auto-close countdown when the sender sets a duration. Closing a popup keeps any saved inbox copy. Remove a saved message from Notifications with its trash button. Action buttons close and remove messages by default; senders can configure different behavior. Ordinary notifications are not saved unless the sender opts in, except when popups are disabled.

The **Allow notifications** switch at the top of Notifications controls all floating popups. When off, new notifications are saved in the inbox even if the sender did not request it; active popups are also moved to the inbox. Turning popups back on does not replay old messages.

In the manager, **Notification settings** provides global **Floating notifications** and **Launcher badge** switches, plus independent **Badge** and **Popup** switches for Framely itself (including update reminders) and every installed plugin, including disabled plugins. Global switches take priority and preserve each app’s preferences. The Framely Dock launcher badge counts unread inbox messages from sources allowed to show a badge, displays `99+` for larger counts, and disappears when you view the inbox. Hidden headset panels do not mark messages as read. Reading keeps messages and their actions available; it does not remove them. Settings and read state survive restart.

Log commands are in the [installation guide](installation.md). A working manager does not establish that all plugins work. Enable plugins individually and verify their sources and runtime users.

## Export logs

Open **Settings → Recovery and diagnostics → Export logs**. In the headset, the ZIP is saved in `~/Downloads` on Frame and the panel shows its path. From the network panel, the ZIP downloads to your browser instead. Collection includes recent Framely/plugin logs, CEF renderer logs, GPU evidence, service status and version information. Missing sources or failed commands are recorded in `report.json`; they do not prevent exporting the remaining information. Each source and the total archive input are bounded.

If the panel cannot open, connect to Frame using the installer and choose **Export logs** in its footer. Select where to save the ZIP on your computer. It collects directly over SSH, includes the installer’s current operation log, and works when Framely services are stopped or its state file is damaged. No release package selection is required.

Plugin configuration, inbox contents and password files are excluded. Common credential patterns in logs are masked; review application logs before sharing the archive. Nothing is uploaded automatically.

For terminal recovery without a running daemon:

```bash
sudo /var/lib/framely/current/bin/framely export-logs --output /tmp/framely-logs.zip
```

The output must be a new file. If the binary cannot run, an extracted release also includes `tools/export-diagnostics.py`, usable with `sudo python3 tools/export-diagnostics.py --output /tmp/framely-logs.zip`.

## Scrolling in VR

Point the laser at a page and move either controller's stick to scroll, without clicking or focusing the page first. Scrolling follows the element under the laser, including nested plugin panels. Keyboard input pauses page scrolling. Launcher stick navigation keeps its category/page controls.

Hold the primary trigger and move the laser up or down to drag scrollable page content. Small movements remain ordinary clicks; once a vertical drag starts, releasing does not activate the pressed button. Text inputs, sliders and explicit custom drag targets keep their own interactions. The launcher retains its icon arrangement and horizontal page-drag gestures.
