# Claude 工作日志（worklog）

> 这是 **Claude 的个人流水账**：每次会话收尾时在最上方追加一条，记录「做了什么 / 为什么 / 改了哪里 / 怎么验证的 / 下一步」。
> 目的：会话上下文太长被截断后，仍能从这里回溯历史，不必重新摸索整个仓库。
>
> 与其它交接文件的分工（别重复写）：
> - 本文件 = 我自己的逐次流水（最细，倒序，最新在最上面）。
> - `../claude-to-codex/latest.md` = 给 Codex 的定向交接（它下一步需要知道什么）。
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

## 2026-06-24 ~03:50 +08:00 — 换机收尾：全量合并推送 + 仓库自包含化 + 从零部署指南

- 做了：用户要重装这台开发机，仓库随后转私有，目标「新机器 clone 后照文档直接重部署，后续 AI 拉取即知全貌」。无功能代码改动，纯收尾 + 文档。
  - **全量合并推送**：`feat/transport-abstraction-m1`(42 commit) 已快进合并进 `main` 并推；前序还单独提交了根 `README.md` 刷新(`b493f3e`)。两分支与 origin 完全同步、工作区干净。
  - **仓库自包含化（关键决策，用户在 AskUserQuestion 里都选「提交进仓库」）**：把两样原本 gitignore、只在本机、重装即丢的东西纳入跟踪——
    - Android 签名密钥库 `android/app/linkhub-release.jks` + `keystore.properties`（dev/test，口令 `linkhub-dev-2026`）。改 `.gitignore`：保留通用 `*.jks`/`*.keystore` 忽略，加 `!` 例外放行这一把 + keystore.properties。**理由**：Android 要同一签名才能给已装 App 推更新，随仓库带走免换机失配。⚠️上架/转公开须轮换+清史。
    - `docs/ai-handoff/`（交接/决策/已知风险/worklog ~194K）。删 `.gitignore` 里的 `docs/ai-handoff/` 整体忽略。`worklog/sessions.log` 仍被通用 `*.log` 命中（无妨，实质历史在本文件）。
  - **`环境部署.md` 整篇重写**为「从零拉取→跑通」指南：工具链清单+版本基线表（Rust/target、VS C++ BuildTools、JDK17、Android SDK/NDK 28.2.13676358、cargo-ndk、Tauri CLI v2；**桌面端无需 Node**——`frontendDist=../src` 纯静态、无 `beforeBuildCommand`）、环境变量、**clone 后需重建的本地文件**（`local.properties` 模板、`.so` 重生成）、各组件构建/验证命令、可选 webrtc/tor feature、验证基线表。
  - **文档导航**：`docs/README.md` 加换机入口指引 + 阶段5设计文档区 + AI 交接区；`项目状态.md` 顶部加本轮小结、标题日期改 2026-06-24。
- 结构审查（无问题）：194→现 +密钥库+交接 文件；无误提交 `target/`/`.so`/`.apk`；`git check-ignore` 确认 `local.properties` 与 `jniLibs/**.so` 仍正确忽略；最大文件为 Cargo.lock + 两张截图 PNG。
- 改动：`.gitignore`；`docs/spec/{环境部署,项目状态}.md`；`docs/README.md`；根 `README.md`(前序 `b493f3e`)；纳入 `android/app/linkhub-release.jks`+`android/keystore.properties`+`docs/ai-handoff/**`。
- 验证：纯文档/配置，无测试影响；分支同步与忽略行为已用 git 命令核对。
- 下一步（新机器上）：照 `环境部署.md` 装工具链 → `local.properties` → `build-android-so.ps1` → 各组件 `cargo test`/`gradlew assembleDebug`/`cargo tauri dev`。真机 onion-over-Tor、公网信令跨网验收仍门控真机。
- commit：本轮待提交（合并 + 文档）。前序 `b493f3e`(README) 已 push。

## 2026-06-23 ~18:50 +08:00 — 真机互传跑通（家里 WiFi 真实网络，非 USB 隧道）+ 桌面/安卓 UI 二次美化 + CLI 补 dh

- 背景：用户回家、手机连家里热点、电脑↔手机同一 `/24`（电脑 192.168.31.60 / 手机 192.168.31.35）。电脑 ProtonVPN 关不掉。承接夜间没做完的 UI 美化 + 终于能做的真机验证。
- **网络诊断（关键，写给后人）**：酒店那次失败 = **酒店 AP 设备隔离**（两端都能 ping 网关、互相不通）。回家后：**电脑→手机 ping 通（51–138ms）**，但**手机→电脑不通**（Windows 防火墙/ProtonVPN 挡入站）。结论：只有 **电脑发起→手机** 这个方向通；要做真机互传必须 **手机当接收端、电脑当发送端**。mDNS 自动发现大概率因入站被挡而不可靠。
- **① 真机互传验证（核心成果，无代码改动）**：交叉编译 `linkhub-cli` 给 `aarch64-linux-android`（`cargo ndk -p 26`——bin 链接需 API≥24 才有 `getifaddrs`，`.so` 不暴露因共享库容许未定义符号；cargo-ndk 链接后报 panic 是 report-gen 既知 quirk，二进制已产出）。push 到手机 `/data/local/tmp`，手机当 listener、电脑当 sender，**走真实家里 WiFi（非 USB 隧道）**：
  - CLI 在真机 ARM64 跑起来（no-arg demo 场景执行）。
  - **基础 `send-file` 1MB → 手机**：256 块全 ACK + FILE_END，手机 `sha256sum` == 源 `04e532a1…65cf` ✓
  - **认证 `send-file-auth` 1MB（Noise KK 握手 + 加密会话）**：`AUTH_OK` → `Noise KK handshake complete` → 收齐，SHA256 == 源 ✓。**这条就是 app 用的端到端加密通道**，在真机 + 真实网络上验通。
  - 验证用 CLI 两端手搓：`identity init` 各生成身份，手搓两份互信 trust store（`device=HEX(id)|HEX(name)|HEX(pubkeyStr)|HEX(dhStr)|0`，双 hex 编码），push 手机身份+trust，`listen-auth`/`send-file-auth`。清理了手机 `/data/local/tmp`。
- **② CLI 补 dh（commit `9be6517`）**：`identity show` 原来只打 ed25519 公钥,不打 X25519 DH 公钥——而手搓 trust store 正缺这个。`print_local_identity` 加 `dh_public_key`。
- **③ 桌面 UI 二次美化（commit `a5827ac`，纯 CSS）**：在既有视觉系统上加高级感——缓动漂移的环境光网格背景、CSS 画的渐变 logo 方块 + 微动标题流光、卡片渐变顶边 + 错峰级联入场、主按钮扫光；圆角略增；全部 `prefers-reduced-motion` 兜底。用 headless Edge 渲染代表性静态预览截图确认暗色主题成品（卡片/确认码面板/设备行）。
- **④ 安卓 UI 二次美化（commit `019898d`）**：底栏 emoji（📷💻📤🧾⚙）换成 Material 矢量图标（QrCodeScanner/Devices/Send/History/Tune）；顶栏改 surface 背景 + 靛蓝→紫罗兰渐变品牌方块 + 加粗标题；配对页扁平 `Text+Divider` 分区重组成圆角 `SectionCard`（`material-icons-extended` 已是依赖）。`gradlew assembleDebug` BUILD SUCCESSFUL（纯 Kotlin，`.so` 不动）→ 装真机 + 截图确认新界面。
- **校准（写给后人）**：夜间 worklog 里的 `9030a06`（mDNS onion）、`50a22a9`（I2P 脚手架）等本会话开始前已在分支上；我一度重做 mDNS onion 写了重复测试，已 `git checkout` 丢弃，工作区干净。
- **现状/缺口**：手机装了**最新 app**（019898d）；**电脑没装桌面 app**（Tauri 没构建，我只编了 CLI）。所以"在两端 app 界面上手动点发送"那条 UI 全流程还没法 DIY——需要先构建+跑桌面 app，且要处理入站防火墙（手机→电脑）。真机互传我是用 CLI 两端手搓验的（结果确定），**app UI 扫码配对→点发送**那条端到端流程仍未在真机走过。
- 验证：默认 `cargo test --frozen` **159** 绿（丢弃重复测试后）；桌面 CSS 预览渲染 OK；安卓 assembleDebug OK；真机互传 SHA 双双一致。
- 改动：`core-rs/src/main.rs`（dh）；`desktop/src/css/style.css`；`android/.../MainActivity.kt` + `ui/PairScreen.kt`。
- commit：`a5827ac`/`019898d`/`9be6517`（本会话）。`30bb5f2`/`9030a06`/`f513107`/`5a157af`/`50a22a9`/`014dd6a` 为会话开始前已在分支。**全部未 push**。
- 下一步：①（用户想 DIY）构建+跑桌面 app + 开 Windows 防火墙入站（手机→电脑）→ app UI 扫码配对 + 点发送走真机全流程；② 真机互传换"手机热点 + ProtonVPN 允许局域网"试自动 mDNS 发现；③ push 待用户点头。

## 2026-06-21 ~05:30 +08:00 — 夜间自驱：Tor Phase 4 device-free 切片 + mDNS onion + 桌面/安卓 UI 美化 + I2P/torrent 评估

