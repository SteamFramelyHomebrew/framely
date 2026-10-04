# Framely releases and updates

[简体中文](../releases.md)

Plugin catalogs and device-update sources are separate settings. Current initialization adds community stable/testing catalogs, subject to server availability. The distributor preconfigures device updates; About offers check/download/install/rollback. Without a source, use a release package. Packages do not require signatures or public keys.

## Update descriptors

After building an archive, generate a plain JSON descriptor containing version, architecture, URL, size and SHA256:

```bash
framely release-manifest --archive "./framely-0.2.0-<build>-linux-arm64.tar.gz" --url "https://example.org/releases/framely-0.2.0-<build>-linux-arm64.tar.gz" --output ./framely-release.json --changelog 'Release changes'
```

Publish archive/descriptor on HTTPS. Downloads allow up to five validated redirects without HTTPS downgrade. Configure the complete descriptor URL from a maintenance terminal:

```bash
framely call system.source.save '{"source":{"url":"https://example.org/framely-release.json"}}'
```

This is a distributor interface, not an ordinary settings item. The official engine and desktop installer initialize `https://github.com/SteamFramelyHomebrew/framely/releases/latest/download/framely-release.json` only when no source is configured, preserving existing sources. Hashes establish content consistency; identity depends on trusted sources, not signatures.

Updates verify API/architecture, length, SHA256, safe extraction and internal `SHA256SUMS`. Downloads do not block the core. After user confirmation, a separate systemd helper switches releases and restarts Framely only. Log: `/var/lib/framely/logs/update.log`. Failed installation attempts to restore release/state; the UI can return to the preceding release.

Sources contain URL only. Legacy signed descriptors/envelopes/public-key fields are unsupported. Rollback preserves plugin data without downgrading its format. Device upgrades, rollback and reboot still need real validation beyond local tests.

## Recovery after SteamOS updates

Management state resides at `/home/.framely/state`, including state, update source, logs, release links, Steam username and Framely UID. `/var/lib/framely` is a compatibility link. Old state migrates after writers stop; conflicting copies cause a stop rather than overwrite.

Accounts/services depend on OS configuration. When writable, `sudo bash /home/.framely/repair.sh` restores links, account, service files and startup from the retained, verified release without another download. It invokes the current release's `install.sh --repair` without creating releases or changing the previous-version record. UID conflicts or changes stop recovery. It never disables read-only protection automatically.

Deleted `/home` data cannot be recovered. OS updates may require manual intervention. After rolling back to a release without repair support, use a newer supporting package. Before stable publication, validate actual OS-update recovery, state/data/UIDs/services and SteamVR compatibility.

## GitHub Actions

Device and installer share the repository with independent versions/tags/workflows:

- Device: root Cargo version, `v<version>` (currently `v0.4.2-preview.5`), `.github/workflows/release.yml`, Linux ARM64 and pinned CEF.
- Installer: `installer/Cargo.toml`, `installer-v<version>` (currently `installer-v0.4.1-preview.3`), `.github/workflows/installer-release.yml`, Linux x64/ARM64, Windows x64, macOS Intel/Apple Silicon, pinned GPUI Kit.

Versions need not match or ship together. Update the relevant Cargo.lock with version changes. Installer package/macOS versions use the installer version.

Both workflows cache dependencies/builds by product/platform/toolchain/configuration, including failed builds. Installer uses `installer/target`; device CI uses `target/cargo`, separate from `target/cef`, for tests and packaging. Branch pushes do not build releases; matching tags or manual runs do. Tags may restore default-branch caches, not another tag's private cache. A manual main run can prime cache and builds artifacts only. Cold builds/toolchain changes need compilation; inspect cache steps rather than assuming a hit.

```bash
# After committing and pushing the intended source:
git tag v0.4.2-preview.5
git push origin v0.4.2-preview.5
# Independent installer release:
git tag installer-v0.4.1-preview.3
git push origin installer-v0.4.1-preview.3
```

Prerelease suffixes create GitHub Prereleases without Latest. Installer releases are never Latest. Only stable device releases become Latest, keeping default installer/update URLs on the device product. The desktop installer filters device archive names and does not mistake ARM64 installer packages for device runtimes. Download desktop installers from their explicit installer tags.

Only tags publish. After every product build succeeds, publication creates a draft, uploads assets/checksums, then makes it public. Failed drafts are not stable download targets. Manual runs leave Actions artifacts only.

Device attachments include the runtime archive, `framely-release.json`, `bootstrap.py`, `install.sh` and `SHA256SUMS`. Installer releases have five platform packages and checksums only. Desktop installers download archive/checksums without user-managed scripts.

External checksums cover downloaded archives; internal checksums cover extracted payload. Neither independently proves author identity. Online installation trusts the chosen repository/HTTPS, local installation the supplied archive/checksums. Redirects never downgrade HTTPS.

The entry fetches the release engine. Default is latest stable; `--version TAG` chooses that Release for both engine and package and works for Preview-only repositories. Locally run `python3 tools/bootstrap.py install --archive /path/package.tar.gz --checksums /path/SHA256SUMS`. Updating requires existing installation; selecting the same installed release repairs it, another version installs while retaining state.

Installation initializes the official update descriptor URL only if no source is configured, preserving existing sources for future in-app updates.

## Unified uninstallation

Scripts and desktop UI expose one uninstall operation. It stops session/downloads, then calls root-only `prepare-uninstall --approve`: persistently disable plugins, stop them in dependency order, run uninstall hooks, remove payloads. Only complete success removes services/core/runtime. Failure retains Framely, restores session access and leaves plugins disabled for troubleshooting/retry. There is no skip-failed-plugin manager uninstall.

Root maintenance can uninstall before consent or after revocation; pages do not expose the maintenance method. Settings/data/accounts remain, and external side effects are not guaranteed reversible. Old releases without this endpoint need updating first.
