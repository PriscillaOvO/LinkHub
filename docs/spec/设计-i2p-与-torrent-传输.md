# 设计 / 评估:I2P 与 BitTorrent(torrent)互联

> 状态:**方案评估 + 抽象层脚手架(2026-06-21)**。本文是 I2P / BitTorrent 两条新互联
> 方式的可行性分析、与现有传输抽象的契合点、诚实代价与**门控分阶段计划**。重活(实际接入
> I2P 路由 / DHT)一律 Phase 0 探雷先行,**不在本文落地**。参照 [设计-tor-onion-传输.md](设计-tor-onion-传输.md)。

## 背景

继 Tor onion 之后,设想再加入 **I2P** 或 **torrent(BitTorrent)** 作为新互联方式,继续往
"网络环境复杂时也能连、且尽量去服务器"的方向走。

但这两者**定位完全不同**,先把概念辨析清楚,否则会做错:

| | 它是什么 | 在 LinkHub 里扮演 |
|---|---|---|
| **I2P** | 匿名覆盖网(大蒜路由),有自己的地址体系(`.b32.i2p` 目的地 = 密钥的确定性函数) | **一条传输**——Tor onion 的兄弟。opt-in 匿名 + 穿 NAT,跑在现有 Noise KK 之上。 |
| **BitTorrent** | P2P 文件分发协议 + **Mainline DHT**(去中心化的 Kademlia 节点发现) | **一个会合/发现机制**——不是传输。我们只要它的 **DHT** 来无服务器地"找到对方的 IP:端口",**不要**它的分片/swarm 传输(我们有自己的 Noise 直传)。 |

**所以本文给两个不同的结论**:I2P 当传输接(像 onion);BitTorrent 的 DHT 当"无服务器会合
provider"接(像 mDNS 之于局域网、signaling-server 之于广域),喂给现有传输,**不新增 TransportKind**。

---

## 一、I2P:Tor onion 的兄弟传输

### 工作方式与架构契合

I2P 用**大蒜路由**(garlic routing)做双向匿名隧道。每个服务有一个 **目的地(Destination)**,
其 `.b32.i2p` 短地址 = 目的地公钥的 SHA-256 的 base32。**这和 onion 的"地址=身份"是同一个故事**:

- 从设备 ed25519 身份**域分隔派生**一对 I2P 目的地密钥(不复用签名密钥,复用 [identity/onion.rs](../../core-rs/src/identity/onion.rs) 同款 HKDF 思路)。
- `.b32.i2p` 随身份在配对时交换、存进 trust store(与 onion 地址并排,trust store 记录字段已是可选扩展式)。
- 重连直接拨 `.b32.i2p`,零服务器零查找——**已配对设备的会合天然解决**,与 onion 完全一致。
- 传输层照搬:I2P 的流是又一条同步 `Read+Write`,套现有 `*_over`(泛型 `Write+BufRead`)+ 同步桥(类比 `OnionStreamDuplex` / `DataChannelDuplex`),跑现有 Noise KK。`tor` 怎么 feature-gate,`i2p` 就怎么来。

### 关键风险(与 Tor 的本质差别)—— 必须 Phase 0 验

Tor 有 **Arti**:纯 Rust、可**嵌入进程**、能交叉编译进 `.so`,所以 onion 能"装进 App"。
**I2P 没有等价物**:

| | Tor(Arti) | I2P |
|---|---|---|
| 纯 Rust 嵌入式实现 | ✅ arti-client | ❌ **没有**。生产级路由是 **i2pd**(C++)或 **Java I2P** |
| 接入方式 | 进程内 API | **SAMv3 / I2CP** 协议,连一个**外部运行的路由进程** |
| Rust 生态 | 成熟 | 只有 SAM 客户端库(如 `i2p` crate),**不含路由** |

