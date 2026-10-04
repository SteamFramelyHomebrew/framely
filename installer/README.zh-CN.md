# Framely Installer

[English](README.md)

Rust + GPUI Kit 的桌面安装器，连接 Steam Frame，安装、更新、修复、回滚和卸载 Framely。默认发行仓库为 `SteamFramelyHomebrew/framely`。

## 许可证

安装器原创代码采用 **GNU Affero General Public License v3.0 only**（`AGPL-3.0-only`）。源码仓库内的全文见 [LICENSE](../LICENSE)，发行包内附带 `LICENSE`；第三方依赖保留各自的许可证。AGPL 允许商业使用，分发时须按许可证提供完整对应源码。

对应源码位于 [SteamFramelyHomebrew/framely](https://github.com/SteamFramelyHomebrew/framely) 的 `installer-v<版本>` 标签，构建说明见下文。重新分发修改版时，须提供与所发布二进制匹配的源码及必要构建脚本。

同一用户只能运行一个安装器实例；重复启动会直接退出。顶部标题栏支持拖拽移动及窗口操作；macOS 使用系统窗口按钮，Linux/Windows 使用 GPUI Kit 标题栏按钮或系统提供的按钮。关闭最后一个窗口会退出程序并释放实例锁。

## 使用

1. 在 Frame 开启开发者模式并设置密码；电脑和设备连接同一局域网。
2. 默认优先扫描 `192.168.*` 网段（每次最多 1024 个地址）。名称发现和 IP 探测同时开始；只列出主机名为 `frame` 的设备，并标注 SSH 是否可用。其他设备和无法确认名称的地址不会显示。手动 IP 和 SSH 端口始终可用。
3. 检查 SSH 握手，核实显示的主机指纹，再输入账号密码登录。用户名默认 `steamos`。已保存的指纹发生变化时拒绝连接；核实设备重装后可手动移除配置目录中的对应 `known-hosts.json` 条目。
4. 从 Releases 选择版本，或选择本地 `framely-*-linux-arm64.tar.gz` 和压缩包外部的 `SHA256SUMS`（也可选择对应 `.sha256` 文件）。本地包不需要访问 GitHub。默认隐藏测试版。
5. 在“安装与维护”选择操作，在弹窗中核对设备、版本和操作后确认；取消或按 Esc 返回，不会执行。错误和操作结果也使用弹窗。SSH 指纹确认仍保留在连接页面。安装器不会解除 SteamOS 根分区的只读保护，所需目录不可写时会报错停止。

执行时主页面显示下载、传输和设备操作进度。在线下载与 SFTP 传输显示实际字节数和百分比；本地安装包跳过下载。设备端依次回报校验、准备目录、解压、配置服务和启动检查等步骤。解压显示实际字节进度，无法计算总量的步骤显示运行指示与当前步骤，日志保留在同一页面。只有操作及后续检查成功后才显示完成。

本机校验完成后使用内置 SSH/SFTP 上传，设备端再次校验并安全解压。密码只在内存中使用，不写入配置文件，不作为命令参数或环境变量传输；操作通过 SSH 标准输入向 sudo 提交密码。SSH 登录和 sudo 密码使用同一输入，适用于 Frame 默认账号配置。

卸载只有一个入口：先停用全部插件并运行卸载钩子，全部成功后才移除 Framely 服务和程序。失败时保留本体、禁用插件，供修复后重试。保存的设置、插件数据和专用账号保留；不承诺清除插件在其他位置造成的修改。旧发行若不支持 `prepare-uninstall`，应先更新到支持此接口的新发行。

连接成功后按设备状态提供操作：未安装时仅提供安装；已安装时默认进入维护页面，提供更新、修复与卸载；设备存在上一发行版本时才提供回滚。修复、回滚和卸载无需选择在线或本地包；安装和更新缺少包时，“选择版本”按钮会返回版本页面。选择与当前安装完全相同的包时提示使用修复，不重复更新。执行前会重新读取设备状态，拒绝过期操作；连接其他设备、切换版本或文件时清除旧确认，操作期间禁止切换页面。

操作结束后在维护页面顶部显示独立的成功／失败结果卡片及重新连接按钮。成功明确显示对应操作完成，失败显示原因；日志继续保留用于排查。结束后丢弃 SSH 连接及已安装版本快照，重新连接才能再次操作。

扫描、下载和 SSH 工作在后台线程运行，界面展示进度和日志。默认网络管理面板按钮使用端口 15915；设备修改端口后应使用实际地址。

## 构建

Linux 需要 C/C++ 构建工具、CMake、pkg-config、Fontconfig/Freetype、X11/XCB、XKBCommon、Wayland 和 Vulkan 开发库。macOS 需要 Xcode 命令行工具；Windows 需要 MSVC、CMake 和 Perl（用于内置 OpenSSL）。GPUI Kit 固定为 0.7.0，配套依赖由 `Cargo.lock` 锁定。

```bash
cargo test --manifest-path installer/Cargo.toml --locked --no-default-features --lib
cargo check --manifest-path installer/Cargo.toml --locked
cargo run --manifest-path installer/Cargo.toml --locked
cargo build --manifest-path installer/Cargo.toml --locked --release
```

可配置 `CARGO_TARGET_DIR` 将构建产物放入其他目录，打包工具也会读取此设置。默认读取 `installer/target/release/framely-installer`，CI 构建时无需配置此变量。

```bash
INSTALLER_PLATFORM=linux-x64 python3 tools/package-installer.py
```

界面采用与 Framely 一致的深色、细边框和淡蓝色选中状态，分为连接设备、选择版本、安装与维护三个页面。连接时显示建立 SSH、认证、权限检查、版本读取及指纹保存的进度；失败在页面中显示原因，并恢复按钮以便重试。未连接设备或未选择安装包时，下一步按钮说明当前缺少的条件。可用模拟数据渲染页面以及连接中、失败和成功状态的预览图，不连接真实设备：

```bash
cargo build --manifest-path installer/Cargo.toml --locked --features visual-test
FRAMELY_INSTALLER_PREVIEW_DIR=installer/dist/previews installer/target/debug/framely-installer
```

预览依赖可用的 GPU 或软件 Vulkan 渲染器；默认发行构建不包含此预览功能。

安装器版本由本目录的 `Cargo.toml` 和 `Cargo.lock` 独立管理，不跟随 Framely 本体版本。推送 `installer-v<版本>` 标签（当前 `installer-v0.4.1-preview.6`）触发 `.github/workflows/installer-release.yml`，只构建并发布安装器；本体使用 `v<版本>` 标签和独立工作流。也可在 Actions 手动运行安装器工作流，仅生成构建产物。安装器 Release 不占用仓库的 Latest，以免影响 Frame 的默认安装和更新地址。

Actions 构建 Linux x64/ARM64、Windows x64、macOS Intel/Apple Silicon。macOS 输出 `.app` ZIP，Windows 输出 EXE ZIP，Linux 输出 tar.gz。当前未配置 Apple 公证或 Windows 代码签名，发布前应分别验证系统的首次启动体验。

安装器标题栏和应用图标使用统一的像素 Logo。Windows 构建将 ICO 嵌入 EXE，macOS 打包使用系统 `iconutil` 生成并配置 `.icns`。Linux 解压后可在解压目录执行 `bash install-desktop-entry.sh`，把带图标的入口注册到当前用户的应用菜单；入口指向该目录，移动后需重新执行脚本。

所有自动测试和 Actions 构建只能验证软件及产物；仍需在真实电脑和 Frame 上完成扫描、登录、权限、升级回滚及卸载验收。

扫描会直接查询 `frame.local` 的 mDNS 地址记录，不要求设备广播 SSH 服务，也不依赖电脑的系统 DNS 支持 `.local`。可单独运行与界面相同的发现逻辑进行诊断：

```bash
cargo run --manifest-path installer/Cargo.toml --locked --no-default-features --example scan -- 192.168.5.0/24
```
