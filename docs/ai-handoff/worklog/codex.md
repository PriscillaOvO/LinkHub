# Codex 工作日志（worklog）

> 这是 **Codex 的个人流水账**：每次会话收尾时在最上方追加一条，记录「做了什么 / 为什么 / 改了哪里 / 怎么验证的 / 下一步」。
> 目的：会话上下文太长被截断后，仍能从这里回溯历史，不必重新摸索整个仓库。
>
> 与其它交接文件的分工（别重复写）：
> - 本文件 = 你自己的逐次流水（最细，倒序，最新在最上面）。
> - `../codex-to-claude/latest.md` = 给 Claude 的定向交接（它下一步需要知道什么）。
> - `../shared/handoff-clock.md` = 谁的笔记最新 + 一行时间线。
>
> 条目模板：
> ```
> ## YYYY-MM-DD HH:MM +08:00 — 一句话标题
> - 做了：
> - 为什么：
> - 改动：<文件/范围>
> - 验证：<跑了什么 / 结果>
> - 下一步：
> - commit：<sha 或 未提交>
> ```

---

## 2026-06-20 02:50 +08:00 - C1-C6 最终验收与交接
- 做了：跑完整最终验收矩阵；在 `docs/ai-handoff/codex-to-claude/latest.md` 顶部写入「给 Claude 的提示词」；刷新 handoff clock。
- 为什么：批量任务 C1-C6 已处理完，需给下一棒留下可直接接力的当前状态、剩余任务和验证结果。
- 改动：`docs/ai-handoff/{worklog/codex.md,shared/handoff-clock.md,codex-to-claude/latest.md}`（git-ignored handoff 文件）。
- 验证：signaling-server `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test`；desktop `cargo test` / 默认+webrtc clippy / `node --check desktop/src/js/send.js`；Android `./gradlew :app:assembleDebug`；core 默认 `cargo fmt --all -- --check` / `cargo test` / `cargo clippy --all-targets -- -D warnings`；core `cargo test --features webrtc` / `cargo clippy --features webrtc --all-targets -- -D warnings`（含 CLI/DataChannel/TURN 文件 e2e）；`cargo check --target aarch64-apple-ios --lib`；`cargo ndk -t arm64-v8a -t x86_64 check --lib`；`cargo ndk -P 24 -t x86_64 -t arm64-v8a build --release --features webrtc`。
- 下一步：Claude 可 push 分支；建议优先做带 webrtc `.so` 的 Android APK 双模拟器/真机 UI 实测，并把 C6 作为独立协议吞吐任务重新设计。
- commit：beeef5f（当前 HEAD；handoff 文件本身 ignored）。

## 2026-06-20 02:30 +08:00 - C6 滑动窗口吞吐优化暂缓
- 做了：按交接允许跳过 C6 代码实现，并在 `docs/spec/项目状态.md`、`docs/spec/开发路线图.md`、`docs/spec/设计-跨网络传输-webrtc.md` 写清暂缓原因、风险边界和后续拆分建议。
- 为什么：滑动窗口会同时影响协议能力协商、ACK/恢复语义、背压窗口、旧端回退和三条 WebRTC 文件 e2e；当前没有足够把握在批量任务末尾安全改动。T8 的二进制分帧已完成线缆体积优化，逐块 ACK 继续作为正确性基线。
- 改动：docs-only：`docs/spec/{项目状态.md,开发路线图.md,设计-跨网络传输-webrtc.md}`。
- 验证：无源码改动；提交前 `git diff --check` 通过。最终全局验收矩阵在 C6 后统一执行。
- 下一步：跑最终 acceptance matrix；更新 `codex-to-claude/latest.md` 顶部「给 Claude 的提示词」并收尾。
- commit：beeef5f（Document sliding window deferral）。

