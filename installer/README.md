# Framely Installer

[简体中文](README.zh-CN.md)

A Rust + GPUI Kit desktop installer that connects to Steam Frame to install, update, repair, roll back, and uninstall Framely. The default release repository is `SteamFramelyHomebrew/framely`.

## License

Original installer code uses **GNU Affero General Public License v3.0 only** (`AGPL-3.0-only`). See [LICENSE](../LICENSE); release packages include it. Third-party dependencies retain their own licenses. AGPL allows commercial use and requires complete corresponding source when distributing software under its terms.

Corresponding source is available in the [SteamFramelyHomebrew/framely](https://github.com/SteamFramelyHomebrew/framely) repository at `installer-v<VERSION>` tags. Build instructions follow below. Redistributors of modified versions must provide source and necessary build scripts matching their binaries.

Only one installer instance can run per user; additional instances exit immediately. The title bar supports dragging and window controls. macOS uses system window buttons; Linux/Windows use GPUI Kit or system controls. Closing the last window exits the application and releases the instance lock.

## Usage

1. Enable developer mode and set a password on Frame. Connect the computer and device to the same LAN.
2. Discovery prioritizes `192.168.*` subnets, with at most 1024 addresses per scan. Name discovery and IP probing run together. Only devices whose hostname is `frame` are listed, with SSH availability indicated. Other devices and addresses without a confirmed name are omitted. Manual IP and SSH port entry is always available.
3. Check the SSH handshake and displayed host fingerprint, then enter credentials. The default user is `steamos`. Changed saved fingerprints cause the connection to be rejected. After verifying a device reinstall, you can remove its entry from `known-hosts.json` in the configuration directory.
4. Select a release or a local `framely-*-linux-arm64.tar.gz` together with its external `SHA256SUMS` or matching `.sha256` file. Local packages do not require GitHub access. Prereleases are hidden by default.
5. Choose an operation on Install & Maintenance and confirm the device, version and action in the modal dialog. Cancel or press Esc to return without executing. Errors and operation results also appear in dialogs. SSH fingerprint verification stays on the connection page. The installer does not disable SteamOS root filesystem read-only protection; it stops with an error when required directories are not writable.

During execution, the main page shows download, transfer and device-operation progress. Downloads and SFTP transfers report actual bytes and percentages; local packages skip downloading. The device reports verification, staging, extraction, service configuration and startup checks. Extraction reports actual bytes; steps without a measurable total show an indeterminate indicator and the current step. Logs remain available on the same page. Completion is reported only after the operation and post-operation checks succeed.

After local verification, built-in SSH/SFTP uploads the package. The device verifies it again and extracts it safely. Passwords are used only in memory and are not written to configuration, command arguments, or environment variables. Operations provide the sudo password through SSH standard input. SSH and sudo use the same entered password, matching the default Frame account configuration.

Uninstall has one path: disable all plugins and run their uninstall hooks before removing Framely services and binaries. If any hook fails, Framely remains installed with plugins disabled for repair and retry. Settings, plugin data, and dedicated accounts remain. Changes plugins made elsewhere may remain. Update older releases that lack `prepare-uninstall` before uninstalling.

Available operations depend on device state. An uninstalled device offers installation only. An installed device opens maintenance with update, repair, and uninstall; rollback is offered only when a previous release exists. Repair, rollback, and uninstall require no package selection. Install/update without a package returns to version selection. Selecting the exact installed package prompts repair instead of updating again. State is re-read before execution to reject stale actions. Changing devices, versions, or files clears prior confirmation, and navigation is disabled during operations.

After an operation, maintenance shows a separate success/failure card and a reconnect button. Success states which operation completed; failure shows the cause. Logs remain available for diagnosis. The SSH connection and installed-version snapshot are discarded; reconnect before another operation.

Scanning, downloads, and SSH work run on background threads, with progress and logs in the UI. The network panel button assumes port 15915; use the actual address if the device port has changed.

After a successful first installation, the installer automatically opens the network management panel in your browser. The highlighted **Management panel** button remains available after installation and updates.

Online first installation always downloads the complete `*-offline-linux-arm64.tar.gz` package with CEF. Online updates download the core package without CEF and reuse the device runtime. A missing or newly required CEF runtime is downloaded separately by the device. For local first installation, choose the complete offline package. See [package selection](../docs/user-guide/installation.md#release-packages-and-cef).

Release queries, checksum files and package downloads automatically use the computer's manual system proxy: Windows Internet Settings, macOS network settings, or Linux GNOME/KDE settings. `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` (or their lowercase forms) override desktop settings; `NO_PROXY` and desktop bypass rules are respected, including on redirects. HTTP CONNECT and SOCKS proxies are supported. PAC scripts and IPv6 proxy endpoints are currently unsupported; use a manual proxy with a hostname or IPv4 address. SSH/SFTP and downloads initiated by Frame use the device connection and network rather than the computer's HTTP proxy.

## Build

Linux requires C/C++ build tools, CMake, pkg-config, and development libraries for Fontconfig/Freetype, X11/XCB, XKBCommon, Wayland, and Vulkan. macOS requires Xcode command-line tools. Windows requires MSVC, CMake, and Perl for built-in OpenSSL. GPUI Kit is pinned to 0.7.0; matching dependencies are locked in `Cargo.lock`.

```bash
cargo test --manifest-path installer/Cargo.toml --locked --no-default-features --lib
cargo check --manifest-path installer/Cargo.toml --locked
cargo run --manifest-path installer/Cargo.toml --locked
cargo build --manifest-path installer/Cargo.toml --locked --release
```

Set `CARGO_TARGET_DIR` to use another build-output directory; packaging also reads this variable. By default, packaging reads `installer/target/release/framely-installer`. CI requires no override.

```bash
INSTALLER_PLATFORM=linux-x64 python3 tools/package-installer.py
```

The UI shares Framely's dark surfaces, thin borders, and pale blue selection state. It has Connect device, Select version, and Install & Maintenance pages. Connection progress covers SSH, authentication, permission checks, version retrieval, and fingerprint storage. Failures show their cause and restore retry controls. Next-step buttons explain missing device/package requirements. Render simulated pages and connecting, failure, and success states without connecting to a real device:

```bash
cargo build --manifest-path installer/Cargo.toml --locked --features visual-test
FRAMELY_INSTALLER_PREVIEW_DIR=installer/dist/previews installer/target/debug/framely-installer
```

Preview requires an available GPU or software Vulkan renderer. Default release builds do not include preview support.

Installer versions are managed independently in this directory's `Cargo.toml` and `Cargo.lock`. Pushing an `installer-v<VERSION>` tag (currently `installer-v0.4.1-preview.9`) triggers `.github/workflows/installer-release.yml` to build and publish only the installer. The core uses `v<VERSION>` tags and a separate workflow. Manual Actions runs produce build artifacts only. Installer releases do not become the repository's Latest release, preserving Frame's default install/update URLs.

Actions builds Linux x64/ARM64, Windows x64, and macOS Intel/Apple Silicon. macOS produces an `.app` ZIP, Windows an EXE ZIP, and Linux a tar.gz. Apple notarization and Windows code signing are not configured; verify first-launch behavior on each system before publishing.

The titlebar and application icons share the pixel logo. Windows builds embed the ICO in the EXE; macOS packaging uses the system `iconutil` to generate and configure `.icns`. On Linux, run `bash install-desktop-entry.sh` from the extracted archive to register an icon-bearing entry in the current user's application menu. The entry points to that directory; rerun the script after moving it.

Installer archives do not include a separate `assets` directory. The UI logo is embedded in the executable; the Linux registration script embeds its menu icon, while macOS keeps its application icon inside the `.app` bundle.

Automated tests and Actions builds validate software and artifacts. Discovery, login, permissions, update, rollback, and uninstall still need acceptance testing on actual computers and Frame hardware.

Discovery queries `frame.local` mDNS address records directly. It does not require an advertised SSH service or system DNS support for `.local`. Run the same discovery logic independently for diagnosis:

```bash
cargo run --manifest-path installer/Cargo.toml --locked --no-default-features --example scan -- 192.168.5.0/24
```

## Export diagnostic logs

After connecting to Frame, click **Export logs** in the footer and choose a ZIP destination on your computer. You do not need to select a release or installation package. Collection runs directly over SSH with administrator access, independently of Framely’s web panel and daemon. It also works with stopped services or damaged state, and includes the installer’s current operation log. Export does not install, restart or change Framely.

The archive includes bounded recent service/plugin/CEF logs, GPU evidence, installation versions and a plugin status summary. Missing sources are listed in `report.json`. Plugin settings, inbox contents and password files are excluded; common credential patterns are masked. Review logs before sharing. Cancelling the save dialog leaves the installation untouched.

### Proxy preferences

The **Settings** tab controls downloads on the installer computer. System proxy discovery is enabled by default. An explicit HTTP/SOCKS proxy takes precedence; leave it empty to follow the system toggle. An optional HTTPS GitHub proxy prefix routes GitHub API and asset URLs as `prefix/original-URL`. Settings persist locally and apply to subsequent version checks and downloads; SSH and Frame settings are unchanged. Saving clears the release selection so the next version check uses the new settings. Package verification remains enabled.
