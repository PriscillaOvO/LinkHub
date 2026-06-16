package com.linkhub.app.ui

import android.content.Context
import com.google.gson.Gson
import com.google.gson.reflect.TypeToken

private const val HISTORY_PREFS_NAME = "linkhub_history"
private const val HISTORY_KEY = "entries_json"
private const val MAX_HISTORY_ENTRIES = 200

data class TransmissionHistoryEntry(
    val timestampSecs: Long = System.currentTimeMillis() / 1000,
    val direction: String = "sent",
    val peerDeviceId: String = "",
    val peerDeviceName: String = "",
    val kind: String = "text",
    val contentPreview: String = "",
    val status: String = "success",
    val detail: String = ""
)

fun appendTransmissionHistory(ctx: Context, entry: TransmissionHistoryEntry) {
    val entries = loadTransmissionHistory(ctx).toMutableList()
    entries.add(entry)
    val trimmed = entries
        .sortedByDescending { it.timestampSecs }
        .take(MAX_HISTORY_ENTRIES)
    ctx.getSharedPreferences(HISTORY_PREFS_NAME, Context.MODE_PRIVATE)
        .edit()
        .putString(HISTORY_KEY, Gson().toJson(trimmed))
        .apply()
}

fun loadTransmissionHistory(ctx: Context): List<TransmissionHistoryEntry> {
    return try {
        val saved = ctx.getSharedPreferences(HISTORY_PREFS_NAME, Context.MODE_PRIVATE)
            .getString(HISTORY_KEY, "[]") ?: "[]"
        val type = object : TypeToken<List<TransmissionHistoryEntry>>() {}.type
        Gson().fromJson<List<TransmissionHistoryEntry>>(saved, type) ?: emptyList()
    } catch (_: Exception) {
        emptyList()
    }.sortedByDescending { it.timestampSecs }
}

fun clearTransmissionHistory(ctx: Context) {
    ctx.getSharedPreferences(HISTORY_PREFS_NAME, Context.MODE_PRIVATE)
        .edit()
        .putString(HISTORY_KEY, "[]")
        .apply()
}
