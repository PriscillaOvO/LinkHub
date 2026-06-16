package com.linkhub.app.ui

import android.content.Context

/**
 * Handles a "file received" event surfaced from the native listener: records a
 * receive-direction history entry and posts a completion notification. Runs on
 * a native worker thread, so it only touches thread-safe APIs.
 */
fun handleReceivedFile(
    ctx: Context,
    peerDeviceId: String,
    peerDeviceName: String,
    fileName: String,
    filePath: String,
    sizeBytes: Long
) {
    val resolvedName = peerDeviceName.ifBlank {
        loadTrustedPeers(ctx).firstOrNull { it.deviceId == peerDeviceId }?.deviceName
            ?: peerDeviceId.ifBlank { "未知设备" }
    }
    val sizeLabel = formatBytes(sizeBytes)

    appendTransmissionHistory(
        ctx,
        TransmissionHistoryEntry(
            direction = "received",
            peerDeviceId = peerDeviceId,
            peerDeviceName = resolvedName,
            kind = "file",
            contentPreview = fileName,
            status = "success",
            detail = "已接收 $sizeLabel · $filePath"
        )
    )

    showReceivedFileNotification(
        ctx,
        peerKey = peerDeviceId.ifBlank { resolvedName },
        title = "已收到文件",
        detail = "$resolvedName: $fileName ($sizeLabel)"
    )
}

private fun formatBytes(bytes: Long): String {
    if (bytes < 1024) return "$bytes B"
    val units = arrayOf("KB", "MB", "GB", "TB")
    var value = bytes.toDouble() / 1024
    var unitIndex = 0
    while (value >= 1024 && unitIndex < units.size - 1) {
        value /= 1024
        unitIndex += 1
    }
    return String.format("%.1f %s", value, units[unitIndex])
}
