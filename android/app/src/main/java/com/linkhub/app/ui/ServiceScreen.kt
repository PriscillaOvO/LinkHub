package com.linkhub.app.ui

import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
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
    val listenPort = listenAddr.substringAfterLast(':', "8787").toIntOrNull() ?: 8787
    val networkHints = remember(listenPort) { localAndroidNetworkHints(listenPort) }

    LaunchedEffect(Unit) {
        while (true) {
            isRunning = LinkHubService.isRunning
            serviceStatus = reconcileServiceStatus(ctx, isRunning)
            if (serviceStatus.listenAddr.isNotBlank()) listenAddr = serviceStatus.listenAddr
            if (serviceStatus.receiveDir.isNotBlank()) receiveDir = serviceStatus.receiveDir
            statusMsg = serviceStatus.error.ifBlank { serviceStatus.detail }
            delay(1_000)
        }
    }

    Column(
        modifier = Modifier.fillMaxSize().padding(16.dp),
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

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Button(
                onClick = {
                    try {
                        if (loadIdentityJson(ctx) == null) {
                            statusMsg = "请先在配对页生成身份"
                            return@Button
                        }
                        ensureRustTrustStore(ctx)
                        val intent = Intent(ctx, LinkHubService::class.java).apply {
                            putExtra("listen_addr", listenAddr)
                            putExtra("receive_dir", receiveDir)
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
