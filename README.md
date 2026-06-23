# LinkHub

LinkHub 是一个跨平台可信设备互联原型，目标是在 Android、iOS/iPadOS、Windows、macOS 和 Linux 之间建立可发现、可配对、可认证加密的本地传输通道。

当前阶段重点不是完整产品 UI，而是验证核心链路：

- 可信设备身份与配对（v2 配对码 + 双向确认 + TTL）
- AirDrop 式首次接触握手（免配对码，TOFU 一键接受）
- mDNS 局域网发现（含 onion 地址传播）
- 文本与文件的认证加密传输（Noise KK，二进制分帧）
- 跨网络传输：自建信令服务器 + WebRTC DataChannel + TURN 中继兜底
- Tor onion 传输（opt-in 隐私增强，地址由设备身份派生）
- 多端壳：Rust core/CLI、Android 前台服务、Tauri 桌面、iOS FFI 脚手架

## 当前状态

截至 2026-06-21，核心链路已从「局域网认证传输」扩展到「跨网络 + 匿名传输」，并在宿主 / 模拟器 / 单测层面得到验证。

**已跑通并验证**

- 局域网认证加密传输：Android↔Android 双模拟器双向文本 / 文件，接收端 SHA-256 与源逐字节一致。
- v2 配对（配对码 + 双向确认 + TTL）双模拟器 UI 实测通过。
- AirDrop 式首次接触握手（免配对码）：core 安全核心 + Android / 桌面 UI 全链路完成，含 MITM 换 DH、device_id 伪造的拒绝测试。
- 跨网络 WebRTC：自建信令服务器 + STUN/TURN，双 Android 模拟器跨 NAT 双向传输 SHA 一致；强制 TURN 中继路径以宿主 CLI 真实跑通。
- 二进制文件分帧（线缆体积砍半，带版本协商向后兼容），在 WebRTC / TURN e2e 上验证。
- Tor onion 传输 Phase 1–3：地址派生（纯 Rust，对齐 Arti 参考向量）+ Arti 传输层 + CLI，onion 地址随身份交换 / mDNS 传播。

**门控真机 / 待办**

- Tor onion Phase 4/5（移动端壳 + 真实「两台真机 onion-over-Tor」数据路径）。
- I2P 真传输（目前仅有 device-free 抽象脚手架 + 评估文档，未拉依赖）。
- iOS：须 macOS 才能生成 Xcode 工程 / 构建 xcframework / 真机实测（当前为可构建脚手架 + FFI）。
- 公网部署 signaling / coturn 后的真实跨 NAT 跨网验收。

测试基线：core 默认 `cargo test` 159 通过；`--features webrtc` 含 WebRTC/TURN 文件 e2e；signaling-server 单测 + 集成测试全绿；Android 双 ABI `cargo ndk check` 与 `assembleDebug` 通过。完整逐轮进展见 [docs/spec/项目状态.md](docs/spec/项目状态.md)。

## 目录

```text
core-rs/                 Rust core、CLI、JNI bridge
signaling-server/        跨网络信令服务器（独立 crate）
android/                 Android App
desktop/                 Tauri desktop 原型
ios/                     iOS FFI 脚手架（须 macOS 构建）
spike/                   一次性交叉编译探雷（libp2p / webrtc）
docs/                    需求、架构、路线图、设计、环境和测试文档
scripts/                 本地验证脚本
```

建议新接手时优先阅读：

```text
docs/spec/项目总览报告.md
docs/spec/项目状态.md
docs/spec/开发路线图.md
docs/spec/设计-跨网络传输-webrtc.md
docs/spec/设计-tor-onion-传输.md
docs/spec/环境部署.md
docs/spec/真机测试指南.md
```

完整文档导航见 [docs/README.md](docs/README.md)。

## 本机环境

当前 Windows 开发机使用：

- JDK 17: `C:\Program Files\Eclipse Adoptium\jdk-17.0.19.10-hotspot`
- Android SDK: `C:\Users\<用户名>\AppData\Local\Android\Sdk`
- Android AVD: `LinkHub_API_34`
- Android target/build tools: API 34 / Build Tools 34.0.0

详细部署和验证命令见 [docs/spec/环境部署.md](docs/spec/环境部署.md)。

## 常用验证

Rust core:

```powershell
cd C:\Dev\VSCode\LinkHub\core-rs
cargo test --quiet
```

Desktop Rust:

```powershell
cd C:\Dev\VSCode\LinkHub\desktop\src-tauri
cargo test --quiet
```

Android Debug APK:

```powershell
cd C:\Dev\VSCode\LinkHub\android
.\gradlew.bat :app:assembleDebug
```

Android 模拟器烟测：

```powershell
emulator -avd LinkHub_API_34
adb install -r android\app\build\outputs\apk\debug\app-debug.apk
adb shell monkey -p com.linkhub.app -c android.intent.category.LAUNCHER 1
adb shell pidof com.linkhub.app
```

## 开发注意

- 不要提交 `android/.gradle/`、`android/app/build/`、`android/local.properties`、`android/.idea/`、`desktop/src-tauri/target/`、`desktop/src-tauri/gen/schemas/` 等本地生成内容。
- Android 身份和信任设备数据使用加密 SharedPreferences 保存，应用备份已关闭，避免长期私钥进入系统备份。
- `ios/` 现为可构建脚手架（XcodeGen 工程定义 + FFI），但须在 macOS 上 `xcodegen generate` 并构建 xcframework，尚未在真机 / 模拟器实测。
