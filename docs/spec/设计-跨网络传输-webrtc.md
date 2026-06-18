# 设计文档：跨网络传输（WebRTC / NAT 穿透 / 中继兜底）

> 状态：草案 v0.1（2026-06-18，Claude）。对应路线图阶段 5。
> 目的：让两台**不在同一局域网**的可信设备也能直接（或经中继）端到端加密传输，突破当前「必须同一 Wi-Fi」的限制。
> 这份文档定架构、协议、各端改动点与分期落地，作为后续实现/重型审查的输入。**末尾「待拍板决策」需要你先选。**

---

## 1. 背景与目标

**现状**：所有传输走 mDNS 发现 + 直连 TCP（`0.0.0.0:8787`），两端必须同网段可直连。这是目前最大的产品短板。

**目标**：
- 两台已配对设备在**不同网络**（家里 ↔ 公司、移动网络 ↔ Wi-Fi）也能传输。
- 端到端加密**不削弱**：信令服务器、中继节点**都看不到内容**，信任锚仍是设备身份密钥（trust store），不是任何服务器。
- 局域网仍走直连（最快、不经服务器）；只有跨网时才升级到打洞/中继。
- 尽量复用现有的 Noise KK 认证会话与 wire 协议，不另起炉灶。

**非目标（本阶段）**：多跳 mesh 转发（阶段 6）、实时音视频（阶段 7）。但本设计要给它们留好接口（中继、信令已在协议里占位）。

---

## 2. 现状盘点（已有的脚手架，复用它）

实现前先确认这些**已经存在**，避免重造：

- **wire 协议已占位**：[protocol.rs](../../core-rs/src/net/protocol.rs) 已有
  - `Signaling { session_id, kind, payload_hex }`（kind = `offer`/`answer`/`ice-candidate`/`done`/`error`）——阶段 5 信令。
  - `RelayRequest` / `RelayResponse` / `RelayForward`——阶段 6 中继（兜底也能用）。
  - 这些目前是 `#[allow(dead_code)]` 的「已定义未接线」状态。
- **传输类型已占位**：[transport.rs](../../core-rs/src/transport.rs) 的 `TransportKind` 已含 `WebRtc`、`CloudRelay`、`LanQuic`。
- **认证会话**：[auth_session.rs](../../core-rs/src/net/auth_session.rs) 跑 `HELLO → AUTH_CHALLENGE → AUTH_SIGNATURE → Noise KK → AUTH_OK → 加密帧`，但**硬编码 `TcpStream`**（第 30/440/592 行等）。
- **信任模型**：trust store 存对端 `device_id + 身份公钥(Ed25519) + DH 公钥`，确认码绑定双方指纹。这是端到端信任的根，跨网后**保持不变**。

**关键结论**：跨网络不是“换个加密”，而是**“换个管道 + 加一个找到彼此的机制”**。加密/信任层（Noise + trust store）原样套在新管道上即可。

---

## 3. 总体架构

```
            ┌─────────────────┐  WebSocket(签名鉴权)  ┌─────────────────┐
            │   设备 A         │ ───── 信令 ─────────▶ │  信令服务器       │
            │ (core-rs)       │ ◀──── 信令 ──────────  │ (presence +     │
            └────────┬────────┘                        │  store&forward) │
                     │                                  └────────┬────────┘
                     │                                           │ 信令
                     │   ① 同网：mDNS 直连 TCP（现状，最优先）       │
                     │   ② 跨网：ICE 打洞 → P2P UDP                ▼
                     │                                  ┌─────────────────┐
                     └───── ③ 打洞失败 → TURN 中继 ─────▶│  TURN/中继服务器  │
                                                        └─────────────────┘
                     ▼
        在选定的管道之上：Noise KK 认证会话（端到端，服务器看不到明文）
                     ▼
            复用现有 TEXT / FILE_* / ACK 帧传输
```

三条路径按优先级自动选择（沿用现有 transport 评分思路）：
1. **LAN 直连**（`LanTcp`）：mDNS 发现到对方就直接连，不碰服务器。
2. **P2P 打洞**（`WebRtc`）：经信令交换 SDP/ICE，NAT 打洞成功后直连 UDP。
3. **中继兜底**（`CloudRelay`）：对称型 NAT 打洞失败时，经 TURN/自建中继转发**密文**。

---

## 4. 关键设计决策

