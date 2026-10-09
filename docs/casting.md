# Framely 内置投屏

管理网页顶部的 **投屏** 面板使用侧边菜单分为 **网页观看**、**画面与声音**、**接收投屏** 和 **投到设备**。手机通过页面选择器切换，底部六个管理入口保持单行。设置保存在核心数据库。接收协议默认关闭；采集默认 **屏幕采集**，也可手动选择 **SteamVR**。没有自动采集模式，也不会因失败自行切换来源。

## 采集与网页观看

**屏幕采集**读取头显实际扫描输出，包括透视、Steam 菜单和空间窗口。核心 root 服务只启动固定的 `framely-panel-grab` 助手，通过受 Steam 用户身份检查的私人 Unix socket 导出 DRM framebuffer 的 DMA-BUF。其余采集、Vulkan 畸变校正、取景及 Iris H.264/H.265 编码均以 Steam 用户运行；不设置应用 capability，也不依赖 Framecorder 插件。

Framecorder 原插件的名称是“面板画面”和“SteamVR 备用画面”（CLI 为 `panel` / `headset`）。Framely 使用“屏幕采集”和“SteamVR”，避免把参考实现的应用名当作采集方式。

- **单眼画面**：校正镜头畸变，读取 SteamVR 的主力眼或手动选左/右眼。使用实时完整视角预览，拖动取景框移动中心、拖动四角缩放，支持方向键和视野滑杆。开始调整会临时切换共享画面到完整参考视角，应用或取消后恢复之前的串流状态。画幅、分辨率和常见帧率使用预设按钮。
- **双眼原始画面**：保留扫描方向、双眼布局及镜头畸变。高度由原始比例和输出宽度计算，不使用主力眼或取景设置。
- **SteamVR**：读取 `/dev/video99`，由头显现有 FFmpeg 缩放、限帧及软件编码。实测输入固定为 RGB3 1920×1080；更改 V4L2 格式不能改变输入尺寸。设置帧率会改变驱动的报告值，但实际独立输入帧仍约 90 fps，因此设置调整的是串流输出。

启动一次采集，多个网页观看者共享同一条所选 H.264 或 H.265 + Opus 串流。管理页预览和 `/cast/watch` 使用同源 WHEP 信令代理和 WebRTC；沿用网络面板的登录与 Origin 检查。观看网页的视频占满视口，播放、音量、重连和全屏控件叠加在画面上，并在静止三秒后收起。始终等比显示完整视频，不裁切或拉伸；默认静音便于浏览器自动播放。手机适配安全区域和横竖屏。

网页优先协商用户选择的 H.264 或 H.265。当前官方 CEF 运行包不能解码所选编码时，头显自带 FFmpeg 以 libvpx 提供 VP8 + Opus 兼容输出；所有需要 VP8 的观看者共享一个额外的软件编码进程。采集来源不变。支持所选编码的观看者直接接收原串流，DLNA 仍使用 H.264。VP8 增加 CPU 消耗；兼容观看结束后收回该进程。Framely 不安装、调用或依赖外部 Chrome。

声音使用小片段和独立的有界输入队列，避免音频阻塞拖慢视频；编码使用无 B 帧和低延迟参数。SteamVR 的 H.265 是软件编码，会增加 CPU 负载；屏幕采集使用 Iris 硬件编码。

## 接收与发送

AirPlay 由固定版本 UxPlay 处理 RAOP 音频和屏幕镜像；只允许一个发送设备，不实现 AirPlay 发送。自定义 GStreamer sink 在用户确认前丢弃音频和画面。断开、拒绝或停止后清除同意状态，新连接重新询问。

DLNA 使用 GUPnP 的标准发现、SOAP 和事件订阅，接收 HTTP(S) 媒体地址。每个新请求先显示通知，接受后才下载、解码和播放。接收的音频打开内置控制窗口；视频采用旋转及像素比例校正后的实际画幅，控件浮在视频上并自动收起，不改变窗口画幅。不同 DLNA 请求可以保持多个空间窗口。播放器支持暂停、音量、停止以及媒体支持时的跳转；关闭空间窗口会停止该投屏。未处理请求 60 秒后过期。

GStreamer 解码结果经用户私人运行目录中的带锁 BGRA frame 文件交给原生 OpenGL 空间窗口，CEF 负责播放器控制，不承担视频解码。播放结束、错误及接收进程退出会关闭对应窗口并清理状态。