- 背景：用户真机暂不可用，让我"按自己的计划直接跑、不用问"。我只挑**不依赖设备、能在宿主/模拟器/单测验证**的活；Tor/I2P 重活继续门控真机/探雷。先清磁盘释放 ~39.5 GB（探雷 spike target 10.8 + `core-rs/target` 21.2 + `desktop/target` 7.5，全是可重编的 `target/`）。
- **关键基建坑（写给后人）**：清了 `target` 后，默认 `cargo test` 因为要**解析** Cargo.toml 里的 optional Arti deps（即便不编译）去拉 USTC 镜像 sparse 索引，curl 反复失败。解法：**`--frozen`（= offline + locked）走 Cargo.lock + 本地缓存,不碰网络**——三个镜像索引(index.crates.io / ustc / rsproxy)都已缓存 Arti 条目。注意:`core-rs/.cargo/config.toml`(USTC 镜像)按 **CWD** 生效——在 core-rs 里跑桌面 check 会用 ustc 镜像(缺桌面专属 `qrcode`)→ **桌面 check 要在 `desktop/src-tauri` 目录跑**(用默认 crates.io 缓存)。
- **① Tor Phase 4 device-free 切片(commit `30bb5f2`)**:对端 `.onion` 进首次接触 `IDENTITY`(可选**末尾**字段,6 段=v1 无 onion / 7 段=带)+ `IncomingPeer` + trust store(可选**第 6** 管道字段,5 段=v1)。`WireMessage::Identity` 加 `onion_address: Option<String>`;`DeviceIdentity` 加 `onion_address` + `with_onion_address`(空白→None);发起端 `perform_initiator_handshake` 发自己 onion,响应端 `resolve_first_contact_identity` 收下挂到身份 + IncomingPeer。桌面 `IncomingPeerPromptPayload`/`identity_from_prompt_payload` 透传持久化。**地址咨询性、不进绑定签名**——伪造只把拨号指向错 onion 让 Noise KK fail-closed,真正认证仍是静态密钥 KK。+4 测试(协议往返带/不带 onion、legacy 6 段、trust store 往返 + legacy 5 段)。
- **② mDNS onion 传播(commit `9030a06`)**:`MdnsAdvertisement`/`DiscoveryEndpoint` 加 `onion_address`,广播加可选 `onion=` TXT、`mdns_runtime` 解析键加 `onion`、`endpoint_with_srv_port` 带出。本机从签名密钥派生(`from_local_identity`);无 onion 的对端略过该字段(既有 7 记录广播不变)。意义:局域网发现也存对端 onion,供日后异网经 Tor 重连(不止 AirDrop IDENTITY 那条)。+2 测试。
- **③ 桌面 UI 美化(commit `f513107`,纯 CSS,标记/JS 不动)**:设计 token 系统 + 亮/暗主题(`prefers-color-scheme`)、毛玻璃 sticky 头/标签/底栏、渐变品牌字、动画标签下划线、卡片 hover 抬升、渐变主按钮 + 聚焦环、脉冲状态点、消息滑入/内容淡入、自定义滚动条、`prefers-reduced-motion` 兜底。`.active` 切换契约与所有类名全留。验证:CSS 括号平衡 + `node --check` 全 JS 过。
- **④ 安卓 UI 美化(commit `5a157af`)**:`LinkHubTheme` 裸 `lightColorScheme()` → 明确 Material 3 品牌明/暗配色(靛蓝→紫罗兰,与桌面同款)+ `isSystemInDarkTheme()` 跟随系统;标签切换 `AnimatedContent` 淡入淡出。验证:`:app:compileDebugKotlin --offline` BUILD SUCCESSFUL(deps 已缓存,17s/4s)。
- **⑤ I2P/torrent 评估 + I2P 脚手架(commit `50a22a9`)**:新 `docs/spec/设计-i2p-与-torrent-传输.md` 厘清——**I2P = 匿名传输(onion 兄弟,地址=身份)**;**BitTorrent 只取 Mainline DHT 当无服务器会合(非传输/非匿名/需打洞)**。诚实代价:I2P **无可嵌入 Rust 路由**(不像 Arti 之于 Tor,需外接 i2pd,移动端硬伤);DHT 暴露 IP。device-free 落地 I2P 抽象脚手架(镜像 onion 插槽):`TransportKind::I2p`(分 460)、`ConnectionPath::I2p`、`PeerReachability.i2p_addr`、计划顺序 LAN→WebRTC→Onion→I2P→Relay;改了 `device.rs` Degraded 臂 + 桌面 `connection_plan` 命令。**不拉 I2P 依赖**,真传输门控 Phase 0。BitTorrent 仅文档(发现层非 TransportKind)。+1 测试。
- **⑥ docs(commit `014dd6a`)**:`项目状态.md` + `开发路线图.md` 顶部加本轮小结。
- 验证(全绿,离线 `--frozen`):`fmt`;默认 `cargo test` **159**;`clippy --all-targets`(默认 + `--features webrtc`)`-D warnings`;`cargo ndk -t arm64-v8a -t x86_64 check --lib` 双 ABI;桌面 `cargo check`;安卓 `compileDebugKotlin`。默认 `.so` 仍精简、无 Arti/I2P 依赖。
- 改动:`core-rs/src/{net/protocol,net/auth_session,net/auth_listener,identity,identity/device_identity,identity/trust_store,discovery,mdns_runtime,transport,device,net/connection_plan}.rs`;`desktop/src-tauri/src/main.rs`、`desktop/src/css/style.css`;`android/.../MainActivity.kt`;`docs/spec/{设计-i2p-与-torrent-传输.md(新),项目状态.md,开发路线图.md}`。
- commit:`30bb5f2`/`9030a06`/`f513107`/`5a157af`/`50a22a9`/`014dd6a`。**全部未 push**(用户历来用"push 吧"做检查点;早前 `2b8e568` 及之前已 push)。
- 下一步(等用户/真机):Tor Phase 4 平台壳剩余(安卓 JNI surface onion + Tor UI 开关、移动端自动 Onion 限前台)、真机验 onion-over-Tor 数据路径;I2P B0 探雷(SAMv3+i2pd 能否进安卓);BitTorrent A0 探雷(`mainline` crate)。push 待用户点头。

## 2026-06-21 ~03:30 +08:00 — Tor onion 传输 Phase 2（Arti 传输）+ Phase 3（接线+CLI）

- 背景：用户说"那你现在搞 phase2 3"——推翻之前的门控,要求把真正的 Arti 传输接进项目。
- **先在 spike 锁死关键 API/不变式**(Arti 缓存、迭代快):`HsIdKeypair::from(ExpandedKeypair::from(&Keypair::from_bytes(seed)))` 构造 host 密钥,其 onion 地址 == 纯公钥派生(Phase 1)——**"我 host 的地址 == 算出的地址"不变式成立**。还查明 `launch_onion_service_with_hsid` 需 `experimental-api`。
- **Phase 2(commit `5cad6c4`)** 新 `core-rs/src/net/tor_transport.rs`(`tor` feature,默认不拉 Arti):
  - `OnionStreamDuplex`——Arti 异步 onion `DataStream` 经 `tokio::io::split` + 后台读泵(VecDeque+condvar)/`handle.block_on` 写,桥成同步 `Read+Write`,**镜像 webrtc 的 `DataChannelDuplex`**,现有 `run_authenticated_*_over` 原样跑。
  - `TorContext::{bootstrap(可选 `BridgeSettings`=obfs4 网桥行+PT 路径+协议), connect_onion, host_onion}` + `OnionListener`(阻塞 `accept()`)。host 用 `launch_onion_service_with_hsid` + 身份种子构造的 `HsIdKeypair` 在**身份派生地址**起服务。装一次 rustls ring provider;`rusqlite/bundled`。
  - `Cargo.toml`:`tor` feature 门控 arti-client(rustls/onion-service-*/pt-client/experimental-api)+ tor-{rtcompat,hsservice,hscrypto,llcrypto,cell}+safelog+rustls+futures+rusqlite,全 optional;tokio 加 io-util/net。
  - **本地 USTC 镜像** `core-rs/.cargo/config.toml`(复用 spike 缓存的 Arti 树,免再下 200 包)——**gitignore、不提交**(加了 `.gitignore` 条目);Cargo.lock 仍 crates.io 源。
  - 验收:默认 fmt/test(150)/clippy + 默认双 ABI ndk **保持无 Arti、`.so` 精简**;`--features tor` clippy 干净;**`cargo ndk -t arm64-v8a -t x86_64 check --lib --features tor` 在项目里双 ABI 交叉编译通过**(Arti 全树为两个 Android target 编过)。
- **Phase 3(commit `8008468`)** 接线 + CLI:
  - `TransportKind::Onion`(Display/FromStr/score=500,LAN/WebRTC 之下 relay 之上;修了 `device.rs` 穷尽 match → onion 记 Degraded)、`ConnectionPath::Onion{addr}` + `PeerReachability.onion_addr`,`plan_connection` 顺序 LAN→WebRTC→Onion→relay(+2 测试);桌面 `connection_plan` 命令补 onion 臂(`onion_addr:None`,Phase 4 再接真实地址)。
  - `LocalIdentity::onion_hs_seed()`(host 用,默认构建)。
  - CLI `listen-tor <identity> <trust_store> [--receive-dir][--bridge…][--pt-binary][--pt-protocol]` / `connect-tor <peer_onion> <identity> <peer_id> <trust_store> <file> […网桥]`(`tor` feature + not-feature stubs)——bootstrap、host/拨 onion、跑现有 Noise KK。**仅已配对**(trust-store 鉴权);对端 .onion 作参数(无法由公钥反推 → 必须设备传给 peer)。
  - 验收:默认 fmt/test(**151**)/clippy + 默认双 ABI ndk 绿;`--features tor` 构建+clippy(含真实 CLI bin)干净;桌面 check + test(14) 绿。
- 关键设计点(写给后人):onion 地址从设备**私钥**派生 → peer 不能用对方公钥反推地址,**必须由设备传给 peer**(Phase 4 把它塞进 IDENTITY+trust store;CLI 暂用参数传)。
- 改动:`core-rs/Cargo.toml`、`Cargo.lock`、`src/net.rs`、`src/net/tor_transport.rs`(新)、`src/transport.rs`、`src/device.rs`、`src/net/connection_plan.rs`、`src/identity/device_identity.rs`、`src/main.rs`;`desktop/src-tauri/{Cargo.lock,src/main.rs}`;`.gitignore`;docs/spec 三文件。
- commit:`5cad6c4`(P2)、`8008468`(P3)、`2b8e568`(docs)。**均未 push**(P1 的 4acf27f/d1c106b 已 push)。
- 下一步:Phase 4/5 **门控真机 onion 数据路径验证**(spike 里单进程自连 onion 没跑通,是 HS 电路而非代码问题)。可先用 CLI `listen-tor`/`connect-tor` 在非审查网络/两台真机验回环。

## 2026-06-21 ~01:30 +08:00 — Tor onion 传输 Phase 0/0.5 探雷 + Phase 1（地址派生）

- 背景：用户想给项目加 **Tor 网络**做"网络复杂/匿名时也能互联"。我先在 plan 模式给了选项,用户选:Phase 0 探雷先行 / 仅已配对设备会合 / 桌面+Android 一起 / 自动兜底路径。
- **会合洞察(关键)**：v3 onion 地址 = ed25519 公钥的确定性函数,而本项目身份就是 ed25519 → 已配对设备的 onion 地址随身份交换+存 trust store,重连直接拨,零服务器零查找。传输层(`*_over` 泛型 + `DataChannelDuplex`)就是现成插槽。
- **Phase 0 探雷(隔离 spike,仓库外 `C:\Dev\tor-spike`,不入主仓)**：
  - Arti `arti-client 0.43`(rustls,非 native-tls)宿主编译 ✅;**双 ABI 交叉编译+链接 ✅**(arm64-v8a/x86_64,需 `rusqlite/bundled`——NDK 无 `-lsqlite3`,`check` 不暴露只在**链接**期暴露 → 必须真做 `.so` 构建);**stripped `.so` arm64 7.61MB / x86_64 8.56MB**(≈webrtc 的 +8.4MB/ABI);启动要装 `rustls::crypto::ring` provider 否则 panic。
  - host API:`launch_onion_service`→`(svc,rend_requests)`,地址 `HsId::display_unredacted()`,接入 `handle_rend_requests`→`StreamRequest::accept(Connected::new_empty())`→`DataStream`。
  - **e2e 裸 Tor bootstrap 90s 超时**——本机(国内)网络封 Tor。