### 4.1 端到端加密：Noise 仍是唯一信任层
WebRTC 自带 DTLS，但 DTLS 的证书是**临时自签**的，无法绑定设备身份。所以：
- **不依赖** DTLS 做身份认证；把现有 **Noise KK 会话整体跑在 WebRTC DataChannel（或中继隧道）之上**。
- 好处：中继/TURN 即使能看到 DTLS 内层，也只看到 Noise 密文；MITM 防护仍来自 trust store 里双方公钥。信令服务器更是只转发 SDP，碰不到数据。
- 代价：DTLS + Noise 双层加密有轻微开销，但安全边界清晰、与现有代码一致，**强烈推荐**。

### 4.2 Transport 抽象（最核心的重构）
把 [auth_session.rs](../../core-rs/src/net/auth_session.rs) 从 `TcpStream` 解耦成**泛型 `R: Read + W: Write`**（或一个 `trait LinkTransport: Read + Write + Send`）。这样同一套握手/Noise/帧逻辑能跑在：
- `TcpStream`（现状 LAN）
- WebRTC DataChannel 的 read/write 适配器
- 中继隧道（把帧塞进 `RELAY_FORWARD` 的 `payload_hex`）

这是所有后续工作的地基，**应作为第一步独立完成并保持现有测试全绿**。

### 4.3 信令通过现有 `SIGNALING` 消息承载
SDP offer/answer、ICE candidate 都塞进 `Signaling{session_id, kind, payload_hex}`。信令在两端之间的传递有两种载体：
- 还没有任何 P2P 通道时 → 经**信令服务器** store-and-forward。
- 已经有 LAN 连接但想升级/换路时 → 直接在现有会话里发 `SIGNALING`。

### 4.4 设备寻址：device_id 是全局地址
跨网后 IP 无意义，**`device_id` 成为路由标识**。信令服务器维护 `device_id → 当前连接` 的在线表（presence）。离线设备的信令可短期暂存或直接失败（先做失败，离线队列留阶段 5 后段）。

> M2-step1 落地细节：服务器实际按**身份公钥**建表（公钥与 `device_id = lh-+sha256(pubkey)[..16]` 1:1，等价但能直接用签名证明拥有权，杜绝冒充他人 id 上线）。

### 4.5 运行时架构决策：同步 core vs 异步 webrtc-rs（2026-06-18，已拍板 = 方案 a）

**问题**：core-rs 整体是**同步 std** 实现（`TcpStream`、`snow`、M1 的认证会话是 `W: Write + R: BufRead` 阻塞流），并被编译成 **Android JNI cdylib** 和 Tauri 共用；而选定的 webrtc-rs 是**全异步 tokio**、DataChannel 是**消息**语义（非字节流）。两者运行时模型不一致，必须先定怎么融合。

候选：
- **(a) 全局 tokio runtime + channel 把异步 DataChannel 桥成同步 `Read`/`Write`**（推荐，已选）。
- (b) 把 `auth_session` 整条异步化，同步入口用 `block_on` 包。
- (c) 维持两套实现。

**决策：选 (a)。** 理由：
- **改动面最小、最低风险**：`auth_session.rs` / 各 sender / `auth_listener.rs` / JNI / Tauri **一行不用改**——它们继续吃 `W: Write + R: BufRead`。只有新增的 webrtc 模块碰 tokio。
- **已被现有代码验证可行**：`auth_session.rs` 的单测 `MemoryDuplex`（`Mutex<VecDeque<u8>> + Condvar` 的阻塞双工）已经证明「认证会话能跑在非 socket 的阻塞 `Read`/`Write` 上」。webrtc 适配层就是把 `MemoryDuplex` 的一端换成「由 tokio 任务泵 DataChannel 的收发」——同一个缝。
- (b) 会把 async 传染到所有同步调用方与 JNI，返工巨大；(c) 维护两套不划算。