**后果**:I2P 要求**设备上有一个常驻 I2P 路由**(i2pd / Java I2P)。桌面端可以随 App 带一个
i2pd 可执行文件(类似 Tor 当初考虑 `tor.exe`/`lyrebird.exe` 网桥);**移动端是硬伤**——
Android 要嵌 i2pd(NDK 编 C++,可行但重)或依赖用户装第三方 I2P App;iOS 后台 + 嵌入限制基本堵死。

> **Phase 0 探雷重点**:① Rust 用 SAMv3 连 i2pd、建 streaming session、`.b32` 目的地能否从身份种子确定性派生;② i2pd 能否交叉编译/打包进 Android(或退化为"仅桌面 I2P");③ 首次隧道建立时延(I2P 比 Tor 还慢)。

### I2P vs Tor onion 速览

| 维度 | Tor onion | I2P |
|---|---|---|
| 匿名 | ✅ | ✅(双向匿名,设计上更适合 P2P) |
| 嵌入式可行性 | ✅ 全平台 | ⚠️ 桌面可带路由,**移动端难** |
| 速度 | 慢 | 更慢(隧道更多) |
| 地址=身份 | ✅ v3 onion | ✅ `.b32.i2p` |
| NAT 穿透 | ✅ | ✅ |
| 生态成熟度 | 高(Arti) | 中(无嵌入式 Rust 路由) |

**判断**:I2P 在架构上和 onion 一样干净(地址=身份、套 Noise KK、feature-gate),但**落地代价更高**
(需外接路由,移动端尤其),适合**桌面优先**、作为 Tor 之外的第二条匿名兜底。**不抢在 Tor 真机
验证之前做。**

---

## 二、BitTorrent:DHT 当"无服务器会合",不是传输

### 我们要的 / 不要的

- ❌ **不要** BitTorrent 的分片下载 / swarm / piece 交换——LinkHub 是两台设备的 E2E 直传,有自己的 Noise 协议,套 torrent 的 swarm 模型是南辕北辙。
- ✅ **只要** **Mainline DHT**(BEP 5,Kademlia):一张全球去中心化的"谁在哪"的表。

### 工作方式

1. 配对双方从**共享密钥/双方身份**确定性派生一个稳定的 **rendezvous key**(20 字节,充当 DHT 的 infohash;域分隔,不泄露身份)。
2. 各自在 Mainline DHT 上 `announce_peer(infohash, my_ip:port)`,并 `get_peers(infohash)` 查对方。
3. 拿到对方 `IP:端口` → **直连 + NAT 打洞**(uTP/μTP 或 STUN 式) → 跑现有 Noise KK。
4. 想存地址也可用 **BEP 44**(DHT 上存可变小数据,按公钥寻址)放一个"当前 IP/端口/能力"记录。

**定位**:这是**发现/会合层**,和 [mdns_runtime.rs](../../core-rs/src/mdns_runtime.rs)(局域网)、
signaling-server(广域)平级——产出一个 `DiscoveryEndpoint`/地址,**之后交给现有传输**(LAN 直连 /
WebRTC 数据通道)。**不是 `TransportKind`**。

### 价值与代价

**补的空档**:现在广域会合靠 signaling-server(要自建/信任一台服务器)。BitTorrent-DHT 给一条
**完全无服务器**的广域会合路径——补在 WebRTC(要信令)和 Tor(慢、匿名)之间。

**诚实代价**:

- **无匿名**:DHT 上 announce 会**暴露你的 IP**,且 infohash 的 announce 可被第三方观测(知道"某两个节点在约会")。要弱化得在 infohash 派生上做隐私处理(轮换、BEP 44 加密载荷),但 IP 暴露免不了。
- **要 NAT 打洞**:DHT 只给地址,真正连上还得 uTP 打洞 / fallback 中继,这块复杂度不低(WebRTC 之所以重就是因为它把打洞做全了)。
- **DHT 投毒/女巫**:Mainline DHT 是开放的,会合记录可被污染;需校验(反正最终 Noise KK fail-closed,投毒只能 DoS 不能冒充)。
- **移动端后台受限**:DHT 要长期在线维护路由表,移动端后台不友好(同 Tor/onion 的结论:限前台/活跃时)。