- **Phase 0.5(用户选先验抗审查)**：下 tor-expert-bundle 15.0.16(`dist.torproject.org` 能下,Tor 协议被封但发布站能通),`lyrebird.exe` 已内置 snowflake。**obfs4 → Bootstrapped 100%**(7 个公共网桥 ~2 个活:`torfnase`/`freenator`);snowflake 逃出审查到 50% 但慢未满;**Arti 自己的 pt-client 用 obfs4 bootstrap 成功(6.8s)**。但 **onion 字节回环始终没跑通**(`Failed to obtain hidden service circuit`,等 75s 发布+重试 12 次仍失败)——疑似单进程自连 onion + 少量网桥撑不起 HS 多电路。如实写进设计文档。
- **结论(写死)**:技术集成 GO(编译/双ABI/体积/国内带网桥可 bootstrap 全过);定位 = **opt-in 隐私增强+无基础设施跨网兜底**,国内要带 PT+网桥(非零配置)、只配文字/小文件;**onion 数据路径需真机/非审查网络再确认**。
- **A 怎么铺(用户选)**:先做 Phase 1,Tor 重活(Phase 2-5)等真机验 onion-over-Tor。
- **Phase 1 done(纯 Rust,进默认构建,零 Arti 依赖)**：新 `identity/onion.rs`——从 ed25519 签名密钥 HKDF(SHA-256 域分隔 `linkhub-onion-hs-v1`,不复用签名密钥)派生 hs 密钥 → rend-spec-v3 地址(SHA3-256 校验 + 小写 base32 内联实现)。`LocalIdentity::onion_address()`。**对齐 Arti `tor_hscrypto` 参考向量**(pk=0..32 → `aaaqeayeaud…ead.onion`,先在 spike 用 tor-hscrypto 算出真值再 bake 进 core 测试)。加 `sha3` 默认依赖(双 ABI 交叉编译 ✅)。
- 改动:`core-rs/Cargo.toml`(+sha3)、`identity.rs`(+mod onion)、`identity/onion.rs`(新)、`identity/device_identity.rs`(+onion_address);新增 `docs/spec/设计-tor-onion-传输.md`(探雷结论 + go/no-go)。
- 验证(全绿):`cargo fmt --check`;默认 `cargo test`(**150**,+6 onion:参考向量/v3 形态/hs种子确定性+不复用/base32 已知值/地址稳定+格式/异签名异地址);`clippy --lib --all-targets`;**`cargo ndk -t arm64-v8a -t x86_64 check --lib` 双 ABI 绿**(sha3 交叉编译,默认 `.so` 仍精简)。
- commit:`4acf27f`(探雷文档)+`d1c106b`(Phase 1 代码)。**未 push**。
- 下一步(Tor 重活,等真机):Phase 2 `tor` feature 传输(`OnionStreamDuplex` 镜像 `DataChannelDuplex`)、Phase 3 `ConnectionPath::Onion` 编排、Phase 4 桌面+Android 壳、Phase 5 文档;**移动端自动用 Onion 兜底限前台/活跃时**(后台不硬保活,耗电)。spike 工程留在 `C:\Dev\tor-spike`(Arti 495 包 + tor-expert-bundle 已缓存,别再走慢网络重下)。

## 2026-06-20 ~04:00 +08:00 — 审查 Codex 的无感互联 B–E + 补提交 E + 收尾

- 背景：用户说"他（Codex）好像弄好了，你检查一下"。Codex 在 A(`9a4b6d4`) 之上提交了 C1/C2/B/D 四个 commit，但 **E(桌面)未提交**（工作区有桌面+discovery 改动）、且**没走收尾闭环**（handoff-clock / codex-to-claude 仍是旧的 C1–C6 内容）。
- 我做的检查：
  - **安全审查（重点）C1 JNI 接受回调**(`jni_bridge.rs` `make_accept_peer_callback`)：✅ **fail-closed**——JVM attach 失败/JNI 调用失败/有异常 都 `return false`(拒绝)；只有用户在 Kotlin `onIncomingPeer` 明确点接受才 true。A 的密码学验证在 core 里(回调前)；Codex 还加了 `verifyIdentityBinding` 让 Kotlin 二次验 + `webrtcSendFileToIdentity`(发送前校验对端 `device_id↔公钥`)。`auth_session.rs` 改动只是把 `run_authenticated_session` 改名加 `on_accept` 透传——没削弱安全。
  - **桌面 E 接受回调**(`make_desktop_accept_callback`/`trust_incoming_peer`)：✅ 接受时再次校验 `has_consistent_device_id` 才入库,镜像安卓那套。
  - **全验收矩阵跑了一遍,全绿**：core `fmt`/默认 `test`(144+,含 A 的 4 个首测)/默认+webrtc `clippy --all-targets`；`cargo ndk -t arm64-v8a -t x86_64 check --lib` 双 ABI；Android `:app:assembleDebug`(B/C/D Kotlin)；桌面 `test`(14)/默认+webrtc `check`/`clippy`；`node --check send.js/app.js`。
- 结论：**Codex 把 B–E 全做完了,且全绿、安全正确,只是没提交 E、没收尾**。
- 我补的：把 E 单独提交(`22c184e`,桌面无感流程 + core mDNS 广播完整签名身份),补本 worklog + handoff-clock(Codex 漏掉的)。
- 至此无感互联 A–E 全部完成：A 首次接触安全核心(我)+ C1 安卓接受回调 + C2 安卓附近设备一键发 + B 安卓配置进高级 + D 安卓系统分享 + E 桌面平移(Codex 主体,我补 E commit)。
- commit：E=`22c184e`。**注意分支有多个未 push commit**(`bbdbc04`/`9a4b6d4`/`365211a`/`368f1fd`/`47dbb09`/`a56e72c`/`22c184e`),待用户决定 push。
- 下一步：真机/双模拟器手验无感流程(发现→发送→对方一键接受→静默)；上线路径仍是部署公网 wss 信令 + coturn。

## 2026-06-20 ~02:30 +08:00 — 无感互联 A：首次接触握手（免配对码安全核心）

- 背景：用户要 AirDrop 式无感互联(选文件→看到附近设备→发→对方一键接受),信任模型定 **TOFU 首次一键接受**。拆 A(安全核心,本条)+ B–E(安卓 UI/发现+接受弹框/分享/桌面)。用户说"你弄好(A)以后 bcde 都弄"。
- 做了 A（core,全绿,commit `9a4b6d4`）：
  - **安全难点(关键,写给后人)**:Noise KK 对端 DH 来自信任库(线下扫码无替换);首次接触从**线缆**收 DH,不绑定的话主动 MITM 能中继真发送方的签名握手却换上自己的 DH 做中间人(握手签名只签 nonce+双方 id,**不签 DH**)。→ 新增 **ed25519 对自身 DH 公钥的绑定签名**(`identity.rs::identity_binding_message` + 域头 `linkhub-identity-binding-v1`;`LocalIdentity::sign_identity_binding`/`DeviceIdentity::verify_identity_binding`)。接收端先验 ①`device_id==hash(ed25519 pubkey)`(`has_consistent_device_id`)②绑定签名,再弹接受,KK 才用这把已验证 DH。
  - **协议**:`WireMessage::Identity{device_id,name,public_key,dh_public_key,binding_sig}`(protocol.rs)。流程:未知发送方→接收端 `AUTH_NEED_IDENTITY`→发送方签名 `IDENTITY`→验证+`AcceptPeerCallback` 弹框→接受则原 challenge/签名/KK(改用线缆身份 `peer_identity`)。**已信任流程字节不变**;发送方仅被问到才发 IDENTITY(`wait_for_auth_challenge` 加 writer+local_identity 处理 `AUTH_NEED_IDENTITY`)→ 所有现有发送端自动兼容,无签名改动。
  - **公共 API**:`IncomingPeer`(已验证待接受对端,带公钥供持久化)+ `AcceptPeerCallback`(`Fn(IncomingPeer)->bool`,阻塞握手等用户)+ `run_authenticated_responder_over_with_accept`。`run_authenticated_session_over` 加第 7 参 `on_accept`,旧公共签名不变(传 None)。
- 改动:`identity.rs`、`identity/device_identity.rs`、`net/{protocol,auth_listener,auth_session}.rs`、`net.rs`;`docs/spec/项目状态.md`、本 worklog。
- 验证(全绿):`cargo fmt --check`;默认 `cargo test`(**+4 新测**:首次接触往返/拒绝/**MITM 换 DH 被拒**/device_id 伪造被拒);默认+`--features webrtc` `clippy --all-targets -D warnings`;`cargo ndk -t arm64-v8a -t x86_64 check --lib` 双 ABI。
- 未做(B–E,交给 Codex,见 `claude-to-codex/latest.md`):**C 把 `AcceptPeerCallback` 接进 Android JNI 接收循环**(`webrtcReceiveFile`/`run_authenticated_listener_*` → 经 JNI 回调到 Compose 弹"接受",接受则 `saveTrustedPeer` + 续连)+ 发送方用 `run_authenticated_responder_over_with_accept`/发现端身份;**B** 主界面瘦身藏信令/STUN/TURN/relay/地址/路径;**D** `ACTION_SEND` 分享目标;**E** 桌面平移。发现端 mDNS 广播完整身份(让发送方也免配对)在 C 里做。
- commit:A 单独 `9a4b6d4`。

## 2026-06-20 ~01:00 +08:00 — 本地强制 TURN 中继验证 + dev TURN fixture（上线第一步）

- 背景：用户确认目标=**真能用/上线**，选「先本地模拟跨网」。第一步验证"打洞失败只能走 TURN 中继"的路径在真实 app 代码里能用。
- 做了：
  - 新增 `core-rs/src/bin/turn_dev_server.rs`（Cargo.toml `[[bin]] name="turn-dev-server" required-features=["webrtc"]`）：复用 `webrtc::turn`（和 `webrtc_turn_e2e` 同款、与 coturn 同模型）起独立 TURN 服务器，relay 地址可传参（`turn:HOST:3478` user `linkhub`/pass `relay-pass`）。默认构建/Android `.so` 不受影响。
  - **✅ 真实多进程强制中继验证**：两个**宿主 `linkhub-cli` 进程**（`identity init`/`pairing-payload`/`pairing-code`/`trust-pairing` 交叉配对，确认码两向一致 `BC529-7A6F8`）+ 真实 `signaling-server`(9000) + `turn-dev-server`(relay `127.0.0.1`)、两端 `--relay-only`，跑通 256KB/64 块认证加密文件，**接收端 SHA `56cb8e9d…` == 源**；发送日志含 `Noise KK ... established` 和 `FILE_START_RECEIVED:0+bin`（T8 二进制分帧在中继上也生效）。relay-only 拒绝 host/srflx ⟹ 成功即证明走了中继。