**落地形态**：
- 新增 `core-rs/src/net/webrtc_transport.rs`（在 `webrtc` feature 后面），提供 `DataChannelDuplex`：实现同步 `Read + Write`，内部用 `std::sync::mpsc` + `Condvar` 缓冲；一个常驻 tokio 运行时把 DataChannel 的 `on_message` 灌进入站缓冲、把 `write()` 的字节经 `dc.send()` 发出。
- webrtc-rs 及 tokio 全部锁在 **`webrtc` feature（默认关）**：默认 `cargo build` / `cargo ndk check --lib` / 现有测试矩阵**完全不受影响、不变重**；M3 的实现与测试用 `--features webrtc` 编译验证（spike 已证明 webrtc-rs 能在 Windows + Android NDK 双 ABI 交叉编译）。
- DataChannel 单帧大小有限（典型 ~16KB/64KB），而认证会话的加密帧 = 2 字节大端长度前缀 + ≤64KB 密文。`DataChannelDuplex` 在 `Read` 侧做**重组**（把多个 DataChannel 消息拼回连续字节流再交给 `BufRead`），在 `Write` 侧按 DataChannel 上限**分片** `dc.send()`，使「字节流语义」在消息信道上成立。

---

## 5. 信令服务器（新组件，尽量薄）

一个**无状态/弱状态**的 WebSocket 服务（建议也用 Rust，可与 core 复用协议解析）：

- **鉴权**：设备连上后用身份私钥对服务器下发的 challenge 签名（复用现有 Ed25519 + AUTH_CHALLENGE 思路），服务器用设备登记的公钥校验。**服务器不需要也不应保存任何明文数据内容**。
- **presence**：内存维护 `device_id → ws 连接`；上线/下线广播给其「可信对端」（可选，先做按需查询）。
- **store-and-forward 信令**：A 要连 B → A 发 `{to: B_device_id, SIGNALING...}`，服务器转发给 B；B 回 answer/candidate 同理。服务器只搬运 `payload_hex`，不解析 SDP 内容。
- **能力**：返回可用 STUN/TURN 地址 + 临时 TURN 凭证（短期有效）。
- **隐私**：服务器能看到「谁在线、谁想连谁、什么时间」这类元数据——这是必须接受的最小暴露，**文档里要对用户讲清楚**。内容/文件名/指纹都看不到。

> 决策点：信令服务器自建（可控、可商用）vs 借用现成（成本低但隐私/可控性差）。见 §11。

---

## 6. 连接建立时序（跨网 P2P）

```
A                      信令服务器                      B
│── 登录+签名鉴权 ──────▶│                              │
│                       │◀──────── 登录+签名鉴权 ───────│
│── want-connect(B) ───▶│── (B在线?) ──────────────────▶│ 收到 A 想连
│── SIGNALING offer ───▶│──────── 转发 offer ──────────▶│
│                       │◀─────── SIGNALING answer ─────│
│◀────── 转发 answer ───│                              │
│◀──▶ ICE candidates 经服务器互换（trickle）◀──▶          │
│                                                      │
│============ ICE 连通性检查（STUN 打洞）==============│
│  成功 → P2P UDP DataChannel 直连                       │
│  失败 → 双方都连 TURN，经 TURN 转发（仍是 DataChannel）  │
│                                                      │
│====== DataChannel 之上跑 Noise KK 认证会话 =========│
│  HELLO→AUTH_CHALLENGE→AUTH_SIG→NOISE_HS→AUTH_OK      │
│====== 之后 TEXT / FILE_* / ACK 原样传输 ============│
```

LAN 优先：发起前先尝试 mDNS/已知地址直连，连上就跳过整个信令流程。

---

## 7. 安全与隐私模型

| 角色 | 能看到 | 看不到 |
|---|---|---|
| 信令服务器 | 谁在线、谁连谁、时间、IP | SDP 内容(可端到端加密)、传输内容、文件名、密钥 |
| TURN/中继 | 有加密流量经过、流量大小/时间 | 明文（Noise 密文）、身份指纹 |
| 中间人(网络) | 加密流量 | 一切明文 |

