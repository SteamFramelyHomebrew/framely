# 发布插件与插件源

[English](../../developer-guide/publishing.md)

[文档首页](../README.md) · [开发全流程](README.md)

## GitHub Release 默认地址

公开 GitHub 插件仓库的**源码 `manifest.json` 可以省略 `downloadUrl`**。社区数据库从登记的仓库地址、版本和插件 ID 自动生成：

```text
https://github.com/<owner>/<repo>/releases/download/v<version>/<id>-<version>.framely
```

例如仓库 `yourname/my-plugin`、ID `yourname.my-plugin`、版本 `0.1.0` 对应标签 `v0.1.0` 和附件 `yourname.my-plugin-0.1.0.framely`。仓库名不必与 ID 相同。必须真正省略字段；空字符串或 null 不表示自动生成。不要使用 `latest`，不要替换同版本附件。

源码登记与打包有一项区别：数据库会补全源码清单的下载地址，并要求**包内清单含相同的最终 `downloadUrl`**。当前 `framely pack` 只计算载荷哈希，不读取 Git remote、不自动拼接 Release URL。若源码省略该字段，打包前生成临时清单；无需把地址手写回源码。

## 生成发布包

先按[开发流程](README.md)创建插件、执行 `npm run build`，在自己的插件 Git 仓库配置正确的 GitHub `origin`。以下命令在插件目录执行，从源码生成 `.framely-build/manifest.json`，支持 HTTPS、SSH 两种常用 remote 写法；已有显式 `downloadUrl` 会保留，源码不会改写：

```bash
python3 - <<'PY'
import json, re, subprocess
from pathlib import Path

manifest = json.loads(Path('manifest.json').read_text(encoding='utf-8'))
manifest.pop('downloadSha256', None)  # Source registration only; never package it.
if 'downloadUrl' not in manifest:
    remote = subprocess.check_output(
        ['git', 'remote', 'get-url', 'origin'], text=True
    ).strip()
    match = re.fullmatch(
        r'(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)'
        r'([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+?)(?:\.git)?/?', remote
    )
    if not match:
        raise SystemExit('Use a GitHub origin or set downloadUrl explicitly.')
    manifest['downloadUrl'] = (
        f"https://github.com/{match[1]}/releases/download/v{manifest['version']}/"
        f"{manifest['id']}-{manifest['version']}.framely"
    )
output = Path('.framely-build/manifest.json')
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(manifest['downloadUrl'])
PY
```

`.framely-build/` 是临时输出，应加入插件 `.gitignore`。模板已忽略它。包名必须与默认规则一致；在插件目录继续执行：

```bash
FRAMELY_REPO=/absolute/path/to/framely
PLUGIN_ID=$(python3 -c 'import json; print(json.load(open("manifest.json"))["id"])')
PLUGIN_VERSION=$(python3 -c 'import json; print(json.load(open("manifest.json"))["version"])')
mkdir -p dist
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- pack --manifest .framely-build/manifest.json --payload payload --output "dist/$PLUGIN_ID-$PLUGIN_VERSION.framely"
cargo run --manifest-path "$FRAMELY_REPO/Cargo.toml" --locked -- verify "dist/$PLUGIN_ID-$PLUGIN_VERSION.framely"
```

提交源码、SDK、锁文件和许可证，将该提交标记为 `v<version>`；创建对应 GitHub Release，上传 `dist/` 内的 `.framely`，并公开 Release。包内清单和源码登记清单在补全默认地址、去掉源码专用字段后应一致。用于登记的 submodule 必须指向这个实际发布版本的源码提交。

发行说明包含功能和使用方法、运行用户及 root 的原因、依赖、变化、实际 Frame 测试和已知限制。提供与包匹配的源码和构建脚本，遵守模板、SDK及依赖许可证。

### 整包哈希与自定义地址

| 方式 | 源码登记 | 校验的预期整包哈希 |
| --- | --- | --- |
| 默认 GitHub Release | 省略 `downloadUrl`；标签和附件遵守上述命名 | 从对应 Release 附件的 `digest` 读取 SHA256，无需手写 |
| 显式固定 GitHub Release | 填 `downloadUrl`，可改变标签或附件名 | 未提供 `downloadSha256` 时读取 Release digest；显式提供时以该哈希校验 |
| 其他 HTTPS 下载地址 | 填 `downloadUrl` 和 `downloadSha256` | 使用源码登记中声明的 64 位整包 SHA256 |

自动地址模式即使另填 `downloadSha256`，也必须与 Release digest 相同。缺少 Release digest、附件未公开、无法下载或哈希不符会阻止登记，不会退回到信任刚下载的文件。

`downloadSha256` 只属于数据库源码登记，指完整 `.framely` 的哈希；不是载荷的 `files` 哈希，也不是 Framely 包内字段。先生成包，执行 `sha256sum dist/<id>-<version>.framely`，再把结果写入源码登记。打包必须移除它，上面的临时清单命令已处理；不要向包内写入自身哈希。直接给 `framely pack` 传含此字段的源码清单会被未知字段校验拒绝。

### 图标与窗口兼容

