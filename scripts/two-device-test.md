# Two-Device LAN Test Checklist

> 目的：在两台真实设备上验证 LinkHub 桌面端的完整闭环
> 前提：两台设备在同一局域网内，均已安装 Rust 工具链

---

## 准备工作

### 在两台设备上都执行：

```powershell
# 1. 克隆项目
git clone https://github.com/PriscillaOvO/LinkHub.git
cd LinkHub

# 2. 编译
cd core-rs
cargo build

# 3. 创建共享目录
mkdir C:\LinkHub
cd C:\LinkHub
mkdir inbox
```

---

## Step 1：设备 A（Receiver）— 初始化身份 + 启动监听

```powershell
cd C:\Dev\VSCode\LinkHub\core-rs

# 生成身份
cargo run -- identity init C:\LinkHub\receiver-id.txt "Windows PC A"

# 启动加密监听（记下 IP:port）
cargo run -- listen-auth 0.0.0.0:8787 C:\LinkHub\receiver-id.txt C:\LinkHub\trust-store.txt --receive-dir C:\LinkHub\inbox
```

**Expected:** 显示 `LinkHub authenticated agent 'Windows PC A' (lh-xxxx) listening on 0.0.0.0:8787`

---

## Step 2：设备 B（Sender）— 初始化身份 + 配对

```powershell
cd C:\Dev\VSCode\LinkHub\core-rs

# 生成身份
cargo run -- identity init C:\LinkHub\sender-id.txt "Windows PC B"

# 生成配对 payload（记下完整的 linkhub-pair-v2|... 字符串）
cargo run -- identity pairing-payload C:\LinkHub\sender-id.txt 300
```

**Expected:** 输出 `linkhub-pair-v2|...` 和 fingerprint

---

## Step 3：设备 A — 接收配对

把设备 B 的 `linkhub-pair-v2|...` 字符串复制到设备 A：

```powershell
# 计算确认码
cargo run -- identity pairing-code C:\LinkHub\receiver-id.txt "<PASTE PAYLOAD HERE>"

# 确认配对（把显示的确认码输入）
cargo run -- identity trust-pairing C:\LinkHub\receiver-id.txt "<PASTE PAYLOAD HERE>" "<CONFIRMATION CODE>" C:\LinkHub\trust-store.txt
```

**Expected:** `trusted_device=lh-xxxx fingerprint=XXXX-XXXX-XXXX-XXXX`

---

## Step 4：设备 A — 也生成 payload 让设备 B 配对（双向）

```powershell
# 设备 A 生成 payload
cargo run -- identity pairing-payload C:\LinkHub\receiver-id.txt 300
```

把 payload 字符串复制到设备 B：

```powershell
# 设备 B 上
cargo run -- identity trust C:\LinkHub\receiver-id.txt C:\LinkHub\sender-trust-store.txt

# 或者用完整 pairing 流程：
cargo run -- identity pairing-code C:\LinkHub\sender-id.txt "<PASTE RECEIVER PAYLOAD>"
cargo run -- identity trust-pairing C:\LinkHub\sender-id.txt "<RECEIVER PAYLOAD>" "<CODE>" C:\LinkHub\sender-trust-store.txt
```

---

## Step 5：设备 B — 发送加密文本

```powershell
# 确保设备 A 的 listener 在运行
# 设备 A 的 IP 可以从 ipconfig 获取（例如 192.168.1.100）

cargo run -- send-text-auth 192.168.1.100:8787 C:\LinkHub\sender-id.txt "<RECEIVER DEVICE ID>" C:\LinkHub\sender-trust-store.txt "Hello from B!"
```

**Expected:**
- 设备 B 显示 `Delivery acknowledged: ... AUTH_OK` 和 `Noise KK handshake complete`
- 设备 A 显示 `Authenticated text from ... [message_id]: Hello from B!`

---

## Step 6：设备 B — 发送加密文件

```powershell
# 先创建一个测试文件
echo "This is a test file for LinkHub" > C:\LinkHub\test.txt

cargo run -- send-file-auth 192.168.1.100:8787 C:\LinkHub\sender-id.txt "<RECEIVER DEVICE ID>" C:\LinkHub\sender-trust-store.txt C:\LinkHub\test.txt
```

**Expected:**
- 设备 B 显示 `FILE_START`, `FILE_CHUNK`, `FILE_END` 的 ACK 确认
- 设备 A 显示 `Authenticated file receive complete`，文件出现在 `C:\LinkHub\inbox\`

---

## Step 7 — 验证状态页

```powershell
# 设备 A
cargo run -- status-html secure:C:\LinkHub\receiver-id.secure.txt C:\LinkHub\trust-store.txt C:\LinkHub\status.html
# 用浏览器打开 C:\LinkHub\status.html
```

**Expected:** 显示本机身份和可信设备（设备 B）

---

## 故障排查

| 问题 | 检查 |
|------|------|
| 连接超时 | 确认两台设备能 ping 通；检查防火墙是否允许 8787 端口 |
| AUTH_UNTRUSTED | 确认 `trust-pairing` 已执行且 TrustStore 保存成功 |
| dh_key 错误 | 确认使用的是最新版 identity（含 X25519 key） |
| 文件收不到 | 检查接收目录是否存在；检查磁盘空间 |

---

## 通过标准

- [ ] 两台设备完成双向配对
- [ ] 文本发送成功且确认加密（看到 `Noise KK handshake complete`）
- [ ] 文件发送成功且文件内容一致
- [ ] 状态页正确显示本机和可信设备
- [ ] 双方都可以作为发送端和接收端
