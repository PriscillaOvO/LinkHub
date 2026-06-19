# LinkHub iOS 端设计（T9 脚手架 · 2026-06-19）

> 本文是 iOS 端的落地方案 + 现状盘点。**iOS 的编译/打包只能在 macOS + Xcode 上完成**（iOS SDK、`lipo`、`xcodebuild` 仅 macOS 有），本仓库当前在 Windows 上开发，故本轮交付的是**可在 Mac 上一键起步的脚手架 + 方案**，真机构建与权限实测留待有 Mac 时进行。

## 1. 现状盘点

- 已有：`ios/LinkHub/LinkHub/` 下 6 个 SwiftUI 文件（`ContentView` 选项卡、`PairView`/`DevicesView`/`SendView`、`BonjourService`、`Bridge/RustBridge.swift`）。
- 已有：core 的 iOS FFI 模块 [ios_bridge.rs](../../core-rs/src/ios_bridge.rs)（`#[cfg(target_os = "ios")] pub mod ios_bridge;`，已在 lib.rs 挂载），导出 `linkhub_generate_identity` / `linkhub_restore_identity` / `linkhub_generate_pairing_payload` / `linkhub_parse_pairing_payload` / `linkhub_confirm_pairing` / `linkhub_free_string`，与 `RustBridge.swift` 一一对应。
- **缺口（本轮 T9 补上）**：① 无 Xcode 工程（`.xcodeproj` 缺失）→ 加 `ios/project.yml`（XcodeGen 文本化定义）；② Swift 看不到 C 符号 → 加 `ios/include/linkhub_core.h` + `module.modulemap`（`import LinkHubCoreFFI`）；③ 无交叉编译产物 → 加 `ios/scripts/build-core-ios.sh`（三 target → `LinkHubCore.xcframework`）+ core `crate-type` 增 `staticlib`；④ 无权限声明 → 加 `ios/LinkHub/Info.plist`（本地网络/Bonjour 键）；⑤ 源树不自洽（`ContentView` 引用了不存在的 `ServiceView`、无 `@main`）→ 补 `LinkHubApp.swift` + `ServiceView.swift`。

## 2. FFI 选型：手写 C ABI + JSON 契约（已定）

| 方案 | 取舍 |
|---|---|
| **手写 C ABI + JSON 串（选中）** | 与现有 Android JNI **完全对齐**（同一套 JSON 契约，Swift `Codable` 解析）；无额外构建步骤/依赖；`ios_bridge.rs` 已是这套，零返工。代价：边界内存需手动 `linkhub_free_string`、错误以 in-band JSON 返回。 |
| UniFFI | Swift 绑定自动生成、类型更丰富。代价：引入 `uniffi` 依赖 + `uniffi-bindgen` 构建步骤；core 需按 UniFFI 风格重塑 API；与 Android 的手写 JNI 形成两套范式。**否决**（为单端引入重机制，不划算）。 |

约定（与 Android 一致）：每个调用收/发 UTF-8 JSON C 串；返回串由库 `malloc`，调用方**必须** `linkhub_free_string` 释放；错误回 `{"error":"..."}`（`confirm` 回 `{"success":false,"error":"..."}`）。

## 3. 交叉编译与工程化

- **target**：device `aarch64-apple-ios`；模拟器 `aarch64-apple-ios-sim`（Apple 芯片）+ `x86_64-apple-ios`（Intel）。两个模拟器切片 `lipo` 合并，与 device 切片一起 `xcodebuild -create-xcframework` → `ios/Frameworks/LinkHubCore.xcframework`。
- **crate-type**：core `[lib]` 增 `staticlib`（产出 `liblinkhub_core.a` 供 iOS 静态链接；`lib`=桌面 rlib、`cdylib`=安卓 `.so` 不受影响）。
- **工程**：`ios/project.yml`（XcodeGen）→ `xcodegen generate` 出 `LinkHub.xcodeproj`；文本化定义入库，避免手维护 `.pbxproj`。`SWIFT_INCLUDE_PATHS=$(SRCROOT)/include` 让 Swift 识别 modulemap；`OTHER_LDFLAGS=-lc++ -framework Security`（getrandom→SecRandom）。
- **链接框架**：默认（无 webrtc）为纯 Rust + 系统能力；`getrandom` 在 iOS 走 `Security.framework`。

## 4. 发现 / 本地网络 / 后台

- **发现走 Swift 侧 Bonjour**：`BonjourService`（`NetService`/`NetServiceBrowser`，类型 `_linkhub._tcp`，TXT 带 `id/name/fp/port`）。core 里的 `mdns-sd` 仅桌面/安卓用；iOS 不复用它，避免与系统网络栈/权限打架。
- **权限（iOS 14+ 强制）**：`NSLocalNetworkUsageDescription`（同意串）+ `NSBonjourServices`（`_linkhub._tcp` 白名单）。**两者缺一**则 `NetService` 浏览静默返回空、且不弹权限框——是 iOS 上最常见的"发现不到"坑。已写入 `Info.plist` + `project.yml`。
- **后台限制**：iOS 后台网络受系统调度强约束，不能像桌面那样常驻监听。策略：传输在**前台**进行；必要时 `beginBackgroundTask` 争取有限后台时长；不承诺"锁屏长时接收"。这与桌面/安卓前台服务模型不同，UI 上要明确告知。

## 5. 跨网络（WebRTC）

与 Android 同策略：**默认关、按构建 opt-in**。但 iOS 上 webrtc-rs（含 C++ 依赖）的交叉编译尚未验证，列为后续；默认 iOS 包不带 webrtc，体积/复杂度最小。端到端加密信任层（Noise KK + trust store）跨端不变。

## 6. 里程碑与未做

1. **（本轮）脚手架**：FFI 模块挂载 + 头/modulemap + 构建脚本 + XcodeGen 工程 + Info.plist + App 入口/ServiceView 补齐。
2. 在 Mac 上 `xcodegen generate` + `build-core-ios.sh` 出 xcframework + Xcode 真机/模拟器跑通最小链路（生成身份、配对、局域网发现）。
3. 补 listener 接收循环 + 文件/文本传输经 core FFI（当前 `RustBridge` 只覆盖 identity/pairing，未含 send/listen——对照 Android JNI 的 `sendText`/`sendFile`/`startListener` 扩 FFI）。
4. 真机本地网络权限实测、后台行为实测。
5. iOS 跨网络（webrtc-rs 交叉编译）+ CI（需 macOS runner）。

## 7. 验收口径

- core 默认矩阵不受影响：`ios_bridge` 仅 `cfg(target_os="ios")`，桌面/安卓构建与测试与改前一致；`staticlib` 仅多产一个 `.a`，不改 `.so`/rlib。
- iOS 侧 Rust 编译：`cargo check --target aarch64-apple-ios --lib`（结论见 [ios/README.md](../../ios/README.md)；在非 Mac 主机上能否纯 `check`（不链接）取决于 Apple std 与各依赖对 iOS target 的支持）。
- Swift/Xcode 构建：须 Mac，见 `ios/README.md`。
