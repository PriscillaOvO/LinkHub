# Two-Device App (GUI) Test Checklist

> 目的：在**真实安装的应用**上验证 LinkHub 的完整闭环 —— 桌面安装包 + Android 签名 APK，
> 通过 GUI 完成配对与双向收发。与 CLI 版 [two-device-test.md](two-device-test.md) 并列：
> 那份验证 core/CLI，这份验证最终用户拿到的桌面应用与手机应用。

## 前提

- 一台 **Windows** + 一台 **Android 真机**，二者在**同一局域网**（同一路由器或同一热点）。
- 桌面安装包已产出：`scripts/build-desktop-installer.ps1` → `desktop/src-tauri/target/release/bundle/{nsis,msi}/`。
- Android 签名 APK 已产出：`scripts/build-android-release.ps1` → `android/app/build/outputs/apk/release/app-release.apk`。
- 安卓真机已开启「USB 调试」（用于 `adb install`），或直接把 APK 拷进手机安装。

> 说明：桌面端没有摄像头，**无法扫码**；所以配对方向是「桌面出二维码 → 手机扫」，
> 反向用「手机出 payload 文本 → 桌面粘贴」。两端都要互相信任后才能双向收发。

---

## Step 0 — 安装

### 桌面（Windows）
1. 运行 `desktop/src-tauri/target/release/bundle/nsis/` 下的 `*-setup.exe`（或 `msi/` 下的 `*.msi`）。
2. 启动 LinkHub，确认：窗口打开、系统托盘出现 LinkHub 图标（右键有「显示 LinkHub / 退出」）。
3. 再次双击启动一次，确认**没有开出第二个窗口**（单实例生效，只把已有窗口拉到前台）。

### Android（真机）
```powershell
adb install -r android\app\build\outputs\apk\release\app-release.apk
```
- 首次打开授予 **相机**（扫码用）与 **通知**（Android 13+，POST_NOTIFICATIONS）权限。

---

## Step 1 — 桌面端：初始化身份 + 启动监听

1. 打开 **Service** 标签页：若无身份，按提示「初始化身份」（设备名如 `Windows PC`）。
2. 在 Service 页**启动监听**（Start listener），记下监听地址 `0.0.0.0:8787`。
3. 打开 **Service / 网络提示**，记下本机局域网 IP（如 `192.168.1.100`）—— 手机发送时要用。

**Expected:** 状态栏从 `Not connected` 变为监听中；Service 页显示 listening。

---

## Step 2 — 桌面端出码，手机扫码配对

1. 桌面 **Pair** 标签页：生成本机配对二维码（对应 `pairing_generate_qr`）。
2. Android 打开 **Pair** 页 → 「扫码导入配对码」→ 对准桌面屏幕二维码。
3. 手机识别到 `linkhub-pair-v2|...` 后自动填入，核对**确认码/指纹**与桌面显示一致 → 确认信任。

**Expected:** Android 侧出现可信设备「Windows PC」；指纹两端一致。

---

## Step 3 — 反向配对（手机出码，桌面粘贴）

1. Android **Pair** 页：生成本机 payload（复制 `linkhub-pair-v2|...` 文本）。
2. 通过任意方式把该文本传到桌面（剪贴板同步 / 临时发一条文本均可）。
3. 桌面 **Pair** 页：把 payload 粘贴进「查看对方信息 / 确认配对」输入框（对应 `pairing_inspect` → `pairing_confirm`），核对确认码后确认。

**Expected:** 桌面 **Devices** 页出现可信设备「Android 手机」，显示设备 ID、指纹、地址。双向信任建立完成。

---

## Step 4 — 手机 → 桌面 收发

1. Android **Send** 页：选中可信设备「Windows PC」，填入桌面 IP:端口（如 `192.168.1.100:8787`）。
2. 发送一段**文本**。
3. 选一个**文件**（建议 >4KB 以覆盖多 chunk）发送。

**Expected:**
- 桌面 **History** 页出现该文本与该文件记录；文件落在桌面接收目录。
- 校验文件 SHA-256 与原文件一致（桌面 PowerShell：`Get-FileHash <收到的文件>`）。

---

## Step 5 — 桌面 → 手机 收发

1. 桌面 **Send** 页：选中可信设备「Android 手机」（地址由后台 mDNS 自动发现回填；未回填时手动填手机 IP:端口）。
2. 发送文本与文件。

**Expected:**
- 手机弹出**接收通知**；Android **History** 页出现记录；文件出现在 App 专属接收目录。
- 文件内容/哈希与原文件一致。

---

## Step 6 — 健壮性抽查

- 关闭桌面窗口（→托盘）后，从托盘「显示 LinkHub」恢复，监听仍在。
- 手机锁屏/切后台再回来，前台服务通知仍在、可继续收发。
- 断开再恢复 Wi-Fi 后，Devices 页能重新发现对端并回填地址。

---

## 故障排查

| 问题 | 检查 |
|------|------|
| 连接超时 | 两台能否互 ping；Windows 防火墙是否放行 8787（首次监听可能弹防火墙授权，选「专用网络」允许） |
| 手机扫不出码 | 相机权限是否授予；屏幕亮度/二维码是否完整在取景框内 |
| 收不到通知 | Android 13+ 是否授予 POST_NOTIFICATIONS；系统通知是否被关 |
| AUTH_UNTRUSTED | Step 2/3 双向信任是否都完成；指纹是否核对一致 |
| 发现不到对端 | 部分路由器隔离 mDNS/组播；改用手机热点或手动填 IP:端口 |
| App 启动即崩 | `.so` ABI 不匹配（真机多为 arm64-v8a）；确认 `build-android-so.ps1` 产出含 arm64-v8a |

---

## 通过标准

- [ ] 桌面安装包安装、启动、单实例、托盘均正常
- [ ] 签名 APK 安装、启动、权限授予正常
- [ ] 桌面↔手机完成双向配对，指纹一致
- [ ] 手机 → 桌面：文本 + 文件成功，哈希一致
- [ ] 桌面 → 手机：文本 + 文件成功，哈希一致，通知与历史正确
- [ ] 健壮性抽查（托盘恢复、后台、断网重连）通过

> 跑完把结果回填到 `docs/ai-handoff/shared/validation-log.md`。
