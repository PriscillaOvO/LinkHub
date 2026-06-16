# Android 闪退修复交接

> 生成时间：2026-06-14
> 目的：记录 Android 应用点击底部“发送”页闪退的根因、修复内容、验证结果和下一步建议，供后续实现者继续接手。

## 问题现象

在 Android 真机上启动 LinkHub 后，点击底部导航栏的“发送”页，应用立即闪退。

复现路径：

1. 安装 debug APK。
2. 启动 `com.linkhub.app/.MainActivity`。
3. 点击底部导航栏“发送”。
4. 应用闪退。

## 崩溃日志

通过 `adb logcat` 捕获到的关键错误：

```text
FATAL EXCEPTION: main
Process: com.linkhub.app
java.lang.IndexOutOfBoundsException: Index -1 out of bounds for length 0
    at java.util.ArrayList.remove(ArrayList.java:558)
    at androidx.compose.runtime.Stack.pop(Stack.kt:26)
    at androidx.compose.runtime.ComposerImpl.exitGroup(Composer.kt:2333)
    at androidx.compose.runtime.ComposerImpl.end(Composer.kt:2499)
```

这是 Compose runtime 在重组 UI 时出现的组合树栈错误。

## 根因

文件：

```text
android/app/src/main/java/com/linkhub/app/ui/SendScreen.kt
```

原来的 `SendScreen` 在 `Column { ... }` 组合内容中使用了：

```kotlin
return@Column
```

这种在 Compose 组合树中提前返回的写法会让不同重组路径产生不稳定的 group 结构，切换 tab 时容易触发 Compose runtime 内部栈不匹配，最终表现为 `IndexOutOfBoundsException`。

## 已修复内容

已将 `SendScreen` 的提前返回逻辑改为稳定的 `if/else` 渲染结构：

- 当 `identity == null` 时，只显示提示文本。
- 当 `identity != null` 时，再渲染可信设备选择、对方地址、文本发送和文件发送区域。
- 不再在 Compose `Column` 内容中使用 `return@Column` 提前退出。

修改文件：

```text
android/app/src/main/java/com/linkhub/app/ui/SendScreen.kt
```

## 构建环境修复

本机命令行环境原来使用 JDK 8：

```text
JAVA_HOME=C:\Program Files\Eclipse Adoptium\jdk-8.0.492.9-hotspot
```

Android Gradle Plugin 8.2.0 需要更高版本 Java。为了不改全局系统环境，已在 Android 项目内指定 Gradle 使用 Android Studio 自带 JBR：

```text
android/gradle.properties
```

新增：

```properties
使用系统 `JAVA_HOME`，不要在仓库中提交本机 `org.gradle.java.home`。
```

同时补齐了 Gradle Wrapper 文件，便于命令行稳定复现构建：

```text
android/gradlew.bat
android/gradle/wrapper/gradle-wrapper.jar
```

后续可以直接运行：

```powershell
cd android
.\gradlew.bat :app:assembleDebug
```

## 已验证结果

已在连接的 Android 真机上验证：

```text
设备 ABI: arm64-v8a
APK 包含: lib/arm64-v8a/liblinkhub_core.so
构建命令: .\gradlew.bat :app:assembleDebug
构建结果: BUILD SUCCESSFUL
安装命令: adb install -r -g app-debug.apk
安装结果: Success
启动结果: MainActivity 可打开
点击“发送”: 不再闪退
logcat: 未再出现 AndroidRuntime / FATAL EXCEPTION / IndexOutOfBoundsException
```

## 当前 Android 端状态

这份文档最初记录了“点击发送页闪退”和命令行构建环境问题。后续又继续修复了 Android 服务启动、Rust listener 集成、Android Trust Store 写入和 Windows/Android 真机互发问题。

## 2026-06-14 追加修复：Android 监听服务

新增/修改内容：

1. 新增 `android/app/src/main/java/com/linkhub/app/ui/AndroidStorage.kt`。
   - 统一保存和读取本机身份。
   - 修正可信设备保存格式。
   - 从配对 payload 保存 peer 的 `public_key` 和 `dh_public_key`。
   - 生成 Rust core 可读取的 `linkhub_trust_store_v1` trust store 文件。
