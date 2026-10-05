# Framely

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/branding/framely-logo-light.svg">
  <img src="assets/branding/framely-logo.svg" alt="Framely" width="320" height="77">
</picture>

[简体中文](README.zh-CN.md)

> Framely is still under rapid development. Preview releases may change frequently; features and interactions are still being refined.

A React plugin manager for Steam Frame, with a Rust core service, a separate CEF/OpenVR host, a network management panel, and a desktop installer. Manage plugins, sources, windows, notifications, and updates.

![Framely spatial launcher](docs/images/launcher.png)

![Framely plugin library](docs/images/plugin-library.png)

## Installation

The target device is a Linux ARM64 Steam Frame. Enable developer mode and SSH on Frame, connect your computer and Frame to the same network, and prepare the `steamos` account password.

### Desktop installer

Download the package for your computer from the [Framely Installer release](https://github.com/SteamFramelyHomebrew/framely/releases/tag/installer-v0.4.1-preview.9). Linux packages are `.tar.gz`; Windows and macOS packages are `.zip`. Extract and launch the installer, enter the Frame IP and SSH port, verify the SSH fingerprint, and connect with the `steamos` account. Select the Framely version, then confirm installation. Enable testing releases to install a Preview version. Installer and device versions are independent.

### Command installation

Run the following command **in Frame's SSH terminal** to install the latest stable release:

```bash
curl -fsSL https://raw.githubusercontent.com/SteamFramelyHomebrew/framely/main/install.sh | bash -s -- install
```

For a Preview release, specify an actual published tag:

```bash
curl -fsSL https://raw.githubusercontent.com/SteamFramelyHomebrew/framely/main/install.sh | bash -s -- install --version v0.4.3-preview.5
```

Replace the example tag with the published version you want to install. Enter the sudo password when prompted. See the [installation guide](docs/user-guide/installation.md) for updates, repair, and uninstallation.

First installation uses the complete offline package with CEF; updates use the smaller core package and reuse CEF; the device needs neither Node.js nor Rust. Installation requests sudo and does not automatically disable SteamOS read-only protection. Press the main trigger on the Framely Dock icon in the SteamVR Dashboard to open the launcher, or the upper trigger to open the quick menu. Open the manager from the launcher’s Settings button. The trigger actions can be swapped in Launcher settings.

The network panel uses port `15915` by default. Visit `http://DEVICE_IP:15915` on your phone or computer. When no password is configured, the first visit requires setting and confirming a password before login. The device password and panel password are independent.

## Documentation

Start at the [documentation index](docs/README.md):

- [Install, update, repair, roll back, and uninstall](docs/user-guide/installation.md)
- [Basic usage](docs/user-guide/basic-usage.md) · [Install and manage plugins](docs/user-guide/plugins.md)
- [APK Manager](docs/user-guide/apk-manager.md) · [Space launcher](docs/launcher.md)
- [Network management panel](docs/user-guide/network-panel.md)
- [Plugin development through publishing](docs/developer-guide/README.md) · [Publish plugins and sources](docs/developer-guide/publishing.md)
- [SDK API](docs/developer-guide/sdk.md) · [Manifest configuration](docs/developer-guide/manifest.md) · [Lifecycle](docs/plugin-lifecycle.md)
- [Plugin template](templates/plugin/README.md) · [Showcase example](examples/showcase/README.md)
- [Core release maintenance](docs/releases.md) · [Build the installer](installer/README.md)

## Create a plugin

Install Node.js 22 and npm on your development computer, clone this repository, and run:

```bash
node tools/plugin-dev.mjs init ../my-plugin yourname.my-plugin
cd ../my-plugin
npm install
npm run dev
npm run build
```

The generated project includes React pages, a Python backend, persistent storage, lifecycle callbacks, the SDK, and build tools. Browser preview does not start the backend; see the development guide for packaging and testing on Frame.

## Build and validate

Building the core from source requires Linux, Rust, Node.js, C/C++, and OpenGL/X11 development libraries. The production native host uses the pinned Linux ARM64 CEF version.

```bash
npm ci
npm run typecheck
npm run build
cargo test --locked
bash tools/test-native.sh
python3 -m unittest discover -s tests -p 'test_*.py'
```

See [release maintenance](docs/releases.md) for CEF builds, integration tests, and publishing commands. Plugins support `steamos` and `root`; hash verification is not a security review, so check the source and runtime user.

## License

Original Framely code, including the core, installer, SDK, template, and repository examples, uses **AGPL-3.0-only**; see [LICENSE](LICENSE). Third-party dependencies and vendored files retain their respective licenses.

AGPL allows commercial use. Distribution requires complete corresponding source under the license; modified software that interacts with users over a network must also offer source to those users. Release source corresponds to the repository's `v<VERSION>` or `installer-v<VERSION>` tags. Redistributors must provide modifications and build scripts matching their binaries.
