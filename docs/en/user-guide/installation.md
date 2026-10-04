# Install, update, repair and uninstall

[简体中文](../../user-guide/installation.md)

[Documentation](../README.md) · [Next: basic usage](basic-usage.md)

## Before installation

The target is Linux ARM64 Steam Frame. The development baseline is SteamOS VR 0.4.2 and SteamVR build 20260928.6175029; other versions need validation. The runtime includes CEF, so users do not need Node.js or Rust on the device.

Connect your computer and Frame to the same network. Enable developer mode and SSH on Frame, and prepare the `steamos` account password. Verify the SSH fingerprint. Installation uses sudo to create an account, systemd services and release directories. It does not automatically disable SteamOS read-only protection; it stops if required directories cannot be written.

## Option 1: desktop installer

1. Download the appropriate installer from an `installer-v<version>` entry in [Releases](https://github.com/SteamFramelyHomebrew/framely/releases). Linux uses tar.gz, Windows an EXE ZIP, and macOS an application ZIP.
2. Extract and run it. Scan for the device or enter its IP and SSH port.
3. Verify the fingerprint, enter `steamos` and its password, connect, and select a Framely device release.
4. Enable testing releases to select a Preview. Installer and device versions are independent.
5. Confirm the device, version and operation on the maintenance page. Alternatively, select a local device archive and its external `SHA256SUMS`.

On Linux, run `bash install-desktop-entry.sh` from the extracted installer directory to add an entry with the Framely icon to your user application menu. The entry points to that directory; run the script again after moving it.

Downloads are checked locally and again on the device. Passwords are not saved in configuration. Observe startup and rendering on Frame after installation.

## Option 2: install from Frame's SSH terminal

Run these commands **on Frame**. To install the latest stable release:

```bash
curl -fsSL https://raw.githubusercontent.com/SteamFramelyHomebrew/framely/main/install.sh | bash -s -- install
```

To select a Preview or another tag:

```bash
curl -fsSL https://raw.githubusercontent.com/SteamFramelyHomebrew/framely/main/install.sh | bash -s -- install --version v0.4.2-preview.3
```

Replace the example with an existing device release tag. An explicit tag selects the engine and package from that Release and works without a stable release. Without a tag, the latest stable release is selected. Enter the sudo password when prompted.

## Option 3: local archive

Download `framely-<version-and-build>-linux-arm64.tar.gz`, the external `SHA256SUMS`, and `bootstrap.py` from the same Release. Place them together on Frame:

```bash
python3 bootstrap.py install --archive "./framely-<version-and-build>-linux-arm64.tar.gz" --checksums ./SHA256SUMS
```

Angle brackets are placeholders; substitute the actual filename. The engine verifies, stages and extracts under `/home`, then checks internal hashes, avoiding the device's limited `/tmp`.

## Verify installation

```bash
systemctl is-active framely.service framely-session.service
/var/lib/framely/current/bin/framely --version
sudo /var/lib/framely/current/bin/framely status
```

Both services should be `active`. Open SteamVR Dashboard and find Framely in the Dock. Read and accept the terms and privacy statement before managing plugins. Continue with the [plugin installation guide](plugins.md).

## Update and rollback

When an update source is configured, check, download and confirm an update in **About**. Alternatively, use the desktop installer or run on Frame:

```bash
curl -fsSL https://raw.githubusercontent.com/SteamFramelyHomebrew/framely/main/install.sh | bash -s -- update --version v0.4.2-preview.3
```

Updates preserve plugins, settings and data, retain the previous release, and restart Framely only. Preview updates require an explicit tag or local package; the default stable source does not automatically select Preview builds.

Choose rollback in About, or run:

```bash
sudo bash /var/lib/framely/current/rollback.sh
```

Rollback preserves plugin data but does not guarantee conversion of newer data formats to older ones.

## Repair and logs

SteamOS updates may reset system accounts and services. State persists under `/home/.framely/state`; `/var/lib/framely` is a compatibility entry. If system configuration is writable and `/home` remains intact:

```bash
sudo bash /home/.framely/repair.sh
sudo journalctl -u framely -u framely-session --no-pager -n 100
```

Repair restores accounts and services from the retained release and stops on UID conflicts. It cannot restore deleted `/home` data. Recovery after SteamOS updates still requires device validation.

## Uninstall

Use the desktop maintenance page or run:

```bash
sudo bash /var/lib/framely/current/uninstall.sh
```

Uninstallation disables and uninstalls every plugin, including uninstall hooks, before removing Framely programs and services. Failed plugin cleanup retains Framely and reports an error; resolve it and retry. Settings, plugin data and the dedicated account remain. External changes made by third-party plugins cannot always be automatically undone.
