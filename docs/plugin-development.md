# 插件开发参考

[English](en/plugin-development.md)

第一次开发请先阅读[完整开发流程](developer-guide/README.md)和[发布指南](developer-guide/publishing.md)。

Dock 右侧快捷入口为与主窗口同朝向的弧面临时浮层，常用/已安装/设置三个底部 tab；插件管理从设置打开独立的原生 Dock 大窗口。安装表单与插件源设置只在管理窗口中出现。

示例源码在 `examples/showcase`；React SDK 在 `sdk/src/index.tsx`。发行包 `tools/source` 提供完整 UI/SDK 示例构建工作区。插件页面需要自行打包为浏览器 JS，调用 `registerPlugin({QuickPage, WindowPage})` 挂载 React，宿主先加载桥接 bootstrap。普通 bundle 插件 iframe 为 `sandbox="allow-scripts"` 的独立不透明来源；通过父窗口转发被允许的 SDK 请求，不能直接调用管理员 HTTP API。

## Manifest

```json
{
  "schemaVersion": 1,
  "apiVersion": 1,
  "id": "example.plugin",
  "name": "Example",
  "author": "Developer",
  "version": "1.0.0",
  "description": "Example plugin",
  "backend": {"entry": "backend.py", "runAs": "steamos", "autostart": false},
  "ui": {
    "quickPage": "page.js",
    "windows": {"main": {"entry": "page.js", "title": "Example", "dockIcon": true}}
  },
  "files": {}
}
```

`backend` 可省略。`runAs` 只支持 `steamos` 和 `root`，省略时为 `steamos`；`autostart` 默认 false。不再使用 `permissions` 字段，窗口、通知和联网无需权限声明。后端使用设备网络，普通插件页面允许 HTTP/HTTPS 请求及 WebSocket。

ID 只允许小写字母、数字、点、短横线、下划线；载荷路径限 ASCII 字母/数字、`/._-+`，禁止绝对路径、空段、`.`、`..`。入口必须在 files 中；pack 会自动计算 files，不必手写哈希。后端入口须可直接执行，脚本需要正确 shebang（示例 `/usr/bin/python3`）。Python/系统依赖不会由 Framely 自动下载。

版本不可覆盖，相同版本再次发布需要使用新版本号。运行身份变化会选择新的数据目录 `/var/lib/framely/data/<id>/<identity>`；旧身份数据保留，安装使用旧身份的版本可恢复访问，不自动跨身份迁移文件。

## React API

```tsx
import {framely, registerPlugin, Button, Section} from '@framely/sdk';

function QuickPage() {
  return <Section title="Example">
    <Button onClick={() => framely.windows.open('main')}>打开窗口</Button>
    <Button onClick={() => framely.call('save', {text: 'hello'})}>保存</Button>
    <Button onClick={() => framely.notifications.send({
      id: 'download', title: '完成', body: '下载已完成',
      durationMs: 8000, actions: [{id: 'open', label: '打开', icon: '↗'}]
    })}>发送通知</Button>
  </Section>;
}
registerPlugin({QuickPage});
```

窗口 key 必须在 manifest 中声明；`framely.windows.close(key)` 关闭。关闭页面不会停止后端。通知 ID 用于更新/撤回；`framely.notifications.remove(id)` 撤回。图片为 PNG/JPEG data URL（最多 1 MB）或 HTTPS URL。时长 1–60 秒，每插件每 10 秒最多 10 次发送。

`framely.onEvent(callback)` 返回取消订阅函数；接收 `{type, data}`。多个窗口分别按游标接收同一后台事件。文本输入框获得焦点时请求 SteamVR 虚拟键盘，完成后通过原生 setter 触发 React input/change；不使用网页 prompt。

## 后端协议

每行一个 JSON，stdin 请求、stdout 响应。stdout 只写协议，日志写 stderr（单文件最多约 2 MiB，保留一份轮转备份）。每条消息最多 64 KiB，输入写入超时 5 秒或响应超时 15 秒会停止后端，下次打开可重试。

```json
{"id":1,"method":"save","params":{"text":"hello"}}
{"id":1,"result":{"saved":"hello"}}
{"id":2,"error":"Unknown method"}
{"event":"progress","data":{"percent":50}}
```