## 2026-06-20 02:23 +08:00 - C5 Android 跨网络 UI/前台服务接线
- 做了：Android 发送页新增 WebRTC 跨网络文件发送表单（signaling/STUN/TURN/relay-only/目标设备/文件），调用 `RustBridge.webrtcSendFile` 并接历史/通知；服务页新增跨网络接收配置和随前台服务启动开关；`LinkHubService` 增加常驻 `webrtcReceiveFile` 循环，复用现有接收通知链路；JNI 增加 `webrtcStopReceiver`，带 feature 时用 `receive_file_over_webrtc_until(stop)` 打断等待 offer。
- 为什么：T7 已有 Android JNI WebRTC 符号，但端侧没有 UI/服务触发；C5 要把发送页和前台服务接上，同时默认包继续保持瘦 `.so`，未启用 WebRTC 时给「需跨网包」提示。
- 改动：`android/app/src/main/java/com/linkhub/app/{bridge/RustBridge.kt,service/LinkHubService.kt,ui/AndroidWebRtcConfig.kt,ui/SendScreen.kt,ui/ServiceScreen.kt}`、`core-rs/src/jni_bridge.rs`、`docs/spec/{项目状态.md,开发路线图.md,设计-跨网络传输-webrtc.md}`。
- 验证：`cargo fmt --all -- --check`；`cargo ndk -t arm64-v8a -t x86_64 check --lib`；Android `./gradlew :app:assembleDebug`；真实跨网 `.so` 路径 `cargo ndk -P 24 -t x86_64 -t arm64-v8a build --release --features webrtc` 全绿。`assembleDebug` 和 webrtc release build 因沙箱网络/NDK clang 权限分别提权重跑后通过。
- 下一步：C6 是滑动窗口吞吐优化，改动会触碰协议/背压/恢复语义；若无足够把握应按交接说明跳过并记录原因，再做最终全局验收和交接提示词。
- commit：27a1d46（Wire Android WebRTC UI）。

## 2026-06-20 01:55 +08:00 - C4 iOS 局域网 send/listen FFI
- 做了：给 `ios_bridge.rs` 增加 `linkhub_send_text`/`linkhub_send_file`/`linkhub_start_listener`/`linkhub_stop_listener`/`linkhub_listener_status`，对齐 Android JNI 的 JSON 契约；更新 `ios/include/linkhub_core.h` 和 Swift `RustBridge` wrapper；同步 iOS README 和设计/状态/路线图文档。
- 为什么：T9 只让 iOS 工程可构建，FFI 还停在 identity/pairing。C4 目标是补局域网 send/listen，不做 iOS WebRTC。
- 改动：`core-rs/src/ios_bridge.rs`、`ios/include/linkhub_core.h`、`ios/LinkHub/LinkHub/Bridge/RustBridge.swift`、`ios/README.md`、`docs/spec/{项目状态.md,开发路线图.md,设计-iOS-端.md}`。
- 验证：`cargo check --target aarch64-apple-ios --lib` exit 0；core 默认 `cargo fmt --all -- --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings`、`cargo ndk -t arm64-v8a -t x86_64 check --lib` 全绿。
- 下一步：C5 Android 跨网络 UI 接线，接 Compose/前台服务调用 `webrtcSendFile`/`webrtcReceiveFile`，默认包要提示需 webrtc 构建。
- commit：cdf5e66（Add iOS local transfer FFI）。

