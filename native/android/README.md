# Android task window helper

`FramelyWindow.java` uses the Android 11 task service to put only the requested
package's foreground task into fullscreen. A startup windowing-mode option alone
can leave a restored Lepton task in freeform mode. This helper changes task mode
without stopping the game or changing its installation, data, Android image or
host compositor. VR launches do not invoke it. It exits after success or a bounded
five-second wait; it does not prevent later manual window adjustments.

The small checked-in DEX is embedded in Framely. `window.sh` supplies ART's runtime
environment for `podman exec`; both files are mounted read-only into the container.
No APK installation or network access is required at runtime. An unsupported task
API fails explicitly instead of guessing Binder transaction numbers.

Rebuild with `bash tools/build-android-window.sh` (JDK and curl required). This
uses Google's D8 8.3.37, pinned by SHA-256. D8 is a build tool and is not shipped.

## 中文

`FramelyWindow.java` 通过 Android 11 的任务接口，只将指定包的前台任务切换为全屏。
Lepton 恢复已有任务时，仅指定启动窗口模式仍可能留下自由窗口。此助手不停止游戏，
不修改安装、数据、Android 镜像或宿主合成器。VR 启动不调用助手。成功后立即退出，
等待最多五秒，不持续限制用户之后手动调整窗口。

小型 DEX 随 Framely 内置；`window.sh` 为 `podman exec` 补充 ART 运行环境。二者以
只读方式挂载，运行时无需安装 APK 或联网。不支持的任务接口会明确失败，不猜测
Binder 调用编号。

执行 `bash tools/build-android-window.sh` 可重新生成，需要 JDK 和 curl。
构建使用 Google D8 8.3.37，并校验固定 SHA-256；安装包不包含 D8。
