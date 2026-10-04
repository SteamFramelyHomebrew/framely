# Framely 插件模板

[English](README.en.md)

模板包含 React 快捷页、独立 Dock 窗口、通知、Python JSON RPC 后端和持久数据示例。默认使用 `steamos` 身份，不申请 root。

推荐从 Framely 仓库生成独立项目，脚手架会复制 SDK、许可证和构建工具：

```bash
node tools/plugin-dev.mjs init ../my-plugin yourname.my-plugin
cd ../my-plugin
npm install
npm run dev
npm run build
```

生成后修改 `manifest.json` 的 ID、名称、作者、描述、版本、标签及更新记录。页面源码在 `page.tsx`，后端在 `backend.py`，构建结果在 `payload/`。`files` 由打包工具计算，保持空对象即可。

浏览器预览为 `http://127.0.0.1:5173`，只模拟窗口和通知。读取/保存按钮调用真实后端，必须安装到 Frame 测试；浏览器中会显示“预览不启动后端”的提示。

在安装了 Rust 的 Linux/WSL 环境设置 Framely 仓库位置，打包并验证（本体 CLI 依赖 Linux 接口）：

```bash
FRAMELY_REPO=/absolute/path/to/framely
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- pack --manifest manifest.json --payload payload --output my-plugin-0.1.0.framely
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- verify my-plugin-0.1.0.framely
```

把包上传到 Frame，在网络管理面板“插件 → 导入插件”选择本地文件，检查运行用户后确认。升级时递增版本号、重新构建打包；后端数据保存在 `FRAMELY_DATA_DIR`，不要写到程序目录。

如果直接在 Framely 仓库内编辑本模板，在 `templates/plugin/` 执行 `npm install`、`npm run dev`、`npm run build`。直接复制模板目录到其他位置时，需同时调整 `package.json` 的 SDK/构建工具路径；使用脚手架可自动完成这些步骤。

模板及随附 SDK 使用 `AGPL-3.0-only`。完整开发到发布说明见 Framely 的 `docs/developer-guide/README.md`；模板的生命周期回调不会删除用户数据。

用于社区 GitHub Release 登记时，源码可省略 `downloadUrl`，默认附件名为 `<id>-<version>.framely`。发布包要用包含自动生成地址的临时清单打包；完整命令、Release digest 校验、自定义地址和数据库窗口尺寸校验规则见 Framely 的 `docs/developer-guide/publishing.md`。上面的直接打包命令用于本体测试，不代表社区登记已经通过。