## 2026-06-20 01:47 +08:00 - C3 桌面常驻 WebRTC 接收循环
- 做了：桌面新增 `webrtc_start_receiver`/`webrtc_stop_receiver`/`webrtc_receiver_status`，用全局状态管理后台 WebRTC 接收循环；前端跨网络卡片改为「开始接收 / 停止接收」并轮询显示 running/stopping/completed/error；core 保留旧 `receive_file_over_webrtc`，新增 `receive_file_over_webrtc_until(stop)` 支撑等待 offer 时停止。
- 为什么：T6 桌面接收只能「监听一次」，C3 要让桌面可以常驻等待下一次跨网络文件接收，同时保持桌面 `webrtc` feature gate 和默认构建轻量。
- 改动：`core-rs/src/net/webrtc_session.rs`、`desktop/src-tauri/src/main.rs`、`desktop/src/js/send.js`；同步 `docs/spec/{项目状态.md,开发路线图.md,设计-跨网络传输-webrtc.md}`。
- 验证：桌面 `cargo test`、`cargo clippy --all-targets -- -D warnings`、`cargo clippy --features webrtc --all-targets -- -D warnings`、`node --check desktop/src/js/send.js` 全绿；core `cargo test --features webrtc`、`cargo clippy --features webrtc --all-targets -- -D warnings` 全绿，三条真实 WebRTC 文件 e2e 仍通过；core/desktop fmt check 全绿。
- 下一步：C4 iOS 局域网 send/listen FFI，补 C header 和 Swift wrapper，并用 `cargo check --target aarch64-apple-ios --lib` 验证。
- commit：35d9b04（Add desktop WebRTC receiver loop）。

## 2026-06-20 01:38 +08:00 - C2 信令客户端常驻 supervisor
- 做了：新增 `SignalingSupervisor`/`SignalingSupervisorConfig`/`SignalingSupervisorEvent`，用 `std::thread` + `flume` 持有同步 `SignalingClient`，循环登录、设置 read timeout、周期 `ping`、断线后按 `RetryPolicy` 退避重连并重登恢复 presence；CLI `signal-listen` 改为消费 supervisor 事件流。
- 为什么：T5 只有 `connect_with_backoff`/`ping` 原语，长连接路径仍会在一次断线后退出；C2 要补「断线→重连→恢复 presence」高层循环。
- 改动：新增 `core-rs/src/net/signaling_supervisor.rs`；更新 `core-rs/src/{net.rs,lib.rs,main.rs}`；扩展 `core-rs/tests/signaling_e2e.rs`；同步 `docs/spec/{项目状态.md,开发路线图.md,设计-跨网络传输-webrtc.md}`。
- 验证：`core-rs` 下 `cargo fmt --all -- --check`、`cargo test`（含新增 supervisor reconnect e2e）、`cargo clippy --all-targets -- -D warnings`、`cargo check --features webrtc`、`cargo ndk -t arm64-v8a -t x86_64 check --lib` 全绿。
- 下一步：C3 桌面常驻 WebRTC 接收循环，重点是 Tauri `webrtc` feature gate、后台任务启动/停止和前端按钮状态。
- commit：b708c0a（Add signaling reconnect supervisor）。

## 2026-06-20 01:25 +08:00 - C1 信令服务器跨连接全局限流
- 做了：给 signaling-server 增加跨连接并发防线：`Limits.max_connections`、`max_connections_per_ip`、`ConnectionRegistry` 和 RAII `ConnectionPermit`；`serve_with_limits` 在 accept 后、WebSocket 握手前登记连接，超出全局或单 IP 并发上限直接拒绝，不进入认证/presence。
- 为什么：T5 只有每连接 payload/速率限制，仍可能被大量未认证连接耗尽握手和 presence 前资源；C1 要补全跨连接级别的抗滥用。
- 改动：`signaling-server/src/limits.rs`、`signaling-server/src/lib.rs`、`signaling-server/tests/forward.rs`；同步 `docs/spec/项目状态.md`、`docs/spec/开发路线图.md`、`docs/spec/设计-跨网络传输-webrtc.md`。
- 验证：`signaling-server` 下 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 全绿（12 单测 + 8 集成，新增同 IP 超限和全局超限握手拒绝测试）。
- 下一步：C2 客户端 signaling supervisor：断线后退避重连、重登恢复 presence，并接进 CLI `signal-listen` 长连接路径。
- commit：5a1da39（Add signaling connection limits）。

