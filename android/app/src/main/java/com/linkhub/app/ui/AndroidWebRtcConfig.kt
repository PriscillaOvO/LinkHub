package com.linkhub.app.ui

import android.content.Context
import com.google.gson.Gson
import com.google.gson.annotations.SerializedName

private const val WEBRTC_PREFS_NAME = "linkhub_webrtc"
private const val DEFAULT_SIGNALING_URL = "ws://10.0.2.2:9000"
private const val DEFAULT_ICE_URLS = "stun:stun.l.google.com:19302"

data class AndroidWebRtcConfig(
    val signalingUrl: String = DEFAULT_SIGNALING_URL,
    val iceUrlsText: String = DEFAULT_ICE_URLS,
    val turnUsername: String = "",
    val turnCredential: String = "",
    val relayOnly: Boolean = false
)

private data class AndroidWebRtcIceConfig(
    @SerializedName("ice_urls") val iceUrls: List<String> = emptyList(),
    @SerializedName("turn_username") val turnUsername: String = "",
    @SerializedName("turn_credential") val turnCredential: String = "",
    @SerializedName("relay_only") val relayOnly: Boolean = false
)

fun loadWebRtcConfig(ctx: Context): AndroidWebRtcConfig {
    val prefs = ctx.getSharedPreferences(WEBRTC_PREFS_NAME, Context.MODE_PRIVATE)
    return AndroidWebRtcConfig(
        signalingUrl = prefs.getString("signaling_url", DEFAULT_SIGNALING_URL) ?: DEFAULT_SIGNALING_URL,
        iceUrlsText = prefs.getString("ice_urls_text", DEFAULT_ICE_URLS) ?: DEFAULT_ICE_URLS,
        turnUsername = prefs.getString("turn_username", "") ?: "",
        turnCredential = prefs.getString("turn_credential", "") ?: "",
        relayOnly = prefs.getBoolean("relay_only", false)
    )
}

fun saveWebRtcConfig(ctx: Context, config: AndroidWebRtcConfig) {
    ctx.getSharedPreferences(WEBRTC_PREFS_NAME, Context.MODE_PRIVATE)
        .edit()
        .putString("signaling_url", config.signalingUrl)
        .putString("ice_urls_text", config.iceUrlsText)
        .putString("turn_username", config.turnUsername)
        .putString("turn_credential", config.turnCredential)
        .putBoolean("relay_only", config.relayOnly)
        .apply()
}

fun webRtcIceConfigJson(gson: Gson, config: AndroidWebRtcConfig): String {
    return gson.toJson(
        AndroidWebRtcIceConfig(
            iceUrls = parseIceUrls(config.iceUrlsText),
            turnUsername = config.turnUsername.trim(),
            turnCredential = config.turnCredential.trim(),
            relayOnly = config.relayOnly
        )
    )
}

fun friendlyWebRtcStatus(status: String): String {
    return if (isWebRtcFeatureUnavailable(status)) {
        "需跨网包: 当前 Android .so 未启用 WebRTC，请用 --features webrtc 构建后打包"
    } else {
        status
    }
}

fun isWebRtcFeatureUnavailable(status: String): Boolean {
    return status.contains("cross-network WebRTC unavailable", ignoreCase = true) ||
        status.contains("--features webrtc", ignoreCase = true)
}

private fun parseIceUrls(text: String): List<String> {
    return text
        .split(Regex("[,\\s]+"))
        .map { it.trim() }
        .filter { it.isNotEmpty() }
}