2. 修改 `PairScreen.kt`。
   - 配对成功后保存完整可信设备信息。
   - 不再把 payload 的错误字段当作 DH public key。
3. 修改 `DevicesScreen.kt`。
   - 自动加载身份和可信设备。
   - 展示可信设备指纹和保存的地址。
4. 修改 `SendScreen.kt`。
   - 自动加载身份和可信设备。
   - 选择设备后读取保存的地址。
   - 编辑地址时写回可信设备记录。
5. 修改 `LinkHubService.kt`。
   - 启动服务时调用 `RustBridge.startListener(...)`。
   - 启动前生成 Rust trust store 文件。
   - 停止服务时调用 `RustBridge.stopListener()`。
6. 修改 `AndroidManifest.xml`。
   - 增加 `android:name=".LinkHubApp"`，确保通知渠道会创建。
   - 这修复了服务启动时的 `Bad notification for startForeground` 闪退。
7. 修改 `core-rs/src/net.rs` 和 `core-rs/src/jni_bridge.rs`。
   - 新增可停止的 authenticated listener 循环。
   - JNI `stopListener()` 现在可以让 listener 循环退出并释放端口。

已验证：

```text
core-rs cargo check --all-targets: passed
scripts/verify-local-e2e.ps1: passed
cargo ndk -t arm64-v8a -o ..\android\app\src\main\jniLibs build --release --lib: passed
android .\gradlew.bat :app:assembleDebug: passed
真机服务页启动监听: 不再出现 Bad notification / FATAL EXCEPTION
真机服务页停止监听: 8787 端口释放
```

最终新版 APK 已安装到真机，服务页启动/停止已复测。

## 2026-06-14 追加验证：Windows 与 Android 真机热点互发

测试拓扑：

```text
Android 手机开启热点
Windows 连接手机热点
USB ADB 只用于安装、控制和抓日志
LinkHub 数据传输走 Wi-Fi 热点局域网
```

实测地址：

```text
Android: 10.23.206.237
Windows: 10.23.206.53
Port:    8787
```

已验证：

```text
Windows -> Android 文本发送: AUTH_OK, Noise KK handshake complete
Android -> Windows 文本发送: Windows 收到 authenticated text
```

这证明 Android JNI listener、Windows CLI listener、Trust Store、Noise KK 加密会话和真实 Wi-Fi 局域网链路已经形成最小闭环。

## 2026-06-14 追加改进：Android 文件发送体验

新增/修改内容：

1. `SendScreen.kt` 新增系统文件选择器。
   - 用户可以点 `选择文件`，从 Android 文档选择器选择任意文件。
   - App 会把 `content://` 文件复制到 cache 目录下的 `linkhub-send` 文件夹。
   - Rust core 仍接收普通文件系统路径，避免 JNI 层直接处理 Android URI。
2. `SendScreen.kt` 的文本/文件发送改为后台线程执行。
   - 使用 `Dispatchers.IO` 调用 `RustBridge.sendText` / `RustBridge.sendFile`。
   - 避免网络发送阻塞 Compose UI。
3. 发送结果解析新增 `error` 字段。
   - JNI 返回 `{"error":"..."}` 时，UI 会显示更明确的失败原因。
4. `MainActivity.kt` 在 Android 13+ 请求 `POST_NOTIFICATIONS`。
   - 降低前台服务启动时通知权限缺失带来的测试干扰。

已验证：

```text
core-rs cargo fmt --check: passed
core-rs cargo test: passed
android .\gradlew.bat :app:assembleDebug: passed
```

## 2026-06-14 追加修复：认证文件断点续传

Rust core 的认证文件发送已经补齐断点续传：

- 加密 `FILE_START` ACK 会解析 `FILE_START_RECEIVED:<chunk_index>`。
- 发送端会跳过接收端已有 chunk。
- `scripts/verify-local-e2e.ps1` 新增 `Auth resume transfer` 验收。
- Android native library 已重新通过 `cargo ndk -t arm64-v8a` 构建并写入 `android/app/src/main/jniLibs/arm64-v8a/liblinkhub_core.so`。