**Rust 生态**:Mainline DHT 有现成 crate(如 `mainline`,支持 BEP 44),纯 Rust、可嵌入、能进 `.so`——
**比 I2P 的落地容易得多**。

---

## 三、推荐与门控计划

### 推荐

1. **BitTorrent-DHT 作为"无服务器广域会合 provider"** —— 性价比最高的一条:纯 Rust 可嵌入、补的正是
   "不想自建信令服务器"的真实空档。定位成**发现层**,产出地址喂给现有 LAN/WebRTC 传输。优先级 **高于 I2P**。
2. **I2P 作为 Tor 之外的第二条匿名传输** —— 架构干净,但需外接路由、移动端难,**桌面优先**,优先级在 BitTorrent-DHT 之后、且**排在 Tor 真机验证之后**。

### 分阶段(每阶段一 commit,过完整矩阵 + 各自 feature 的构建/测试;不碰 v2 配对加密;首次接触 accept 回调 fail-closed 不弱化)

**A. BitTorrent-DHT 会合**
- A0 探雷:`mainline` crate 连公网 DHT、announce/get_peers 跑通、双 ABI 编译 + `.so` 增量;rendezvous key 确定性派生 + 隐私评估。
- A1(默认构建,不依赖 DHT):`rendezvous_key` 派生(域分隔,纯算)+ 单测;`DiscoveryEndpoint` 增加来源标记。
- A2(新 `dht` feature):DHT 会合 provider(announce/lookup),产出地址进现有发现注册表。
- A3:NAT 打洞(uTP / STUN fallback / 中继兜底);**门控**在 A0 + 打洞 spike。
- A4:平台壳(桌面优先;移动端限前台)。

**B. I2P 传输**(排在 Tor 真机验证之后)
- B0 探雷:SAMv3 连 i2pd、`.b32` 从身份派生、i2pd 能否进 Android;首次隧道时延。
- B1(默认构建):I2P 目的地 / `.b32.i2p` 地址派生(纯算,复用 onion 派生思路)+ 单测 + trust store 记录。**本次已可做的就是这一层的抽象脚手架**(见下)。
- B2(新 `i2p` feature):I2P streaming 同步桥 `I2pStreamDuplex`,镜像 `OnionStreamDuplex`。
- B3:接进 `connection_plan`(`ConnectionPath::I2p`)。
- B4:平台壳(桌面优先,移动端视 B0 结论)。

### 本次落地(抽象脚手架,device-free)

为给 I2P 留好插槽(和当初给 onion 留 `TransportKind::Onion` 一样),本次在**默认构建**加:
`TransportKind::I2p`(Display/FromStr/健康分,介于 Onion 与 CloudRelay 之间)、`ConnectionPath::I2p`、
`PeerReachability.i2p_addr`,并排进 `plan_connection` 自动兜底顺序(LAN → WebRTC → Onion → I2P → Relay)。
**不引入任何 I2P 依赖**,纯路由元数据 + 单测。BitTorrent-DHT 因为是**发现层不是传输**,本次**不动枚举**,
按上面的 A 计划单独做。

---

## 诚实边界(写死,避免后续踩)

- **I2P 没有 Arti 那样的嵌入式 Rust 路由**——必须外接 i2pd/Java I2P,移动端是硬伤。别假设"像 onion 一样装进 App"。
- **BitTorrent-DHT 不匿名**——会暴露 IP,且只是"会合",连上还得自己打洞。它替代的是**信令服务器**,不是 Tor。
- 两者都**不解决"发现身边陌生设备"**(那永远是 LAN/mDNS 的活),也都**首次接触需带外交换**(地址/会合密钥)。
- **移动端后台**:DHT/I2P 都要长期在线,移动端限前台/活跃时,后台不硬保活(同 onion 结论)。
- **门控**:I2P 的 B 阶段、BitTorrent 的 A3 打洞,都要 Phase 0 探雷过了才做。