- 关键发现/限制（写给后人，省得重试）：**两台同宿主 Android 模拟器无法验证 relay-only**——TURN 在宿主，relay 地址填 `10.0.2.2`（模拟器够得到）则服务器无法在两 relay 间转发、填 `127.0.0.1`（服务器能转发）则模拟器够不到（死结），加 QEMU slirp NAT 让 TURN CreatePermission 源地址对不上 → relay 包被丢、DataChannel 永不打开。我先在模拟器试了 relay-only（写 SharedPreferences `linkhub_webrtc.xml` via `run-as dd` + `am force-stop` 重载 + UI 启动）确实失败（`DataChannel never opened`），分析确认是拓扑死结非 bug，遂改宿主 CLI（无 NAT）跑通。STUN/直连路径模拟器已验（见上一条 C5）。
- 备忘：webrtc-rs 连接关闭时 webrtc-sctp 有个良性 `JoinError::Cancelled` panic（发生在文件已发送并确认**之后**，rc=0，不影响传输）。`adb input text` 长串/分块边界会丢字符（文件路径 `app`→`pp`），要逐次校验。
- 改动：`core-rs/src/bin/turn_dev_server.rs`(新)、`core-rs/Cargo.toml`；docs/spec 设计§11.1 + 项目状态 顶部；本 worklog。
- 验证（全绿）：`cargo fmt --check`；默认 `cargo build`/`clippy --all-targets`（bin 正确门控不进默认）；`--features webrtc` `clippy --all-targets`。无 lib/协议改动，测试集不受影响。
- 下一步（上线路径）：部署公网 `wss://` 信令 + coturn（设计§11.1 已有最小配置）→ 手机蜂窝 ↔ 桌面真实跨网验收。iOS 真正落地仍需 Mac。
- commit：`bbdbc04`（单独，未 push）。

## 2026-06-19 ~23:30 +08:00 — 双模拟器真机验证 Android 跨网络 WebRTC（C5 + 顺带证 T8）

- 做了：用户只有一台真机，要求在模拟器里验证 Codex 的 C5（Android 跨网络 WebRTC UI/前台服务）。两台 AVD（5554/5556）端到端跑通 **A→B 经真实 webrtc-rs Android 运行期的跨网络文件传输，SHA-256 逐字节一致**。
  - 构建链：`cargo ndk -P 24 -t x86_64 ... build --release --lib --features webrtc` 出带 webrtc 的 x86_64 `.so`（13.1 MB，对比默认 1.6 MB 印证门控）→ `gradlew :app:assembleDebug`（55 MB APK）→ 两台 clean install。
  - 联网：宿主跑 `signaling-server 127.0.0.1:9000`；App 默认信令 `ws://10.0.2.2:9000`（模拟器→宿主 loopback，直达）+ 公共 STUN `stun.l.google.com:19302`。netstat 确认 B↔信令 ESTABLISHED、B 状态文本 `waiting for WebRTC offer`。
  - 配对：v2 三步（粘贴→查看对方信息出确认码→输入确认码→确认配对）双向互信，两端确认码一致 `08B34-63B4D`（对称码）。两端 `linkhub-trust-store.txt` 落盘确认。
  - 传输：B 服务页勾「随前台服务启动」+「启动监听」→「跨网络: 接收中」；A 发送页选对端 B（地址自动填 `10.0.2.17:8787`、设备 ID `lh-8a205a4e…`）+ 文件路径 → 「跨网络发送文件」→「跨网络文件已发送」。B 收到 `lh-d477…-wrtc_test.bin-262144-8f69aeff…_wrtc_test.bin`，**SHA-256 `8f69aeff…f9ac` == 源**。
  - 两端均为含 T8 的新构建，发送端协商到 `+bin` 走**二进制分帧**——故本次同时在真实 Android 上验证了 T8。
- 排障备忘（非 bug，写给后人）：① `adb input text` 长串会**静默截断**——配对 payload(224 字符) 必须分块输入（每块 28 字符）；Compose 字段清空用 `input keycombination 113 29`(Ctrl+A)+`keyevent 67`，tap 定位光标不可靠。② 配对码 **120s TTL**，对慢速 adb 驱动偏紧，必须「现生成现确认」一气呵成。③ 同名「生成配对码」既是区块标题又是按钮，要点第 2 个。④ **scoped storage**：`adb push` 进 App 外部目录的文件 App uid 读不到（FUSE 跨 uid，os err 13）——改用 `run-as com.linkhub.app dd if=/dev/urandom of=files/x.bin`（相对路径，cwd=app home；`sh -c` 会把 cwd 重置到 `/`）让 App 自己生成，再用内部绝对路径 `/data/data/com.linkhub.app/files/x.bin` 发送。⑤ Compose chip 的 `selected` 不映射到 uiautomator `selected` 属性——靠截图/设备 ID 显示判断选中。
- 改动：无代码改动（纯端到端验证）。`.screenshots/` 临时文件已清。
- 验证：见上（SHA 一致 = 跨网络加密传输 + 二进制分帧在真实 Android/webrtc-rs/STUN 上端到端正确）。
- 下一步：真机 arm64 复跑（用户有第二台设备时）；C6 滑动窗口仍待独立设计。
- commit：未提交（纯验证）。

## 2026-06-19 ~21:30 +08:00 — T9 iOS 端脚手架（FFI 挂载 + 可构建工程 + 方案）

- 做了：T9。把 iOS 从「一堆引用着不存在符号的 Swift 文件」做成「Mac 上 clone→生成→构建」的可起步工程 + 定方案。**iOS 编译/打包只能在 macOS**，本仓库在 Windows，故交付脚手架 + 方案，真机构建留待有 Mac。
  - 盘点发现：core 的 `ios_bridge.rs` 其实**早已存在且已在 lib.rs 挂载**（`#[cfg(target_os="ios")]`），导出 6 个 `linkhub_*` C 函数，且与现有 `RustBridge.swift` 一一对应——只是从没被编译验证过、且 `ios/` 没有可构建工程。
  - 补缺口：① `ios/include/linkhub_core.h` + `module.modulemap`（Swift `import LinkHubCoreFFI`）；② `ios/scripts/build-core-ios.sh`（device `aarch64-apple-ios` + sim `aarch64-apple-ios-sim`/`x86_64-apple-ios` → `lipo` → `LinkHubCore.xcframework`）+ core `crate-type` 增 `staticlib`；③ `ios/project.yml`（XcodeGen 文本化工程）；④ `ios/LinkHub/Info.plist`（`NSLocalNetworkUsageDescription`+`NSBonjourServices=_linkhub._tcp`，iOS14+ 缺这两键 Bonjour 静默失败）；⑤ 源树自洽：补 `@main` 的 `LinkHubApp.swift` + `ContentView` 引用却缺失的 `ServiceView.swift`；⑥ `.gitignore` 加 iOS 产物。
- FFI 选型：**手写 C ABI + JSON 串**（与 Android JNI 同契约，Swift `Codable` 解析），否决 UniFFI（为单端引重机制不划算）。
- 为什么发现走 Swift 侧 Bonjour：core 的 `mdns-sd` 仅桌面/安卓用，iOS 用 `NetService` 避免与系统网络栈/权限打架；后台网络受 iOS 强约束，传输定位前台。
- 改动：新增 `ios/{include/*,scripts/build-core-ios.sh,project.yml,LinkHub/Info.plist,LinkHub/LinkHub/{LinkHubApp,UI/ServiceView}.swift,README.md}`；`core-rs/Cargo.toml`(+staticlib)；`.gitignore`；新增 `docs/spec/设计-iOS-端.md`；项目状态/路线图/本 worklog。**未改** `ios_bridge.rs`（已存在且 iOS 下编过）。
- 验证：core 默认矩阵不受影响（`ios_bridge` 仅 iOS cfg；staticlib 仅多产 `.a`）——`cargo build --lib` + `clippy --lib -D warnings` + `cargo ndk -t arm64-v8a check --lib` 均绿；**iOS 真验证**：Windows 上 `rustup target add aarch64-apple-ios` 后 `cargo check --target aarch64-apple-ios --lib` **exit 0**（含 ios_bridge cfg 路径 + mdns-sd/tungstenite/全 crypto，纯 check 不链接）。
- 未做（须 Mac）：`xcodegen generate` + 出 xcframework + 真机/模拟器跑通；补 send/listen FFI（当前只 identity/pairing，对照 JNI 的 sendText/sendFile/startListener 扩）；权限/后台实测；iOS 跨网络 + macOS CI。
- commit：T9 单独 commit `b001bd2`。

## 2026-06-19 ~20:40 +08:00 — T8 二进制文件分帧（版本协商，去掉 hex 翻倍）

- 做了：T8。加密路径的文件分块从「hex 文本」换「二进制」，线缆体积砍半，版本协商保旧端兼容。
  - 洞察：Noise 帧本就是 `u16 大端长度 + 密文`（`send_encrypted_frame`），每帧自带长度前缀。原 `FILE_CHUNK` 把裸字节 hex 编码（2×）只是为了塞进 Tab 文本行——对分帧毫无必要。
  - `protocol.rs`：加 `WireMessage::FileChunkBin{transfer_id,chunk_index,data:Vec<u8>}`；`serialize_message_bytes` 产 ASCII 头 `FILE_CHUNK_BIN\t{id}\t{index}\t`+裸字节；`parse_binary_frame` 命中前缀就切前两个 Tab（结构性）、余下原样作 data（块内 Tab/换行/NUL/0xFF 保真），否则回退 UTF-8 `parse_message`。`serialize_message`（String 版）的 FileChunkBin 分支 `unreachable!`（只走字节路径）。
  - `auth_session.rs`：收发帧改字节路径（`recv_encrypted_frame` 不再 `String::from_utf8`，改 `parse_binary_frame`）；抽出共享 `receive_file_chunk` 给 hex/bin 两臂复用；加密接收端在 FILE_START 的 ACK 尾加 `+bin` 能力标记；`send_encrypted_file_start_with_retries` 返回 `(续传起点, 对端是否支持二进制)`。
  - `ack.rs`：`parse_file_start_ack_status` 容忍 `+bin` 后缀（先 strip_suffix）；加 `file_start_ack_supports_bin`。
  - `net.rs`：加密发送端仅当对端带 `+bin` 才发 `FileChunkBin`，否则回落 hex。**明文 TCP 路径**（按行分隔，承不了裸字节）保持 hex → 全兼容。
- 为什么协商放 FILE_START ACK：逐传输协商最自然，无需改 HELLO/加握手往返；v1 接收端不发 `+bin`，发送端自动回落 hex。背压仍是逐块停等 ACK，未变。
- 改动：`core-rs/src/net/{protocol,auth_session,ack}.rs`、`net.rs`；docs/spec 设计§4.6、项目状态、路线图、本 worklog。
- 验证（全绿）：`fmt`；`clippy --all-targets -D warnings`（默认+webrtc）；默认 `cargo test`（**+6 新单测**：分帧序列化/解析、含 Tab/NUL 保真、文本回退、畸形拒绝、能力探测、内存双工真跑二进制文件 e2e=9KB 多块含坏字节逐字节还原）；`--features webrtc`——`webrtc_cli_e2e`/`webrtc_e2e`(DataChannel)/`webrtc_turn_e2e`(强制 TURN) 三文件 e2e **现自动走二进制路径且 SHA 一致**=真实 WebRTC/TURN 上验证；`cargo ndk -t arm64-v8a -t x86_64 check --lib` 双 ABI。
- 未做：滑动窗口/多块在途吞吐优化（停等 ACK 已是正确背压）；明文 TCP 仍 hex。
- commit：T8 单独 commit `debd005`。

## 2026-06-19 ~14:00 +08:00 — T3 信令消息签名（用户开 ultra 模式让我连做 T3/T4/T6/T7/T5）