- **MITM 防护**：不变——Noise KK 绑定 trust store 里双方公钥，确认码已是 40 bit。
- **信令防伪造 ✅（T3，2026-06-19 完成）**：转发的 `SIGNALING`（offer/answer SDP）由发起方用身份私钥 Ed25519 签名，接收方用**它本就预期的对端身份公钥**（发起端=trust store 里的目标设备；响应端=已在 `accept_signaling_delivery` 校验过的可信设备）校验后才喂给 webrtc。签名经域分隔（header `linkhub-signaling-sdp-v1`，与 p2p 握手 / 信令登录两个域隔离）并**绑定 session_id + 角色(offer/answer)**：服务器改一个字节、把 offer 当 answer 重放、或重放别的会话的 SDP，接收端都会拒绝（防连接重定向/降级——把双方钉到攻击者控制的中继做流量分析或 DoS）。实现：core `net/signaling_signed.rs`（`seal_sdp`/`open_sdp`/`verify_signaling_sdp`，纯 Ed25519+serde，默认构建即编译并单测）+ `LocalIdentity::sign_signaling_sdp` / `identity::signaling_sdp_message`；`payload_hex` 改为 hex(JSON `{v,sdp,sig}`)。CLI 的 `drain_outbound_sdp` 发送前 `seal_sdp`、`delivery_to_sdp_signal` 收取时 `open_sdp`。验收：6 个单测（往返/换签名者/篡改 SDP/角色互换/跨会话重放/坏载荷全部正确 accept/reject）+ `webrtc_cli_e2e` 经真实服务器跑签名路径仍字节一致。
- **TURN 凭证**：短时效、按会话发放，避免长期凭证泄漏被滥用。
- **抗滥用**：信令服务器要做基础限流/配额（防被拿来当任意中继）。
- **隐私披露**：产品内要明确告知「使用跨网功能时，一台元数据级服务器会知道你的设备在线与连接意图」。

---

## 8. 各端改动点

- **core-rs**
  - 新增 `trait LinkTransport: Read + Write + Send`，把 `auth_session` 泛型化（§4.2）。
  - 新增 `net/signaling_client.rs`：WebSocket 客户端 + 鉴权 + SIGNALING 收发。
  - 新增 WebRTC 适配层：把所选 WebRTC 实现的 DataChannel 包成 `LinkTransport`。
  - 接线 `protocol.rs` 里已占位的 `Signaling`/`Relay*`（去掉 dead_code）。
  - 连接编排器：LAN→P2P→中继 的选路与回退（可挂到现有 transport 评分）。
- **信令服务器**：全新组件（独立 crate / 部署单元）。
- **desktop (Tauri)**：基本能直接用 core；注意 ICE 收集需要 UDP 出网，企业网防火墙降级到 TURN。
- **Android**：WebRTC/UDP 在 Doze/后台受限——传输期要前台服务（已有）；要不要 push 唤醒离线设备是后续问题。NDK 交叉编译 WebRTC 依赖需验证。
- **iOS**：本地网络权限、后台限制最严（阶段 4 还没补齐）；跨网 P2P 在后台基本不可行，需 push 唤醒 + 用户在前台时建连。

---

## 9. 分期落地（建议顺序）

- **M1 — Transport 抽象重构 ✅（2026-06-18 完成）**：`auth_session` 已从 `TcpStream` 解耦为泛型 `W: Write` / `R: BufRead`；新增传输无关入口 `run_authenticated_session_over`（responder）与 `perform_initiator_handshake`（initiator），保留 `run_authenticated_session(TcpStream)` / `open_authenticated_stream(addr)` 作为 TCP 薄封装；`ack::write_message` 也泛型化。新增内存双工单测 `authenticated_text_round_trips_over_in_memory_transport` 证明会话可跑在**非 TCP** 管道上。现有 TCP e2e 全绿、cargo ndk 双 ABI + desktop check 通过。这是 WebRTC/中继的接入缝。
- **M2 — 信令服务器 + presence**：最薄可用版（鉴权、在线表、转发 SIGNALING）。core 加 signaling_client。两端能通过服务器互发 SIGNALING（先不接 WebRTC，发个 ping/pong 验证链路）。
  - **M2-step1 ✅（2026-06-18 完成）**：新增独立 crate `signaling-server/`（tokio + tokio-tungstenite，72 依赖）。已实现：① **Ed25519 登录鉴权**——服务器先发 `Challenge{nonce}`，设备回 `Auth{device_id, public_key_hex, signature_hex}`，服务器用 `verify_strict` 校验签名（域分隔串 `linkhub-signaling-auth-v1\0{nonce}`，与 p2p 握手签名隔离）；② **presence**——按**已证明的身份公钥**（与 `device_id = lh-+sha256(pubkey)[..16]` 1:1）建内存在线表，杜绝冒充他人 id 上线；③ **store-and-forward**——`Forward{to_public_key_hex,…}` → `Deliver{from_public_key_hex,…}`，服务器只搬运 `payload_hex` 不解析；离线对端回 `Error{peer offline}`；④ ping/pong。JSON 信封（与 p2p 的 tab 行协议分离）。**验收**：crate 内集成测试 `tests/forward.rs` 起服务器 + 两个 ws 客户端各自鉴权、A→B 转发 SIGNALING 断言 B 收到（外加离线报错/ping-pong/坏签名拒绝）；7 单测 + 4 集成全绿，`cargo fmt`/`clippy -D warnings` 干净。**这就是 M2 的"ping/pong 验证链路"验收，WebRTC 未接（M3）。**
  - **M2-step2 ✅（2026-06-18 完成）**：core-rs 新增 [signaling_client.rs](../../core-rs/src/net/signaling_client.rs)——**同步 `tungstenite`** WebSocket 客户端（贴合 core 现有同步 std 网络层，不给默认/Android 构建引入 tokio）；实现对服务器的 Ed25519 登录鉴权（`LocalIdentity::sign_signaling_login` 签 `linkhub-signaling-auth-v1\0{nonce}`）+ `Forward`/`Deliver` 收发，公共类型 `SignalingClient`/`SignalingEvent`/`SignalingDelivery`。CLI 加 `signal-listen` / `signal-relay` 子命令。验收：`tests/signaling_e2e.rs` 起真实服务器 + 两个 core 客户端互发 SIGNALING、离线报错（3 测试全绿）。
