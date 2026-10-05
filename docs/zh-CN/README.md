# Framely 文档

[English](../README.md)

Framely 是 Steam Frame 的插件管理器，包含设备端服务、SteamVR 界面、网络管理面板和电脑端安装器。

## 用户指南

按顺序阅读即可完成安装并使用第一个插件。

1. [安装、更新、修复与卸载](user-guide/installation.md)：桌面安装器、SSH、Preview、本地包、回滚。
2. [基本使用](user-guide/basic-usage.md)：Dock 入口、管理窗口、启停、收藏、安全模式。
3. [安装与管理插件](user-guide/plugins.md)：插件库、来源、订阅、本地包和更新。
4. [网络管理面板](user-guide/network-panel.md)：地址、首次密码、登录、手机/电脑使用。

## 插件开发与发布

- [完整流程](developer-guide/README.md)：生成项目 → 浏览器预览 → 后端 → 打包 → 实机测试 → 发布。
- [SDK API](developer-guide/sdk.md) · [Manifest 配置说明](developer-guide/manifest.md) · [生命周期](plugin-lifecycle.md)
- [发布插件与插件源](developer-guide/publishing.md)：GitHub 默认地址生成、自定义地址与哈希、目录历史、数据库登记。
- [可用插件模板](../../templates/plugin/README.zh-CN.md)：React + Python，脚手架与仓库内编辑两种使用方式。
- [功能展示插件](../../examples/showcase/README.zh-CN.md)：更多 UI 和设备交互示例。

## 技术参考

| 参考 | 内容 |
| --- | --- |
| [清单、SDK 与后端协议](plugin-development.md) | Manifest 字段、窗口、通知、JSON RPC、打包规则 |
| [生命周期](plugin-lifecycle.md) | 安装、更新、启动、停止、卸载及恢复钩子 |
| [依赖与冲突](plugin-relationships.md) | 版本范围、来源选择、可选依赖、独占资源 |
| [源订阅格式](source-subscriptions.md) | JSON 格式、刷新、地址变更及去重 |
| [语言与翻译](localization.md) | 语言包、文案键、社区翻译 |

## 维护者资料

- [Framely 发行与更新机制](releases.md)
- [电脑端安装器构建](../../installer/README.zh-CN.md)

- [空间启动台](launcher.md)
- [启动台操作与本体兼容](developer-guide/launcher.md)