社区数据库的图标通常只需顶层 `icon: "icon.png"`：同一路径 PNG 同时提交到源码仓库和 `payload/`。数据库从登记的仓库、固定提交及路径生成 GitHub Raw 地址，并下载比较包内图标；不需要额外填写 `publish.icon`。自行托管目录的图片由 `publish.icon` / `publish.screenshots` 提供固定 HTTPS 地址。[插件库资料参考](../plugin-development.md#插件库资料与图标)

社区数据库支持窗口 `entry`、`title`、`dockIcon`、`localWeb`、`width`、`height`、`widthMeters`。像素宽度为整数 640–2560，高度为整数 360–1440，物理宽度按本体 float32 规则校验 0.4–4.0 米。源码省略尺寸时，数据库会按 1600×900、3 米补全，与 pack 输出一致；自定义尺寸也会参与源码和包内清单比较，未声明的变化会被拒绝。模板包含的默认独立窗口已通过实际打包和数据库校验；登记仍需完成整包哈希、归属及其他字段检查。

## 方式一：自行托管插件源

自托管不要求社区登记，也不要求 GitHub Release digest。可以省略包内 `downloadUrl`，由 `framely catalog` 使用 `--base-url` 与实际包文件名拼接；填写时优先采用包内地址。若包名来自默认 GitHub 规则，按前述方式发布即可。

把要保留的各版本 `.framely` 放入 `packages/`，在 Framely 仓库执行：

```bash
mkdir -p target/plugin-catalog
cargo run --locked -- catalog --name '我的插件源' --base-url https://example.org/packages --packages ./packages --output ./target/plugin-catalog/catalog.json
```

将 `example.org` 换成实际 HTTPS 地址。CLI 计算完整包的 SHA256并生成 JSON，不上传包、不复制图片。没有外部 `publish.icon` 时，本地 CLI 不会把包内图标转换成插件库 URL。

**上传整个生成目录，不能只上传 `catalog.json`：**

```text
catalog.json
plugins/
  yourname.my-plugin/
    versions.json
```

主目录每个 ID 一条推荐最新版；历史条目在相对 `plugins/<ID>/versions.json`，不包含主目录当前条目。每次生成需提供要保留的所有版本，本地 CLI 不合并之前的历史。社区数据库会延续之前发布的历史，当前每插件最多保留 19 个历史条目。不要混淆两种工具的保留规则。

将目录放在 HTTPS 静态服务，用户添加完整 `catalog.json` URL；各条目指向的包和图片也必须可访问。多个渠道可再发布[源订阅文件](../source-subscriptions.md)。

## 方式二：登记社区数据库

登记入口为 [framely-plugin-database](https://github.com/SteamFramelyHomebrew/framely-plugin-database)，具体规则以其 [CONTRIBUTING.md](https://github.com/SteamFramelyHomebrew/framely-plugin-database/blob/testing/CONTRIBUTING.md) 和当前校验器为准。源码根目录须有 `manifest.json`，作者 Release 及附件须先公开，登记使用 Git submodule 固定提交；数据库不镜像包或证明源码与二进制可复现一致。

首次登记：先 fork 数据库，使用自己 fork 的同名 `testing` 分支：

```bash
git clone --branch testing https://github.com/yourname/framely-plugin-database.git
cd framely-plugin-database
git submodule add https://github.com/yourname/my-plugin.git plugins/yourname.my-plugin
git -C plugins/yourname.my-plugin checkout v0.1.0
git add .gitmodules plugins/yourname.my-plugin
git commit -m 'Add yourname.my-plugin 0.1.0'
git push origin testing
```

向上游 `testing` 提交 PR，附实际 Frame 测试记录。只改 `.gitmodules` 与 `plugins/<ID>` 的 submodule 固定提交，其他文件变更不会走登记自动合并。

数据库 ID 要求 `namespace.name`：小写字母和数字，名称部分可带点、连字符；它比本体 ID 校验严格。路径必须是 `plugins/<ID>`，仓库必须是公开 GitHub 仓库。首次登记确认仓库归属和作者维护权限；命名空间绑定 GitHub 所有者数字 ID，每个所有者最多五个历史前缀，删除插件不释放归属。已登记 ID 不能换源仓库。协作者需要可验证的 write/maintain/admin 权限，不能以 manifest 的 `author` 或 Git 提交作者替代。[完整归属规则](https://github.com/SteamFramelyHomebrew/framely-plugin-database/blob/testing/CONTRIBUTING.md#插件归属与自动合并)

更新登记：

```bash
git -C plugins/yourname.my-plugin fetch --tags
git -C plugins/yourname.my-plugin checkout v0.2.0
git add plugins/yourname.my-plugin
git commit -m 'Update yourname.my-plugin to 0.2.0'
git push origin testing
```

stable 对应数据库 `main`，testing 对应 `testing`。自动合并要求 `testing → testing` 或 `main → main`，跨渠道 PR 留给人工处理。测试后按数据库规则提升到稳定渠道；`publish` 是生成分支，不提交插件变更。

## 发布后验证

检查用户实际能访问的包、目录、图标、历史选择、确认安装、更新及数据保留。只有本体 `verify` 通过还不够，登记还须通过数据库对源码、整包哈希和字段的检查。后续按“提升版本 → 重新生成临时清单 → 构建打包 → 实机测试 → 新 Release → 更新固定登记”发布；不要覆盖旧附件。
