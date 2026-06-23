## 给 Claude 的提示词

继续接力 `feat/transport-abstraction-m1`：Codex 已完成 C1-C5 并提交，C6 滑动窗口按你的允许暂缓并用 docs-only commit 记录；当前最终验收矩阵全绿。下一步优先安排带 `--features webrtc` 的 Android APK 双模拟器/真机 UI 实测，另把 C6 拆成独立协议/吞吐优化任务（先设计窗口协商、ACK/恢复语义和单测）。

# Codex to Claude Latest

Last updated: 2026-06-20 02:50 +08:00
Updated by: Codex
Base branch: main
Working branch: `feat/transport-abstraction-m1`
Latest known commit: **beeef5f** (Document sliding window deferral — C6 skipped)
Workspace status: C1-C6 batch handled. C1-C5 implemented as independent commits; C6 intentionally skipped per handoff because sliding-window transfer affects protocol/backpressure/resume compatibility and needs a dedicated design/test pass. Final validation matrix is green. `docs/ai-handoff/` is local/git-ignored and updated for handoff continuity; only untracked `.claude/settings.local.json` is present.

## Completed In This Batch

- **C1** (`5a1da39`): signaling-server cross-connection limits.
- **C2** (`b708c0a`): synchronous signaling supervisor for reconnect + presence recovery; CLI `signal-listen` uses it.
- **C3** (`35d9b04`): desktop resident WebRTC receive loop.
- **C4** (`cdf5e66`): iOS local-network transfer FFI.
  - New C ABI: `linkhub_send_text`, `linkhub_send_file`, `linkhub_start_listener`, `linkhub_stop_listener`, `linkhub_listener_status`.
  - Updated C header + Swift `RustBridge`.
  - iOS listener exposes polling status only; no Swift UI callback yet.
  - No iOS WebRTC FFI was added.
- **C5** (`27a1d46`): Android cross-network WebRTC UI and foreground-service wiring.
  - Send page now exposes signaling/STUN/TURN/relay-only config and calls `RustBridge.webrtcSendFile` off the main thread.
  - Service page can start a WebRTC receive loop inside `LinkHubService`; completed receives reuse the existing `onFileReceived` notification/history path.
  - Added Android JNI `webrtcStopReceiver`; with the `webrtc` feature it uses `receive_file_over_webrtc_until(stop)` to interrupt waiting for an offer.
  - Default `.so` still does not include webrtc-rs/tokio; UI maps the stub error to "需跨网包".
- **C6** (`beeef5f`): skipped and documented.
  - Reason: sliding window touches capability negotiation, ACK/backpressure, resume semantics, old-peer fallback, and WebRTC/TURN file e2e correctness.
  - Tracked docs now state the deferral and recommend a separate task with protocol design + window state-machine tests before implementation.

## Verified

- C1: `signaling-server` fmt/clippy/test.
- C2: core default fmt/test/clippy, `cargo check --features webrtc`, default NDK dual ABI.
- C3: desktop test/default+webrtc clippy/JS check; core `--features webrtc` test/clippy.
- C4: `cargo check --target aarch64-apple-ios --lib`; core default fmt/test/clippy; default Android NDK dual ABI.
- C5: core `cargo fmt --all -- --check`; default `cargo ndk -t arm64-v8a -t x86_64 check --lib`; Android `./gradlew :app:assembleDebug`; WebRTC `.so` path `cargo ndk -P 24 -t x86_64 -t arm64-v8a build --release --features webrtc`.
- Final full matrix after C6: signaling-server `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`; desktop `cargo test`, default + webrtc clippy, `node --check desktop/src/js/send.js`; Android `./gradlew :app:assembleDebug`; core default `fmt/test/clippy`, core `--features webrtc` `test/clippy` including CLI/DataChannel/TURN file e2e, `cargo check --target aarch64-apple-ios --lib`, default Android NDK dual ABI, and WebRTC Android release `.so` dual ABI build.

## Guardrails

- Did not touch v2 pairing.
- Did not add webrtc/tokio to default paths.
- Default Android `.so` path remains protected by no-feature checks where relevant.

## Next Step

Recommended next work:

- Build a real Android package with the `webrtc` `.so` and run Compose UI two-emulator or true-device cross-network send/receive validation.
- Treat C6 as a new standalone protocol optimization task, not a quick patch.
- Push branch when ready; local branch is ahead of origin with the six C1-C6 commits plus prior T8/T9 commits.
