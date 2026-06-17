# LinkHub

LinkHub 是一个跨平台可信设备互联原型，目标是在 Android、iOS/iPadOS、Windows、macOS 和 Linux 之间建立可发现、可配对、可认证加密的本地传输通道。

当前阶段重点不是完整产品 UI，而是验证核心链路：

- 可信设备身份与配对
- mDNS 局域网发现
- Rust core 认证监听与发送
- Android 前台服务监听
- Windows/桌面端原型集成
- 文本与文件的认证加密传输

## 当前状态

截至 2026-06-16，本仓库已在 Windows 开发机完成基础环境部署和回归验证：

- Rust core 单元测试通过。
- Tauri desktop Rust 测试通过。
- Android Debug APK 构建通过。
- Android Studio SDK 与模拟器已配置，AVD 名称为 `LinkHub_API_34`。
- APK 已在 Android API 34 模拟器上安装并启动验证。

本轮修复了 core/JNI/desktop 监听器重复绑定同一端口的问题，并清理了误提交的 Android/Gradle/Tauri 构建产物跟踪。

## 目录

```text
core-rs/                 Rust core、CLI、JNI bridge
android/                 Android App
desktop/                 Tauri desktop 原型
ios/                     iOS 早期源码草稿
docs/                    需求、架构、路线图、环境和测试文档
scripts/                 本地验证脚本
```

建议新接手时优先阅读：

```text
docs/spec/项目总览报告.md
docs/spec/项目状态.md
docs/spec/环境部署.md
docs/spec/真机测试指南.md
docs/spec/开发路线图.md
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
- `ios/` 当前仍是源码草稿，没有完整 Xcode 工程，不能视为可构建客户端。
