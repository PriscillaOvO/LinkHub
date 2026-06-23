# LinkHub · AI 交接说明（给接手的 AI 读）

> 你是接手 LinkHub 开发的 AI（如 Codex）。**先读完本文件，再动手。** 本文件给你项目地图、已完成的工作、必须遵守的约定、省 token 守则和任务清单。照着做，不要从零重新探索整个仓库。

---

## 0. 省 Token 守则（重要，请严格遵守）

1. **先只读本文件**；需要架构细节再读 `docs/spec/项目总览报告.md`，需要现状读 `docs/spec/项目状态.md`。其余文档按需。
2. **不要全量扫描整个仓库**，也不要把大文件整文件读进来。巨型文件 `core-rs/src/identity.rs`(~1500行)、`core-rs/src/net.rs`(~1600行)、`core-rs/src/main.rs`(~1600行) 只用 grep + 按行号范围读相关片段。
3. **复用已有函数**，不要重写已存在的能力（下面「代码地图」列了关键入口）。
4. **按需构建**：改 Rust 逻辑用 `cargo check`（快）；只有改了 `core-rs` 且要更新 Android 的 `.so` 才跑 `cargo ndk`；只有改了 Android Kotlin 才 `gradlew assembleDebug`；只有改了 `desktop/src-tauri` 才 `cargo check` 那个 crate。别每次全量重建。
5. **并行/批量**工具调用；避免逐个小步往返。
6. 一次专注一个任务，做完更新文档 + 提交，再开下一个。

---

## 1. 项目一句话

LinkHub = 跨平台可信设备互联原型：Android/iOS/Windows/macOS/Linux 设备先**安全配对**，再 **mDNS 局域网自动发现**，最后在 **Noise KK + ChaCha20-Poly1305 端到端加密通道**上互传文本/文件。Rust core 是唯一大脑，各平台是外壳。

完整架构看 `docs/spec/项目总览报告.md`。

## 2. 代码地图（改之前先定位，别瞎找）

```
core-rs/src/           Rust 核心（唯一事实来源）
  lib.rs               公共 API re-export（看这个就知道 core 能干什么）
  net.rs               TCP 会话：认证监听/发送、文本/文件收发、Noise 帧
                       关键: run_authenticated_listener_on[_with_callback], run_authenticated_session,
                             run_authenticated_text_sender, run_authenticated_file_sender
  net/protocol.rs      线协议编解码     net/file_transfer.rs 文件分片/续传/SHA-256   net/ack.rs ACK
  crypto.rs            Noise KK + AEAD（NoiseTransport）
  identity.rs          身份/配对/信任/TrustStore；Windows DPAPI 安全身份（secure:<path>）
  discovery.rs         发现端点 + mDNS TXT 合同 + TTL 注册表
  mdns_runtime.rs      MdnsRuntime（browse_for / register，基于 mdns-sd）
  jni_bridge.rs        Android JNI（#[cfg(android)]）— 含 onFileReceived 回调
  ios_bridge.rs        iOS FFI（#[cfg(ios)]，草稿）
  device.rs/presence.rs/transport.rs/routing.rs  在线状态/心跳/评分/路由
  main.rs              linkhub-cli（功能最全，是各平台行为的「合同」与本地验收基准）

desktop/               Tauri 2 桌面端
  src-tauri/src/main.rs  Rust 外壳，~21 个 #[tauri::command]（含 default_config, scan_trusted_mdns,
                         start_listener, send_encrypted_*, pairing_* 等）
  src/js/                原生 JS 前端：app.js(IPC/设置/自动发现轮询/默认路径seed)
                         devices.js / send.js / pairing.js / service.js / history.js

android/app/src/main/
  java/com/linkhub/app/
    bridge/RustBridge.kt          JNI 声明 + onFileReceived 静态回调
    service/LinkHubService.kt      前台监听服务（注册接收回调）
    ui/  PairScreen/DevicesScreen/SendScreen/ServiceScreen/HistoryScreen
         AndroidDiscovery.kt(scanTrustedMdnsPeers/NSD广播) AndroidStorage.kt(身份/信任/接收目录)
         AndroidReceivedFiles.kt(收文件→通知+历史) AndroidTransferNotifications.kt
  jniLibs/{arm64-v8a,x86_64}/liblinkhub_core.so   ← 由 core-rs 用 cargo ndk 交叉编译产出（已提交）

ios/                   SwiftUI 草稿，无 Xcode 工程，不可构建（暂不碰）
docs/                  中文文档（索引见 docs/README.md）
scripts/verify-local-e2e.ps1   本地双进程端到端验收脚本（文本/文件/续传）
```