已验证：

```text
scripts/verify-local-e2e.ps1: passed
cargo ndk -t arm64-v8a -o ..\android\app\src\main\jniLibs build --release --lib: passed
android .\gradlew.bat :app:assembleDebug: passed
```

待验证：

```text
Android 真机选择文件 UI
Android -> Windows 真实热点 Wi-Fi 文件发送
Windows -> Android 真实热点 Wi-Fi 文件发送
```

## 当前 Android 端仍然存在的限制

仍需注意：

1. `SendScreen` 发送功能目前仍依赖手动或半自动地址：
   - 已生成身份。
   - 已配对可信设备。
   - 需要选择可信设备。
   - 需要填写或保存对方 `IP:端口`。
   - 对方必须已经有可用监听器。
2. Android 文件选择器已接入，但真实文件互发仍需真机验证。
3. APK 当前只打包了 `arm64-v8a` 的 `liblinkhub_core.so`，真机可用，但 x86_64 模拟器会缺少 native library。
4. Android 13+ 通知权限、文件选择器、接收目录写入权限还需要继续产品化处理。
5. Rust listener 虽然已可停止，但 UI 状态仍依赖进程内变量，应用被系统杀掉后的状态恢复还需要完善。

## 2026-06-14 追加改进：默认接收目录

Android 服务页默认接收目录已从公开下载路径调整为 App 专属外部 Downloads 目录：

```text
/sdcard/Android/data/com.linkhub.app/files/Download/LinkHub
```

原因：

- Android 13+ 对公开存储路径权限更严格。
- App 专属外部目录不需要额外存储权限，真机验证更稳定。
- UI 仍允许手动改成 `/sdcard/Download/LinkHub` 做公开下载目录测试。

## 2026-06-14 追加改进：Android NSD/mDNS 发现

新增/修改内容：

1. 新增 `android/app/src/main/java/com/linkhub/app/ui/AndroidDiscovery.kt`。
   - 使用 Android `NsdManager` 注册和发现 `_linkhub._tcp` 服务。
   - 广播 TXT 字段与 Rust core mDNS 保持一致：`lh/id/name/fp/port`。
   - 扫描和广播期间会获取 Wi-Fi multicast lock。
2. 修改 `LinkHubService.kt`。
   - Rust listener 启动成功后自动注册 Android NSD 服务。
   - 服务销毁时停止 Rust listener，并注销 NSD 广播。
3. 修改 `DevicesScreen.kt`。
   - 新增 `扫描局域网` 按钮。
   - 只保存已经配对可信设备的扫描地址。
   - 扫描到地址后写回 `TrustedPeer.address`，发送页可自动填入。
4. 修改 `SendScreen.kt`。
   - 新增 `扫描并填入地址` 按钮。
   - 选择可信设备后可直接扫描并填入该设备最新地址。
5. 修改 `AndroidStorage.kt`。
   - 修复配对 payload header 兼容性，当前 core 使用 `linkhub-pair-v1`。
   - 保留旧 `linkhub-pair` 兼容，避免旧测试数据突然失效。

已验证：

```text
android .\gradlew.bat :app:assembleDebug: passed
```

待真机验证：

```text
Android 服务启动后 Windows 桌面端 Scan LAN 能发现 Android
Windows 桌面端服务启动后 Android 设备页扫描能发现 Windows
扫描写回地址后 Android 发送页自动填入最新地址
```

## 2026-06-14 追加改进：Android 服务真实状态显示

新增/修改内容：

1. 新增 `android/app/src/main/java/com/linkhub/app/ui/AndroidServiceStatus.kt`。
   - 前台服务会把 listener 状态、监听地址、接收目录、错误、mDNS 服务名和更新时间写入 SharedPreferences。
2. 修改 `LinkHubService.kt`。
   - 启动中、启动成功、启动失败、停止时都会记录真实服务状态。
   - listener 启动失败时保留错误原因，不再被 `onDestroy` 覆盖成普通停止。
