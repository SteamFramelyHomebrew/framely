# SDK API 文档

[English](../../developer-guide/sdk.md)

[开发流程](README.md) · [Manifest](manifest.md) · [生命周期](../plugin-lifecycle.md)

SDK 源码位于 `sdk/src/index.tsx`、`sdk/src/lifecycle.ts` 和 `sdk/python/framely.py`。页面从 `@framely/sdk` 导入，Python 协议库由构建工具复制到 `payload/framely.py`。

## 页面注册

```tsx
import {registerPlugin, Section} from '@framely/sdk';
function QuickPage() {return <Section title="我的插件">快捷内容</Section>;}
function MainWindow() {return <Section title="主窗口">详细内容</Section>;}
registerPlugin({QuickPage, windows: {main: MainWindow}});
```

`QuickPage` 必填。`WindowPage` 是独立窗口的默认组件；`windows` 可按清单窗口 key 指定不同组件。没有对应组件时回退到 `WindowPage`，再回退到 `QuickPage`；`quick` 用于快捷页路由，避免将它用作窗口 key。页面运行在宿主的 sandbox iframe，宿主先建立桥接再加载你的 bundle。注册函数会挂载 React、基础样式和滚动条。

## 宿主接口 `framely`

| API | 参数与返回 | 用途 |
| --- | --- | --- |
| `framely.call<T>(method, params = {})` | `Promise<T>` | 调用本插件后端方法；错误会 reject |
| `framely.windows.open(key)` | 窗口 key；异步结果 | 打开已在 Manifest 声明的窗口 |
| `framely.windows.close(key)` | 窗口 key；异步结果 | 关闭窗口，不停用后端 |
| `framely.notifications.send(notification)` | 通知对象；异步结果 | 发送或按 ID 更新通知 |
| `framely.notifications.remove(id)` | 通知 ID；异步结果 | 撤回通知 |
| `framely.dependencies()` | `Promise<DependencyStatus[]>` | 查询本插件声明的依赖状态 |
| `framely.language.get()` | `Promise<{preference, language}>` | 获取语言偏好和当前生效语言 |
| `framely.onEvent(callback)` | 返回取消订阅函数 | 接收宿主转发的事件 |

所有调用只通过插件桥接，不提供管理员 API 或跨插件后端调用。用 `try/catch` 或 `.catch` 处理失败，异步操作期间禁用重复提交。

```tsx
try {
  const result = await framely.call<{text: string}>('settings.get');
  console.log(result.text);
} catch (error) {console.error(error);}
```

## 通知

```tsx
await framely.notifications.send({
  id: 'task', title: '完成', body: '文件已处理', durationMs: 8000,
  actions: [{id: 'open', label: '打开', icon: '↗'}],
});
```

`id`、`title`、`body` 必填。`image` 可选，接受 PNG/JPEG data URL 或 HTTPS URL，字符串最大 1 MiB。`actions` 最多 3 项，每项有 `id`、`label`、`icon`。`durationMs` 范围 1000–60000，省略时默认 8000 ms。`id` 和按钮 `id` 遵循本体 ID 规则，按钮 ID 在同一通知内不得重复；`title` 最大 160 字节、`body` 4096 字节、按钮 `label` 非空且最多 80 字节、`icon` 最多 16 字节。每插件每 10 秒最多发送 10 次，全局最多 64 条同时存在的通知；同 ID 更新不额外占队列位置。

按钮动作调用后端 `notification.action`，参数 `{id, action}`；同时转发 `notification.action` 页面事件。无后端插件可只监听页面事件。同一 ID 用于更新，调用 remove 撤回。

## React hooks

| Hook | 返回与行为 |
| --- | --- |
| `useBackend<T>(method, params = {})` | `{data, error, loading, call}`；不会自动发起请求，主动调用 `call()`；该调用失败仍会抛出异常 |
| `usePluginEvent(callback)` | 自动订阅并在页面卸载时取消订阅 |
| `useDependencies()` | `{items, error}`；自动加载依赖并在 `dependencies.changed` 时刷新 |

手动订阅时通过 React effect 返回取消函数：

```tsx
React.useEffect(() => framely.onEvent(event => console.log(event)), []);
```

`DependencyStatus` 包含 `id`、`required`、`constraint`、`version`、`enabled`、`matches`、`available` 和可选 `state.phase`。`constraint` 为版本范围字符串或 `{version, source}`；未安装时 `version` 为 null。[关系参考](../plugin-relationships.md)


宿主语言变化转发 `{type: "language.changed", data: {preference, language}}`；依赖变化转发 `{type: "dependencies.changed"}`。`onEvent` 的类型为 unknown，访问字段前应做类型检查。Python `emit("progress", {...})` 转发为 `{type: "progress", data: {...}}`。

## UI 控件

| 组件 | 主要属性 |
| --- | --- |
| `Section` | `title`、`children` |
| `Button` | 标准 React button 属性，包括 `onClick`、`disabled` |
| `Toggle` | `label`、`checked`、`onChange(boolean)`；可选 `description`、`disabled` |
| `Slider` | `label`、`value`、`onChange(number)`；默认 `min=0`、`max=100`、`step=1` |
| `TextField` | `label`、`value`、`onChange(string)`；可选 `multiline`、`password`、`placeholder`、`disabled` |
| `Select` | `label`、`options: {value,label}[]`、`value`、`onChange`、`disabled` |
| `Tabs` | `tabs: {id,label}[]`、`value`、`onChange(id)` |
| `Notice` | `children`；`error=true` 显示错误 |

`Select` 默认单选，`value` 为 string；设置 `multiple` 时 `value` 为 string[]，回调返回数组，可使用 `emptyLabel` 和 `clearLabel`。下拉在页面内渲染，适配离屏 CEF。

标准输入控件在 Frame 中自动连接 SteamVR 键盘，标准可交互控件自动获得悬停反馈。自定义控件应使用正确 ARIA role，可增加 `data-framely-interactive`。网页不自行控制其他窗口、手柄或系统账号。

## Python 后端 SDK

```python
from framely import serve, emit

def dispatch(method, params):
    if method == 'ping':
        emit('progress', {'percent': 100})
        return {'ok': True}
    raise ValueError('Unknown method')

serve(dispatch)
```

`serve(dispatch, lifecycle=None)` 读取逐行请求、调用处理器，并把返回值或异常转换成 JSON 响应。`emit(event, data)` 输出事件，页面侧收到 `{type, data}`；`notification` 事件可主动发送通知。日志写 stderr，stdout 专用于协议。生命周期字典写法见[生命周期文档](../plugin-lifecycle.md)。

## TypeScript 生命周期辅助 API

从 `@framely/sdk/lifecycle` 导入 `registerLifecycle`、`LifecycleContext`、`LifecyclePhase`、`LifecycleCallbacks`。`registerLifecycle(callbacks)` 返回异步 `(method, context)` dispatcher；把保留的生命周期方法交给它处理。它不实现 stdin/stdout 协议、不启动进程，也不赋予网页后台权限。

## 预览与权限边界

`npm run dev` 只模拟窗口、通知及部分桥接调用；后端、真实依赖状态、VR 输入、振动和身份行为需要实机测试。插件页面不能直接调用管理面板 API，也不能调用 `framely.lifecycle.*` 保留方法。SDK 本身不声明权限，运行用户由 Manifest 决定。