- **M3 — P2P DataChannel + Noise ✅（2026-06-18 完成）**：core-rs 新增 [webrtc_transport.rs](../../core-rs/src/net/webrtc_transport.rs)（`webrtc` feature 后面）——`DataChannelDuplex` 把 webrtc-rs 的异步消息 DataChannel 桥成同步 `Read+Write`（tokio runtime 泵收发 + buffer/condvar，Write 侧按 16KB 分片、Read 侧重组），`connect_initiator`/`accept_responder` 用非 trickle（gather-complete）交换 offer/answer。net 抽出传输无关公共入口 `run_authenticated_{text,file}_sender_over` / `run_authenticated_responder_over`，让**现有 Noise KK 会话原样跑在 DataChannel 上**。验收：`tests/webrtc_e2e.rs`（`--features webrtc`）两个进程内 PeerConnection 经回环建 DataChannel，跑通 40KB 认证加密文件传输、接收端 **SHA-256 与源一致**、回调触发。webrtc-rs/tokio 默认关，`cargo ndk` 双 ABI 默认 check 与现有测试矩阵不受影响。
- **T1 — WebRTC 接进 CLI ✅（2026-06-19 完成）**：`linkhub-cli` 新增 `listen-webrtc` / `connect-webrtc`（需 `--features webrtc`）。CLI 用 `SignalingClient` 登录真实 signaling-server，将本端 `SdpSignal` 的 SDP 编为 hex 后 `Forward{kind=offer/answer}`，将对端 `Deliver` 解回 `SdpSignal`，驱动 `webrtc_transport::{connect_initiator, accept_responder}` 建立真实 DataChannel；随后直接跑 `run_authenticated_file_sender_over` / `run_authenticated_responder_over`。`--ice <url>` 可重复传入 STUN/TURN URL；发送端从 trust store 按 `peer_device_id` 查对端 identity/DH key，接收端只响应 trust store 中可信设备的 offer。验收：`tests/webrtc_cli_e2e.rs`（`--features webrtc`）启动真实 signaling-server + 两个 CLI 子进程，经服务器交换 SDP，随后经真实 webrtc-rs DataChannel 传输 40KB 文件，接收端字节与源一致。**已验证（T2）**：真实跨 NAT / 双模拟器跨网络（见下）。**未覆盖**：TURN 强制 relay（T4）、端侧 UI/JNI（T6/T7）。
- **T2 — 真实 Android 模拟器跨网络实测 ✅（2026-06-19 完成）**：把带 `--features webrtc` 的 Android x86_64 `linkhub-cli`（`cargo ndk -P 24`，**minSdk 24**——webrtc-rs 需 `getifaddrs`/`freeifaddrs`，API 21 无；不影响默认 `cargo ndk check --lib`）推到两个 AVD，经 host signaling-server（`10.0.2.2:9000`）+ 公共 Google STUN，跑**真实 webrtc-rs Android 运行期 + 跨模拟器 NAT** 的双向文件传输：A→B SHA `605a4914…1980d7`、B→A SHA `eb610cdc…bc264c`，接收端均与源一致。**首次证明 M3 路径在真实 Android 上能跑且能穿真 NAT**（非仅交叉编译/进程内回环）。备忘：`/data/local/tmp` noexec → 经 `/system/bin/linker64` 运行。**未覆盖**：物理真机、arm64 运行期、TURN 强制 relay（T4，本次公共 STUN 足够）、webrtc-on `.so` 体积（T7）。
- **T3 — 信令消息签名 ✅（2026-06-19 完成）**：见 §7「信令防伪造」。补上跨网信任链最后一环——SDP 端到端签名/验签，恶意信令服务器无法篡改/替换/重放 offer/answer。core `net/signaling_signed.rs` + `LocalIdentity::sign_signaling_sdp`，CLI 收发两侧接线；6 单测 + e2e 全绿，`cargo ndk` 双 ABI 默认 check 不受影响（纯 Ed25519，无新依赖）。
- **M4 — TURN 中继兜底（部分）**：编排选路已落地（见下），TURN 实际拨号留待。
- **M5 — 各端集成 + 选路（编排逻辑已落地）**：core-rs 新增 [connection_plan.rs](../../core-rs/src/net/connection_plan.rs)——`plan_connection` 按 LAN 直连 → WebRTC → 中继 的固定优先级（恰是 `TransportHealth` 基础分顺序）产出有序尝试计划，`attempt_with_fallback` 逐条回退，`preferred_established_route` 复用 `select_best_route` 在多路并存时按分选最优。各端 UI 接线 + TURN 实拨 + 真机跨网实测留待。
- 之后（阶段 5 尾）：离线队列、云端在线状态优化。