## 3. 已完成（最近三次提交，main 当前 = ddbfe33）

- `ff66891` 基线：单一干净提交（已做隐私清理，详见第 4 节）。
- `5a4e891` 地址自动发现/回填：桌面前端 ~8s 轮询 `scan_trusted_mdns` 自动填地址 + 在线状态；Android 设备页/发送页 ~10s 屏幕级自动扫描 `scanTrustedMdnsPeers`+`updatePeerAddress`。未改 core 协议。
- `ddbfe33` 桌面端去硬编码：`default_config` 命令按 OS 返回应用数据目录默认路径（不再写死 `C:\LinkHub`）；history 兜底跨平台化。

更早已完成（背景）：Android 收文件回调通知+历史（JNI `onFileReceived`）、可复现的 `.so` 交叉编译（NDK 28.2 + cargo-ndk）、文件互发已在模拟器+CLI 端到端验收通过、docs 全量中文化重组。

## 4. 工作约定（必须遵守）

1. **每次改代码后同步更新 docs**：至少 `docs/spec/项目状态.md`（加「本轮新增」块）、`docs/spec/开发路线图.md`（阶段标记），以及相关文档（如改测试流程则更新 `docs/spec/真机测试指南.md`）。把更新文档当作「任务完成」的一部分，和代码同一次提交。
2. **隐私**：仓库是 public。**不要把真实 Windows 用户名/个人路径写进任何文件**——文档里一律用占位 `<用户名>`。提交作者邮箱已设为 `PriscillaOvO@users.noreply.github.com`（仓库本地 `git config user.email`），**不要改回真实邮箱**。
3. **提交**：用户偏好**直接提交并推送到 `main`**，保持单一干净线性历史。提交信息结尾加：
   `Co-Authored-By: <你的署名>`（原作者用的是 Claude；你按自己来）。提交前先 `git status` / `git diff --stat` 自检；`*.apk` 已被 .gitignore，别提交 APK。
4. **不要回退/破坏**已通过的能力；core 旧接口（如 `run_authenticated_listener_on`）保持兼容，新增能力用新函数或可选参数。

## 5. 环境与构建命令

环境详情见 `docs/spec/环境部署.md`（把里面的 `<用户名>` 换成本机实际 Windows 用户名）。要点：
- JDK 17、Android SDK（`%LOCALAPPDATA%\Android\Sdk`）、AVD `LinkHub_API_34`、NDK `28.2.13676358`、`cargo-ndk` 4.1.2、Rust target `aarch64/x86_64-linux-android`。

常用命令（按需，别全跑）：
```
# core 逻辑检查（快）
cd core-rs && cargo check
# core 单测
cd core-rs && cargo test --quiet
# 本地双进程端到端验收（文本/文件/续传）
pwsh scripts/verify-local-e2e.ps1
# 改了 core 后重建 Android .so（两个 ABI，输出到 jniLibs）
#   先 set ANDROID_NDK_HOME=...\ndk\28.2.13676358
cd core-rs && cargo ndk -t arm64-v8a -t x86_64 -o ../android/app/src/main/jniLibs build --release --lib
# 改了 Android Kotlin 后
cd android && ./gradlew.bat :app:assembleDebug
# 桌面 Rust 外壳检查
cd desktop/src-tauri && cargo check
# 模拟器冒烟（无头）：emulator -avd LinkHub_API_34 -no-window -no-snapshot -gpu swiftshader_indirect -no-audio
#   注意：模拟器 NAT 下 mDNS 多播不通；跨设备发现/传输用 adb forward 或真机
```

