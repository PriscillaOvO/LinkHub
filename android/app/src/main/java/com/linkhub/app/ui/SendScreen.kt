package com.linkhub.app.ui

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Divider
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.unit.dp
import com.google.gson.Gson
import com.linkhub.app.bridge.RustBridge
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class SendResultJson(
    val success: Boolean = false,
    val detail: String = "",
    val error: String = ""
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendScreen() {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var identity by remember { mutableStateOf<IdentityJson?>(null) }
    var peers by remember { mutableStateOf<List<TrustedPeer>>(emptyList()) }
    var selectedPeer by remember { mutableStateOf<TrustedPeer?>(null) }
    var peerAddr by remember { mutableStateOf("") }
    var textInput by remember { mutableStateOf("") }
    var filePath by remember { mutableStateOf("/sdcard/Download/test.txt") }
    var pickedFileName by remember { mutableStateOf("") }
    var statusMsg by remember { mutableStateOf("") }
    var sending by remember { mutableStateOf(false) }
    var loaded by remember { mutableStateOf(false) }
    var lastAutoAddr by remember { mutableStateOf("") }
    val gson = remember { Gson() }

    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri: Uri? ->
        if (uri == null) return@rememberLauncherForActivityResult
        try {
            val picked = copyContentUriToSendCache(ctx, uri)
            filePath = picked.absolutePath
            pickedFileName = picked.name
            statusMsg = "已选择文件: ${picked.name}"
        } catch (e: Exception) {
            statusMsg = "选择文件失败: ${e.message}"
        }
    }

    LaunchedEffect(Unit) {
        try {
            identity = loadIdentity(ctx)
            peers = loadTrustedPeers(ctx)
        } catch (_: Exception) {
        }
        loaded = true
    }

    // Background auto-discovery: keep the selected device's LAN address
    // current without the user pressing 扫描并填入地址 (manual edits preserved).
    LaunchedEffect(Unit) {
        while (true) {
            try {
                val found = scanTrustedMdnsPeers(ctx)
                if (found.isNotEmpty()) {
                    found.forEach { updatePeerAddress(ctx, it.deviceId, it.address) }
                    peers = loadTrustedPeers(ctx)
                    val sel = selectedPeer
                    if (sel != null) {
                        val refreshed = peers.firstOrNull { it.deviceId == sel.deviceId }
                        if (refreshed != null) selectedPeer = refreshed
                        val latest = refreshed?.address ?: ""
                        if (latest.isNotBlank() && (peerAddr.isBlank() || peerAddr == lastAutoAddr)) {
                            peerAddr = latest
                            lastAutoAddr = latest
                        }
                    }
                }
            } catch (_: Exception) {
            }
            delay(10_000)
        }
    }

    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Text("加密发送", style = MaterialTheme.typography.titleMedium)

        if (!loaded) {
            Text("加载中...")
        } else if (identity == null) {
            Text("未找到身份，请先在配对页生成", color = MaterialTheme.colorScheme.error)
        } else {
            if (peers.isNotEmpty()) {
                Text("选择可信设备", style = MaterialTheme.typography.titleSmall)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                    peers.forEach { peer ->
                        FilterChip(
                            selected = selectedPeer?.deviceId == peer.deviceId,
                            onClick = {
                                selectedPeer = peer
                                peerAddr = peer.address.ifEmpty { peerAddr.ifEmpty { "192.168.1.100:8787" } }
                                lastAutoAddr = peerAddr
                                statusMsg = "已选择: ${peer.deviceName}"
                            },
                            label = { Text(peer.deviceName) }
                        )
                    }
                }
            } else {
                Text("暂无可信设备，请先在配对页添加。", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }

            OutlinedTextField(
                value = peerAddr,
                onValueChange = { value ->
                    peerAddr = value
                    selectedPeer?.let { peer -> updatePeerAddress(ctx, peer.deviceId, value) }
                },
                label = { Text("对方地址 (IP:端口)") },
                modifier = Modifier.fillMaxWidth()
            )
            OutlinedButton(
                onClick = {
                    val currentPeer = selectedPeer ?: return@OutlinedButton
                    sending = true
                    statusMsg = "正在扫描 ${currentPeer.deviceName} 的局域网地址..."
                    scope.launch {
                        try {
                            val found = scanTrustedMdnsPeers(ctx)
                                .firstOrNull { it.deviceId == currentPeer.deviceId }
                            if (found == null) {
                                statusMsg = "未发现 ${currentPeer.deviceName} 的局域网地址"
                            } else {
                                updatePeerAddress(ctx, currentPeer.deviceId, found.address)
                                peers = loadTrustedPeers(ctx)
                                selectedPeer = peers.firstOrNull { it.deviceId == currentPeer.deviceId } ?: currentPeer
                                peerAddr = found.address
                                statusMsg = "已更新地址: ${found.address}"
                            }
                        } catch (e: Exception) {
                            statusMsg = "扫描失败: ${e.message}"
                        } finally {
                            sending = false
                        }
                    }
                },
                enabled = selectedPeer != null && !sending
            ) {
                Text("扫描并填入地址")
            }

            selectedPeer?.let { peer ->
                Text("设备 ID: ${peer.deviceId}", style = MaterialTheme.typography.bodySmall)
            }

            Divider()

            Text("发送文本", style = MaterialTheme.typography.titleSmall)
            OutlinedTextField(
                value = textInput,
                onValueChange = { textInput = it },
                label = { Text("消息内容") },
                modifier = Modifier.fillMaxWidth(),
                maxLines = 3
            )
            Button(
                onClick = {
                    if (selectedPeer == null || textInput.isEmpty()) return@Button
                    val currentIdentity = identity ?: return@Button
                    val currentPeer = selectedPeer ?: return@Button
                    val currentAddr = peerAddr.trim()
                    val currentText = textInput
                    sending = true
                    statusMsg = "文本发送中..."
                    scope.launch {
                        statusMsg = sendTextOnIo(ctx, gson, currentIdentity, currentPeer, currentAddr, currentText)
                        sending = false
                    }
                },
                enabled = selectedPeer != null && textInput.isNotEmpty() && !sending
            ) {
                Text(if (sending) "发送中..." else "发送文本")
            }

            Divider()

            Text("发送文件", style = MaterialTheme.typography.titleSmall)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                Button(onClick = { filePicker.launch("*/*") }, enabled = !sending) {
                    Text("选择文件")
                }
                OutlinedButton(
                    onClick = {
                        if (selectedPeer == null || filePath.isEmpty()) return@OutlinedButton
                        val currentIdentity = identity ?: return@OutlinedButton
                        val currentPeer = selectedPeer ?: return@OutlinedButton
                        val currentAddr = peerAddr.trim()
                        val currentPath = filePath.trim()
                        sending = true
                        statusMsg = "文件发送中..."
                        scope.launch {
                            statusMsg = sendFileOnIo(ctx, gson, currentIdentity, currentPeer, currentAddr, currentPath)
                            sending = false
                        }
                    },
                    enabled = selectedPeer != null && filePath.isNotEmpty() && !sending
                ) {
                    Text(if (sending) "发送文件中..." else "发送文件")
                }
            }
            OutlinedTextField(
                value = filePath,
                onValueChange = {
                    filePath = it
                    pickedFileName = ""
                },
                label = { Text("文件路径") },
                modifier = Modifier.fillMaxWidth()
            )
            if (pickedFileName.isNotBlank()) {
                Text("将发送: $pickedFileName", style = MaterialTheme.typography.bodySmall)
            }

            Divider()
        }

        if (statusMsg.isNotEmpty()) {
            Text(
                statusMsg,
                color = if (statusMsg.contains("失败") || statusMsg.contains("Error")) {
                    MaterialTheme.colorScheme.error
                } else {
                    MaterialTheme.colorScheme.primary
                }
            )
        }
    }
}

