# LinkHub 文档导航

本目录是 LinkHub 的中文文档集。建议新接手按下面顺序阅读。

> **换机 / 重新部署？** 直接看 [环境部署.md](spec/环境部署.md) —— 从全新机器 + `git clone` 到构建跑通每个组件的完整步骤（依赖、版本、clone 后需重建的本地文件）。

## 推荐阅读顺序

1. [项目总览报告.md](spec/项目总览报告.md) — 架构总览、技术栈、模块关系、进度与风险、接手路径（先看这个）
2. [项目状态.md](spec/项目状态.md) — 最新状态快照与已完成/待办（逐轮历史）
3. [产品需求.md](spec/产品需求.md) — MVP 产品需求 + 长期愿景与阶段拆分
4. [技术架构.md](spec/技术架构.md) — 系统形态、发现/传输/安全模型、组件现状
5. [开发路线图.md](spec/开发路线图.md) — 阶段 0–8 路线图
6. [环境部署.md](spec/环境部署.md) — 从零部署/换机指南、工具链（含 NDK/cargo-ndk/Tauri CLI）与验证基线
7. [真机测试指南.md](spec/真机测试指南.md) — Windows↔Android 真机/模拟器联调与文件互发验收

## 设计文档（阶段 5+：跨网络与匿名传输）

- [设计-跨网络传输-webrtc.md](spec/设计-跨网络传输-webrtc.md) — 信令服务器 + WebRTC + TURN 中继架构
- [设计-tor-onion-传输.md](spec/设计-tor-onion-传输.md) — Tor onion（Arti）opt-in 隐私增强，地址由身份派生
- [设计-i2p-与-torrent-传输.md](spec/设计-i2p-与-torrent-传输.md) — I2P / BitTorrent DHT 评估与定位
- [设计-iOS-端.md](spec/设计-iOS-端.md) — iOS FFI/工程方案与未决项

## AI 交接

[ai-handoff/](ai-handoff/) 存放 AI 之间的逐轮交接、决策、已知风险与 worklog。**后续接手的 AI 应先读 [ai-handoff/README.md](ai-handoff/README.md) 与 worklog**，以继承完整上下文。

## 归档

[archive/](archive/) 存放一次性或历史文档：

- `Android闪退修复交接.md` — 早期发送页闪退修复交接（问题已修复）
- `需求澄清.md` — 早期需求澄清原件（核心内容已并入产品需求.md）

## 资源

[assets/](assets/) 存放截图等静态资源。