每个里程碑都要有**双设备实测**（可沿用本项目的双模拟器 + adb 驱动 + SHA-256 校验方法）。

---

## 10. 技术选型（候选，需拍板）

跨网传输的「管道」实现有三条路线，差异很大：

- **A. webrtc-rs（纯 Rust WebRTC）**
  - 优点：完整 ICE/STUN/TURN/DataChannel，与现有纯 Rust core 统一；社区有移动端使用案例。
  - 风险：依赖重；Android NDK / iOS 交叉编译要实测；二进制体积。
- **B. 自建 UDP 打洞 + QUIC（quinn）+ Noise**
  - 优点：轻、纯 Rust、可控；QUIC 自带多路复用/拥塞控制；不背 WebRTC 全栈。
  - 风险：要自己实现 ICE-lite/打洞与回退逻辑（工作量不小，易踩 NAT 细节坑）；对称 NAT 仍需中继。
- **C. rust-libp2p（DCUtR 打洞 + Circuit Relay v2 + Noise 已内置）**
  - 优点：**打洞 + 中继 + Noise 加密本就是 libp2p 的核心能力**，最贴合需求；纯 Rust；省掉信令服务器很多自研（用 libp2p 的 relay/identify）。
  - 风险：引入较大框架与其寻址/多路复用模型，要把现有 device_id/trust 模型映射到 libp2p PeerId；学习曲线。

> 我的倾向：先认真评估 **C（libp2p）**，因为「打洞+中继+Noise」正是它解决的问题，能少写一个信令服务器和大量 NAT 代码；若它在移动端交叉编译或与现有信任模型集成受阻，退回 **A（webrtc-rs）**。**B** 适合想要极致轻量且愿意自研 NAT 逻辑时。
> 这个选型本身就值得用重型模式 / 专项调研定夺。

STUN/TURN：自建用 **coturn**；STUN 可先用公共服务器，TURN 因涉及流量与凭证建议自建。

### 10.1 交叉编译 spike 结果（2026-06-18，Claude）

用两个一次性 crate（`spike/libp2p-spike`、`spike/webrtc-spike`）分别对 **Windows host** 和 **Android NDK 28.2 双 ABI（arm64-v8a + x86_64）** 跑 `cargo check`，验证设计文档反复点名的最大未知风险——「WebRTC/libp2p 能否在 Android NDK 上交叉编译」。

| 候选 | Windows host | Android arm64-v8a | Android x86_64 | 依赖 crate 数 | C/C++ sys 依赖 |
|---|---|---|---|---|---|
| **C. libp2p 0.54.1**（tcp+quic+dns+noise+yamux+**dcutr**+**relay**+identify+ping+macros+tokio） | ✅ 编过 | ✅ 编过 | ✅ 编过 | **330** | 无（纯 Rust，`ring 0.16/0.17` 均交叉编过） |
| **A. webrtc-rs 0.11.0**（完整 ICE/STUN/TURN/DTLS/SCTP/DataChannel） | ✅ 编过 | ✅ 编过 | ✅ 编过 | **231** | 无（纯 Rust，`ring 0.17` 交叉编过） |

