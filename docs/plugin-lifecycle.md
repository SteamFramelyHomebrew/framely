# 插件生命周期

[English](en/plugin-lifecycle.md)

生命周期由 root 核心服务调度，但钩子使用插件声明的运行身份；省略身份为 `steamos`。有后端时，独立钩子与后端身份必须一致；纯 UI 插件可在 `lifecycle.runAs` 声明身份，安装时展示运行用户。

```json
{
  "backend": {
    "entry": "backend.py",
    "runAs": "steamos",
    "autostart": false,
    "restart": "on-failure",
    "restartLimit": 3
  },
  "lifecycle": {
    "onInstall": {"entry": "backend.py"},
    "onUpdate": {"entry": "backend.py"},
    "onStart": true,
    "onStop": true,
    "onUninstall": {"entry": "backend.py"},
    "onCrashCleanup": {"entry": "backend.py", "args": ["--cleanup"]},
    "timeoutSeconds": 10
  }
}
```

所有钩子可省略。独立命令的 entry 必须是包内文件并包含在 files 哈希清单；不能指定 shell 字符串或包外路径。没有为插件增加身份或网络权限。timeoutSeconds 为 1–15 秒，默认 10 秒，每个钩子分别计时。

| 钩子 | 执行时机与方式 |
| --- | --- |
| onInstall | 第一次安装，新版本激活前执行独立命令 |
| onUpdate | 更新或回退，停止旧后端后、新版本激活前执行目标版本的独立命令 |
| onStart | 启动后端后发送 `framely.lifecycle.start` RPC，成功后才接受业务调用 |
| onStop | 禁用、重启、更新、回退、卸载和管理器正常退出时，停止进程前发送 `framely.lifecycle.stop` RPC |
| onUninstall | 停止后端后、删除包前执行独立命令 |
| onCrashCleanup | 异常退出、信号、OOM、调用超时或初始化失败后，由独立进程执行清理 |

关闭快捷菜单或大窗口只卸载对应 React 页面，不停止后端。React effect 清理仍用于页面订阅、预览租约等；不能替代后端生命周期。按需启动与 autostart 是启动时机，restart 是已经启动后发生失败的恢复策略。

## 上下文与 SDK

所有回调收到以下上下文：

```ts
interface LifecycleContext {
  pluginId: string;
  phase: 'onInstall'|'onUpdate'|'onStart'|'onStop'|'onUninstall'|'onCrashCleanup';
  reason: string;
  version: string;
  previousVersion: string|null;
  dataDir: string;
  exit: {reason:string;exitCode?:number|null;signal?:number|null;oom?:boolean;message?:string}|null;
}
```

独立命令读取 `FRAMELY_LIFECYCLE` 与 JSON 编码的 `FRAMELY_LIFECYCLE_CONTEXT`。所有后端与命令均有 `FRAMELY_PLUGIN_ID`、`FRAMELY_PLUGIN_VERSION`、`FRAMELY_DATA_DIR` 及对应身份 HOME。独立命令 stdout/stderr 是日志，退出码 0 表示成功；后端钩子按普通 JSON 行协议回复 result 或 error。`framely.lifecycle.*` 为核心服务保留，插件业务 API 不能调用。

Python SDK 位于 `@framely/sdk/python`，构建时复制为载荷中的 `framely.py`，不需额外 pip 依赖：

```python
from framely import serve

def initialize(context):
    # 可重复执行；仅操作本插件的数据和资源。
    return {"ready": True}

def cleanup(context):
    return {"cleaned": True}

def dispatch(method, params):
    return {"ok": True}

serve(dispatch, {
    "onInstall": initialize, "onUpdate": initialize,
    "onStart": initialize, "onStop": cleanup,
    "onUninstall": cleanup, "onCrashCleanup": cleanup,
})
```

SDK 检测独立钩子环境时只执行相应回调并退出，不进入 RPC 循环。TypeScript 后端可从 `@framely/sdk/lifecycle` 导入 `registerLifecycle` 和类型，将保留方法交给返回的 dispatcher；它不负责启动进程或实现 JSON 行传输。网页侧 SDK 也导出上下文类型，但不提供后端运行权限。

## 失败、清理与恢复

安装或更新的命令钩子失败时不激活新版本。若安装/更新当时需要启动后端（常驻或原本在运行），初始化失败也恢复原版本与 current 链接；初次安装则撤销安装。按需插件首次安装通常只执行 onInstall，onStart 在首次打开页面/业务调用等实际启动时执行；此时失败走启动恢复，不会追溯撤销之前已完成的安装。回退的初始化失败时恢复回退前的版本。文件版本可恢复，数据迁移和设备操作无法自动撤销：迁移应可重复执行、采用备份/临时文件与原子替换，并保持上一版本能读取数据。

onStop 失败或超时会记录错误，仍会停止整个插件进程组。崩溃后先关闭该插件的窗口、通知，再执行 onCrashCleanup；清理失败不会阻止错误记录或后续恢复。崩溃清理必须是独立命令，无法依赖已经崩溃进程里的内存、析构器或回调。确认资源属于本插件后再删除；不要重启 SteamVR 或删除其他插件的文件。

onUninstall 失败保留插件并展示错误，用户可重试或选择强制卸载。强制卸载仍尝试有超时限制的清理，随后移除插件包；用户数据默认保留，仅 purge 删除。强制卸载不能保证外部设备设置已还原。

restart 默认为 on-failure；never 只记录失败，不自动重启。restartLimit 默认为 3，范围 1–10，达到连续失败阈值后停用；失败计数在后端稳定运行 60 秒后重置。重试等待为 1、2、4、8、16、32 秒，最多 32 秒。正常自主退出（退出码 0）不重启；管理器主动停止也不计入失败。退出码、信号、systemd OOM 和调用超时记录在状态与日志中。

状态包含 starting、running、stopping、stopped、recovering、failed 等阶段，并发管理操作沿核心服务的串行调度执行。管理器正常 SIGTERM/SIGINT 会尝试逐个停止插件；强制终止、断电不能保证任何钩子执行。清理应同时可在下次启动时安全恢复。

日志页面合并展示后端日志与 lifecycle 日志；每种日志保留当前 2 MiB 与上一份。安装失败且插件尚未登记时，可从 `/var/lib/framely/logs/<id>.lifecycle.log` 查看。运行身份的数据目录为 `/var/lib/framely/data/<id>/<身份>/`（自定义核心数据目录时相应变化）。