- 做了：用户让我一口气把 backlog 的 T3、T4、T6、T7、T5 都做了（开了最高 effort）。每个任务独立一个 commit、保持全绿。本条 = T3。
  - **T3 信令 SDP 签名**（设计 §7 唯一未补的安全缺口）：转发的 offer/answer SDP 现由发起方身份私钥 Ed25519 签名、接收方用预期对端公钥验签后才喂 webrtc，堵住恶意信令服务器篡改/替换/重放 SDP 的连接重定向攻击（把双方钉到攻击者中继做流量分析/降级/DoS）。
- 设计：纯密码学放 core 新模块 `net/signaling_signed.rs`（`seal_sdp`/`open_sdp`/`verify_signaling_sdp`，纯 Ed25519+serde，**默认构建即编译并单测**，不拉 webrtc/tokio）；签名原语 `LocalIdentity::sign_signaling_sdp` 复用 `identity::signaling_sdp_message`（域 header `linkhub-signaling-sdp-v1`，绑定 session_id+kind，与握手/登录两域隔离，`verify_strict`）。`payload_hex` 从 hex(SDP) 升级为 hex(JSON `{v,sdp,sig}`)。
- 接线：CLI `drain_outbound_sdp` 发送前 `seal_sdp(identity,…)`（给该 fn 加 `identity` 参数 + 调用点传 `&identity`）、`delivery_to_sdp_signal` 收取时 `open_sdp(from_public_key_hex,…)`（预期签名者用 `accept_signaling_delivery` 已校验过的对端公钥）。删掉因此不再用的 `hex_encode` helper。
- 为什么用 `from_public_key_hex` 当预期签名者：`accept_signaling_delivery` 已把发起端=trust store 目标设备 / 响应端=可信设备校验过，到 `delivery_to_sdp_signal` 时该字段已是被信任的对端公钥；服务器即便伪造此字段也会先在 accept 被拒，伪造 SDP 又没对端私钥签不出。
- 改动：新增 `core-rs/src/net/signaling_signed.rs`（含 6 单测）；改 `identity.rs`（+`SIGNALING_SDP_HEADER`+`signaling_sdp_message`）、`identity/device_identity.rs`（+`sign_signaling_sdp`）、`net.rs`（挂模块+导出）、`lib.rs`（导出）、`main.rs`（CLI 收发两侧 + 删 hex_encode）。docs/spec 设计§7+§9、项目状态、路线图、本 worklog 同步。
- 验证（全绿）：6 新单测（往返/换签名者/篡改 SDP/角色互换/跨会话重放/坏载荷）；core 默认 `cargo test`（128 lib + 18 CLI + e2e/signaling/webrtc）；`cargo test --features webrtc`（含 `webrtc_cli_e2e` 经真实 server 跑**签名后** SDP 路径仍 40KB 字节一致）；`cargo fmt --check`、`clippy -D warnings`（默认 + `--features webrtc`）；`cargo ndk -t arm64-v8a -t x86_64 check --lib` 双 ABI（纯 Ed25519 无新依赖，`.so` 不变）。
- 下一步：T4 TURN 中继兜底（把 RTCIceServer 的 TURN URL+短时凭证接进 `new_peer_connection`，加强制 relay 路径/测试，文档写 coturn）。
- commit：T3 单独一个 commit（见 git）。

## 2026-06-19 ~14:40 +08:00 — T4 TURN 中继兜底（强制 relay 经真实 in-process TURN 跑通）

- 做了：T4。把打洞失败时的 TURN 中继接进 webrtc 传输层，并用真实中继验证。
  - `webrtc_transport.rs`：新增 `IceServer`（`stun()`/`turn()` 构造）、`IceConfig { servers, force_relay }`（+ `from_stun_urls` 便捷构造）。`new_peer_connection(&IceConfig)` 把 servers 转成 `RTCIceServer`——**TURN（有 credential）自动设 `credential_type=Password`**（默认 `Unspecified` 会被 webrtc-ice 当「invalid turn credentials」拒，踩了一次坑），`force_relay` 时设 `RTCIceTransportPolicy::Relay`（只收 relay candidate）。`connect_initiator`/`accept_responder`/`new_peer_connection` 签名从 `ice_urls: Vec<String>` 换成 `ice: IceConfig`。
  - CLI：`split_webrtc_options` 加 `--turn-username`/`--turn-credential`/`--relay-only`；`WebRtcOptions::to_ice_config()` 把 `turn:`/`turns:` 的 `--ice` URL 配上凭证、其余当 STUN；`force_relay=relay_only`。两个 run_*_webrtc + 命令解析改用 `IceConfig`。
- 验证：新增 `tests/webrtc_turn_e2e.rs`——用 webrtc-rs 自带 `turn` crate（`webrtc::turn` 再导出，**无需新增依赖**）在回环起真实 in-process TURN 服务器（`Server::new` + `RelayAddressGeneratorStatic` + 长期凭证 `generate_auth_key`/自定义 `AuthHandler`），两端 `force_relay=true` 只走 TURN，跑通 40KB 认证加密文件、**接收端 SHA-256 与源一致**。全绿：默认 `cargo test`（128 lib + 19 CLI）、`--features webrtc`（含 turn e2e + 新增 CLI turn/relay 参数单测）、`fmt --check`、`clippy -D warnings`（默认 + webrtc，期间把 `into_ice_config` 改名 `to_ice_config` 过 `wrong_self_convention`）、`cargo ndk` 双 ABI 默认 check。
- 为什么用 in-process TURN：要"真的"证明强制中继能用，不能只测配置映射。webrtc-rs 把 `turn` crate 再导出了，直接 `webrtc::turn::server::Server` 在回环起服务器最省事且真实。`force_relay`（ICE policy=Relay）保证 DataChannel 只能经 TURN 建立，传输成功即等于走通了 relay。
- 改动：`core-rs/src/net/webrtc_transport.rs`、`src/main.rs`（CLI 参数 + 两测试改 `parsed.ice`）、`tests/{webrtc_turn_e2e.rs(新),webrtc_e2e.rs(call site 改 IceConfig::default())}`；docs/spec 设计 §11.1（coturn 部署 + 短时效凭证）+§9 T4、项目状态、路线图、本 worklog。
- 下一步：T5（信令服务器抗滥用/限流 + 客户端重连/心跳）。
- commit：T4 单独一个 commit（见 git）。

## 2026-06-19 ~15:20 +08:00 — T5 信令服务器抗滥用 + 客户端重连/心跳

- 做了：T5。服务器抗滥用 + 客户端韧性。
  - **服务器**（`signaling-server/src/limits.rs` 新增 + `lib.rs`）：`Limits`（帧 64KiB / payload_hex 32K / 40 条·秒）+ `RateLimiter`（固定窗口，`allow(now)` 纯函数可测）。`serve` → `serve_with_limits(listener, Limits)`（测试用紧配置）。handle_connection 用 `accept_async_with_config` + `WebSocketConfig{max_message_size,max_frame_size}`（协议层帧上限）；pump 循环每条入站消息先过 `rate_limiter.allow`，超出回 `Error{rate limit exceeded}` 并断连；`handle_client_msg` 的 Forward 先查 `payload_hex.len() > max_payload_hex_len` 回 `Error{too large}`（保持会话）。
  - **客户端**（`core-rs/src/net/signaling_client.rs`）：`RetryPolicy{max_attempts,base_delay,max_delay}` + `delay_after_attempt`（指数退避封顶，纯函数）；`SignalingClient::connect_with_backoff`（sync，`thread::sleep` 退避，到上限返回最后错误）；`ping()`（发 `ClientMsg::Ping`，去掉 Ping 的 `#[allow(dead_code)]`）。re-export `RetryPolicy`（net.rs + lib.rs）。
- 验证：server `cargo test`（10 单测含 3 RateLimiter + 6 集成含 `oversized_payload_is_rejected`/`message_flood_is_rate_limited_and_dropped`）；core 默认 `cargo test`（131 lib 含 3 退避单测 + signaling_e2e 5 含退避成功+ping/退避失败两测）；`--features webrtc` 全过；两 crate fmt+clippy（core 默认+webrtc）干净；`cargo ndk` 双 ABI（客户端改动在默认构建，确认不破 `.so`）。
- 坑：`WebSocketConfig` 不是 non_exhaustive 但有 `#[deprecated] max_send_queue` 字段——用 `WebSocketConfig{ max_message_size:.., max_frame_size:.., ..Default::default() }` 结构更新语法即可（不碰废弃字段、也躲开 `field_reassign_with_default` lint）。
- 改动：`signaling-server/src/{limits.rs(新),lib.rs}`、`tests/forward.rs`；`core-rs/src/net/signaling_client.rs`、`net.rs`、`lib.rs`、`tests/signaling_e2e.rs`；docs/spec 设计§7+§9、项目状态、路线图、本 worklog。
- 未做：跨连接全局配额/IP 限流、连接数上限；客户端「断线→重连→恢复 presence」高层循环（本期只给原语）。
- 下一步：T6（桌面 Tauri 集成：WebRTC connect/transfer 命令 + UI 显示当前路径）。
- commit：T5 单独一个 commit（见 git）。

## 2026-06-19 ~16:40 +08:00 — T6 桌面 (Tauri) 跨网络集成 + 抽出共享桥到 core

- 做了：T6。先把 WebRTC 桥逻辑从 CLI 抽进 core 复用（也给 T7 铺路），再接桌面。
  - **共享入口（core）**：新增 `net/webrtc_session.rs`（feature `webrtc`）——`send_file_over_webrtc`/`receive_file_over_webrtc` 封装信令登录 + 签名 SDP(T3) + DataChannel 建立 + 认证 Noise 传输。把 CLI main.rs 里的 `WebRtcSignalingRole`/`RunningSignalingBridge`/`start_webrtc_signaling_bridge`/`drain_outbound_sdp`/`accept_signaling_delivery`/`delivery_to_sdp_signal`/`finish_*`/`new_webrtc_*`/`is_poll_timeout`/`sanitize_cli_token`/`dh_key_from_identity` 全删（~250 行），`run_connect_webrtc`/`run_listen_webrtc` 改薄封装委托（保留 println UX + `lookup_peer_identity`）。CLI 顶部 import 收窄（去掉 accept_responder/connect_initiator/SdpSignal/BufReader/AtomicBool/Ordering）。`webrtc_cli_e2e` 仍绿 → 行为等价。
  - **桌面命令**：`connection_plan(lan_addr,signaling_available,relay_available)` 纯函数（core `plan_connection` → 直连/打洞/中继有序 TransportPath，UI 显示，无 webrtc feature）；`webrtc_send_file`/`webrtc_receive_file` async 命令（`tauri::async_runtime::spawn_blocking` 把阻塞传输移出 UI 线程），gate 桌面新增 `webrtc` cargo feature（`= ["linkhub-core-prototype/webrtc"]`），关时回友好错误。`build_ice_config` 助手（turn:/turns: → 带凭证）。
  - **前端**：`send.js` 加「跨网络传输 (WebRTC)」卡片（信令URL/ICE/TURN/仅中继/设备/文件 + 发送/监听一次/查看路径）；`style.css` 加 `.hint`/`.inline-check`。
