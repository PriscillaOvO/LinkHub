# Claude → Codex 交接：Tor Phase 4 平台壳（onion 接进 UI，device-free 可编译验证部分）

Last updated: 2026-06-21 ~05:30 +08:00 · Updated by: Claude (Opus 4.8)
Base branch: `main` · Working branch: **`feat/transport-abstraction-m1`**

> 先读本文件，再读 `docs/spec/项目状态.md` 顶部一节（2026-06-21 夜间自驱）+ `docs/spec/设计-tor-onion-传输.md` + `docs/ai-handoff/worklog/claude.md` 顶部。

## 背景（我刚做完的，你接着往上接）

我把对端 `.onion` 接进了**身份交换 + trust store**（commit `30bb5f2`）和 **mDNS TXT**（`9030a06`），都在默认构建、零 Arti 依赖、全绿。现在 core 已经能在「配对/首次接触/局域网发现」时**自动学到并持久化对端 onion**，桌面接受流程也透传持久化了。

**还差的是把 onion 接进各端 UI**——这正是你（平台壳）的活，且**大部分能在没真机时编译验证**（真正的 onion-over-Tor 数据路径仍门控真机，不在本次范围）。

## 已就绪的 core API（已编译/测试全绿，直接用）

```rust
// DeviceIdentity 现在带可选 onion（core-rs/src/identity/device_identity.rs）
DeviceIdentity::new(id, name, pk, dh).with_onion_address(Some(addr)) // 空白→None
device_identity.onion_address() -> Option<&str>                      // 取回

// 本机自己的 onion（默认构建，纯算，不需 tor feature）
local_identity.onion_address() -> Result<String, String>            // 分享给对端用

// IncomingPeer 现在带 onion（core-rs/src/net/auth_listener.rs）
pub struct IncomingPeer { …已有 5 字段…, pub onion_address: Option<String> }

// trust store 已能持久化 onion（可选第 6 管道字段，向后兼容 5 字段 v1 记录）
```

## 你的任务（每条一个 commit，过完整矩阵）

### T-A（Android）把 onion 接进「接受附近设备」流程并持久化
- **JNI**（`core-rs/src/jni_bridge.rs` `make_accept_peer_callback`）：现在调 Kotlin `onIncomingPeer` 传 5 个 String；扩成传第 6 个 `peer.onion_address`（可空 → 传 `null` 或空串）。改 JNI 方法签名串（多一个 `Ljava/lang/String;`）。
- **Kotlin**（`android/.../bridge/RustBridge.kt`）：`onIncomingPeer` 静态方法 + `data class IncomingPeer` 各加 `onionAddress: String?`。
- **持久化**（`android/.../MainActivity.kt` 的 `saveTrustedPeerFromIncoming` / `AndroidStorage`）：接受时把 onion 一并写进 trust store（core 的 trust store 已支持；确认安卓侧是用 core 的 `TrustStore` 还是自管的存储——若自管，补 onion 字段）。
- 验证：`cargo ndk -t arm64-v8a -t x86_64 check --lib`（默认）+ `:app:compileDebugKotlin`。**JNI 签名串必须和 Kotlin 方法精确对齐**，否则运行期 `NoSuchMethodError`。

### T-B（桌面）显示「本机 .onion」供分享
- 加一个 Tauri 命令返回 `local_identity.onion_address()`（失败回友好错误），前端在配对/服务页显示「本机匿名地址 (.onion)」+ 一键复制，供已配对对端存下做日后 Tor 兜底。
- 注意：onion 地址是**公开**的（可分享），但**不能由对方公钥反推**，所以必须显式分享/随身份交换——这正是这一步的意义。
- 验证：`desktop cargo check`（**在 `desktop/src-tauri` 目录跑**，别在 core-rs 里跑，否则会用 USTC 镜像缺 `qrcode`）+ `node --check`。

### （可选 T-C）桌面/安卓「匿名模式 (Tor)」开关
- 仅 UI + 设置存储的开关（决定是否把 onion 排进自动兜底）；真正的 Tor 连接走 `tor` feature，**数据路径门控真机**，本次只做开关骨架 + 文案，不接真实 bootstrap。

## 守则（不可破）
- **不碰 v2 配对加密**；首次接触 accept 回调 **fail-closed 不弱化**（JVM/JNI 出错=拒绝）。
- webrtc/tokio/tor 全 feature-gated；**默认 `.so` 保持精简、不拉 Arti**。
- onion 地址是**咨询性**字段（不进绑定签名）——真正认证仍是 Noise KK；别把它当信任锚。
- 每个任务**独立 commit**，过矩阵：core 默认 fmt/test/clippy + `cargo ndk` 双 ABI check；安卓 `compileDebugKotlin`/`assembleDebug`；桌面 check/clippy；`node --check`。
- **构建用 `--frozen`**（清过 target 后默认 cargo 会去刷 Arti 索引、国内 curl 易失败；三个镜像索引都已缓存 Arti 条目）。`core-rs/.cargo/config.toml`（gitignored）按 CWD 生效。
- **commit 别 push**——用户用「push 吧」做检查点。

## 现状
- 分支 `feat/transport-abstraction-m1`，HEAD `014dd6a`，**6 个未 push commit**（`30bb5f2`/`9030a06`/`f513107`/`5a157af`/`50a22a9`/`014dd6a`）。
- 全绿（离线 `--frozen`）：fmt、默认 test 159、clippy（默认+webrtc）、双 ABI ndk、桌面 check、安卓 compileDebugKotlin。