## 6. 任务清单（按优先级，做完一个再下一个）

> 选能在**当前 Windows 机器上验证**的任务优先（标 [Win可验证]）。标 [需mac/linux] 的只能写代码、留待对应平台验证。

### 任务 A [Win可验证] — core 巨型文件拆分 + 跨进程自动化测试（推荐先做）
- **目标**：把 `identity.rs` / `net.rs` 按关注点拆成子模块（如 identity 拆成 device_identity / pairing / trust_store / secure_store；net 已部分拆到 net/，继续把会话流程、认证、文件接收拆清楚），降低维护风险；并把 `scripts/verify-local-e2e.ps1` 的覆盖纳入 `cargo test` 友好的集成测试。
- **复用**：现有 `lib.rs` re-export 必须保持不变（外部 API 稳定），只重排内部模块；用现有 `verify-local-e2e.ps1` 作为行为基准。
- **验证**：`cargo test` 全绿、`cargo check`（desktop 也要过，因为它链接 core）；`.so` 不必动（若公共 API 没变，Android 无需重编，但建议跑一次 `cargo ndk ... check` 确认 android target 仍编译）。
- **完成后更新**：`docs/spec/项目状态.md`、`docs/spec/开发路线图.md`（阶段5/工程化）、`docs/spec/技术架构.md` 第 8 节组件清单。

### 任务 B [需mac/linux] — macOS Keychain / Linux Secret Service 安全身份后端
- **背景**：`core-rs/Cargo.toml` 已声明 `security-framework`(macOS) / `secret-service`(linux) 依赖，但后端**未实现**；`identity.rs` 注释标了「仍需补齐」。Windows 用 DPAPI 已实现（`secure:<path>`）。
- **目标**：实现非 Windows 的安全身份存储，让 `secure:` 在 mac/linux 也可用；之后把 `desktop/src-tauri/src/main.rs` 的 `default_config` 改为非 Windows 也默认 `secure:`。
- **注意**：这部分在 Windows 上**无法编译验证**（`#[cfg(target_os=...)]`）。如果你也在 Windows，只能写好代码并标注「待目标平台编译验证」，不要假装验证过。
- **完成后更新**：`docs/spec/项目状态.md`、`docs/spec/开发路线图.md` 阶段2、`docs/spec/项目总览报告.md` 风险项3 与任务4。

### 任务 C [Win可验证] — 把 `.so` 移出 git 跟踪，改为构建产出
- **目标**：`android/app/src/main/jniLibs/*.so` 现在被 git 跟踪（二进制在仓库里）。改为 `.gitignore` 忽略 + 提供清晰的重建脚本/说明（cargo ndk 命令已在第 5 节），让 CI/开发者按需生成。
- **注意**：这会改变构建契约——移除跟踪后，没跑过 cargo ndk 的人 `assembleDebug` 会缺 `.so`。需在 `docs/spec/环境部署.md` 和 `android` 说明里写清「先 cargo ndk 再 assembleDebug」。先和用户确认是否要现在做（可能影响其他人 clone 后直接构建）。
- **完成后更新**：`.gitignore`、`docs/spec/环境部署.md`、`docs/spec/项目状态.md`。

### 其它待办（来自路线图，优先级较低）
- 桌面端在 macOS/Linux 的实跑验证（`default_config` 已就绪，缺平台环境）。
- 交互式可信设备列表 UI、传输历史/失败原因的桌面端完善。
- Android 相机扫码导入对端配对码；公共下载目录接收（MediaStore/SAF）。
- iOS 重新脚手架化（阶段4，暂缓）。

## 7. 验收与真机测试

- 文件互发/发现的真机与模拟器验收步骤见 `docs/spec/真机测试指南.md`（含 adb forward 法、SHA-256 校验、`direction=received` 历史检查）。
- 提交前最低要求：相关 `cargo check`/`cargo test` 通过；改了 Android 则 `assembleDebug` 通过；如实记录哪些验证过、哪些因平台限制没验证。