- 验证：桌面 `cargo test` 12 smoke（+2 connection_plan）；桌面默认 + `--features webrtc` 两构建 `cargo check`/`clippy -D warnings` 干净（顺手清桌面既存 clippy 债：14 处 `format!("{e}")`→`to_string`、tray needless borrow、补 webrtc_receive_file 的 too_many_arguments allow、补测试 `use std::time::Instant`）；`node --check src/js/send.js`；core 默认 `cargo test`（131 lib + 16 CLI）+ `--features webrtc`（cli/turn/e2e 全过）；core fmt/clippy（默认+webrtc）干净。
- 坑：① 桌面默认构建 `cargo test` 暴露既存 latent 错误——测试模块用了 `Instant` 但没 import（之前没跑过桌面 test？）。② `spawn_blocking` 用 `tauri::async_runtime::spawn_blocking`，无需给桌面加 tokio 直接依赖。
- 改动：core `net/webrtc_session.rs`(新)、`net.rs`、`main.rs`(大删+委托)；desktop `Cargo.toml`(feature)、`src/main.rs`(命令+测试+清债)、`src/js/send.js`、`src/css/style.css`；docs/spec 设计§8/§9、项目状态、路线图、本 worklog。
- 未做：真实双机跨网桌面实测（需两台不同网络机器+公网信令）；桌面常驻监听循环。
- 下一步：T7（Android JNI WebRTC 路径 + 真机 webrtc-rs 运行期 + `.so` 体积增量）。
- commit：T6 单独一个 commit（见 git）。

## 2026-06-19 ~17:40 +08:00 — T7 Android JNI 跨网络桥 + webrtc-rs 运行期 + `.so` 体积实测（收官 5 任务）

- 做了：T7（最后一个）。把跨网络接进 Android JNI，并量带 webrtc 的 `.so` 体积坐实门控。
  - **JNI（`jni_bridge.rs`）**：加 `Java_..._webrtcSendFile`/`webrtcReceiveFile`。符号常在（Kotlin `external fun` 每种构建都链接），body 分 `#[cfg(feature="webrtc")]`(真实，复用 `webrtc_session::{send,receive}_file_over_webrtc`) / `#[cfg(not)]`(回 JSON 错误，默认 `.so` 不拉 webrtc-rs/tokio)。接收侧在 JNI 线程抓 JVM+类引用建 `onFileReceived` 回调（复用 `make_file_received_callback`）。ICE 经 JSON `{ice_urls,turn_username,turn_credential,relay_only}` → `parse_ice_config`（gated）。send 侧从 trust store 按 device_id 查 peer `DeviceIdentity`。
  - **Kotlin（`RustBridge.kt`）**：加两个 `external fun` + 注释（阻塞、后台线程 + 前台服务、minSdk 24）。
  - **体积实测（x86_64 release，llvm-strip）**：默认 raw 1.56/strip 1.20/gzip 0.58 MiB；webrtc raw 12.5/**strip 9.62**/gzip 3.98 MiB → **strip +8.4 MiB/ABI，压缩进 APK +3.4 MiB**。坐实 webrtc 默认关、按构建 opt-in（默认 `.so` 不变）。
- 验证：`cargo ndk -t arm64-v8a -t x86_64 check --lib`（默认，含 JNI stub）干净；`cargo ndk -P 24 -t x86_64 check --lib --features webrtc`（真实 JNI impl）干净；core host `cargo fmt --all -- --check`（手工对齐 `jni_bridge.rs`——`cargo fmt` 默认跳过非当前 target 的 `cfg(android)` 模块，没自动格式化，按 `--check` 给的 diff 手改了两处换行）。host core/desktop 不受影响（JNI 仅 android）。
- 决策记录：**android clippy `-D warnings` 不在验收矩阵**（矩阵是 `cargo ndk check`）。android clippy 会报 `redundant_closure_call`/`useless_format`，但那是**整个 jni_bridge 文件的既有惯用风格**（每个 JNI fn 都 `(|| {...})()` + `format!("{e}")`）；新代码与文件一致，没为 2 个函数引入异类风格。`cargo ndk check` 对这些 clippy-only lint 不报，干净。
- 改动：`core-rs/src/jni_bridge.rs`、`android/.../bridge/RustBridge.kt`；docs/spec 设计§8/§9、项目状态、路线图、本 worklog。
- 未做：端侧 Compose UI/前台服务去调这两个 `external fun`（符号+桥就绪，差 UI 触发）；真机 arm64 JNI 实跑（T2 已证模拟器 x86_64 运行期）；跨网 release APK 打包。
- 收官：T3/T4/T5/T6/T7 全部完成，每个独立 commit、全矩阵绿。下一步交给用户决定（push / 真机端到端实测 / 跨网安卓出包）。
- commit：T7 单独一个 commit（见 git）。

## 2026-06-18 07:30 +08:00 — 跨网络主线 M2-step2 + M3 + 编排（Noise 跑通真实 WebRTC DataChannel）

- 做了：用户用高 effort 模式让我一口气把跨网络主线做完。按 4 步推进，每步保持仓库可编译、测试全绿：
  1. **架构决策**（设计 §4.5）：core 保持同步、新 webrtc 模块用 tokio runtime 把异步 DataChannel 桥成阻塞 `Read+Write`（方案 a，最低风险，复用认证会话单测 `MemoryDuplex` 的同一缝），webrtc-rs/tokio feature-gate（默认关）。
  2. **M2-step2**：`core-rs/src/net/signaling_client.rs`（同步 `tungstenite`，不给默认/Android 引 tokio）+ `LocalIdentity::sign_signaling_login`（签 `linkhub-signaling-auth-v1\0{nonce}`，与服务器对齐）+ CLI `signal-listen`/`signal-relay` + `tests/signaling_e2e.rs`（两 core 客户端经真实服务器互发，3 测试）。
  3. **M3**：`core-rs/src/net/webrtc_transport.rs`（`webrtc` feature）——`DataChannelDuplex`（异步 DataChannel↔同步字节流，Write 按 16KB 分片/Read 重组，tokio `Handle::block_on` 发、`on_message`+condvar 收），`connect_initiator`/`accept_responder`（非 trickle，gather-complete 交换 offer/answer）。net 抽出传输无关公共入口 `run_authenticated_{text,file}_sender_over`/`run_authenticated_responder_over`（TCP 版改委托，行为不变），让现有 Noise KK 会话原样跑在 DataChannel 上。`tests/webrtc_e2e.rs`（`#![cfg(feature="webrtc")]`）两进程内 PeerConnection 回环建 DataChannel，跑通 40KB 认证加密文件、**接收端 SHA-256 与源一致**。
  4. **编排**：`core-rs/src/net/connection_plan.rs`——`plan_connection`（LAN→WebRTC→中继固定优先级=TransportHealth 基础分顺序）+ `attempt_with_fallback`（逐条回退）+ `preferred_established_route`（复用 `select_best_route`），6 单测。
- 为什么：跨网络是最大产品短板；M1 地基 + M2-step1 服务器已就位，本轮把客户端、P2P 传输、选路补齐，让两台跨网设备能端到端加密传文件。
- 关键难点处理：core 同步 vs webrtc-rs 异步 + DataChannel 消息语义 vs 字节流——用 `DataChannelDuplex`（buffer+condvar 阻塞读、分片写）一层解决，认证/JNI/Tauri/现有同步 API 一行不改。
- 改动：新增 `core-rs/src/net/{signaling_client,webrtc_transport,connection_plan}.rs` + `tests/{signaling_e2e,webrtc_e2e}.rs`；改 `core-rs/src/net.rs`（抽 `_over` 入口 + 模块挂载 + 导出）、`lib.rs`（导出）、`main.rs`（CLI）、`identity/device_identity.rs`（`sign_signaling_login`）、`Cargo.toml`（tungstenite 默认依赖 + webrtc/tokio/bytes 可选 feature + dev-deps signaling-server/tokio）。顺手修了 rust 1.96 新 clippy lint 的若干**既存**告警（gossip/secure_store(DPAPI &mut→&)/routing/webrtc.rs/device_identity），使 `-D warnings` 全 crate 干净。docs/spec 三文件 + 本 worklog 同步。
- 验证（全绿）：core 默认 `cargo test`（122 lib+16 CLI+1+4+3）；`cargo test --features webrtc`（+1 M3 DataChannel 文件传输，SHA 一致）；`cargo fmt --check`；`cargo clippy --all-targets -- -D warnings`（默认 + `--features webrtc` 均干净）；`cargo ndk -t arm64-v8a -t x86_64 check --lib`（默认，webrtc 关，`.so` 不变重）；signaling-server `cargo test`（7+4）。
- 未做（如实）：真机/双模拟器跨真实 NAT 实测（M3 用进程内回环验证）；TURN 实拨（仅占位+回退）；webrtc 路径接进 CLI/Tauri/Android UI；webrtc-rs 的 Android 运行期（spike 仅证明可交叉编译）。
- 下一步：把 webrtc 路径接进 CLI（加 `connect-webrtc`/`listen-webrtc` 子命令，signaling_client 桥 SdpSignal）→ 双模拟器/真机跨网实测 → TURN → 各端 UI 显示当前路径。
- commit：未提交（待用户确认；本轮全部改动在 core-rs + docs/spec，spike/signaling-server 上一提交已入库）。

## 2026-06-18 05:30 +08:00 — M2-step1：薄信令服务器（新 crate signaling-server）

- 做了：spike 后用户拍板选 **webrtc-rs**（连带定了信令必须自建），并让我直接落地 M2。建独立 crate `signaling-server/`（tokio + tokio-tungstenite，72 依赖），实现最薄可用信令服务器并自带集成测试验收——**先不接 WebRTC**（M3）。
- 为什么：webrtc-rs 不像 libp2p 自带 relay/rendezvous，§5 的信令服务器要自己写，这是 M2 的核心。本步只做"两端经服务器互转 SIGNALING"的链路验证。
- 设计要点：
  - 鉴权 `auth.rs`：服务器先发 `Challenge{nonce}`，设备回 `Auth{device_id,public_key_hex,signature_hex}`，`verify_strict` 校验。签名串域分隔 `linkhub-signaling-auth-v1\0{nonce}`，**故意不复用** core 的 `handshake_challenge`（那个带双方 device_id、是 p2p 用），避免跨协议签名重放。
  - presence 按**已证明的身份公钥**建表（公钥与 `device_id=lh-+sha256(pubkey)[..16]` 1:1，按公钥路由即天然防冒充他人 id 上线）。`unregister` 用 `same_channel` 只删自己那条，避免重登替换后误删新连接。
  - `protocol.rs`：device↔server 用 JSON tagged enum（`ClientMsg`/`ServerMsg`），与 p2p 的 tab 行协议分开；`payload_hex` 对服务器全程不透明。
  - `lib.rs`：每连接一个 task，`tokio::select!` 在「自己 mpsc 出站队列」和「ws 入站」之间 pump，保证单写者。
