# 启动台、操作与本体兼容要求

[English](../../developer-guide/launcher.md) · [Manifest](manifest.md) · [SDK](sdk.md)

从 Framely **0.4.3-preview.2** 开始支持。旧插件无需修改：短按默认打开快捷页面，长按显示“快捷面板、管理插件、卸载插件”。没有快捷页面时对应按钮置灰，卸载始终需要确认。

## 本体版本要求

```json
"engines": {"framely": ">=0.4.3-preview.2 <0.5.0"}
```

`engines.framely` 是强制执行的本体兼容条件；`apiVersion` 表示协议版本，两者独立。旧插件可以省略；使用启动台新增声明时必须填写，并明确约束最低版本为 `0.4.3-preview.2` 或更新版本。

与插件依赖共用范围解析：精确版本（`0.4.3` 或 `=0.4.3`）、比较符、空格或逗号连接的交集、`^`、`~` 和通配符。不支持 `||` 和连字符区间。普通范围不匹配预览版；需要在比较条件中显式写出对应主、次、补丁版本及预览标识。例如 `>=0.4.3-preview.2 <0.5.0` 接受后续 0.4.3 预览版和兼容正式版，但不会自动接受 0.4.4 预览版。构建标识不影响匹配。

安装、更新、降级安装和启动均检查兼容性；不兼容安装在执行钩子或替换文件之前被拒绝。插件库仍展示不兼容版本及要求，依赖解析跳过这些版本。本体降级后保留插件文件及启用偏好，但阻止执行。已发布的旧本体会拒绝未知清单字段，无法追溯增加友好升级提示。

## 不同入口的行为

```json
"ui": {
  "quickPage": "page.js",
  "windows": {"main": {"entry": "page.js", "title": "Example"}},
  "launch": {
    "launcher": {"type": "window", "window": "main"},
    "quickPanel": {"type": "quickPage"},
    "manager": {"type": "window", "window": "main"}
  }
}
```

未配置的入口默认进入对应插件快捷页面。管理面板独立管理按钮不受影响。长按菜单内置的“快捷面板”始终进入快捷页，不受入口配置影响。窗口目标必须引用已声明窗口，快捷页目标必须声明快捷页面。现有 `plugin.open` 仍仅启动后台，不执行入口导航。

## 长按操作

自定义按钮按声明顺序显示在三个内置操作上方；最多 16 个，ID 合法且不重复，名称非空且不超过 120 字节。名称由插件提供，发布时自行选择合适的语言。

```json
"launcherActions": [
  {"id": "open", "label": "打开窗口", "target": {"type": "window", "window": "main"}},
  {"id": "connect", "label": "连接", "target": {"type": "backend", "method": "connect", "params": {"profile": "default"}}},
  {"id": "notify", "label": "发送通知", "target": {"type": "frontend", "entry": "actions.js"}}
]
```

后台目标必须有后台，接收 `{params, context}`，禁止使用保留的 `framely.*` 方法。前端目标必须指向包内带哈希的脚本；点击时在独立沙箱中加载，不渲染快捷页面，可使用现有 SDK 能力。前端操作最多等待 90 秒，失败保留菜单并支持重试。操作不会取得插件管理或任意系统命令权限。

新模板会自动把 `actions.ts` 构建为 `actions.js`：

```ts
import {registerLauncherActions, framely} from '@framely/sdk';
registerLauncherActions({
  notify: async context => {
    await framely.notifications.send({
      id: 'hello', title: '你好', body: `来自 ${context.source}`, inbox: true,
    });
  },
});
```

## 启动上下文

```ts
const initial = await framely.ui.launchContext.get();
const unsubscribe = framely.ui.launchContext.onChanged(context => {
  console.log(context.source, context.trigger, context.actionId);
});
```

`source` 为 `launcher`、`quickPanel` 或 `manager`；`trigger` 为 `shortPress` 或 `menuAction`；自定义操作还包含 `actionId`。首次加载即可查询，复用页面或窗口时接收 `ui.launch` 事件。前端操作直接接收上下文，后台操作随参数接收。入口导致后台首次启动时，`onStart` 生命周期参数也包含 `launchContext`。没有已知来源的旧调用默认标记为快捷面板。
