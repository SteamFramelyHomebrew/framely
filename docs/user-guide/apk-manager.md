# APK Manager

[简体中文](../zh-CN/user-guide/apk-manager.md)

[Documentation](../README.md) · [Launcher](../launcher.md)

APK Manager installs and manages sideloaded Lepton applications on Steam Frame. Use the header switch to move between Plugin Manager and APK Manager in the headset or authenticated network panel. Visiting the device IP opens Plugin Manager by default. APK Manager has its own Apps, Containers and Data cleanup navigation. It is a file-based manager: it has no store, recommendations, download sources, or automatic APK updates.

## Requirements and discovery

Install Lepton through Steam first. Framely uses the Steam session user's Lepton and Podman; it does not install Android globally or operate as root. Lepton compatibility varies: Google services, vendor APIs and DRM may be unavailable.

Open **APK Manager → Refresh**. Discovery reads the standard Lepton context directories, saved Framely records and running containers' compatibility-data labels. It does not start containers during scanning. Steam-managed APKs appear in the container view; use Steam for their updates and removal.

For data stored elsewhere, open **Containers → Additional data locations** and add the `baked` directory containing `data_overlay/system/packages.xml`. Enter the original Lepton context name when it differs from the enclosing directory. Names cannot conflict with containers belonging to other data locations. Paths must belong to the session user. Removing a scan location does not delete its data. Custom tools can use layouts Framely cannot discover; missing locations must be registered explicitly.

Applications are identified by context plus package name. The same package in two containers is treated as two applications. A context name or location change can therefore create a different identity.

## Install and update

Choose **Install APK**, select an individual `.apk`, and review its name, package, version, minimum Android SDK and target container. Uploads from the network panel are staged on Frame's persistent filesystem, not its limited `/tmp`. Transfer shows actual bytes; subsequent work shows named stages instead of invented percentages.

New apps default to independent containers. Existing containers are offered only when they can accept the target package without overwriting another app's shared APK mount. Lepton recommends one application per container because Android apps do not have the usual enforced isolation inside its containers.

After installation, choose **Open** if desired; installation does not automatically launch the APK. An app with no detected launch entry remains manageable but does not appear in the launcher.

To update, open the app's management dialog and choose **Install newer APK**. Package identity and version are checked; Android enforces signing compatibility during replacement. Downgrades are refused, and signature failures never trigger an uninstall/reinstall workaround. Same-package replacement retains data. Updating may stop the target container, affecting its other applications.

Before replacing an existing installation, Framely saves a container-data snapshot while it is stopped. Scratch overlay work directories, runtime sockets and FIFOs are excluded. Backups include a SHA-256 integrity inventory checked before restoration. Backup failure aborts replacement. Snapshots are recovery material, not a guarantee that every Android data migration is reversible. One-click restore supports sideloaded containers with a single recorded application and a matching data location, including external containers; shared snapshots require manual recovery. Restoration creates another snapshot of the current state and requires launching the app to verify saved data afterward.

Once a modifying operation starts, cancellation is disabled until its actual outcome is known. Uploads and preparation can be cancelled. Switching panels does not stop a submitted task; reopening APK Manager resumes its progress display. Interrupted restores are recovered from a local transaction journal before the next modifying operation. After interruption, use **Reconcile installed state** before retrying or deleting data. An unreadable container displays Installation state unknown, preserves the previous record and disables launching, reinstallation and deletion until its actual state is reconciled.

## Launch and window display

The launcher uses declared MAIN/LAUNCHER, television launcher and recognized VR entries, plus the running Android package manager's resolved launcher entry. Disabled packages/components are excluded. A launch entry does not guarantee that an app will run successfully.

Window handling is automatic by default. **Advanced settings → Show Android window** is a troubleshooting override, applied on container restart. It does not convert a flat application into VR or a VR application into a flat application. Lepton development contexts may mount their APK at a different path after a restart. Framely re-registers the same signed APK when Android can no longer resolve it, preserving data and reporting failures. Advanced settings also allow selecting an enabled, declared Activity when a nonstandard app needs a different entry.

Launcher favorites, search and manual ordering continue to use stable context/package identities. Updating and refreshing do not reorder icons. Uninstalling hides an icon without deleting its favorite/order record; reinstalling in the same context restores it. Long-press a local Lepton icon to open APK management or removal confirmation.

## Close, uninstall and clean data

**Close app** force-stops only the selected package. **Stop container** affects every app inside it. All non-Steam sideloaded containers with a verified data location support deletion after confirmation of affected apps and permanent data loss. Standard Lepton context directories are removed as a whole; registered external locations remove only the baked directory, preserving unrelated files in the parent. Uninstalling an app, even with Delete app data enabled, does not delete its container.

Uninstallation retains saved data by default. Enable **Delete app data** to remove it. Retained apps appear under **Uninstalled, data retained**, where you can reinstall or remove retained data.

Android may require the original signed APK to remove an already uninstalled package's retained state. Framely keeps the original APK when it manages an uninstall. If an externally uninstalled app has no recoverable APK, supply its original APK through reinstallation before removing its retained state. Framely does not bypass signing checks or delete arbitrary directories to simulate success.

**Data cleanup** lists retained data, old APKs, recovery snapshots and expired temporary files. Saved data and backups are not selected by default. Unidentified data is displayed without an automatic deletion action. Each selected item is checked again before removal; a reinstalled application cannot be cleaned as an uninstalled one. Batch results report actual released file bytes and individual failures. File-byte counts may differ from physical disk usage due to compression, reflinks and shared storage.

## Logs and recovery without the panel

Use the app's **Logs → Export logs** to save its operation log. Detailed Android errors remain in logs. Review logs before sharing them.

As the Steam session user, the installed binary also provides these commands:

```bash
/home/.framely/current/bin/framely apk list
/home/.framely/current/bin/framely apk inspect --file /path/to/app.apk
/home/.framely/current/bin/framely apk install '{"ticket":"REVIEW_TICKET","approve":true}'
/home/.framely/current/bin/framely apk launch '{"app":"CONTEXT/PACKAGE"}'
/home/.framely/current/bin/framely apk logs '{"app":"CONTEXT/PACKAGE"}'
/home/.framely/current/bin/framely apk cleanup-list
```

Reviews expire after 15 minutes. APKs, operation records and recovery material are stored in `~/.local/share/framely/apk-manager`; managed Lepton data lives under `~/.local/share/lepton/contexts`. Framely uninstallation retains these user-owned directories. Back them up before removing Framely; the APK CLI requires a compatible Framely binary, while Lepton remains independently installed through Steam.

For a Framely-managed context, a manual Lepton launch must preserve data:

```bash
LEPTON_NO_CLEANUP=true ~/.local/share/Steam/steamapps/common/Lepton/lepton start CONTEXT
```

Do not invoke Lepton's global cleanup commands as an application-specific uninstall.

## Supported formats

The initial implementation supports individual APKs. Split APK bundles, XAPK/APKM, OBB import, Google Play installation and automatic Steam shortcut creation are outside this version. Unsupported bundles are rejected rather than treated as ordinary APKs.
