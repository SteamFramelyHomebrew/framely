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
5. Choose an operation on Install & Maintenance and confirm. The installer does not disable SteamOS root filesystem read-only protection; it stops with an error when required directories are not writable.

After local verification, built-in SSH/SFTP uploads the package. The device verifies it again and extracts it safely. Passwords are used only in memory and are not written to configuration, command arguments, or environment variables. Operations provide the sudo password through SSH standard input. SSH and sudo use the same entered password, matching the default Frame account configuration.

Uninstall has one path: disable all plugins and run their uninstall hooks before removing Framely services and binaries. If any hook fails, Framely remains installed with plugins disabled for repair and retry. Settings, plugin data, and dedicated accounts remain. Changes plugins made elsewhere may remain. Update older releases that lack `prepare-uninstall` before uninstalling.

Available operations depend on device state. An uninstalled device offers installation only. An installed device opens maintenance with update, repair, and uninstall; rollback is offered only when a previous release exists. Repair, rollback, and uninstall require no package selection. Install/update without a package returns to version selection. Selecting the exact installed package prompts repair instead of updating again. State is re-read before execution to reject stale actions. Changing devices, versions, or files clears prior confirmation, and navigation is disabled during operations.

After an operation, maintenance shows a separate success/failure card and a reconnect button. Success states which operation completed; failure shows the cause. Logs remain available for diagnosis. The SSH connection and installed-version snapshot are discarded; reconnect before another operation.

Scanning, downloads, and SSH work run on background threads, with progress and logs in the UI. The network panel button assumes port 15915; use the actual address if the device port has changed.

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

Installer versions are managed independently in this directory's `Cargo.toml` and `Cargo.lock`. Pushing an `installer-v<VERSION>` tag (currently `installer-v0.4.1-preview.5`) triggers `.github/workflows/installer-release.yml` to build and publish only the installer. The core uses `v<VERSION>` tags and a separate workflow. Manual Actions runs produce build artifacts only. Installer releases do not become the repository's Latest release, preserving Frame's default install/update URLs.

Actions builds Linux x64/ARM64, Windows x64, and macOS Intel/Apple Silicon. macOS produces an `.app` ZIP, Windows an EXE ZIP, and Linux a tar.gz. Apple notarization and Windows code signing are not configured; verify first-launch behavior on each system before publishing.

The titlebar and application icons share the pixel logo. Windows builds embed the ICO in the EXE; macOS packaging uses the system `iconutil` to generate and configure `.icns`. On Linux, run `bash install-desktop-entry.sh` from the extracted archive to register an icon-bearing entry in the current user's application menu. The entry points to that directory; rerun the script after moving it.

Automated tests and Actions builds validate software and artifacts. Discovery, login, permissions, update, rollback, and uninstall still need acceptance testing on actual computers and Frame hardware.

Discovery queries `frame.local` mDNS address records directly. It does not require an advertised SSH service or system DNS support for `.local`. Run the same discovery logic independently for diagnosis:

```bash
cargo run --manifest-path installer/Cargo.toml --locked --no-default-features --example scan -- 192.168.5.0/24
```
