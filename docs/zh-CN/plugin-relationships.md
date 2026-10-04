# 插件依赖与冲突（Framely 0.4）

[English](../plugin-relationships.md)

```json
{
  "dependencies": {
    "example.shared-api": "^1.0.0",
    "example.camera-api": {
      "version": ">=1.2.0 <2.0.0",
      "source": "https://example.org/plugins/catalog.json"
    }
  },
  "optionalDependencies": {"example.notifications": "^1.0.0"},
  "conflicts": {"example.other-camera": "*"},
  "exclusiveResources": ["steamvr.passthrough-color"]
}
```

四个字段都可省略。声明关系的插件使用合法 SemVer；作为依赖或冲突版本匹配目标的插件也应使用 SemVer。版本范围支持精确版本、`^`、`~`、`*` 和逗号/空白分隔的比较条件。`1.2.0` 表示精确匹配；`>=1.2.0 <2.0.0` 表示两个条件同时满足，不支持 `||`。拒绝自身依赖、重复必需/可选声明、依赖与冲突矛盾和重复独占资源。

`source` 必须是 HTTPS 插件目录 URL，不是订阅或包下载 URL。优先复用版本与来源满足要求的已安装插件；未声明来源时优先当前插件所在源，其他来源必须明确选择。指定来源不可用或没有匹配版本时不换源。明确依赖源尚未添加时，进入安装计划统一确认；已停用的源需用户先启用。删除源后保留已安装插件的来源 URL 记录，以便依赖约束继续识别其来源。

本地包没有默认来源时，未安装依赖需要选择来源；也可通过 CLI 指定已添加源 ID：

```bash
framely install plugin.framely --source my-source --approve
framely install plugin.framely \
  --dependency-source example.shared-api=https://example.org/catalog.json \
  --approve
```

`--dependency-source` 可重复使用，不能覆盖清单中的明确来源。CLI 和界面使用同一解析器。需要接受已有插件运行用户变化时再加 `--approve-run-as`。

主目录每个插件 ID 只出现一次，历史版本来自相对路径 `plugins/<ID>/versions.json`；主目录版本是推荐版本。用户可选择旧版本安装或重新安装当前版本；包的 ID、版本、哈希及依赖均重新校验。依赖解析优先复用已安装版本，否则选取源中第一个满足约束的版本。多个版本/来源约束无法同时满足、出现循环依赖、缺少必需依赖时停止。缺失可选依赖不阻止安装，也不自动安装。

安装计划展示所有包、版本、来源、运行用户、新增源、依赖启用和冲突停用。确认后使用同一份已校验包；设备插件/来源状态改变后要求重新检查。最多 32 个插件，不限制插件包大小或解压后的总大小。先完成下载和校验，再依次安装，检查反向依赖，防止更新破坏已有依赖方。

批量操作记录持久事务日志；失败恢复版本链接、来源和启用状态，撤销本次新增的包安装，并尝试恢复此前运行的后端。安装期间管理器意外退出时，下次启动恢复原记录。用户数据保留；生命周期迁移和设备操作不能由文件版本回退自动撤销，插件必须设计可重复执行、可恢复的迁移。

启用时统一确认需要启用的必需依赖及需要停用的冲突插件。冲突按任意一方声明处理，允许同时安装，禁止同时启用；同名独占资源同样禁止同时启用。停用/卸载基础插件需确认一起停用依赖方，依赖方不会被自动卸载。

后端按依赖顺序启动，`onStart` 成功才启动依赖方；没有该钩子的后端沿用进程启动成功判断。停止顺序相反。关闭页面不停止后端。基础后端异常时暂停正在运行的依赖方，进入“等待依赖”，不累计依赖方失败次数；基础后端恢复后重新按顺序启动。基础后端被停用或禁止重启时等待用户处理。

SDK 提供：

```tsx
import {framely, useDependencies} from '@framely/sdk';
const states = await framely.dependencies();
// React 页面：const {items, error} = useDependencies();
```

只返回当前插件声明的必需/可选依赖，含版本、启用状态、约束是否满足、后端是否可用和运行阶段。依赖状态变化触发 `dependencies.changed` 事件；不开放跨插件后端调用，不增加运行权限。独占资源和 conflicts 都依赖作者声明，不自动检测任意文件或设备冲突。

不提供插件手动回滚接口；安装旧版本统一走正常安装计划和 `onUpdate`。安装失败的事务恢复仍保留，不是用户选择版本的入口。同 ID 同版本必须保持发布内容不变。