DLNA 发送需要开启网络管理面板并启动串流。GUPnP 发现设备后，使用该设备所在网卡的地址发送带随机会话令牌的 HLS 地址。FFmpeg 复制 H.264 视频，将 H.265 转为兼容的 H.264，并在需要时将 Opus 转为 AAC。令牌只允许读取该串流的播放列表和 MPEG-TS 分段，停止串流后失效。设备需要支持 HLS；接收端支持的具体媒体格式取决于头显已安装的 GStreamer 解码器。DRM 保护媒体不在此实现范围。

## 构建与打包

头显运行依赖现有 FFmpeg（libx264、libvpx、libopus、AAC、V4L2、PulseAudio）、GStreamer 1.24+、FFmpeg 7 共享库与随包的 gst-libav 插件、GUPnP 1.6、Avahi、OpenVR、Vulkan/DRM、Qualcomm Iris。设备上已确认 FFmpeg、GStreamer 1.24.2、GUPnP 1.6.6 和上述编码器可用。

构建需要 Rust、C/C++ 编译器、CMake、Meson、Ninja、pkg-config、glslc，以及 DRM/Vulkan、GStreamer app/video、GUPnP 1.6、Avahi、OpenSSL/libplist 开发文件。`tools/build-media.sh` 构建采集助手、接收器、gst-libav 插件和 UxPlay，并下载校验固定版本的 MediaMTX。可用 `FRAMELY_SYSROOT` 为采集 crate 指定 ARM64 sysroot；gst-libav 链接 FFmpeg 7 ABI，其余组件使用目标系统的开发库。

`tools/package-release.sh <CEF目录>` 同时构建媒体能力；助手放入 `bin`，其他媒体程序放入 `lib/media`。第三方许可证和媒体改动源码随包提供，版本和 SHA-256 位于 `media/dependencies.json`。

## 验证

```bash
CARGO_HOME=/tmp/framely-cast-cargo cargo test --locked
npm run typecheck
node tools/test-localization.mjs
bash tools/test-native.sh
python3 tools/test-casting-media.py --binaries media/bin --upnp media/upnp
```

`test-casting-media.py` 使用生成的测试媒体与临时音频 sink，不改变系统音频设置；验证 UxPlay 启动、确认门控、DLNA SOAP、已知色条的解码像素、暂停、音量、精确跳转、并发请求及超过会话上限次数的释放。测试结束回收进程和临时设备。

实机采集与实际 CEF WebRTC 测试：

```bash
FRAMELY_MEDIA_BIN=/绝对路径/media/bin cargo test --locked steamvr_stream_has_requested_output_and_stops_all_workers -- --ignored --nocapture
DISPLAY=:0 FRAMELY_MEDIA_BIN=/绝对路径/media/bin \
  FRAMELY_TEST_ASSETS=/绝对路径/ui/dist FRAMELY_CAST_WEB_PORT=28189 \
  FRAMELY_CEF_ROOT=/绝对路径/CEF运行目录 \
  FRAMELY_CAST_BROWSER_PROBE=/绝对路径/cast-browser-probe \
  cargo test --locked casting_browser_fixture -- --ignored --nocapture
```

`tests/casting_browser_probe.cpp` 可使用现有 CEF 开发头和 libcef 编译。运行时需要 ICU、资源和 locales 与测试可执行文件一起可用。测试使用独立的临时数据库、网络端口及浏览器缓存，检查网页解码尺寸、连续播放时间和实际帧数，不修改设备上已安装的 Framely。设置 `FRAMELY_CAST_TEST_SOURCE=screen` 可以通过已安装核心的私有 IPC 请求屏幕助手；设置 `FRAMELY_CAST_TEST_CODEC=h265` 验证 H.265；启动请求走实际 HTTP 接口，以验证媒体进程不随请求线程结束而退出。

需用户在头显上完成的验收：

1. 戴上/唤醒头显，以 root 执行 `tools/test-casting-panel.py --binaries /绝对路径/media/bin`，验证普通、取景及原始双眼三种屏幕输出。
2. 打开 **投屏**，验证默认屏幕串流、视野与主力眼设置、应用后输出变化、网页画面和声音。
3. 用真实 AirPlay 发送端分别发送音频和屏幕镜像，确认拒绝前没有声音或画面；接受后可控，断开后重连重新询问。
4. 用真实 DLNA 发送端打开两个媒体窗口，核对各自比例、独立暂停/停止，关闭一个不影响另一个。
5. 选择真实 DLNA 播放设备，验证头显串流播放和停止；比较 H.264 与 CEF 的 VP8 兼容输出在目标分辨率下的流畅度和延迟。

自动通过的协议/编解码测试不能替代真实发送端兼容性、头显内视觉比例和用户感知延迟验收。

`tests/cast_frame.cpp` 验证原生视频与透明控件的预乘 Alpha 合成、控件纹理保留和收起后的完整画面，需要 GLX 环境。
