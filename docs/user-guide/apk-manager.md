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

During APK installation, choose **Display mode → Flat window / VR app**. The initial selection follows APK detection; updates keep your previous window override. Flat mode displays the Android window, while VR mode hides it and requires the app itself to support VR. A successful installation saves the choice; failed installations preserve the previous setting.

Choose **Install APK**, select an individual `.apk`, and review its name, package, version, minimum Android SDK and target container. Inside Frame, the selected APK is inspected directly on the device without a browser upload. A private local snapshot is retained for the confirmation step; the original file is not changed. Uploads from the network panel are staged on Frame's persistent filesystem, not its limited `/tmp`. Transfer shows actual bytes; subsequent work shows named stages instead of invented percentages.

Inside Frame, file inputs in both management panels use Framely’s built-in file picker, including APKs, plugin packages and language files. It opens Downloads by default, remembers the last selected folder, and provides a lazily loaded folder tree for Home, temporary files and mounted storage, with folder navigation, path entry, search, hidden files and file-type filtering. Select a file and confirm; B/Escape cancels the picker without closing the panel. External browsers continue using their system file picker. The launcher’s **Install APK** shortcut opens APK Manager.

Framely installs packages into separate Android package directories even when they share a container. Container restarts preserve downloaded resources in Android external storage. Older single-APK mounts are repaired from the saved APK when available, and legacy OBB resource links are migrated before replacement without deleting their original target. This prevents future loss; resources already deleted by an older startup cannot be reconstructed from the APK alone.

Each new app uses its own independent container. Existing shared containers remain discoverable; their existing packages can still be updated or managed, but new package identities cannot be added. Framely does not automatically split, migrate or delete older shared containers or their data. Steam-managed containers remain excluded.

Installation runs in a headless background container without opening the Android desktop, regardless of the selected display mode. After installation, choose **Open** if desired; installation does not automatically launch the APK. An app with no detected launch entry remains manageable but does not appear in the launcher. Opening an already visible app reuses its running container and virtual gamepad. A headless or fully hidden display, a physical window-size change, or first attaching a new gamepad requires a restart because Lepton cannot safely perform those display or device transitions in place.

To update, open the app's management dialog and choose **Install newer APK**. Package identity and version are checked; Android enforces signing compatibility during replacement. Downgrades are refused, and signature failures never trigger an uninstall/reinstall workaround. Same-package replacement retains data. Updating may stop the target container, affecting its other applications.

Before replacing an existing installation, Framely saves a container-data snapshot while it is stopped. Scratch overlay work directories, runtime sockets and FIFOs are excluded. Backups include a SHA-256 integrity inventory checked before restoration. Backup failure aborts replacement. Snapshots are recovery material, not a guarantee that every Android data migration is reversible. One-click restore supports sideloaded containers with a single recorded application and a matching data location, including external containers; shared snapshots require manual recovery. Restoration creates another snapshot of the current state and requires launching the app to verify saved data afterward.

Once a modifying operation starts, cancellation is disabled until its actual outcome is known. Uploads and preparation can be cancelled. Switching panels does not stop a submitted task; reopening APK Manager resumes its progress display. Interrupted restores are recovered from a local transaction journal before the next modifying operation. After interruption, use **Reconcile installed state** before retrying or deleting data. An unreadable container displays Installation state unknown, preserves the previous record and disables launching, reinstallation and deletion until its actual state is reconciled.

## Launch and window display

The launcher uses declared MAIN/LAUNCHER, television launcher and recognized VR entries, plus the running Android package manager's resolved launcher entry. Disabled packages/components are excluded. A launch entry does not guarantee that an app will run successfully.

