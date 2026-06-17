package com.linkhub.app.ui

import android.content.Context

private const val SERVICE_STATUS_PREFS = "linkhub_service_status"

data class LinkHubServiceStatus(
    val running: Boolean = false,
    val listenAddr: String = "",
    val receiveDir: String = "",
    val detail: String = "",
    val error: String = "",
    val mdnsServiceName: String = "",
    val updatedAtMillis: Long = 0
)

fun saveServiceStatus(
    ctx: Context,
    running: Boolean,
    listenAddr: String = "",
    receiveDir: String = "",
    detail: String = "",
    error: String = "",
    mdnsServiceName: String = ""
) {
    ctx.getSharedPreferences(SERVICE_STATUS_PREFS, Context.MODE_PRIVATE)
        .edit()
        .putBoolean("running", running)
        .putString("listen_addr", listenAddr)
        .putString("receive_dir", receiveDir)
        .putString("detail", detail)
        .putString("error", error)
        .putString("mdns_service_name", mdnsServiceName)
        .putLong("updated_at_millis", System.currentTimeMillis())
        .apply()
}

fun loadServiceStatus(ctx: Context): LinkHubServiceStatus {
    val prefs = ctx.getSharedPreferences(SERVICE_STATUS_PREFS, Context.MODE_PRIVATE)
    return LinkHubServiceStatus(
        running = prefs.getBoolean("running", false),
        listenAddr = prefs.getString("listen_addr", "") ?: "",
        receiveDir = prefs.getString("receive_dir", "") ?: "",
        detail = prefs.getString("detail", "") ?: "",
        error = prefs.getString("error", "") ?: "",
        mdnsServiceName = prefs.getString("mdns_service_name", "") ?: "",
        updatedAtMillis = prefs.getLong("updated_at_millis", 0)
    )
}

fun reconcileServiceStatus(ctx: Context, liveRunning: Boolean): LinkHubServiceStatus {
    val status = loadServiceStatus(ctx)
    if (liveRunning || !status.running) return status

    ctx.getSharedPreferences(SERVICE_STATUS_PREFS, Context.MODE_PRIVATE)
        .edit()
        .putBoolean("running", false)
        .putString("listen_addr", status.listenAddr)
        .putString("receive_dir", status.receiveDir)
        .putString("detail", status.detail.ifBlank { "listener stopped" })
        .putString("error", status.error)
        .putString("mdns_service_name", status.mdnsServiceName)
        .putLong("updated_at_millis", System.currentTimeMillis())
        .apply()

    return status.copy(running = false, detail = status.detail.ifBlank { "listener stopped" })
}
