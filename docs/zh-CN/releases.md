# Framely 发行与更新

[English](../releases.md)

插件源和 Framely 自身更新源是独立设置。当前代码首次初始化加入社区 stable/testing 来源；目录能否访问以实际服务为准。Framely 更新服务由发行者预配置，管理窗口“关于”提供检查、下载、安装和回滚；尚未配置时使用发行安装包更新。插件和 Framely 发行包均不要求签名或公钥。

构建发行包后生成包含版本、架构、下载地址、长度与 SHA256 的普通 JSON 更新清单：

```bash
framely release-manifest \
  --archive "./framely-0.2.0-<build>-linux-arm64.tar.gz" \
  --url "https://example.org/releases/framely-0.2.0-<build>-linux-arm64.tar.gz" \
  --output ./framely-release.json \
  --changelog '本次更新说明'
```

将包与清单发布到 HTTPS 静态服务；下载最多跟随五次经过 URL 校验的重定向，不允许从 HTTPS 降级到 HTTP。发行维护终端配置清单完整 URL：

```bash
framely call system.source.save '{"source":{"url":"https://example.org/framely-release.json"}}'
```

此接口用于发行维护，不作为普通用户设置项。当前官方安装引擎和桌面安装器会在更新源尚未配置时写入 `https://github.com/SteamFramelyHomebrew/framely/releases/latest/download/framely-release.json`，已有配置保留。哈希用于检查下载内容一致性，发行者身份依赖更新源；不再通过签名验证。

更新依次检查 API/架构兼容、声明长度、SHA256，安全解压后检查发行包自身 SHA256SUMS；下载不阻塞核心服务。用户确认安装后，独立 systemd 助手切换发行版本，只重启 Framely 服务。日志在 `/var/lib/framely/logs/update.log`。安装失败会尝试恢复原发行和安装前状态；UI 可回滚上一版本。

更新源仍仅包含 URL；渠道独立保存在 `database.updateChannel`，取值为 `stable`（默认正式版）或 `testing`（测试版），旧版本回滚时仍能读取原更新源。可通过“关于”选择，或执行 `framely call system.channel.save '{"channel":"testing"}'`。修改更新源或渠道会清除原候选版本和已下载包。发行清单使用普通 JSON，不支持旧签名包、签名信封或公钥字段。回滚切换上一发行版本，保留插件数据，不进行旧版格式迁移。

设备上的实际升级/回滚和重启恢复需要验收；本地自动测试验证无签名格式、包边界和插件安装更新，不能代替设备服务切换测试。

官方 GitHub `releases/latest/download/framely-release.json` 更新源会查询仓库 Release 列表，按渠道严格筛选，选择带本体包和清单的最高 SemVer 标签；跳过草稿、安装器标签和附件不完整的发行。没有匹配版本时显示该渠道暂无发行，包括只有 Preview 时检查正式版的情况。清单必须与所选标签、包地址和长度一致。自定义 HTTPS 清单地址仍直接读取，其版本必须属于所选渠道；发行者需要配置相应渠道的清单 URL。

## SteamOS 更新后的恢复

安装器将整个管理目录保存在 `/home/.framely/state`，包括 `state.json`、更新源、日志、当前/上一发行链接、Steam 用户名和 Framely 账号 UID；`/var/lib/framely` 仅作兼容链接。旧安装会在停止服务后迁移。两份状态冲突时停止，不自动覆盖任何一份。

系统服务和账号仍依赖 SteamOS 系统配置，操作系统更新后可能需要手动修复。在系统配置可写时运行 `sudo bash /home/.framely/repair.sh`；无需重新下载发行包，恢复兼容链接、专用账号、服务文件和开机启动。修复先校验保留的发行，再使用当前发行的 `install.sh --repair`，不创建发行或改写上一版本记录。保存的账号 UID 若被占用或现有账号 UID 已变化则停止，避免错误地接管旧数据。系统只读时停止，不自动解除保护。

该机制不能恢复已被系统清除的 `/home` 数据，也不保证操作系统更新后无需人工操作。回滚到不支持修复模式的旧发行后，需要使用支持该模式的新发行包恢复安装。正式发布前应进行一次真实 SteamOS 更新，验证设置、插件数据、账号身份、服务恢复和 SteamVR 宿主兼容性。

## 本体包与独立 CEF

每次构建生成不含 CEF 的本体包 `framely-<版本及构建号>-linux-arm64.tar.gz`、完整离线包 `framely-<版本及构建号>-offline-linux-arm64.tar.gz`、独立运行库 `framely-cef-<运行库标识>-linux-arm64.tar.gz`。更新清单 `framely-release.json` 固定指向不含 CEF 的本体包；首次安装固定选择完整离线包。`framely-cef.json` 提供独立 CEF 的 HTTPS 下载地址、大小及 SHA256。

