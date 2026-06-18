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
- **信令防伪造**：转发的 `SIGNALING` 应由发起方用身份私钥签名，接收方校验（防服务器或他人冒充注入假 candidate 做重定向攻击）。
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
- **M3 — P2P 打洞（同/跨网，非对称 NAT）**：接 WebRTC DataChannel，Noise 跑在其上，完成一次跨网文件传输（SHA-256 校验）。
- **M4 — TURN 中继兜底**：对称 NAT/打洞失败时经 TURN，自动回退。
- **M5 — 各端集成 + 选路**：LAN 优先、自动升级/降级、UI 显示当前传输路径（直连/打洞/中继）。
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

1. **管道选型**：libp2p（C，推荐先评估）／ webrtc-rs（A）／ 自研 QUIC 打洞（B）？
2. **信令/中继**：自建服务器（可控、可商用）还是尽量 serverless（靠 libp2p relay）？
3. **服务器语言/部署**：Rust + 自托管？放哪（你有云资源吗）？
4. **离线唤醒**：本期是否要 push 唤醒离线设备，还是只支持「两端都在线」？
5. **隐私底线**：能接受「元数据级信令服务器」吗？这是跨网几乎绕不开的最小暴露。

> 你定完 1–5，我就可以把对应的 M1（Transport 抽象重构，纯地基、低风险）先落地，或先做选型 spike（实测 libp2p/webrtc-rs 在三端能否编译跑通）。