3. 修改 `ServiceScreen.kt`。
   - 服务页每秒读取真实服务状态。
   - 显示真实运行状态、接收目录、mDNS 服务名和最近错误。
   - 点击启动后先显示“正在启动”，再由服务真实状态刷新为运行或失败。

已验证：

```text
android .\gradlew.bat :app:compileDebugKotlin --rerun-tasks --console=plain: passed
```

意义：

- 真机测试时，如果 Rust listener、trust store、通知、NSD 广播任一环节失败，用户可以直接在服务页看到原因，而不是误以为服务已经运行。

## 2026-06-14 追加修复：Android listener 启动前 bind 预检查

文件：

```text
core-rs/src/jni_bridge.rs
android/app/src/main/jniLibs/arm64-v8a/liblinkhub_core.so
```

修复点：

- JNI `RustBridge.startListener(...)` 在线程启动前先尝试绑定监听地址。
- 如果端口被占用或监听地址非法，会直接返回 `{"error":"failed to bind listener..."}`。
- Android 服务页会通过真实状态显示这个错误，不再先显示运行中再静默停止。

已验证：

```text
core-rs cargo fmt --check: passed
core-rs cargo test: passed
cargo ndk -t arm64-v8a -o ..\android\app\src\main\jniLibs build --release --lib: passed
android .\gradlew.bat :app:assembleDebug --console=plain: passed
scripts/verify-local-e2e.ps1: passed
```

## 2026-06-14 追加改进：Android 本机地址提示

新增/修改内容：

1. 新增 `android/app/src/main/java/com/linkhub/app/ui/AndroidNetworkHints.kt`。
   - 枚举 Android 当前可用的非 loopback IPv4 地址。
   - 按当前监听端口生成 `IP:端口` 候选地址。
2. 修改 `ServiceScreen.kt`。
   - 服务页显示“本机地址提示”。
   - 真机测试时可以直接把这里显示的地址填到 Windows 或其他设备发送页。

已验证：

```text
android .\gradlew.bat :app:compileDebugKotlin --rerun-tasks --console=plain: passed
android .\gradlew.bat :app:assembleDebug --console=plain: passed
core-rs cargo test: passed
scripts/verify-local-e2e.ps1: passed
```

## 2026-06-14 追加改进：Android 传输历史

新增/修改内容：

1. 新增 `android/app/src/main/java/com/linkhub/app/ui/AndroidHistory.kt`。
   - 使用 SharedPreferences 保存最近 200 条传输记录。
   - 记录时间、方向、对端设备、类型、预览、成功/失败和详情。
2. 新增 `android/app/src/main/java/com/linkhub/app/ui/HistoryScreen.kt`。
   - 新增“历史”页面，按时间倒序展示文本/文件发送记录。
   - 支持一键清空历史。
3. 修改 `MainActivity.kt`。
   - 底部导航新增 `历史` tab。
4. 修改 `SendScreen.kt`。
   - 文本和文件发送成功/失败都会写入 Android 本地历史。
   - 文件不存在、JNI 返回错误、网络异常都会留下失败记录。

已验证：

```text
android .\gradlew.bat :app:compileDebugKotlin --rerun-tasks --console=plain: passed
```

## 2026-06-14 追加改进：Android 发送结果通知

新增/修改内容：

1. 新增 `android/app/src/main/java/com/linkhub/app/ui/AndroidTransferNotifications.kt`。
   - 使用已有 `LinkHubApp.CHANNEL_TRANSFERS` 通知渠道。
   - Android 13+ 会先检查 `POST_NOTIFICATIONS`，没有权限时静默跳过通知，不影响发送。
2. 修改 `SendScreen.kt`。
   - 文本发送开始、成功、失败都会发通知。
   - 文件发送开始、成功、失败都会发通知。
   - 文件不存在或不可读时也会记录历史并发失败通知。

已验证：

```text
android .\gradlew.bat :app:compileDebugKotlin --rerun-tasks --console=plain: passed
```

