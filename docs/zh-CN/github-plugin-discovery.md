# GitHub 插件自动发现与 Release 元数据

[English](../github-plugin-discovery.md)

Framely 在现有插件库中搜索带 `framely-plugin` topic 的公开 GitHub 仓库，排除 fork 和 archived 仓库，再读取 Releases。无需 npm 或额外目录文件。现有社区源、自建源继续使用；同 ID 合并为一个列表项，优先推荐语义版本最高的兼容版本。来源、正式版／预览版和历史版本可在原界面选择。同 ID、版本和包哈希相同的条目合并；同版本不同哈希仍保留，版本选项显示短哈希。更换来源继续经过安装确认。

## 新 Release：release.json

每个 Release 上传一个 `release.json` 附件，可描述多个插件。不要放在仓库默认分支来代替 Release 附件。

```json
{
  "schemaVersion": 1,
  "plugins": [{
    "id": "example.plugin",
    "name": "Example Plugin",
    "version": "1.2.0",
    "apiVersion": 1,
    "author": "Developer",
    "description": "插件简介",
    "engines": {"framely": ">=0.8.3"},
    "asset": "example.plugin-1.2.0.framely",
    "size": 123456,
    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "runAs": "steamos",
    "icon": "example.plugin-1.2.0-icon.png",
    "screenshots": [],
    "tags": ["工具"],
    "details": "详细介绍",
    "changelog": "更新记录",
    "homepage": "https://github.com/example/plugin",
    "dependencies": {},
    "optionalDependencies": {},
    "conflicts": {},
    "exclusiveResources": []
  }]
}
```

必填：`id`、`name`、`version`（SemVer）、`apiVersion`、`author`、`description`、`asset`、`size`（整包字节数）、`sha256`（整包 64 位十六进制 SHA256）、`runAs`。有后端或生命周期脚本时，`runAs` 是实际运行用户；纯界面插件为 `null`，不能省略。其他字段遵循现有插件目录格式；图片可以是同一 Release 的附件名或固定 HTTPS 地址。`authorUrl`、`documentationUrl` 也支持现有格式。

`asset` 必须对应同一 Release 的 `.framely` 附件；大小必须与 GitHub 附件资料一致。GitHub 提供 tag、预览版标记及附件下载地址，JSON 无需重复 `downloadUrl`。有 GitHub asset digest／`SHA256SUMS` 时必须与 JSON 一致。声明不合法、缺附件或哈希冲突的 Release 不会作为可安装版本，也不会退回旧格式掩盖错误。

## 自动生成及上传

先完成正常构建、打包和验证，把最终 `.framely` 包放入一个专用发行目录，再运行新版 Framely：

```bash
framely release-json --packages ./release --output ./release/release.json
```

在 Framely 源码目录也可用 `cargo run --locked -- release-json ...`。生成器验证包内文件哈希，读取最终包内清单，自动计算整包 SHA256、字节数、运行用户及依赖。没有 `publish.icon`／非空 `publish.screenshots` 外链时，自动导出包内图标和截图到同一发行目录，JSON 使用这些附件名。拒绝覆盖内容不同的同名发行图片。每个插件 ID 在一次发行中只出现一次。

将生成步骤加入自己的发布 CI，并上传发行目录中的全部包、JSON 和图片，例如已有 GitHub CLI 发布流程中：

```bash
framely release-json --packages ./release --output ./release/release.json
gh release upload "$TAG" ./release/*
```

不要在生成 JSON 后重新打包，也不要覆盖已发布的同版本附件。想加入自动发现，由仓库维护者添加 `framely-plugin` topic。本次 Framely 实现不会替维护者修改外部插件仓库。

## 没有 release.json 的旧 Release

只有完全没有该附件时才采用兼容路径：

1. 将 Release 的 tag 解析为提交 SHA，读取该 SHA 的根目录 `manifest.json`；不读取默认分支。
2. tag 为 `v<version>` 或 `<version>`，且必须与清单版本完全一致。
3. 清单有 `downloadUrl` 时必须精确对应本 Release 的 `.framely` 附件；否则匹配 `<id>-<version>.framely`。
4. 使用 GitHub asset digest 或 Release 的 `SHA256SUMS`；二者都存在必须一致。均缺失则不允许安装，不猜测哈希。可选 `downloadSha256` 也必须一致。
5. 有 `publish` 图片外链时沿用；否则图标使用固定提交 SHA 的 Raw 地址。

旧插件和现有 `catalog.json`／`versions.json` 来源无需迁移，旧客户端仍沿用原有来源，忽略新 Release 附件。`release.json` 只是增加一种发现方式，不替换现有安装包格式和兼容性检查。

## 缓存、安装及失败处理

搜索和 Releases 分页读取，最多各 10 页，每页 100 条；Release 先全部读取再按 SemVer 排序，不依赖 API 返回顺序。达到搜索上限会提示；超过 Release 历史上限会报错，避免错误推荐未完整读取的版本。元数据缓存 10 分钟，携带 ETag，手动刷新重新验证；网络失败时可显示旧缓存并提示，证书、解析和普通 404 等错误不隐藏。所有 GitHub 请求最多同时 6 个，支持取消，复用现有 HTTP／手动 GitHub 代理和自动代理策略。浏览目录不下载插件包。

首次选择 GitHub 插件时来源是规范仓库地址 `https://github.com/<owner>/<repo>`，来源 ID 根据仓库名稳定生成，安装确认成功才写入配置。后续更新、依赖和历史版本从该仓库 Releases 读取；禁用来源不会被自动发现重新启用。安装时再次核对整包哈希及包内 ID、版本、API、Framely 要求、实际运行用户、依赖与冲突；权限和来源切换使用现有确认流程。部分仓库失败不会隐藏其他来源已成功加载的插件。