CEF 按内容哈希保存在 `/home/.framely/cef/<运行库标识>`，更新时复用；旧安装中相同的运行库自动迁移。所需运行库缺失或 CEF 版本更换时单独下载并校验，各发行的运行库保留用于回滚。完整包可离线提供 CEF。参见[发行包与运行库恢复](user-guide/installation.md#发行包与-cef)。

## GitHub Actions 发布

Framely 和桌面安装器保留在同一仓库，使用独立的版本号、标签和发布工作流：

- Framely：版本由根 `Cargo.toml` 定义，标签为 `v<版本>`（当前 `v0.7.0-preview.1`）；`.github/workflows/release.yml` 只构建 Frame Linux ARM64 发行，使用锁定的 CEF 并执行自动检查。
- 安装器：版本由 `installer/Cargo.toml` 定义，标签为 `installer-v<版本>`（当前 `installer-v0.4.1-preview.10`）；`.github/workflows/installer-release.yml` 只构建 Linux x64/ARM64、Windows x64、macOS x64/ARM64 的原生安装器，使用锁定的 GPUI Kit。

两个版本不需要相同，也不需要同时发布。修改各自版本时同步对应的 `Cargo.lock`；安装器包名及 macOS 应用版本使用安装器版本。

两个工作流均缓存 Cargo 下载的依赖和已编译的依赖产物，按产品、系统、架构、Rust 工具链及依赖配置区分。安装器使用 `installer/target`；本体在 CI 使用 `target/cargo`，与单独缓存的 `target/cef` 分开，测试和发行打包共用同一 Cargo 目录。失败构建也会保存已完成的依赖编译。

普通分支推送不触发发行构建；只有对应产品的标签推送或手动运行会触发。缓存恢复、编译和缓存保存均在同一次构建内完成。GitHub 允许标签构建读取默认分支的缓存，不允许一个标签读取另一个标签的专属缓存；需要跨标签复用时，可手动在 `main` 运行一次对应工作流来生成默认分支缓存，该运行只生成构建产物，不发布 Release。首次构建、没有可访问的缓存或工具链变化后仍需重新编译，缓存命中情况见 “Cache Cargo dependencies and build artifacts” 步骤。

```bash
# 提交并推送代码后，选择需要发布的产品
git tag v0.7.0-preview.1
git push origin v0.7.0-preview.1

# 独立发布安装器，不触发 Framely 构建
git tag installer-v0.4.1-preview.10
git push origin installer-v0.4.1-preview.10
```

带 `-preview.N` 等预发布后缀的标签会发布为 GitHub Prerelease，不设置为 Latest。安装器 Release 始终设置为非 Latest；只有 Framely 正式 Release 设置为 Latest。因此设备安装入口和默认更新清单的 `releases/latest/download/...` 继续指向 Framely 本体。安装器读取发行列表时按本体包名筛选，不把 Linux ARM64 安装器误当成设备发行包。安装器下载应使用具体的 `installer-v<版本>` Release。

只有标签触发会发布。各工作流在自己的构建全部成功后创建草稿 Release，上传本产品产物及外部 `SHA256SUMS`，最后公开草稿。Framely Release 额外包含 `bootstrap.py`、入口脚本和更新清单；安装器 Release 只包含五个平台的安装器包和校验文件。失败的草稿不作为最新稳定版提供下载。手动触发用于验证，仅保留 Actions artifacts。

Framely 本体 Release 附件用途：`framely-*-linux-arm64.tar.gz` 为不含 CEF 的更新包，`framely-*-offline-linux-arm64.tar.gz` 为首次安装用的完整离线包，`framely-cef-*` 为独立运行库，`framely-cef.json` 提供运行库下载信息；`framely-release.json` 为内置更新功能的版本清单；`install.sh` 是命令行入口，调用 `bootstrap.py` 完成下载、校验和维护；`SHA256SUMS` 为附件校验清单。桌面安装器只需自动下载运行包和校验清单，用户无需手动下载脚本。安装器的五个平台包发布在独立的安装器 Release 中。

外部 `SHA256SUMS` 包含压缩包哈希，校验传输内容；包内同名文件校验解压后的载荷。哈希不能独立证明作者身份，在线安装依赖指定 GitHub 仓库与 HTTPS；本地安装依赖用户提供的可信包与校验文件。下载遵循 HTTPS 重定向，不允许降级。

入口脚本从 Release 下载设备引擎；在线安装默认选择最新正式版，也支持 `--version TAG`。指定标签时，入口从该 Release 下载 `bootstrap.py`，引擎再选择同标签运行包；入口脚本本身来自用户下载或 curl 指定的位置，可在仅有 Preview、尚无正式版时安装预发布版本。本地使用 `python3 tools/bootstrap.py install --archive /path/package.tar.gz --checksums /path/SHA256SUMS`。更新要求已经安装；指定已安装版本会修复服务，指定其他版本会安装目标发行，保留状态。

安装器将官方更新清单地址预配置到尚未配置更新源的设备；既有更新源保留。之后可继续通过 Framely 内置更新界面更新。

## 统一卸载

脚本和桌面安装器均只有一个卸载选项。`uninstall.sh` 先停止会话及下载工作，再通过 root 专用 `prepare-uninstall --approve` 请求核心服务：持久停用所有插件、按依赖顺序停止插件、执行各插件 `onUninstall`，移除插件包。全部完成后才取消 Framely 自启、停止核心并移除程序。失败时保留本体并恢复会话，插件保持停用，允许处理原因后重试；不支持跳过失败插件强制卸载本体。

协议尚未确认或已撤回也允许 root 通过此专用维护接口卸载；普通会话及插件界面不开放该接口。保存的设置、插件数据和系统账号保留，不承诺清除外部副作用。旧发行没有该接口时需先更新。