Flat apps use Lepton’s full-display boot path to avoid the Gamescope window-creation failure in per-app mode. A headless container from installation, or a closed display, is restarted when needed; this also stops other apps in the same container. VR apps keep background boot. After ActivityManager reports success, Framely checks that the application and Android display service stay alive before reporting completion. Framely keeps Lepton’s development-context data handling instead of switching an existing container to APK bake mode. A temporary launch adapter uses the installed Lepton libraries without editing Steam’s installation; an unsupported entry script reports an error before starting a cold container. Manually choosing **Start container** still opens its desktop.

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
/var/lib/framely/current/bin/framely apk list
/var/lib/framely/current/bin/framely apk inspect --file /path/to/app.apk
/var/lib/framely/current/bin/framely apk install '{"ticket":"REVIEW_TICKET","approve":true}'
/var/lib/framely/current/bin/framely apk launch '{"app":"CONTEXT/PACKAGE"}'
/var/lib/framely/current/bin/framely apk logs '{"app":"CONTEXT/PACKAGE"}'
/var/lib/framely/current/bin/framely apk cleanup-list
```

Reviews expire after 15 minutes. APKs, operation records and recovery material are stored in `~/.local/share/framely/apk-manager`; managed Lepton data lives under `~/.local/share/lepton/contexts`. Framely uninstallation retains these user-owned directories. Back them up before removing Framely; the APK CLI requires a compatible Framely binary, while Lepton remains independently installed through Steam.

For a Framely-managed context, a manual Lepton launch must preserve data:

```bash
LEPTON_NO_CLEANUP=true ~/.local/share/Steam/steamapps/common/Lepton/lepton start CONTEXT
```

Do not invoke Lepton's global cleanup commands as an application-specific uninstall.

## Supported formats

The initial implementation supports individual APKs. Split APK bundles, XAPK/APKM, OBB import, Google Play installation and automatic Steam shortcut creation are outside this version. Unsupported bundles are rejected rather than treated as ordinary APKs.

### Flat window orientation

In **Manage app → Advanced settings → Window orientation**, choose **Automatic**, **Landscape**, or **Portrait**. The choice is saved per app and applied at its next launch. Forced portrait uses a tall window rather than rotating content sideways. Changing orientation may restart the shared container and stop other apps in it. VR apps ignore this setting.

Moonlight V+ 12.12.12’s connection-creation screen currently crashes on the tested Lepton build because Android’s clipboard service is absent. This is distinct from the window-startup failure. Framely does not modify or re-sign the APK; a compatible Lepton runtime or an app-side fallback is required.

### Optional Frame gamepad input

**APK → Apps → Frame gamepad input** is off by default, including after upgrading an existing installation. Enable it to use the two Frame controllers as one standard Android gamepad for APKs opened through Framely. It includes ABXY, the D-pad and diagonals, both sticks and stick clicks, shoulder buttons, analog triggers, Select and Start. Applications must support Android gamepad input; this does not map controls to touchscreen gestures or provide Android VR controller tracking or vibration.

Enabling applies on the next application launch. Framely may restart its container to attach the input device, stopping other apps in that container. Disabling stops forwarding immediately. Only one Framely-launched APK receives input at a time. Switching apps, closing an app, transient process-query failures and foreground changes neutralize input without removing the virtual pad. Each running container retains its device for reuse; container stop, disabling the feature or session-service restart releases it. After a service restart, launch the APK again to reconnect. Keep the intended Android window selected in Dock; the bridge does not change Steam's physical-controller bindings.

The bridge runs as the Steam session user and requires access to `/dev/uinput`, active SteamVR and the supported Lepton startup hooks. It does not modify Steam's installed Lepton files. Only its virtual event node is mounted into the target container, where Android claims it exclusively. If this claim fails, no input is forwarded and the launch reports an error. Disable the option to use the normal launch path. Bridge errors are recorded in `~/.local/share/framely/apk-manager/logs/gamepad.log`; key presses are not logged. The native helper and Android adapter ship in both offline and core update packages.

When this option is enabled, launch APKs through the management panel or launcher. A one-shot CLI launch cannot own the persistent input bridge and reports an error instead.