private suspend fun sendTextOnIo(
    ctx: Context,
    gson: Gson,
    identity: IdentityJson,
    peer: TrustedPeer,
    peerAddr: String,
    text: String
): String = withContext(Dispatchers.IO) {
    try {
        showTransferNotification(ctx, peer, "text", "正在发送文本", peer.deviceName, inProgress = true)
        val result = RustBridge.sendText(
            gson.toJson(identity),
            peerAddr,
            peer.deviceId,
            peer.dhPublicKey,
            text
        )
        val parsed = parseSendResult(gson, result)
        val status = resultStatus(parsed, result, "文本已发送")
        recordSendHistory(ctx, peer, "text", text.take(100), parsed?.success == true, status)
        showTransferNotification(
            ctx,
            peer,
            "text",
            if (parsed?.success == true) "文本已发送" else "文本发送失败",
            "${peer.deviceName}: $status"
        )
        status
    } catch (e: Exception) {
        val status = "发送失败: ${e.message}"
        recordSendHistory(ctx, peer, "text", text.take(100), false, status)
        showTransferNotification(ctx, peer, "text", "文本发送失败", "${peer.deviceName}: $status")
        status
    }
}

private suspend fun sendFileOnIo(
    ctx: Context,
    gson: Gson,
    identity: IdentityJson,
    peer: TrustedPeer,
    peerAddr: String,
    filePath: String
): String = withContext(Dispatchers.IO) {
    try {
        val file = File(filePath)
        if (!file.exists() || !file.isFile) {
            val status = "发送失败: 文件不存在或不可读取"
            recordSendHistory(ctx, peer, "file", filePath, false, status)
            showTransferNotification(ctx, peer, "file", "文件发送失败", "${peer.deviceName}: $status")
            return@withContext status
        }
        showTransferNotification(ctx, peer, "file", "正在发送文件", "${peer.deviceName}: ${file.name}", inProgress = true)
        val result = RustBridge.sendFile(
            gson.toJson(identity),
            peerAddr,
            peer.deviceId,
            peer.dhPublicKey,
            file.absolutePath
        )
        val parsed = parseSendResult(gson, result)
        val status = resultStatus(parsed, result, "文件已发送")
        recordSendHistory(ctx, peer, "file", file.name, parsed?.success == true, status)
        showTransferNotification(
            ctx,
            peer,
            "file",
            if (parsed?.success == true) "文件已发送" else "文件发送失败",
            "${peer.deviceName}: ${file.name} - $status"
        )
        status
    } catch (e: Exception) {
        val status = "发送失败: ${e.message}"
        recordSendHistory(ctx, peer, "file", filePath, false, status)
        showTransferNotification(ctx, peer, "file", "文件发送失败", "${peer.deviceName}: $status")
        status
    }
}

