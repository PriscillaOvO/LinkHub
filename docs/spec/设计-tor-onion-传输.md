# 设计 / 探雷:Tor onion 传输(Phase 0 结论)

> 状态:**Phase 0 探雷完成（2026-06-21）**。本文记录隔离 spike 的实测结果与 go/no-go。
> spike 工程在仓库外（`C:\Dev\tor-spike`，不入主仓），本文是唯一留存的产出。

## 背景与目标

LinkHub 跨网现在依赖 signaling-server + STUN + TURN。设想加入 **Tor onion 传输**，
让设备在**网络环境复杂时也能匿名互联**，并作为一种新互联方式。核心顾虑是"两台设备
怎么在 Tor 上无感找到对方"。

**关键洞察（架构契合）**：v3 onion 地址 = ed25519 公钥的确定性函数，而 LinkHub 设备身份
本来就是 ed25519。所以**已配对设备**的会合天然解决——onion 地址随身份在配对时交换、存进
trust store，重连直接拨，零服务器零查找。传输层也已抽象（`*_over` 泛型 `Write+BufRead`
+ `DataChannelDuplex` 同步桥），onion 流只是又一条 `Read+Write`，照跑现有 Noise KK。

Phase 0 不写产品代码，只用最小代价回答"这条路能不能走 + 代价多大"。

## 测了什么 / 怎么测

隔离 spike：`arti-client 0.43.0`（`default-features=false`，启用 `rustls`/`tokio`/
`compression`/`onion-service-client`/`onion-service-service`）+ `tor-hsservice`/`tor-cell`/
`tor-hscrypto`/`safelog`。一个 cdylib（量 `.so`，用导出符号保住真实 host+client 调用图，
否则 `--gc-sections` 会把没被导出符号可达的代码全删掉、体积失真）+ 一个 e2e bin
（起 onion 服务 → 自连走一圈 Tor → 回显 64 KiB → 校验 SHA-256）。

## 结果

| 项 | 结果 | 说明 |
|---|---|---|
| **API 可用性** | ✅ | host 端 `launch_onion_service` → `(RunningOnionService, rend_requests)`；地址用 `HsId::display_unredacted()`（Arti 故意不给 `Display`，防日志泄露）；接入流 `handle_rend_requests` → `StreamRequest::accept(Connected::new_empty())` → `DataStream`；client 端 `TorClient::connect("<onion>:port")`。全部编过并实跑。 |
| **宿主编译** | ✅ | 整棵 Arti 树（**495 个包**）在 Windows 宿主 `cargo check` 全绿。 |
| **双 ABI 交叉编译** | ✅ | `cargo ndk -t arm64-v8a -t x86_64`，连同全部 C 依赖（sqlite/lzma/zstd/ring）用 NDK 28.2 编译 + **链接**成功。 |
| **`.so` 体积（stripped+LTO+opt-z）** | ✅ 可接受 | **arm64-v8a 7.61 MB / x86_64 8.56 MB**（独立 lib）。参照:webrtc feature 当初 +8.4 MB/ABI → **Tor 与 WebRTC 同量级**，叠加到 core 上因共享 tokio/ed25519/x25519/sha2 会略少。基线精简 core ≈ 1.5 MB(arm64)。 |
| **e2e（真连 Tor）** | ⚠️ **本网络封锁，未能验证** | crypto provider 修好后真正打网络，**Tor bootstrap 90s 超时**。本机网络（含已切换的网络）对 Tor 直连审查/封锁,裸 Tor 连不出，需 bridges/可插拔传输。**这是环境/审查问题，不是代码问题**——e2e bin 本身编过、跑通到 bootstrap 边界。**加网桥后已可连(见 Phase 0.5:obfs4 实测 100% bootstrap)**。 |

## 集成时必须注意（实测踩出来的）

1. **TLS 用 `rustls` 不用 `native-tls`**——否则交叉编译会拖 OpenSSL。
2. **Android 必须 `rusqlite/bundled`**——`tor-dirmgr` 的目录缓存拉 sqlite，NDK sysroot 没有
   `-lsqlite3`，要把 sqlite3.c 源码静态编进来（`check` 不报、只在**链接**期暴露 → Phase 0
   必须真做 `.so` 构建，不能只信 `check`）。
3. **启动要装一次 rustls CryptoProvider**——`rustls::crypto::ring::default_provider().install_default()`，
   否则 rustls 0.23 运行时 panic。
4. **首次启动慢 + 体积**——Arti 自带目录管理、磁盘 keystore/state-dir。

## 会合方式（已定:仅已配对设备）

onion 地址 = 身份 ed25519 的确定性函数。Phase 1 从设备身份 HKDF 派生独立 hs 密钥、算出
onion 地址，随 `IDENTITY` 交换并存 trust store；重连直接拨。**首次接触仍走同网 mDNS/扫码**，
不在 Tor 上做陌生设备发现。

## go / no-go

**技术集成层面:GO。** 三大风险（编译 / 双 ABI 链接 / 体积）全部清掉，API 形态确认,
架构契合度高（地址即身份、跑在 Noise KK 之上、传输已抽象）。

**但定位要修正 —— 一个硬约束写死在这:**