系统传入 `FRAMELY_PLUGIN_ID`、`FRAMELY_DATA_DIR` 和对应身份 HOME。通知按钮向后端调用 `notification.action`，参数 `{id, action}`。后端主动发送通知使用 `{"event":"notification","data":{...通知...}}`，受同一 manifest 与频率限制。

## 打包与源发布

```bash
framely pack --manifest manifest.json --payload payload --output example.plugin-1.0.0.framely
framely verify example.plugin-1.0.0.framely
framely catalog --name 'My plugins' --base-url https://example.org/plugins --packages ./packages --output ./packages/catalog.json
```

包中包含 manifest.json 和文件载荷，files 记录每个载荷的 SHA256；不生成公钥或签名。只支持 manifest.json 与清单中声明的载荷文件，拒绝未声明的附加文件。

源需静态托管 catalog.json 及相对路径下的 plugins/<ID>/versions.json；插件包可放在作者自己的 GitHub Release。在插件源管理中添加目录完整 URL；当前代码首次初始化加入社区 stable/testing 来源，实际内容以服务器为准。目录条目有版本、下载 URL、SHA256。每个源独立显示，插件记录所选来源；另一个源同 ID 不会静默替换。HTTP 必须用户明确允许；目录和包支持最多五次重定向，HTTPS 不允许降级为 HTTP。

### 悬停反馈

Framely 主页面和插件 iframe 自动为可交互控件提供轻微的手柄振动，插件无需自行触发。进入按钮、链接、输入控件、可操作 ARIA role、可聚焦控件或 `cursor:pointer` 区域时触发一次；同一控件的子元素之间移动不会重复触发，禁用或 inert 控件不触发。自定义 React 控件应采用正确的 ARIA role，也可以加 `data-framely-interactive` 标记。

振动由 OpenVR 的当前激光控制器执行（12 ms、120 Hz、幅度 0.15），宿主只接受当前可见且被激光命中的视图，限制相邻脉冲间隔为 80 ms。插件发出的反馈消息由 iframe 宿主绑定到实际视图，不能指定另一个窗口或手柄。

## 商店资料与图标

Manifest 可声明 `icon`（载荷内 PNG，最多 1 MiB、1024×1024）、`details`、`tags`、`screenshots`（最多 8 个载荷内 PNG/JPEG 路径）、`changelog`。这些文件会和页面一样加入文件哈希清单。图标用于插件列表和插件独立窗口的原生 Dock 缩略图，省略时使用默认拼图图标。

清单还支持三个可选网页地址，未填写的入口不显示：`authorUrl`（作者主页）、`documentationUrl`（插件文档）、`homepage`（插件主页）。地址必须是 HTTP 或 HTTPS，可包含文档锚点，不接受带用户名密码的链接。这些字段会保留在安装后的清单，并由 `framely catalog` 导出到商店目录；作者主页链接直接显示在作者名称上，未提供时作者名称为普通文本；商店列表、详情、管理弹窗和安装确认使用相同行为。文档和插件主页保留独立入口，点击后由系统默认浏览器打开。

```json
{
  "authorUrl": "https://github.com/author",
  "documentationUrl": "https://github.com/author/plugin#readme",
  "homepage": "https://github.com/author/plugin"
}
```

插件仓库可声明发布信息：

```json
{
  "downloadUrl": "https://github.com/author/plugin/releases/download/v1.0.0/plugin.framely",
  "publish": {
    "icon": "https://github.com/author/plugin/releases/download/v1.0.0/icon.png",
    "screenshots": ["https://github.com/author/plugin/releases/download/v1.0.0/screen.png"]
  }
}
```

上述为显式地址示例，`downloadUrl` 并非源码必填字段：GitHub 社区登记可按仓库、`v<version>` 和 `<id>-<version>.framely` 自动生成。包内仍需最终地址，当前 pack 不会自动读取 Git remote；按[发布指南](developer-guide/publishing.md)生成临时清单即可。社区图标通常只需 `icon`，数据库从固定源码提交生成并校验 Raw 地址；无需额外 `publish.icon`。本地 catalog CLI 使用 `publish` 外部图片，省略时不会导出包内图标/截图，商店采用默认图标。包内资源仍用于已安装插件。

