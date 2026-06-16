package com.linkhub.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

@Composable
fun HistoryScreen() {
    val ctx = LocalContext.current
    var entries by remember { mutableStateOf(loadTransmissionHistory(ctx)) }
    val timeFormat = remember { SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault()) }

    Column(modifier = Modifier.fillMaxSize().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text("传输历史 (${entries.size})", style = MaterialTheme.typography.titleMedium)
            TextButton(onClick = {
                clearTransmissionHistory(ctx)
                entries = emptyList()
            }, enabled = entries.isNotEmpty()) {
                Text("清空")
            }
        }

        if (entries.isEmpty()) {
            Text("暂无传输记录", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                items(entries) { entry ->
                    HistoryEntryCard(entry, timeFormat)
                }
            }
        }
    }
}

@Composable
private fun HistoryEntryCard(entry: TransmissionHistoryEntry, timeFormat: SimpleDateFormat) {
    val isSuccess = entry.status == "success"
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                val directionLabel = if (entry.direction == "received") "↓ 接收" else "↑ 发送"
                Text(
                    "$directionLabel ${if (entry.kind == "file") "文件" else "文本"} · ${entry.peerDeviceName.ifBlank { entry.peerDeviceId }}",
                    style = MaterialTheme.typography.titleSmall
                )
                Text(
                    if (isSuccess) "成功" else "失败",
                    color = if (isSuccess) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall
                )
            }
            Text(
                timeFormat.format(Date(entry.timestampSecs * 1000)),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            if (entry.contentPreview.isNotBlank()) {
                Text(entry.contentPreview, style = MaterialTheme.typography.bodySmall)
            }
            if (entry.detail.isNotBlank()) {
                Text(entry.detail, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}
