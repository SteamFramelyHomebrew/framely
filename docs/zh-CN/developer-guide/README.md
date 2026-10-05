# 插件开发到发布：完整流程

[English](../../developer-guide/README.md)

[文档首页](../README.md) · [发布指南](publishing.md)

## 1. 准备开发环境

开发机准备 Git、Node.js 22、npm；发布指南中的临时清单脚本还需要 Python 3。本体 CLI 使用 Linux 专用接口，源码打包命令需在安装 Rust 的 Linux/WSL 环境执行，不能直接在 Windows/macOS 原生编译本体。也可将清单和 payload 拷到 Frame，使用已安装的 CLI 打包。设备端无需 Node.js/Rust，Python 后端依赖设备的 Python 3。开发机可以用于界面预览和打包，VR 交互必须在 Frame 验证。

```bash
git clone https://github.com/SteamFramelyHomebrew/framely.git
cd framely
node tools/plugin-dev.mjs init ../my-plugin yourname.my-plugin
cd ../my-plugin
npm install
npm run dev
```

这里的 `yourname.my-plugin` 是示例 ID，发布前换成你自己的稳定命名空间。脚手架拒绝覆盖已有目录，复制 SDK 和构建工具，生成后不依赖原 Framely 仓库的位置。将生成项目放入自己的 Git 仓库，并提交 `package-lock.json` 和 vendored SDK；不要提交 `node_modules/`、`payload/`、发行包和密码。

## 2. 认识模板

| 文件 | 用途 |
| --- | --- |
| `manifest.json` | ID、名称、版本、运行用户、页面、生命周期、依赖 |
| `page.tsx` | React 快捷页和独立窗口 |
| `backend.py` | Python 后端及数据保存 |
| `vendor/framely-sdk/` | UI、桥接接口和 Python 协议辅助库 |
| `dev.mjs` | 构建和本地预览工具 |
| `LICENSE` | AGPLv3 许可证全文 |

模板的“读取/保存”使用 `settings.get` / `settings.set`，内容保存在后端的 `FRAMELY_DATA_DIR/settings.json`。它还演示打开 `main` 窗口、发送通知、启动/停止和安装/更新/卸载回调。默认身份 `steamos`，默认按需启动。

## 3. 修改清单与界面

先修改 `manifest.json` 中的 ID、作者、名称、描述、版本、标签和 `changelog`。使用完整 SemVer，例如 `0.1.0`、`0.2.0-preview.1`；已发布版本不覆盖。

`ui.quickPage` 和 `ui.windows.main.entry` 指向构建后的 `page.js`。页面通过 `registerPlugin({QuickPage, WindowPage})` 注册。窗口 key 必须与清单一致。`files` 留空，打包时自动计算哈希。

在浏览器打开 `http://127.0.0.1:5173`；修改页面后自动重载。预览只模拟窗口和通知，不启动 Python 后端，不拥有 Frame 权限；读取/保存按钮在预览中报出后端不可用是正常情况。不要据此声称设备功能已验收。

UI 与后端 API 见 [SDK 文档](sdk.md)，字段、默认值和范围见 [Manifest 配置](manifest.md)；窗口与插件库资料补充见[插件开发参考](../plugin-development.md)。

## 4. 实现后端与生命周期

后端 stdin/stdout 使用逐行 JSON RPC；stdout 仅输出协议，调试日志输出 stderr。Python 模板使用 `framely.serve` 处理请求、错误和生命周期。修改业务逻辑时校验参数长度与类型，并返回能由 JSON 序列化的结果。

写数据使用 `FRAMELY_DATA_DIR`，不要写到插件载荷或 Framely 程序目录。升级、回退和运行用户变更的状态迁移由插件设计；`steamos` 与 `root` 对应不同的数据目录。普通功能优先使用 `steamos`，确需系统权限时才使用 `root` 并解释原因。

安装/更新钩子应可重复执行；停止/卸载时释放本插件持有的外部资源。关闭 UI 窗口不等于停止后端，React effect 清理不能替代后端钩子。系统依赖不会由 Framely 自动安装，需在说明中写清。[生命周期参考](../plugin-lifecycle.md)

## 5. 构建、打包、校验

在插件项目中执行：

```bash
npm run build
FRAMELY_REPO=/absolute/path/to/framely
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- pack --manifest manifest.json --payload payload --output yourname.my-plugin-0.1.0.framely
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- verify yourname.my-plugin-0.1.0.framely
```

将 `FRAMELY_REPO` 换成开发机上的 Framely 仓库绝对路径。也可在 Frame 使用已安装的 `/var/lib/framely/current/bin/framely pack` / `verify`，这些离线操作无需 sudo。

`payload/` 只放要分发的页面、后端、Python 协议库及必要资源。构建会复制 `backend.py`、`framely.py`；额外图标等资源需要自行放入该目录。打包检查入口、路径、清单和哈希，不允许软链接或额外包内文件。

## 6. 安装到 Frame 测试

将包下载到手机/电脑，进入网络面板，在“插件”的本地导入入口上传并确认；或通过 SSH 复制到 Frame 后执行：

```bash
sudo /var/lib/framely/current/bin/framely install ./yourname.my-plugin-0.1.0.framely --approve
```

首次协议确认后再管理插件。测试读取/保存、窗口和通知，停用再启用，重启后读回数据；修改版本后测试更新和数据保留，最后测试卸载钩子。涉及 VR 键盘、手柄或硬件的功能，记录实际 Frame 验证结果。

日志可通过 Framely 状态与 systemd 日志检查；后台插件 stdout 不应混入调试文字。更完整的安装操作见[用户教程](../user-guide/plugins.md)。

## 7. 发布

GitHub Release 默认使用标签 `v<version>` 和附件 `<id>-<version>.framely`，源码清单可以省略 `downloadUrl`。社区登记会自动拼接地址；打包前按[发布指南](publishing.md)生成包含最终地址的临时清单，因为当前 `framely pack` 不会自己读取仓库地址。自定义下载地址再显式填写 `downloadUrl`。随后公开 Release、上传包，提供使用说明、运行用户、更新记录和实际测试范围，再自行托管插件源或登记固定源码提交。社区数据库还需校验整包预期哈希和字段兼容性。

后续更新：提升版本号 → 重新生成发布清单（自定义地址时更新地址）→ 构建打包并校验 → 实机验证 → 新 Release → 更新来源登记。不要替换旧版本附件。

## 启动台操作与本体兼容

[启动台操作与本体兼容](launcher.md)
