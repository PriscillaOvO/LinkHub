# Next Claude Task — "task b" (pairing-security hardening)

> Reserved for the next **Claude** session (NOT Codex). Created 2026-06-18 by Claude after a `/code-review xhigh` on b3810c9 surfaced deeper pre-existing pairing-security gaps. Full rationale: `shared/validation-log.md` (top two entries) + `claude-to-codex/latest.md` ("Hands off").
>
> How to use: paste the prompt block below verbatim into a fresh Claude session.

---

```
你是 LinkHub 项目（c:\Dev\VSCode\LinkHub，分支 main）的负责开发。这次接手“task b：配对安全加固”，
它来自上一轮 /code-review xhigh 的发现（详见 docs/ai-handoff/shared/validation-log.md 顶部两条）。

【先做】按顺序读：docs/ai-handoff/codex-to-claude/latest.md（Codex 这几天做了什么、有没有动到相关文件，
避免冲突/重复）、docs/spec/项目状态.md 顶部、docs/ai-handoff/claude-to-codex/latest.md 里“Hands off / task b”那段。
读完先用 git log --oneline -15 和 git status 确认当前实际状态，再动手。

【背景/根因，要修的两个核心安全问题】
1) 配对码 TTL 形同虚设：core-rs/src/identity/pairing.rs 的 PairingInvitation 用 created_at: Instant，
   而 jni_bridge.rs 的 parsePairingPayload/confirmPairing 解析时都传 Instant::now()，confirm 又用 now 校验，
   于是 is_expired 永远 false——根因是 payload 只带“有效时长 ttl”不带“绝对签发时间”，且 Instant 不可序列化。
2) 确认码可离线预计算：pairing.rs 的 confirmation_code 现在是 sha256(sorted(双方指纹))[..6]=24 位，
   太短，配合 #1 可被中间人离线撞短码。

【实现 linkhub-pair-v2（一次性、协议升级）】
- payload 里加一个“绝对签发时间戳”（用 SystemTime / Unix 秒，因为 Instant 不能跨设备传），
  让接收端能用 SystemTime::now() - issued_at > ttl 真正判断过期；据此修正 PairingInvitation（加 issued_at 字段）、
  to_payload/from_payload、以及 jni_bridge/ios_bridge/main.rs 里所有传 created_at 的调用点。
- 加长确认码：仍然只取 sorted(双方指纹) 的摘要（保持两端对称、可比对），但取更多位（比如 ≥40 位 / 10 hex 分组），
  不要为了“加长”重新引入 nonce——nonce 会破坏“两端确认码一致”这个刚修好的性质。
- 删掉 wire 格式里已无安全作用的 nonce 字段（header 升到 linkhub-pair-v2，字段数从 7 变 6）。
- 协议是跨端的，务必同步改所有解析点：Rust（pairing.rs to_payload/from_payload、jni_bridge.rs、ios_bridge.rs、main.rs CLI）；
  Kotlin（android/app/.../ui/AndroidStorage.kt 的 trustedPeerFromPayload——它硬编码 fields.size != 7 且
  publicKey=fields[3]、dhPublicKey=fields[4]，删 nonce/改格式后这些下标和数量都要改）；iOS（ios_bridge.rs 及 Swift 侧若有解析）。
  现有 v1 已配对设备的 dhPublicKey 存在信任库里，格式变更需要重新配对，文档里说明即可。

【顺带（同属安全/可观测，按需）】
- 把 Codex 已加的“仅 std 的 panic 捕获”升级为全局 std::panic::set_hook + android_logger（让原生 panic 进 logcat）；
  确认 Codex 的 Task 3/4 是否已落地，没落地的话一并补。

【验证】core-rs cargo test 全绿且零警告；加新单测：过期 payload 被拒、未过期通过、v2 round-trip、两端码一致、码长度符合预期；
cargo ndk -t arm64-v8a -t x86_64 check --lib 通过；重建两 ABI .so（scripts/build-android-so.ps1 或 cargo ndk）+ debug/release APK；
如条件允许，用两个模拟器（AVD：LinkHub_API_34=5554、LinkHub_API_34_b=5556；helper 在 .screenshots/drive.sh；
网络用 adb forward(接收端 8787) + adb reverse(发送端 127.0.0.1:<port>)；发送文件要放 app 外部目录
/storage/emulated/0/Android/data/com.linkhub.app/files/ 而非 /sdcard/Download）实跑复验一次双向配对+收发，
确认过期 payload 现在会被拒、确认码两端一致且变长。

【收尾】每个逻辑改动一个 commit（作者邮箱 PriscillaOvO@users.noreply.github.com，末尾加 Co-Authored-By 行），
push 到 main；更新 docs/spec/项目状态.md + 开发路线图.md、docs/ai-handoff/claude-to-codex/latest.md、
worklog/claude.md、shared/handoff-clock.md、validation-log.md。keystore.properties / *.jks / *.apk / *.aab 不要进 git。
```