- 改动：新增 `signaling-server/{Cargo.toml,src/{lib,main,protocol,auth}.rs,tests/forward.rs}`；`docs/spec/设计-跨网络传输-webrtc.md`（§9 M2-step1/step2 状态）、`项目状态.md`、`开发路线图.md` 同步。tokio-tungstenite 用 0.23（0.24 把 `Message::Text` 改成 `Utf8Bytes` 会编不过）。
- 验证：`cargo test` = 7 单测 + 4 集成（`relays_signaling_between_two_authenticated_clients` 跑通 A→B 转发、外加离线报错/ping-pong/坏签名拒绝）全绿；`cargo fmt --all --check`、`cargo clippy --all-targets -- -D warnings` 均干净。未碰 core-rs/Android/桌面，无回归。
- 下一步：M2-step2——core-rs 加 `net/signaling_client.rs`（倾向同步 `tungstenite`，贴合现有同步网络层）+ CLI 子命令，用 core 客户端对接本服务器；`protocol.rs` 的 `Signaling` 去 dead_code。之后 M3 接 webrtc-rs DataChannel。
- commit：未提交（spike + 选型 + M2-step1 + docs/spec 三文件，待用户确认一起提交）。

## 2026-06-18 04:30 +08:00 — 跨网络选型交叉编译 spike：libp2p vs webrtc-rs（Windows + Android NDK）

- 做了：用户问「项目后续要做什么」，我判断阶段 5 跨网络是最大短板、M1 地基已就位，但 M2/M3 卡在「管道选型」未拍板（设计文档 §12 决策 1），且文档反复说选型前要做交叉编译 spike。用户选「跨网络选型 spike」。于是建两个一次性 crate（`spike/libp2p-spike`、`spike/webrtc-spike`），分别对 Windows host + Android NDK 28.2 双 ABI（arm64-v8a + x86_64）跑 `cargo check`。
- 为什么：设计文档把「WebRTC/libp2p 在 Android NDK 上能否交叉编译」列为最大未知风险，必须在投入 M2/M3 前先证伪/证实，避免选错框架返工。
- 结果（全绿）：
  - **libp2p 0.54.1**（tcp+quic+dns+noise+yamux+**dcutr**+**relay**+identify+ping+macros+tokio）：host ✅ / arm64 ✅（6m50s）/ x86_64 ✅；**330** 依赖；纯 Rust，`ring 0.16/0.17` 均交叉编过。spike 里写了 `peer_id_from_ed25519_seed`（用现有 Ed25519 身份密钥派生 PeerId，验证 device_id↔PeerId 映射不麻烦）。
  - **webrtc-rs 0.11.0**（完整 ICE/STUN/TURN/DTLS/SCTP/DataChannel）：host ✅ / arm64 ✅ / x86_64 ✅；**231** 依赖；纯 Rust、无 C/C++ sys 依赖。
  - **结论：交叉编译这一最大风险对 A、C 都已证伪。**选型回到架构契合度（libp2p 省自研信令/NAT 但重、要接受 PeerId 寻址；webrtc-rs 轻、与"身份当信任锚+自建薄信令"耦合更自然但信令要自己写）。
- 改动：新增 `spike/libp2p-spike/`、`spike/webrtc-spike/`（一次性，`target/` 已被根 `.gitignore` 忽略，源码可留作可复现产物或删除）；`docs/spec/设计-跨网络传输-webrtc.md`（新增 §10.1 spike 结果表 + 更新 §12 决策 1）、`项目状态.md`、`开发路线图.md` 同步。
- 验证：4 个 `cargo check`/`cargo ndk check` 全 exit 0（见上）。网络抖动时靠 `CARGO_HTTP_MULTIPLEXING=false`+`CARGO_HTTP_LOW_SPEED_LIMIT=0`+`CARGO_NET_RETRY=10` 啃过 crates.io 慢速中断。
- 未做：真实 `.so` 体积（只 check 未 release 链接）、iOS 交叉编译（工程未脚手架化）——留 M3。
- 下一步：等用户在 §12 决策 1 拍板（libp2p / webrtc-rs）→ 进 M2 信令服务器 + presence。spike crate 是否保留待用户定。
- commit：未提交（docs/spec 三个 tracked 文件 + spike 源码；ai-handoff/.screenshots git 忽略）。

## 2026-06-18 03:30 +08:00 — 跨网络传输 M1：把认证会话从 TcpStream 解耦（阶段 5 地基）

- 做了：用户拍板「地基先行 / 倾向 libp2p / Claude 实现」。先写了设计文档 `docs/spec/设计-跨网络传输-webrtc.md`（架构/信令/选型/分期/待拍板决策），再落地 M1——把认证会话改成传输无关，为后续 WebRTC/中继铺路。
- 为什么：当前只能局域网直连（mDNS+TCP），跨网是最大短板。M1 不依赖管道选型、低风险，是所有后续工作的地基。
- 改动（core-rs，公共 API 不变）：
  - [auth_session.rs](../../core-rs/src/net/auth_session.rs)：抽出 `run_authenticated_session_over<W: Write, R: BufRead>`（responder 核心）与 `perform_initiator_handshake<W, R>`（initiator 握手）；`run_authenticated_session(TcpStream)`/`open_authenticated_stream(addr)` 变成 TCP 薄封装；`send_encrypted_*`/`recv_encrypted_frame`/`wait_for_*` 全部泛型化；去掉不再需要的 `Read` import。
  - [ack.rs](../../core-rs/src/net/ack.rs)：`write_message` 泛型化。
  - 新增内存双工单测，用非 TCP 管道跑通完整认证会话（证明解耦真的成立，而不只是类型层面）。
- 验证：`cargo fmt`；`cargo test` 全绿（115+16+1+4+1 新测）；`cargo clippy` 改动文件零告警；`cargo ndk` 双 ABI check 通过（jni_bridge 不受影响）；desktop `cargo check` 通过。
- 下一步：M2 信令服务器 + presence；动手前先做 libp2p/webrtc-rs 三端交叉编译 spike。等用户开 PR 可对 M1 跑 `/code-review ultra`。
- commit：未提交（设计文档 + 开发路线图 §5 指针 + 项目状态 + M1 代码 + 本 worklog/handoff 均待用户确认提交）。

## 2026-06-18 02:58 +08:00 — 双模拟器在 release APK 上实测复验 task b（v2 配对 + 双向收发）

- 做了：接手验证 Codex 的 task b（`linkhub-pair-v2`，提交 1438ed2）。先确认工作区干净、`main`==`origin/main`；`apksigner verify` release APK 通过（V2，`ae2bbe…a6bc`）。然后用两个 AVD（5554/5556）clean install **release** 版（顺带过 R8），adb 驱动 Compose UI 跑通 v2 全链路。
- 验证结果：
  - 配对码解析为 v2 七段（`issued_at`+`ttl`、无 nonce）；**确认码 10 hex/40 bit、两端一致 `CCBE4-7578F`**（只依赖排序后双方指纹）。
  - **TTL 真生效**：贴入 >120s 的 payload → 对方信息全空、`确认配对` 禁用（被拒）；新鲜 payload 正常。`confirmPairing` 会重解析粘贴框 payload 再校验过期，故确认时必须仍新鲜（120s 窗口对慢速 adb 驱动偏紧，靠"现生成现确认"过关）。
  - 双向互信建立（两端「可信设备 (1)」）。
  - 双向收发：A→B/B→A 文本+文件全成功；接收文件 `lhtest_a.txt`(SHA `cd5d5c…02ab8`)、`lhtest_b.txt`(SHA `f0fc32…cc81`) 与源**完全一致**；收发历史正确。
- 排障备忘（非 bug）：① Android 只注册 `onFileReceivedListener`，收到的文本不进 UI/历史，文本成功只在发送端看；② release（不可调试）版 native 读不到 `adb shell` 写到 app 外部目录的文件，要用 App 内「选择文件」SAF（复制到内部 `cacheDir/linkhub-send/` 后再发）。一开始那条 `失败` 历史是我用错路径，换 SAF 后即成功。
- 改动：无代码改动（纯验证）；`docs/spec/项目状态.md`+`开发路线图.md`、本 worklog、`shared/validation-log.md`、`shared/handoff-clock.md`、`claude-to-codex/latest.md` 同步。`.screenshots/drive.sh` 把 `payload_from_dump` 的 grep 从 v1 改成 `v[12]`（git 忽略）。
- 验证命令：`apksigner verify --print-certs`（PASS）；其余为 UI 实测（见上）。
- 下一步：真机安装 smoke + arm64 运行期复跑（用户单机时）；docs/spec 两个文件可在用户确认后 commit。
- commit：未提交（仅 docs/spec 两个 tracked 文件有改动；ai-handoff/.screenshots 均 git 忽略）。

## 2026-06-18 01:58 +08:00 - Codex 接手并完成 task b 配对安全加固
- 备注：本条由 Codex 写入，用于覆盖上一条“task b 待续/保留”的旧状态。用户在 2026-06-18 明确要求 Codex 接手 task b。
- 做了：`linkhub-pair-v2` 已实现，payload 删除 nonce、加入 `issued_at` Unix 秒 + `ttl`，确认码加长到 40 bit 并保持基于排序后双方指纹的对称性质；同步 Rust/JNI/iOS/Tauri/Android 解析；补 native panic hook + android_logger；补 stale `service_status.running` reconciliation。
- 验证：core cargo test/build、cargo ndk 双 ABI check、Android compileDebugKotlin、重建 `.so`、assembleDebug、assembleRelease、desktop cargo check、desktop pairing.js node check 均通过；构建产物未进 git。
- 未做：未重跑双模拟器 UI v2 实测、未真机安装 smoke、未对新 release APK 跑 apksigner verify。
- commit：未提交（待 Codex 本轮收尾）。

## 2026-06-17 22:10 +08:00 — 对 b3810c9 跑 xhigh code-review，修了 @Volatile，安全加固待续

- 做了：用户体验 `/code-review xhigh`（多智能体）审刚提交的 b3810c9。审查确认 5 个修复本身没问题，但挖出配对安全机制里**多为既存**的深层隐患。随后修了其中最易的一条（#3）+ 部分处理 #7。
- 审查发现（按严重度）：
  - **#1 配对码 TTL 形同虚设**：`jni_bridge` 的 `parsePairingPayload`/`confirmPairing` 用 `Instant::now()` 当 created_at，`confirm` 又用 `now` 校验，`is_expired` 永远 false。根因：payload 只带"有效时长"不带"绝对生成时间"，接收方无从判断。**既存**。
  - **#2 确认码可离线预计算**：去 nonce 后确认码是双方公钥的稳定函数（这点对，像 Signal 安全码），但只取 6 hex=24bit，配合 #1，中间人可**离线**暴力撞短码。修法**不是加回 nonce**（会重新弄坏两端一致），而是**加长确认码 + payload 加绝对时间戳**。
  - **#3 `LinkHubService.isRunning` 没 @Volatile**：本会话让它成唯一存活判据、跨线程读写，旁边 `monitorActive` 却是 volatile。**已修**（加 @Volatile + 注释）。
  - **#4 panic 只在 accept 线程被 catch_unwind**：每连接 worker（`auth_listener.rs:123`）+ 无全局 panic hook → 原生 panic 仍不可见。应设一次 `std::panic::set_hook` + android_logger。
  - **#5 过期 `service_status.running` 仍被写入**：只停了"读"，没停"写"，未来读者（开机自启/小组件）会重新踩死锁。
  - #6 启动按钮乐观置位被 1s 轮询覆盖（轻微）；#7 nonce 成无用负担（**部分处理**：加注释说明仅为兼容旧格式保留、非重放保护，完整删除并入 v2）；#8 测试只验两端相等没验唯一性；#9 AndroidDiscovery 收尾回调改快照（无害既存）。