## 2026-06-14 追加改进：Android 配对二维码显示

新增/修改内容：

1. 修改 `android/app/build.gradle.kts`。
   - 新增 `com.google.zxing:core:3.5.3`，用于本地生成二维码。
2. 修改 `PairScreen.kt`。
   - 生成配对码后显示“二维码配对”卡片。
   - 二维码内容就是 Rust core 生成的 `linkhub-pair-v1` payload。
   - 保留原有 payload 文本框和复制按钮，作为扫码失败或跨端调试兜底。

已验证：

```text
android .\gradlew.bat :app:compileDebugKotlin --rerun-tasks --console=plain: passed
```

## 2026-06-15 追加审查：Android 稳定性收敛

本轮发现工作区混入了构建缓存、错误路径测试文件和若干行为回归，已处理：

1. 清理工作区生成物。
   - 移除误生成的 `CLinkHubtest.txt`。
   - 清理 `android/.gradle` 与 `android/app/build` 构建输出，不纳入提交。
2. Pair 页修正。
   - 撤销“自动填入确认码”行为，避免绕过用户人工核对短码。
   - 解析新的对端 payload 时清空旧确认码，避免误用上一次输入。
   - 保留确认码复制按钮，方便手动输入但不替代确认动作。
3. 保留 `android/app/src/main/jniLibs/x86_64/liblinkhub_core.so`。
   - 解决 x86_64 模拟器上可能因缺少 native library 导致的 `UnsatisfiedLinkError`。
   - ARM64 真机仍继续使用既有 `arm64-v8a/liblinkhub_core.so`。

已验证：

```text
android .\gradlew.bat :app:assembleDebug --console=plain: passed
core-rs cargo test: passed
scripts/verify-local-e2e.ps1: passed
adb install -r android/app/build/outputs/apk/debug/app-debug.apk: passed on emulator-5554
emulator 启动 MainActivity: no FATAL EXCEPTION / no UnsatisfiedLinkError
emulator 底部 tab 连点: no FATAL EXCEPTION
emulator 服务页 listener 状态显示: 运行中，显示接收目录、mDNS、本机地址提示
```

意义：

- 真机测试不再只能依赖临时 toast/status 或 logcat，可以在 App 内回看每次发送尝试。

## 下一步建议

下一轮 Android 优先级建议：

1. 在当前热点局域网环境下验证 Android -> Windows 文件发送。
2. 验证 Windows -> Android 文件发送和 Android 接收目录写入。
3. 真机验证 Android NSD/mDNS 与 Windows 桌面端 mDNS 互相发现。
4. 将 mDNS 发现从手动扫描演进为后台定时刷新和候选地址评分。
5. 发送失败时在 UI 中显示明确错误，不只依赖 logcat。
6. 如果要支持模拟器，补充 `x86_64` 的 Rust `.so`。

## 给后续实现者的注意事项

- 不要在 Compose 组合内容中使用 `return@Column`、`return@Scaffold` 这类提前退出写法来控制 UI 分支。
- 对于条件 UI，优先使用稳定的 `if/else`、独立 Composable、或提前在函数顶部计算状态。
- 修改 Android UI 后，至少验证：
  - `.\gradlew.bat :app:assembleDebug`
  - `.\gradlew.bat :app:compileDebugKotlin --rerun-tasks --console=plain`
  - 真机安装
  - 点击四个底部 tab
  - `adb logcat` 中无 `FATAL EXCEPTION`
- 当前目标应是 Android 最小闭环，不建议继续扩展新功能面。

## Windows 桌面端相关补充

虽然本文主要记录 Android 修复，但 Windows 桌面端也补了 listener 诊断能力：

- `start_listener` 会先做 TCP bind 预检查，端口被占用或地址非法时直接返回错误。
- listener 后台线程最近错误会保存在 `listener_status.error`。
- 桌面 Service 页会展示 Last error，避免 Windows 侧误判 listener 已经成功运行。
- 桌面端发送失败也会写入 History 页，并显示失败详情，便于和 Android 历史页一起对照真机测试。
