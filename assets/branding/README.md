# Framely branding assets / 品牌资源

The pixel window sprite is the shared identity for Framely and its installer. `framely-mark.svg` and `framely-logo.svg` are dark artwork on transparent backgrounds; `*-light.svg` variants are for dark surfaces. `framely-app-icon.svg` uses a charcoal tile with a sky blue sprite. Square marks center the window body, allowing the left tail to extend outward; the native Dock sprite is 10% smaller. The wordmark uses vector paths and needs no font installation.

像素窗口小精灵是本体和安装器的统一标识。`framely-mark.svg` 和 `framely-logo.svg` 为透明底深色版本，`*-light.svg` 用于深色背景；`framely-app-icon.svg` 为黑底蓝色应用图标。正方形图标以窗口主体居中，尾巴向左伸出；原生插件入口中的图形缩小 10%。字标已转成路径，无需安装字体。

After editing the SVGs, regenerate PNGs, the Windows ICO and native Dock artwork from the repository root, then commit the generated files together:

修改 SVG 后，在仓库根目录重新生成 PNG、Windows ICO 和原生入口图形，并一同提交生成文件：

```bash
npm ci
npm run branding:build
```

macOS packaging uses the system `iconutil` to generate `.icns` from the PNGs. Runtime does not require Node.js. These brand assets use the repository's AGPL-3.0-only license.

macOS 打包时使用系统 `iconutil` 从 PNG 自动生成 `.icns`。运行时无需 Node.js；品牌资源采用仓库的 AGPL-3.0-only 许可证。