- Tor onion 适合**非审查网络**下的匿名 + 穿 NAT,集成代价约等于 WebRTC（opt-in、~7–9 MB/ABI）。
- **它不是"在中国也能开箱即连"的方案**:裸 Tor 在本机网络 bootstrap 失败,审查网络要 bridges
  (obfs4/snowflake/meek)。而 bridges (a) 破坏零配置"无感",(b) 会被封、要轮换,(c) 引入
  `tor-ptmgr` + 可插拔传输的额外复杂度,是场军备竞赛。

## Phase 0.5 实测:bridge/可插拔传输能否从国内 bootstrap（2026-06-21）

用 tor-expert-bundle 15.0.16 的 `tor.exe` + `lyrebird.exe`（新版 lyrebird 已内置 snowflake，
不再需要单独的 snowflake-client）实测，**回答"裸 Tor 被封时,加网桥能不能从这个国内网络连出去"**。
（先用 C-tor 验网络层:能出去则 Arti+同一 PT 必然也能,出不去则都不能——网络结论等价。）

| 传输 | 结果 | 说明 |
|---|---|---|
| **裸 Tor（无网桥）** | ❌ bootstrap 90s 超时 | Tor 协议直连被封。 |
| **obfs4** | ✅ **Bootstrapped 100% (done)**，约 2–3 分钟 | 7 个公共网桥里 ~2 个可用（`torfnase` 212.83.43.74 / `freenator` 212.83.43.95）,其余被封(如 146.57.248.225 SOCKS 失败)。**够用即跑通**。 |
| **snowflake** | ⚠️ 逃出审查、连上 Tor(到 50% 共识),但**太慢/不稳,窗口内未到 100%** | 域前置 broker 找到志愿者代理(`broker rendezvous peer received`),已下到 networkstatus 共识,卡在 descriptor 加载——是吞吐问题不是被封。snowflake 速度本就飘忽。 |

**结论:抗审查卖点成立——Tor 能从国内完整 bootstrap,前提是配网桥(obfs4 已实测 100%)。**
但有现实代价(写死):
- **要可用网桥**:公共 obfs4 网桥部分被封,得有还活着的;长期要能拿到/轮换网桥(BridgeDB/moat,
  国内获取本身也不易)。**这破坏零配置"无感"** —— 至少首次要带上一批网桥。
- **慢**:snowflake 慢且不稳(本次没跑满);obfs4 完整 bootstrap 也要 2–3 分钟。叠加 onion 自身
  延迟,**只适合文字/小文件兜底,不适合大文件**。
- **lyrebird.exe ~17 MB**:PT 是独立可执行,Android/桌面都要随包带(又一块体积)。
- **e2e 字节回环**:本次以"tor 能否 100% bootstrap"为决定性指标已达成;onion 数据回环可在
  Phase 2 用 Arti pt-client 接好后跑(Arti 自身测试已覆盖 onion 传输正确性)。

## 最终 go / no-go

- **技术可行性:GO。** 编译 / 双 ABI 链接 / 体积 / API / 国内可 bootstrap(带网桥)全部验证通过。
- **定位(写死)**:Tor = **opt-in 的隐私增强 + 无基础设施跨网兜底**;在审查网络(国内)需随包带
  PT(lyrebird)+ 一批网桥,**不是零配置**,且只适合**文字/小文件**。非审查网络下开箱即用、纯匿名/穿 NAT。
## Phase 0.5 (B) 实测:Arti 自己的 pt-client(不只是 C-tor)

spike 加 `pt-client` feature,用 `TorClientConfig` 配 obfs4 网桥(实测能用的那几条)+ `TransportConfigBuilder`
指向 lyrebird.exe,`TorClient::create_bootstrapped`。结果:

| 项 | 结果 |
|---|---|
| **Arti + obfs4 bootstrap(国内）** | ✅ `*** ARTI BOOTSTRAPPED over obfs4 from this network in 6.8s ***`（复用缓存状态后极快）。**OUR 栈(Arti)而非只是 C-tor,确认能从国内带网桥连出去。** |
| **onion 服务起 + 取地址** | ✅ host 侧初始化成功,拿到 `…rqd.onion`。 |
| **onion 字节回环(自连)** | ❌ **未跑通**:`Failed to obtain hidden service circuit`,即便等 75s 发布描述符 + 重试 12 次仍失败。 |

**诚实结论(写死)**:**Arti 带网桥从国内 bootstrap = 已证;但 onion 数据回环在本机这套环境始终没跑通**——
裸 Tor 被审查(连不出),加 obfs4 后 HS 电路又建不起来。两个可能原因:(1) spike 是**单进程自连自己的
onion**(非典型场景,Arti/Tor 对此本就别扭);(2) 只有 1–2 个可用 obfs4 网桥,**撑不起 onion 服务所需的
多条电路**(intro point + 描述符上传 + rendezvous)。真实场景是两台独立设备、各自 Tor、接收端早把描述符
发布到 DHT,约束不同;且 Arti 自身测试套件已覆盖健康网络下的 onion 传输正确性。

**因此 onion-over-Tor 的端到端字节路径,需要在以下任一条件下再确认一次**:换非审查出口网络 / 更多更健康的
网桥 / 两台真实设备。**集成代码路径无问题,卡的是国内审查网络 + 少量网桥下 HS 电路的现实可行性。**

- **未做(留给 Phase 2)**:onion 字节回环在上述更现实条件下的一次性确认。
