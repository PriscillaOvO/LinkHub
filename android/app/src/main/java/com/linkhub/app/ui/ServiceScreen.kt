package com.linkhub.app.ui

import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Divider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import com.google.gson.Gson
import com.linkhub.app.service.LinkHubService
import kotlinx.coroutines.delay

@Composable
fun ServiceScreen() {
    val ctx = LocalContext.current
    var serviceStatus by remember {
        mutableStateOf(reconcileServiceStatus(ctx, LinkHubService.isRunning))
    }
    // The persisted serviceStatus.running survives process death, so after the
    // app process is killed (reinstall, swipe-away, OS restart) it can still read
    // `running = true` from a previous session even though no service/listener is
    // actually alive. Trusting it would leave the UI showing "运行中" and disable
    // 启动监听 forever — a dead-lock where the listener can never be (re)bound.
    // LinkHubService.isRunning is a process-scoped static that is correctly false
    // in a fresh process, so use it as the single source of truth for liveness.
    var isRunning by remember { mutableStateOf(LinkHubService.isRunning) }
    var listenAddr by remember { mutableStateOf("0.0.0.0:8787") }
    var receiveDir by remember { mutableStateOf(defaultReceiveDir(ctx)) }
    var statusMsg by remember { mutableStateOf(serviceStatus.error.ifBlank { serviceStatus.detail }) }
    var webRtcEnabled by remember { mutableStateOf(false) }
    var webRtcConfig by remember { mutableStateOf(loadWebRtcConfig(ctx)) }
    var webRtcRunning by remember { mutableStateOf(LinkHubService.isWebRtcReceiving) }
    var webRtcDetail by remember { mutableStateOf(LinkHubService.webRtcDetail) }
    var webRtcError by remember { mutableStateOf(LinkHubService.webRtcError) }
    val gson = remember { Gson() }
    val listenPort = listenAddr.substringAfterLast(':', "8787").toIntOrNull() ?: 8787
    val networkHints = remember(listenPort) { localAndroidNetworkHints(listenPort) }

    fun updateWebRtcConfig(next: AndroidWebRtcConfig) {
        webRtcConfig = next
        saveWebRtcConfig(ctx, next)
    }

    LaunchedEffect(Unit) {
        webRtcConfig = loadWebRtcConfig(ctx)
        while (true) {
            isRunning = LinkHubService.isRunning
            serviceStatus = reconcileServiceStatus(ctx, isRunning)
            if (serviceStatus.listenAddr.isNotBlank()) listenAddr = serviceStatus.listenAddr
            if (serviceStatus.receiveDir.isNotBlank()) receiveDir = serviceStatus.receiveDir
            statusMsg = serviceStatus.error.ifBlank { serviceStatus.detail }
            webRtcRunning = LinkHubService.isWebRtcReceiving
            webRtcDetail = LinkHubService.webRtcDetail
            webRtcError = LinkHubService.webRtcError
            delay(1_000)
        }
    }

    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Text("监听服务", style = MaterialTheme.typography.titleMedium)
        Text(
            "启动前台服务，让本机持续接收已配对设备的加密传输。",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )

        OutlinedTextField(
            value = listenAddr,
            onValueChange = { listenAddr = it },
            label = { Text("监听地址") },
            modifier = Modifier.fillMaxWidth()
        )
        OutlinedTextField(
            value = receiveDir,
            onValueChange = { receiveDir = it },
            label = { Text("接收目录") },
            modifier = Modifier.fillMaxWidth()
        )
        TextButton(onClick = { receiveDir = defaultReceiveDir(ctx) }) {
            Text("使用应用专属接收目录")
        }

        Divider()
        Text("跨网络接收 (WebRTC)", style = MaterialTheme.typography.titleSmall)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(
                checked = webRtcEnabled,
                onCheckedChange = { webRtcEnabled = it },
                enabled = !isRunning
            )
            Text("随前台服务启动")
        }
        OutlinedTextField(
            value = webRtcConfig.signalingUrl,
            onValueChange = { updateWebRtcConfig(webRtcConfig.copy(signalingUrl = it)) },
            label = { Text("信令服务器 WebSocket URL") },
            modifier = Modifier.fillMaxWidth(),
            enabled = !isRunning
        )
        OutlinedTextField(
            value = webRtcConfig.iceUrlsText,
            onValueChange = { updateWebRtcConfig(webRtcConfig.copy(iceUrlsText = it)) },
            label = { Text("STUN/TURN URL") },
            modifier = Modifier.fillMaxWidth(),
            enabled = !isRunning,
            maxLines = 3
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
            OutlinedTextField(
                value = webRtcConfig.turnUsername,
                onValueChange = { updateWebRtcConfig(webRtcConfig.copy(turnUsername = it)) },
                label = { Text("TURN 用户名") },
                modifier = Modifier.weight(1f),
                enabled = !isRunning
            )
            OutlinedTextField(
                value = webRtcConfig.turnCredential,
                onValueChange = { updateWebRtcConfig(webRtcConfig.copy(turnCredential = it)) },
                label = { Text("TURN 凭证") },
                modifier = Modifier.weight(1f),
                enabled = !isRunning
            )
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(
                checked = webRtcConfig.relayOnly,
                onCheckedChange = { updateWebRtcConfig(webRtcConfig.copy(relayOnly = it)) },
                enabled = !isRunning
            )
            Text("仅使用 TURN 中继")
        }

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Button(
                onClick = {
                    try {
                        if (loadIdentityJson(ctx) == null) {
                            statusMsg = "请先在配对页生成身份"
                            return@Button
                        }
                        ensureRustTrustStore(ctx)
                        saveWebRtcConfig(ctx, webRtcConfig)
                        val intent = Intent(ctx, LinkHubService::class.java).apply {
                            putExtra("listen_addr", listenAddr)
                            putExtra("receive_dir", receiveDir)
                            putExtra("webrtc_receive_enabled", webRtcEnabled)
                            putExtra("webrtc_signaling_url", webRtcConfig.signalingUrl.trim())
                            putExtra("webrtc_ice_config_json", webRtcIceConfigJson(gson, webRtcConfig))
                        }
                        ContextCompat.startForegroundService(ctx, intent)
                        isRunning = true
                        statusMsg = "正在启动前台服务 ($listenAddr)"
                    } catch (e: Exception) {
                        statusMsg = "启动失败: ${e.message}"
                    }
                },
                enabled = !isRunning
            ) {
                Text("启动监听")
            }

            Button(
                onClick = {
                    try {
                        ctx.stopService(Intent(ctx, LinkHubService::class.java))
                        isRunning = false
                        statusMsg = "监听已停止"
                    } catch (e: Exception) {
                        statusMsg = "停止失败: ${e.message}"
                    }
                },
                enabled = isRunning,
                colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error)
            ) {
                Text("停止")
            }
        }

        Text(
            if (isRunning) "状态: 运行中 ($listenAddr)" else "状态: 已停止",
            style = MaterialTheme.typography.bodyMedium
        )
        if (serviceStatus.receiveDir.isNotBlank()) {
            Text("接收目录: ${serviceStatus.receiveDir}", style = MaterialTheme.typography.bodySmall)
        }
        if (serviceStatus.mdnsServiceName.isNotBlank()) {
            Text("mDNS: ${serviceStatus.mdnsServiceName}", style = MaterialTheme.typography.bodySmall)
        }
        Text(
            if (webRtcRunning) "跨网络: 接收中 (${webRtcConfig.signalingUrl})" else "跨网络: 未接收",
            style = MaterialTheme.typography.bodySmall
        )
        val webRtcStatusText = webRtcError.ifBlank { webRtcDetail }
        if (webRtcStatusText.isNotBlank()) {
            Text(
                friendlyWebRtcStatus(webRtcStatusText),
                color = if (webRtcError.isNotBlank()) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary,
                style = MaterialTheme.typography.bodySmall
            )
        }

        Divider()
        Text("本机地址提示", style = MaterialTheme.typography.titleSmall)
        if (networkHints.isEmpty()) {
            Text(
                "未检测到可用 IPv4 地址",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        } else {
            networkHints.forEach { hint ->
                Text("${hint.interfaceName}: ${hint.address}", style = MaterialTheme.typography.bodySmall)
            }
        }

        if (statusMsg.isNotEmpty()) {
            val isError = serviceStatus.error.isNotBlank() || statusMsg.contains("失败")
            Text(
                statusMsg,
                color = if (isError) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary
            )
        }
    }
}
