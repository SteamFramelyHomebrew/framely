# 安装、更新、修复与卸载

[English](../../user-guide/installation.md)

[文档首页](../README.md) · [下一步：基本使用](basic-usage.md)

## 安装前

目标设备为 Linux ARM64 Steam Frame。当前开发基线为 SteamOS VR 0.4.2、SteamVR build 20260928.6175029；其他系统版本需验证。运行包自带 CEF，设备使用时无需安装 Node.js 或 Rust。

电脑与 Frame 连接同一网络，在 Frame 开启开发者模式并启用 SSH，准备 Steam 用户 `steamos` 的登录密码。安装会使用 sudo 创建账号、systemd 服务和发行目录；请核对 SSH 指纹。安装器不会自动解除 SteamOS 只读保护；实际所需目录不可写时会停止。

## 方式一：电脑端安装器

1. 从 [Framely Releases](https://github.com/SteamFramelyHomebrew/framely/releases) 的 `installer-v<版本>` 下载适合电脑的安装器。Linux 为 tar.gz，Windows 为 EXE ZIP，macOS 为应用 ZIP。
2. 解压并运行，扫描设备或手动填写设备 IP 与 SSH 端口。
3. 核对指纹，填写 `steamos` 和密码，连接后选择 Framely 本体发行版本。
4. 如果要安装 Preview，开启显示测试版；安装器版本与设备版版本相互独立。
5. 在“安装与维护”确认设备、目标版本及操作，等待完成。也可选择本地运行包和对应的外部 `SHA256SUMS`。

Linux 用户可在安装器解压目录执行 `bash install-desktop-entry.sh`，把带 Framely 图标的入口添加到当前用户的应用菜单。入口指向解压目录；移动目录后需重新执行脚本。

安装器会校验下载内容，并在设备端再次校验。密码不保存到配置文件。连接后仍需在实际设备上观察 Framely 的启动和显示。

## 方式二：在 Frame 的 SSH 终端安装

以下命令在 **Frame 上** 执行。默认安装最新正式版：

```bash
curl -fsSL https://raw.githubusercontent.com/SteamFramelyHomebrew/framely/main/install.sh | bash -s -- install
```

指定 Preview 或其他标签：

```bash
curl -fsSL https://raw.githubusercontent.com/SteamFramelyHomebrew/framely/main/install.sh | bash -s -- install --version v0.4.2-preview.3
```

将示例标签换成 Releases 中实际存在的本体标签。指定标签时从该 Release 下载引擎和包，不要求已有正式版；不指定标签时使用最新正式版。终端会按需请求 sudo 密码。

## 方式三：本地发行包

下载设备运行包 `framely-<版本及构建号>-linux-arm64.tar.gz`、外部 `SHA256SUMS` 和同一 Release 的 `bootstrap.py`，放入 Frame 的同一目录：

```bash
python3 bootstrap.py install --archive "./framely-<版本及构建号>-linux-arm64.tar.gz" --checksums ./SHA256SUMS
```

尖括号是占位符，执行前替换为真实文件名。引擎校验后在 `/home` 暂存、解压并检查包内哈希，避免耗尽设备的 `/tmp`。

## 验证安装

```bash
systemctl is-active framely.service framely-session.service
/var/lib/framely/current/bin/framely --version
sudo /var/lib/framely/current/bin/framely status
```

两个服务正常应显示 `active`。打开 SteamVR Dashboard，在 Dock 找到 Framely；首次使用需阅读用户协议和隐私声明。之后按[插件安装教程](plugins.md)添加第一个插件。

## 更新与回滚

已配置更新服务时，在“关于 → Framely 更新”选择“正式版”或“测试版”，再检查、下载并确认安装。正式版只检查正式 Release，测试版只检查 Preview、Beta、RC 等预发布版本；渠道选择保存在设备上。切换渠道会清除原候选版本和已下载包，需要重新检查和下载。也可以用桌面安装器，或在 Frame 执行：

```bash
curl -fsSL https://raw.githubusercontent.com/SteamFramelyHomebrew/framely/main/install.sh | bash -s -- update --version v0.4.2-preview.3
```

更新保留插件、设置和数据，并保留前一个发行用于回滚；只重启 Framely。默认渠道为正式版；在本体内选择测试版即可检查 Preview。命令行更新 Preview 仍需明确指定标签或本地包。切回正式版时可能选到比当前版本更旧的发行，界面会在安装前提示；回滚不会迁移新版插件数据格式。

需要回滚时，从“关于”选择上一版本，或执行：

```bash
sudo bash /var/lib/framely/current/rollback.sh
```

回滚本体保留插件数据，但不保证将新版本的数据格式转换为旧格式。

## 修复与日志

SteamOS 更新可能重置账号和系统服务。管理数据保存在 `/home/.framely/state`，`/var/lib/framely` 为兼容入口。在系统配置可写且 `/home` 数据仍存在时执行：

```bash
sudo bash /home/.framely/repair.sh
sudo journalctl -u framely -u framely-session --no-pager -n 100
```

修复会使用已保留的发行恢复账号及服务；UID 冲突时停止。它不能找回已删除的 `/home` 数据，SteamOS 更新后的恢复仍需实机验证。

## 卸载

桌面安装器“安装与维护”提供卸载，也可直接执行：

```bash
sudo bash /var/lib/framely/current/uninstall.sh
```

卸载先停用并卸载全部插件、执行其卸载钩子，成功后再移除 Framely 程序和服务。某个插件清理失败会保留 Framely并报错，处理后重试。已保存的设置、插件数据和专用账号保留；不承诺自动清除第三方插件在其他位置造成的修改。