private fun parseSendResult(gson: Gson, result: String): SendResultJson? {
    return try {
        gson.fromJson(result, SendResultJson::class.java)
    } catch (_: Exception) {
        null
    }
}

private fun resultStatus(parsed: SendResultJson?, rawResult: String, successMessage: String): String {
    if (parsed?.success == true) return successMessage
    val reason = parsed?.error?.ifBlank { parsed.detail }?.ifBlank { rawResult } ?: rawResult
    return "失败: $reason"
}

private fun recordSendHistory(
    ctx: Context,
    peer: TrustedPeer,
    kind: String,
    preview: String,
    success: Boolean,
    detail: String
) {
    appendTransmissionHistory(
        ctx,
        TransmissionHistoryEntry(
            peerDeviceId = peer.deviceId,
            peerDeviceName = peer.deviceName,
            kind = kind,
            contentPreview = preview,
            status = if (success) "success" else "failed",
            detail = detail
        )
    )
}

private fun copyContentUriToSendCache(ctx: Context, uri: Uri): File {
    val displayName = ctx.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
        val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
        if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
    } ?: "linkhub-file"

    val safeName = displayName.replace(Regex("""[\\/:*?"<>|]"""), "_")
    val sendDir = File(ctx.cacheDir, "linkhub-send").apply { mkdirs() }
    val target = uniqueCacheFile(sendDir, safeName)

    ctx.contentResolver.openInputStream(uri).use { input ->
        requireNotNull(input) { "无法打开文件" }
        target.outputStream().use { output -> input.copyTo(output) }
    }
    return target
}

private fun uniqueCacheFile(dir: File, name: String): File {
    val baseName = name.ifBlank { "linkhub-file" }
    var candidate = File(dir, baseName)
    if (!candidate.exists()) return candidate

    val dot = baseName.lastIndexOf('.')
    val stem = if (dot > 0) baseName.substring(0, dot) else baseName
    val ext = if (dot > 0) baseName.substring(dot) else ""
    var index = 1
    while (candidate.exists()) {
        candidate = File(dir, "${stem}_$index$ext")
        index += 1
    }
    return candidate
}