**结论：交叉编译这一最大风险，对 A 和 C 都已证伪——两者都能在 Windows + Android NDK 双 ABI 上干净 `cargo check` 通过，且都是纯 Rust 栈（无 openssl-sys 等 C 依赖），历史坑点 `ring` 也都交叉编过。**所以选型不再由"能不能编"决定，而回到架构契合度：

- **libp2p（C）**：打洞（dcutr）+ 中继（relay v2）+ Noise 都是内置能力，能省掉自研信令服务器和大量 NAT 回退代码；代价是依赖更重（330 crate，含整套 quinn/QUIC、hickory-dns、upnp），且要把现有 `device_id`/trust 模型映射到 libp2p `PeerId`、接受其 swarm/多路复用模型。spike 里已写了 `peer_id_from_ed25519_seed`（用我们已有的 Ed25519 身份密钥直接派生 PeerId），这层映射看起来不麻烦。
- **webrtc-rs（A）**：更轻（231 crate）、更底层——给你 ICE/STUN/TURN/DataChannel，但**信令服务器要自己写**（即文档 §5 / M2），Noise 仍需自己套在 DataChannel 上。胜在与「设备身份当信任锚 + 自建薄信令」的现有设计耦合最自然，没有 PeerId 寻址模型的概念负担。

**尚未测的两点（留作 M3 原型时补）**：① 真实 `.so` **体积**（本次只 `cargo check`，未 release 链接出 cdylib；cdylib 不导出符号会被裁剪，需要写个 `#[no_mangle]` 入口才能测准）；② iOS 交叉编译（阶段 4 工程尚未脚手架化，无法测）。

> spike 复现命令（任一 crate 目录下）：`cargo check --lib`（host）、`cargo ndk -t arm64-v8a -t x86_64 check --lib`（Android，需 `ANDROID_NDK_HOME`）。网络抖动时加 `CARGO_HTTP_MULTIPLEXING=false`、`CARGO_HTTP_LOW_SPEED_LIMIT=0`、`CARGO_NET_RETRY=10`。`spike/` 下的 `target/` 已被根 `.gitignore` 忽略。

---

## 11. 风险与未决问题

- WebRTC/libp2p 在 **Android NDK + iOS** 的交叉编译与体积，必须早验证（放 M1/M3 前做 spike）。
- **移动端后台**：iOS/Android 后台无法长期维持 P2P；离线设备唤醒要不要做 push（涉及各家推送服务，复杂）。
- **对称型 NAT** 必然要 TURN，中继**流量成本**与运维。
- 信令/中继服务器的**部署、可用性、隐私合规、防滥用**。
- 是否需要 device_id ↔ libp2p PeerId 的映射与迁移。
- 与现有 transport 评分/心跳如何融合成统一选路。

---

## 12. 待你拍板的决策（先定这些再动手）

1. ~~**管道选型**~~ → **已拍板：A（webrtc-rs）**（2026-06-18，用户决定）。理由：与现有"Noise KK 是唯一信任层、跑在 DataChannel 之上"（§4.1）耦合最自然，无 PeerId 寻址/swarm 概念负担，依赖更轻（231 vs 330）。代价：**信令服务器要自己写**（即 §5 / M2，正是下一步）。交叉编译 spike 见 §10.1。
   - **连带影响决策 2**：选了 webrtc-rs ⇒ 没有 libp2p 的内置 relay/rendezvous，**信令必须自建**（serverless 路线随 libp2p 一起被排除）。中继兜底用 coturn（TURN），不自己写转发逻辑。
2. **信令/中继**：自建服务器（可控、可商用）还是尽量 serverless（靠 libp2p relay）？
3. **服务器语言/部署**：Rust + 自托管？放哪（你有云资源吗）？
4. **离线唤醒**：本期是否要 push 唤醒离线设备，还是只支持「两端都在线」？
5. **隐私底线**：能接受「元数据级信令服务器」吗？这是跨网几乎绕不开的最小暴露。

> 你定完 1–5，我就可以把对应的 M1（Transport 抽象重构，纯地基、低风险）先落地，或先做选型 spike（实测 libp2p/webrtc-rs 在三端能否编译跑通）。