- 为什么 #7 没直接删：nonce 是 `linkhub-pair-v1` 线缆格式的第 6 段，**Kotlin 端硬要求正好 7 段**（`trustedPeerFromPayload` 的 `fields.size != 7`），删它=破协议、要同步改 Rust+Kotlin+iOS。而 #1/#2 的修复**本来就要给 payload 加绝对时间戳=一次 v2 升级**，所以"删 nonce + 加时间戳"应在 task b 一次性做成 v2。
- 改动（**未提交**）：`android/.../service/LinkHubService.kt`（@Volatile）、`core-rs/src/identity/pairing.rs`（nonce 字段注释）。验证：`core-rs cargo check` 通过、`:app:compileDebugKotlin` 通过。Rust 改动纯注释（`.so` 不用重编）；@Volatile 要重打 APK 才生效。
- 下一步（**task b，下个会话**）：① payload v2——加绝对签发时间戳让 TTL 真正可校验、顺手删 nonce；② 加长确认码（>24bit）；③ core 设一次性 `std::panic::set_hook` + android_logger 让原生 panic 进 logcat；④ 停止持久化 `service_status.running` 作为存活源（#5）。建议先对"配对安全设计"单独再跑一次 `/code-review ultra`。
- commit：未提交（#3/#7 两处）；上一批已是 b3810c9（已 push）。

## 2026-06-17 21:40 +08:00 — 修配对确认码两端不一致 + 提交本会话全部改动

- 做了：① 修 #2（确认码）：根因是 `confirmation_code` 混入了对方 payload 的 nonce，双向流程两端各用对方 nonce → 码不同（session 拿不到自己的 nonce）。改为只用排序后的双方指纹（去 nonce），MITM 防护仍由指纹绑定双方公钥保证。`identity.rs` 回归测试改成两端用不同 nonce 仍断言相等。双模拟器实测两端码一致（6DE-352=6DE-352）。② 修 #1（提交）：把本会话 5 个修复 + spec 文档提交到 main。
- 为什么：用户「先2后1」。
- 改动：`core-rs/src/identity/pairing.rs`、`core-rs/src/identity.rs`（测试）；重建两 ABI `.so` + debug/release APK；`docs/spec/*`、validation-log 同步。
- 验证：`core-rs cargo test` 全绿（16+1+4+...）；双模拟器确认码 MATCH ✓。
- commit：**b3810c9**（8 files；含本会话全部代码修复 + 项目状态/路线图）。docs/ai-handoff（本 worklog、validation-log）git 忽略，未入提交。未 push（用户未要求）。

## 2026-06-17 21:15 +08:00 — 双模拟器跑通 Android↔Android，顺手揪出并修了 4 个 bug

- 做了：用户出门，要求用两个模拟器验证 Android↔Android（他只有一台真机）。建第二个 AVD，两台 headless 起，全程 adb 驱动 Compose UI（uiautomator dump + 坐标 tap + input text；helper 在 `.screenshots/drive.sh`）。跨模拟器网络：接收端 `adb forward` 暴露 8787，发送端 `adb reverse` 让 app 拨 `127.0.0.1`（`10.0.2.2` 那条路不通）。最终 A→B / B→A 文本+文件全通，收到文件 SHA-256 与源一致、通知 + 收发历史正确。
- 揪出并修的 4 个真 bug（都在模拟器复现，不是环境问题）：① **监听进程被杀后永不重绑**——持久化 `service_status.running` 残留致「启动监听」被禁用、死锁，UI 假显示运行中而 8787 实际没绑（这才是我一开始所有传输失败的真因）；改 `ServiceScreen` 只信进程内 `LinkHubService.isRunning`。② **NSD 后台扫描 `RejectedExecutionException` 闪退**（打开发送/设备页约 10s 必崩）；`scanTrustedMdnsPeers` 的 executor 改 `DiscardPolicy`。③ **release(R8) 可信设备列表不持久化**——泛型 `TypeToken<List<TrustedPeer>>` 被 R8 剥离；proguard 加 TypeToken keep，已在 release 验证设备页恢复为 1。④ 监听 worker 加 `catch_unwind` 防御假运行态。
- 为什么：用户提醒「多次测试排除外部问题后要怀疑是不是软件 bug」——照做，逐层缩小（host→forward→guest loopback / debug vs release）定位到每个根因。
- 改动：`android/.../ui/ServiceScreen.kt`、`android/.../ui/AndroidDiscovery.kt`、`android/app/proguard-rules.pro`、`core-rs/src/jni_bridge.rs`；重建两 ABI `.so`；`docs/spec/项目状态.md` + `开发路线图.md` + 本 worklog + validation-log 同步。
- 验证：`core-rs cargo test` 全绿；`:app:assembleDebug` + `:app:assembleRelease` 均成功；A↔B 双向文本+文件在 debug 上实跑、哈希一致；R8 修复在 release 实跑（设备页 1）；NSD 修复实跑（发送页 25s 0 崩溃）；监听绑定实跑（`ss` 见 8787 + 真实 HELLO 回 AUTH_CHALLENGE）。
- 下一步：建议用户在真机复跑一遍（arm64 运行期）；处理「两端确认码不一致」UX/安全问题；改动均未提交，待用户确认 commit。
- commit：未提交

## 2026-06-17 13:30 +08:00 — 整理：截图清理 + docs 归入 spec/ + 建立 worklog 体系

- 做了：① 删掉项目根目录 21 张用完的 `phone-*.png` 测试截图，新建 git 忽略的 `.screenshots/` 作为以后截图的家，并在 `.gitignore` 加 `.screenshots/` + `phone-*.png`。② 把 docs/ 根的 7 份活动文档全部移进 `docs/spec/`，docs/ 根只剩 README + spec/ + archive/ + assets/ + ai-handoff/，并修好所有内部/外部引用链接（根 README、docs/README、ai-handoff/README、各文档相对路径 ../→../../）。③ 新建 worklog 体系（本文件 + `codex.md`），并准备加 SessionEnd 兜底钩子。
- 为什么：用户反馈截图占空间、docs 散乱、希望有可回溯的逐次工作记录。
- 改动：`.gitignore`、`README.md`、`docs/README.md`、`docs/ai-handoff/README.md`、`docs/spec/*`（移动 + 链接修正）、本 worklog 目录；记忆库新增 screenshot-housekeeping / worklog-routine、更新 update-docs-after-code-changes 指向 docs/spec/。
- 验证：`git status` 已无散落图片；grep 确认除一条历史 changelog 外，无残留 `docs/<name>.md`（非 spec）引用。
- 下一步：把本会话所有改动 commit + push（需用户确认）；截图/ai-handoff 按约定排除或保留本地。
- commit：未提交

## 2026-06-17 13:18 +08:00 — 在用户真机重装带修复的签名 release APK

- 做了：`adb` 重装。`install -r` 因旧版是不同密钥（debug）签名报 `INSTALL_FAILED_UPDATE_INCOMPATIBLE`；改 uninstall + 全新 install 签名 release 版成功，App 正常启动。
- 为什么：要让本轮安卓修复真正跑在用户手机上验证。
- 改动：无代码改动（部署动作）。设备 Vivo V2454A。
- 验证：`dumpsys package` 显示 firstInstallTime=lastUpdateTime=今天（干净安装）；截图确认 UI 起来（截图已按约定删除）。
- 下一步：请用户真机复验「停止→重启监听不再失败」「release 版生成身份正常」。
- commit：未提交

## 2026-06-17 11:05 +08:00 — 续做中断的安卓 bug 修复（监听重启 + release 生成身份）

- 做了：补全上一会话半成品的 `jni_bridge.rs` 监听修复——把 `LISTENER_EPOCH` / `LISTENER_HANDLE` / `stop_and_join_listener()` 真正接进 `startListener`/`stopListener`（启动前 join 旧线程释放 socket、存 handle、代际保护退出清标志；停止时 join 到线程退出）。proguard Gson keep 规则确认完整。
- 为什么：停止后立刻启动监听会重绑失败/状态错乱；release(R8) 版会裁掉 Gson DTO 导致生成身份「创建失败」。
- 改动：`core-rs/src/jni_bridge.rs`、`android/app/proguard-rules.pro`（后者上轮已写）；`docs/spec/项目状态.md` + `开发路线图.md` 同步。
- 验证：`cargo ndk -t arm64-v8a check --lib` 通过（无新告警，新代码全被使用）；`core-rs cargo test` 全绿；`desktop cargo check` 通过；重建两 ABI release `.so` + `gradlew :app:assembleRelease` 成功，`apksigner verify` V2 签名同证书 `ae2bbe…a6bc`。
- 下一步：真机复验（见上一条）。
- commit：未提交

## 2026-06-21 — Real-device validation (vivo V2454A, Android 16, ARM64)

Hotel WiFi + computer ProtonVPN. Findings:
- adb OK (USB debug). Device-to-device LAN BLOCKED: both phone+computer reach gateway 192.168.64.254, but cannot reach each other -> hotel AP **client isolation** (NOT the VPN; computer reaches gateway fine). Real mDNS/LAN discovery impossible on this network.
- APK install BLOCKED: vivo 'Install via USB' / INSTALL_FAILED_ABORTED 'User rejected permissions' — needs on-phone Allow tap (user asleep). LinkHub + ProtonVPN installs both gated on this.
- Real-device DATA PATH (no install needed) — cross-compiled linkhub-cli for aarch64-linux-android (cargo ndk --platform 26; getifaddrs needs API>=24), adb push to /data/local/tmp, ran over adb forward USB tunnel:
  - core binary runs natively on ARM64 (demo scenario executed)
  - PLAIN send-file 256KB -> phone listener: SHA256 MATCH
  - AUTHENTICATED send-file-auth 300KB (Noise KK handshake + encrypted session): SHA256 MATCH
- ProtonVPN: official APK = GitHub release 5.18.75.1, SHA256 289971271c16ab860dac485bada37e6c7f64dae023dee39c280edd2fbe8fd39d, signing cert DC:C9:43:9E:... (protonvpn.com). Download crawled through VPN; deferred to install-time.
- Cleaned phone /data/local/tmp; removed adb forwards.

NEXT (user-gated): enable vivo 'Install via USB' + keep phone unlocked -> install LinkHub APK + ProtonVPN (verify checksums) + drive Compose UI. For real LAN test: phone hotspot or non-isolated WiFi (+ ProtonVPN allow-LAN).
