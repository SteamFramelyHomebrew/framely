# Manifest 配置说明

[English](../../developer-guide/manifest.md)

[开发流程](README.md) · [SDK](sdk.md) · [生命周期](../plugin-lifecycle.md)

项目根目录 `manifest.json` 描述插件；打包后同名文件位于包内。字段采用 camelCase，安装包清单拒绝未知字段，不要添加已废弃的 `permissions`。以[模板清单](../../../templates/plugin/manifest.json)为起点。

## 最小示例

```json
{
  "schemaVersion": 1,
  "apiVersion": 1,
  "id": "yourname.my-plugin",
  "name": "我的插件",
  "version": "0.1.0",
  "author": "Your name",
  "backend": {"entry": "backend.py", "runAs": "steamos"},
  "ui": {"quickPage": "page.js"},
  "files": {}
}
```

源码清单的 `files` 留空；`framely pack` 遍历 payload 并生成实际文件哈希。至少一个载荷文件，每个入口必须存在于 payload。完整模板见 [manifest.json](../../../templates/plugin/manifest.json)。

## 顶层字段

| 字段 | 要求与默认值 |
| --- | --- |
| `schemaVersion` / `apiVersion` | 必填，目前均为 `1` |
| `id` | 必填，最多 80 字符；允许小写字母、数字、点、短横线、下划线；不能以点开头，禁止 `..`；脚手架要求字母/数字开头，推荐 `namespace.name` |
| `name` / `author` | 必填且非空，各最多 120 字节 |
| `version` | 必填，最多 64 字节，仅 ASCII 字母、数字和 `.-+`；建议完整 SemVer，声明关系时必须 SemVer |
| `description` | 简介，默认空字符串；社区数据库要求最多 4096 字节 |
| `details` / `changelog` | 详细说明/本版本变化，默认空字符串；分别最多 32768 / 16384 字节 |
| `tags` | 标签字符串数组，默认空；最多 12 项，每项非空且最多 80 字节；用于搜索与筛选 |
| `icon` | 可选，payload 内 PNG 路径；最大 1 MiB、1024×1024 |
| `screenshots` | 默认空数组，最多 8 个 payload 内 PNG/JPEG 路径 |
| `authorUrl` / `documentationUrl` / `homepage` | 可选 HTTP/HTTPS 地址，禁止 URL 内用户名密码 |
| `downloadUrl` | 可选，固定版本 HTTPS 包下载地址；GitHub 数据库源码可省略，按仓库/版本/ID 拼接，社区登记的包内需补全；见发布指南 |
| `publish` | 可选外部商店图片配置，见下文 |
| `backend` / `lifecycle` / `ui` | 可选，配置后端、钩子和页面 |
| `dependencies` / `optionalDependencies` / `conflicts` | 默认空对象，见关系参考 |
| `exclusiveResources` | 默认空数组，禁止同时启用占用同名资源的插件 |
| `files` | 必填对象，打包自动生成路径到 SHA256 的映射，最多 2048 个文件 |

旧 `category` 字段仅兼容读取，不再导出；新插件使用 `tags`。`downloadSha256` 是数据库源码登记专用的整包 SHA256，不能放入严格校验的包内清单。默认 GitHub Release 模式从附件 `digest` 读取整包哈希；自定义 HTTPS 地址需要填写源码专用哈希，打包时移除。省略源码 `downloadUrl` 时，社区登记会补全，但当前 `framely pack` 不会自动补全；[发布指南](publishing.md)提供临时清单命令。社区登记还有比本体严格的 ID、简介长度等校验，不能将本体校验通过等同于登记成功。

## `backend`

| 字段 | 说明 |
| --- | --- |
| `entry` | 必填，包内可执行入口，脚本需有效 shebang |
| `args` | 默认空数组，最多 64 项，每项最多 4096 字节且无 NUL；每项是独立参数，不是 Shell 命令 |
| `runAs` | `steamos`（默认）或 `root` |
| `autostart` | 默认 false，按需启动；true 表示启动时常驻 |
| `restart` | `on-failure`（默认）或 `never` |
| `restartLimit` | 连续失败阈值 1–10，默认 3 |
| `memoryLimitMiB` | 内存上限，默认 512 MiB；1–4294967295 的整数，不能用 0 或 null 取消限制 |

