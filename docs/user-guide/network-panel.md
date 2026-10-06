# Network management panel

[简体中文](../zh-CN/user-guide/network-panel.md)

[Documentation](../README.md) · [Plugin installation](plugins.md)

## Find the address

The panel is enabled by default on IPv4 port `15915` and does not require SteamVR to run. Connect your phone/computer to Frame's network and open:

```text
http://DEVICE_IP:15915
```

Settings → Network panel shows current IP addresses and full URLs. The desktop installer also provides an entry after connecting. If no IP is detected, check Wi-Fi/Ethernet. Port conflicts are reported in settings.

## First password setup

With no configured password, first access shows setup only and does not expose management APIs. Enter at least eight characters and confirm the password, save, then log in. Password setup may precede terms acceptance; consent is still required to manage plugins.

A randomly salted PBKDF2 hash is stored, survives reboot, and is not displayed by the UI. Unauthenticated setup cannot overwrite an existing password. The panel password is independent of SSH/sudo credentials.

Change passwords from authenticated settings or the device's native manager. If forgotten, prefer changing it from Frame's native window. Existing passwords are retained; a previously configured password with verification explicitly disabled retains that choice. The panel uses HTTP: use a trusted network and do not forward the port directly to the public internet.

## Available operations

Manage sources and subscriptions, browse the plugin library, upload local packages, confirm installation, enable/disable plugins, change settings and check device updates. Backends always run on Frame; uploading a file transfers it from your phone/computer.

VR windows, controller vibration and SteamVR keyboard need headset validation; a browser cannot replace those interactions. Some external links open in the device's default browser.

## Port changes, shutdown and expired sessions

Change the port or switch in Settings → Network panel. Password/authentication changes invalidate old sessions; use the new address after a port change. Turning the panel off disconnects web access without closing the native manager.

Restarting the session service also invalidates old logins. Saving identical settings retains the session. Incorrect password checks are rate-limited; retry later when asked.

If unreachable, check changing device IPs, network reachability, enabled state and port. Settings and passwords live in `/home/.framely/state` and are retained by uninstall; reinstalling may still require the old password.

The top switcher provides Plugin, APK, Terminal, Files and Settings; the device address defaults to Plugin. [Terminal and files](terminal-and-files.md) provides user-owned shell sessions, uploads/downloads, editing, archives and previews. These tools use the same login and port.