插件数据库只登记 submodule 固定提交，读取该提交的清单，补全默认下载地址，先取得 Release digest 或源码 `downloadSha256` 中的预期整包哈希，再下载并比对 SHA256、清单声明和载荷。发布分支提供各渠道目录及历史版本 JSON，不保存插件包或图片，不需要 entries 登记。数据库 CI 自动展示版本和运行用户变化，发布时禁止同 ID、版本更换 SHA256；不执行作者代码，不要求逐插件人工源码审核，也不验证源码与二进制可复现关系。

`framely catalog` 也只生成 JSON：下载 URL 优先使用 `downloadUrl`，没有该字段时采用 `--base-url` 加包文件名。图标与截图来自 `publish` 的外部 URL，不复制资源。商店支持关键词、标签、来源、已安装和可更新筛选；源暂时不可用时显示会话缓存。安装使用已经校验的同一份包，不在确认后重新下载。

后端默认按失败自动恢复，包括已经启动的按需插件；可用 `backend.restart: "never"` 关闭。默认第三次连续失败后停用，运行稳定一分钟后重置计数，详见[生命周期](plugin-lifecycle.md)。

## 脚手架与预览

```bash
node tools/plugin-dev.mjs init ./my-plugin example.my-plugin
cd my-plugin
npm install
npm run dev
npm run build
```

发行包中的工具位置为 `tools/source/tools/plugin-dev.mjs`。预览监听本机 127.0.0.1:5173，修改页面后自动重载；它不会启动后端或获得 Frame 系统权限。窗口和通知在浏览器里模拟，身份、键盘、振动及后台调用仍需安装到设备测试。

SDK 提供 `Section`、`Button`、`Toggle`、`Slider`、`TextField`、`Select`、`Tabs`、`Notice`、`useBackend`、`usePluginEvent`。Select 使用页面内弹层，兼容离屏 CEF。`registerPlugin` 还可传入 `windows: {main: Component, other: Component}` 为多个声明窗口选择不同 React 页面。

通知按钮向后端发送 `notification.action`，同时广播给当前打开的插件页面，事件为 `{type:'notification.action',data:{id,action}}`；无后端插件可以通过 `framely.onEvent` 接收。

插件安装、更新、停止和异常恢复的约定见[生命周期开发说明](plugin-lifecycle.md)。

窗口可选声明 `width`、`height`（逻辑像素）和 `widthMeters`（VR 物理宽度，米）。默认 1600×900（16:9）、VR 宽度 3 米；像素范围为宽 640–2560、高 360–1440，物理宽度 0.4–4.0 米。示例：`{"entry":"page.js","title":"温控","dockIcon":true,"width":1600,"height":900,"widthMeters":3.0}`。尺寸在重新打开窗口时生效。

依赖、可选依赖、冲突和独占资源见[插件关系](plugin-relationships.md)，多个插件源聚合见[源订阅](source-subscriptions.md)。

插件只使用 `tags` 描述用途，不再使用分类；为兼容已发布的包，旧 `category` 字段读取时忽略，新的清单及目录不再导出它。插件数据库每次生成目录时同时生成 `tags.json`，格式为 `{ "schemaVersion": 1, "tags": ["工具", "温控"] }`，聚合当前目录全部插件的标签、精确去重并排序。stable/testing 分别生成自己的标签文件，不生成跨渠道并集；程序加载多个源后在本地聚合去重。标签随插件更新自动增删，无须手工维护。

上传的插件包、商店下载及其依赖包暂存于 `/tmp/framely-package-*`，确认安装后才写入插件目录。取消、安装完成或失败后删除暂存包；未完成上传和待确认安装包闲置 15 分钟后过期，每 30 秒清理一次。

本机网页窗口在 `ui.windows.<key>` 设置 `localWeb: true`，宿主通过后端 `window.get` 获取窗口 URL；这是窗口加载方式，不是权限声明。URL 使用 `http://localhost:<端口>/framely-window/<key>`。普通窗口无需设置此项。

商店主目录 `catalog.json` 中每个插件 ID 仅出现一次，代表最新版。历史条目存放在相对目录 `plugins/<插件ID>/versions.json`，格式为 `{ "schemaVersion": 1, "id": "作者.插件", "versions": [...] }`；版本条目沿用目录条目的字段，列表不包含主目录的最新版。商店进入详情时按需读取历史，依赖解析仅在最新版不满足范围时读取历史。附加目录字段可扩展，读取端忽略未知字段，但校验已知字段；改变必需语义应升级 `schemaVersion`。插件安装包清单仍严格校验。