省略 backend 可制作纯 UI 插件。系统/Python 第三方依赖不会自动安装。运行用户变化需用户确认，数据目录按运行身份区分，不自动迁移。

`backend.memoryLimitMiB` 映射到 systemd 的 `MemoryMax`，限制后端及其所有子进程的合计内存；它是上限，不会预分配内存。独立生命周期钩子沿用该值，没有后端的钩子使用 512 MiB。安装计划、CLI 和已安装插件管理页展示上限。省略或填写 512 时打包省略该字段，保持旧包兼容；使用其他值需要支持此字段的新版宿主及插件数据库。

例如，为较长回放声明 2 GiB：

```json
{"backend": {"entry": "backend", "memoryLimitMiB": 2048}}
```

## `ui`

`quickPage` 是可选包内页面 bundle 路径；`windows` 默认空对象，最多 8 个窗口，key 必须是合法 ID。每个窗口：

| 字段 | 说明 |
| --- | --- |
| `entry` / `title` | 必填；入口需在 payload；标题非空且最多 120 字节 |
| `dockIcon` | 默认 false，true 为独立 Dock 窗口 |
| `width` / `height` | 默认 1600×900；宽 640–2560，高 360–1440 |
| `widthMeters` | 默认 3.0；有效值 0.4–4.0 米。源码可读取 null，但打包会省略，再读取采用默认 3.0 |
| `localWeb` | 默认 false；特殊本机网页窗口由后端 `window.get` 提供 localhost URL |

`localWeb` 只支持 Frame 本机打开，后端 `window.get` 参数为 `{window: key}`，返回 `{url: "http://localhost:<port>/framely-window/<key>"}`；不得有查询串、锚点或使用管理面板端口。它使用本机网页所需的不同 sandbox 标志。普通 React 插件无需 `localWeb`。窗口 key 与 `framely.windows.open(key)` 及注册的组件映射一致。尺寸在重新打开窗口时生效。

## `lifecycle`

`onInstall`、`onUpdate`、`onUninstall`、`onCrashCleanup` 是可选 `{entry, args?}` 独立命令；`onStart` / `onStop` 是默认 false 的布尔开关，需要 backend。`timeoutSeconds` 默认 10，范围 1–15 秒。`runAs` 可选，有后端时必须与后端一致，纯 UI 插件独立钩子可用它声明身份。独立命令的 `args` 限制与 backend 相同。执行时机、失败行为和上下文见[生命周期](../plugin-lifecycle.md)。

## 发布资料与关系

`publish.icon` 为 HTTPS 外部图标 URL，`publish.screenshots` 为最多 8 个 HTTPS 截图 URL。包内 `icon` 和 `screenshots` 是文件路径，不能混用。下载地址优先使用 `downloadUrl`，同版本内容必须保持不变。[发布指南](publishing.md)

依赖值可为版本范围字符串，或 `{version, source}`；source 是 HTTPS `catalog.json` 地址。冲突值是版本范围。可选依赖缺失不阻止安装，独占资源只按作者声明校验。[依赖与冲突](../plugin-relationships.md)

清单 JSON 最大 256 KiB，载荷路径最多 512 字节。载荷路径仅允许 ASCII 字母、数字及 `/._-+`，禁止绝对路径、空段、`.`、`..`、软链接和未声明的附加包文件。清单通过校验后，运行权限仍需用户判断；文件哈希不代表插件安全审核。

`backend.uiVisibilityEvents`（boolean，默认 false）：订阅宿主专用 `framely.ui.visibility` RPC。后端必须及时回复；通知无需页面挂载。该字段需要支持可见性事件的新版宿主。