## 2026-06-19 01:52 +08:00 - T1 WebRTC CLI 接线
- 做了：在 `linkhub-cli` 增加 `listen-webrtc` / `connect-webrtc`（`--features webrtc`）两个子命令；CLI 内部用一个阻塞 signaling bridge 线程持有 `SignalingClient`，把 webrtc 的 `SdpSignal` 编码成 signaling-server 的 `Forward{kind=offer/answer,payload_hex}`，再把 `Deliver` 解回 `SdpSignal`，驱动 `webrtc_transport::{connect_initiator, accept_responder}` 建真实 DataChannel；建链后复用现有 `run_authenticated_file_sender_over` / `run_authenticated_responder_over`，让 Noise KK 文件传输跑在 DataChannel 上。
- 为什么：T1 的目标是解锁真实跨网测试入口；M3 只证明库内 DataChannel loopback，缺少 CLI + 真实 signaling-server 的端到端可操作路径。
- 改动：`core-rs/src/main.rs`（命令解析、feature-gated WebRTC CLI runner、SDP/signaling bridge、`--ice` 参数、可信 peer 校验）；新增 `core-rs/tests/webrtc_cli_e2e.rs`（真实 signaling-server + 两个 CLI 子进程传 40KB 文件）；同步 `docs/spec/{项目状态.md,开发路线图.md,设计-跨网络传输-webrtc.md}`。
- 验证：第 0 步基线矩阵已先跑全绿；T1 收尾完整矩阵全绿：core-rs `cargo fmt --all -- --check`、`cargo test`、`cargo test --features webrtc`（含新增 `cli_file_transfer_over_webrtc_signaling_server` 和原 `noise_file_transfer_over_webrtc_datachannel`）、`cargo clippy --all-targets -- -D warnings`、`cargo clippy --features webrtc --all-targets -- -D warnings`、`cargo ndk -t arm64-v8a -t x86_64 check --lib`；signaling-server `cargo test`（7+4）通过。
- 下一步：跑完整验证矩阵；更新 handoff-clock 和 codex-to-claude；提交 T1。后续 T2 用该 CLI 入口做真实跨 NAT / 双模拟器跨网络验证并回填 validation-log。
- commit：b8ef2e8（Wire WebRTC transport into CLI）。

## 2026-06-18 01:58 +08:00 - task b pairing security hardening
- 做了：实现 `linkhub-pair-v2`，payload 删除 nonce、加入 `issued_at` Unix 秒并保留 `ttl`；确认码加长到 40 bit（`XXXXX-XXXXX`）；同步 Rust core/CLI/JNI/iOS bridge/Tauri/Android Kotlin 解析；补 native panic hook + Android logcat；补 stale `service_status.running` reconciliation；清掉 `net/protocol.rs` dead_code warning。
- 为什么：修复 xhigh code-review 顶部两条核心问题：旧 payload 不带绝对签发时间导致 TTL 在接收端形同虚设；旧 24-bit 确认码过短，配合无 TTL 可被离线撞码。
- 改动：`core-rs/src/identity/pairing.rs`、`core-rs/src/identity.rs`、`core-rs/src/main.rs`、`core-rs/src/jni_bridge.rs`、`core-rs/src/ios_bridge.rs`、`desktop/src-tauri/src/main.rs`、Android `AndroidStorage.kt`/`PairScreen.kt`/`AndroidServiceStatus.kt`/`ServiceScreen.kt`，以及相关脚本/文档。
- 验证：`cargo test --manifest-path core-rs/Cargo.toml`、`cargo build --manifest-path core-rs/Cargo.toml`、`cargo ndk -t arm64-v8a -t x86_64 check --lib`、`:app:compileDebugKotlin`、`scripts/build-android-so.ps1`、`:app:assembleDebug`、`:app:assembleRelease`、`cargo check --manifest-path desktop/src-tauri/Cargo.toml`、`node --check desktop/src/js/pairing.js` 均通过；`git ls-files` 确认 so/apk/aab/jks/keystore 未被跟踪。
- 下一步：提交并 push；如有条件再跑双模拟器 UI 实测，重点看 v2 payload 过期拒绝、确认码两端一致、重配后双向收发。
- commit：未提交（待本会话收尾 commit）。

<!-- Codex：把你的第一条记录加在这一行下面（最新在最上）。 -->
