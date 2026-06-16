package com.linkhub.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.Divider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

data class DeviceEntry(
    val deviceId: String = "",
    val deviceName: String = "",
    val fingerprint: String = "",
    val dhPublicKey: String = ""
)

private const val DEVICE_OFFLINE_AFTER_MS = 24_000L

fun isDeviceOnline(lastSeen: Long?, nowMs: Long): Boolean {
    return lastSeen != null && nowMs - lastSeen <= DEVICE_OFFLINE_AFTER_MS
}

fun devicePresenceLabel(address: String, lastSeen: Long?, nowMs: Long): String {
    if (isDeviceOnline(lastSeen, nowMs) && address.isNotBlank()) {
        val secs = ((nowMs - (lastSeen ?: nowMs)) / 1000).coerceAtLeast(0)
        val ago = if (secs <= 1) "刚刚" else "${secs}秒前"
        return "在线 · $address · $ago"
    }
    if (address.isNotBlank()) return "离线 · 上次 $address"
    return "离线 · 地址未知"
}

@Composable
fun DevicesScreen() {
    val ctx = LocalContext.current
    var identity by remember { mutableStateOf<IdentityJson?>(null) }
    var trustedDevices by remember { mutableStateOf<List<TrustedPeer>>(emptyList()) }
    var loaded by remember { mutableStateOf(false) }
    var scanning by remember { mutableStateOf(false) }
    var statusMsg by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    var presence by remember { mutableStateOf<Map<String, Long>>(emptyMap()) }
    var refreshTick by remember { mutableStateOf(0L) }

    LaunchedEffect(Unit) {
        try {
            identity = loadIdentity(ctx)
            trustedDevices = loadTrustedPeers(ctx)
        } catch (_: Exception) {
        }
        loaded = true
    }

    // Background auto-discovery: keep trusted devices' LAN addresses fresh
    // while this screen is visible, without the user pressing 扫描局域网.
    LaunchedEffect(Unit) {
        while (true) {
            try {
                val found = scanTrustedMdnsPeers(ctx)
                if (found.isNotEmpty()) {
                    found.forEach { updatePeerAddress(ctx, it.deviceId, it.address) }
                    val now = System.currentTimeMillis()
                    presence = presence + found.associate { it.deviceId to now }
                    trustedDevices = loadTrustedPeers(ctx)
                }
            } catch (_: Exception) {
            }
            refreshTick = System.currentTimeMillis()
            delay(10_000)
        }
    }

    val nowMs = if (refreshTick > 0L) refreshTick else System.currentTimeMillis()

    Column(
        modifier = Modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Text("本机", style = MaterialTheme.typography.titleMedium)

        if (identity != null) {
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(12.dp)) {
                    Text("名称: ${identity!!.deviceName}")
                    Text(
                        "ID: ${identity!!.deviceId}",
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall
                    )
                    Text(
                        "指纹: ${identity!!.fingerprint}",
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall
                    )
                }
            }
        } else if (loaded) {
            Text("未找到身份，请先在配对页生成", color = MaterialTheme.colorScheme.error)
        }

        Divider()

        Text("可信设备 (${trustedDevices.size})", style = MaterialTheme.typography.titleMedium)

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Button(
                onClick = {
                    scope.launch {
                        scanning = true
                        statusMsg = "正在扫描局域网可信设备..."
                        try {
                            val found = scanTrustedMdnsPeers(ctx)
                            found.forEach { updatePeerAddress(ctx, it.deviceId, it.address) }
                            trustedDevices = loadTrustedPeers(ctx)
                            statusMsg = if (found.isEmpty()) {
                                "未发现已配对的局域网设备"
                            } else {
                                "发现并保存 ${found.size} 个可信设备地址"
                            }
                        } catch (e: Exception) {
                            statusMsg = "扫描失败: ${e.message}"
                        } finally {
                            scanning = false
                        }
                    }
                },
                enabled = !scanning && trustedDevices.isNotEmpty()
            ) {
                Text(if (scanning) "扫描中..." else "扫描局域网")
            }
            OutlinedButton(
                onClick = {
                    trustedDevices = loadTrustedPeers(ctx)
                    statusMsg = "已刷新"
                }
            ) {
                Text("刷新")
            }
        }

        if (statusMsg.isNotBlank()) {
            Text(
                statusMsg,
                color = if (statusMsg.contains("失败")) {
                    MaterialTheme.colorScheme.error
                } else {
                    MaterialTheme.colorScheme.primary
                },
                style = MaterialTheme.typography.bodySmall
            )
        }

        if (trustedDevices.isEmpty()) {
            Text(
                "暂无可信设备，请先到配对页添加。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        } else {
            LazyColumn {
                items(trustedDevices) { device ->
                    Card(modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text(device.deviceName, style = MaterialTheme.typography.titleSmall)
                            Text(
                                "ID: ${device.deviceId}",
                                fontFamily = FontFamily.Monospace,
                                style = MaterialTheme.typography.bodySmall
                            )
                            if (device.fingerprint.isNotBlank()) {
                                Text(
                                    "指纹: ${device.fingerprint}",
                                    fontFamily = FontFamily.Monospace,
                                    style = MaterialTheme.typography.bodySmall
                                )
                            }
                            val online = isDeviceOnline(presence[device.deviceId], nowMs)
                            Text(
                                devicePresenceLabel(device.address, presence[device.deviceId], nowMs),
                                style = MaterialTheme.typography.bodySmall,
                                color = if (online) MaterialTheme.colorScheme.primary
                                    else MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    }
                }
            }
        }
    }
}
