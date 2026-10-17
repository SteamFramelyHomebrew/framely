# GitHub plugin discovery

[简体中文及完整 JSON 示例](zh-CN/github-plugin-discovery.md)

Framely searches public GitHub repositories with topic `framely-plugin`, excluding forks and archived repositories. Releases appear in the existing plugin library alongside catalogs. One row per plugin ID recommends the highest compatible SemVer; version selection retains different package hashes and origins. Existing install confirmation, runtime-user checks and dependency resolution apply.

Upload a `release.json` asset with `schemaVersion: 1` and a nonempty `plugins` array. Required plugin fields are `id`, `name`, `version`, `apiVersion`, `author`, `description`, `asset`, `size`, `sha256`, and `runAs`. `asset` identifies a `.framely` asset in that Release; size is whole-package bytes and SHA256 is 64 hexadecimal characters. `runAs` is the effective backend/lifecycle user, or explicit `null` for UI-only plugins. Optional presentation, engines and relation fields follow existing catalog metadata. Images may name Release assets or fixed HTTPS URLs. Release tag, prerelease flag and download URLs come from GitHub.

Generate metadata from final, verified packages rather than maintaining hashes manually:

```bash
framely release-json --packages ./release --output ./release/release.json
gh release upload "$TAG" ./release/*
```

The generator also exports packaged images into that directory unless external publishing images are configured. Upload every generated file. Add these commands to your own publishing CI and add the repository topic to opt into discovery. Existing external plugin repositories are not changed by this feature.

Only Releases without `release.json` use legacy compatibility: resolve the tag to a commit SHA, read its root `manifest.json`, require tag/version agreement, match the explicit download URL or `<id>-<version>.framely`, and require GitHub's SHA256 asset digest or `SHA256SUMS`. All available hashes must agree. Malformed new metadata never falls back to legacy metadata. Old clients continue using their existing catalogs; package format is unchanged.

Metadata uses pagination, SemVer sorting, a ten-minute cache, ETag revalidation and a global six-request concurrency limit. Manual refresh revalidates. Retryable network failures may use stale cache with a warning; invalid JSON, certificate errors and ordinary 404s do not. Proxy settings and cancellation are reused. Browsing does not download package bytes.

GitHub sources use canonical repository URLs and stable IDs. A newly discovered source is persisted only when installation is confirmed. Updates, history and dependencies subsequently read its Releases. Disabled sources remain disabled. Installation verifies actual package hash, identity, version, API, engine requirements, runtime user and relations against Release metadata.
