package com.linkhub.app.ui

import android.content.Context
import android.content.SharedPreferences
import android.os.Environment
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import com.google.gson.Gson
import com.google.gson.reflect.TypeToken
import java.io.File

private const val PREFS_NAME = "linkhub"
private const val SECURE_PREFS_NAME = "linkhub_secure"
private const val TRUSTED_PEERS_KEY = "trusted_peers_json"
private const val IDENTITY_KEY = "identity_json"
private const val TRUST_STORE_FILE = "linkhub-trust-store.txt"

data class TrustedPeer(
    val deviceId: String = "",
    val deviceName: String = "",
    val fingerprint: String = "",
    val publicKey: String = "",
    val dhPublicKey: String = "",
    val address: String = "",
    val pairedAtSecs: Long = System.currentTimeMillis() / 1000
)

fun saveIdentity(ctx: Context, identityJson: String) {
    securePrefs(ctx)
        .edit()
        .putString(IDENTITY_KEY, identityJson)
        .apply()
}

fun loadIdentityJson(ctx: Context): String? {
    return securePrefs(ctx)
        .getString(IDENTITY_KEY, null)
}

fun loadIdentity(ctx: Context): IdentityJson? {
    val saved = loadIdentityJson(ctx) ?: return null
    return try {
        Gson().fromJson(saved, IdentityJson::class.java)
    } catch (_: Exception) {
        null
    }
}

fun saveTrustedPeer(ctx: Context, deviceId: String, deviceName: String, fingerprint: String, payload: String) {
    val peer = trustedPeerFromPayload(deviceId, deviceName, fingerprint, payload) ?: return
    val prefs = securePrefs(ctx)
    val list = loadTrustedPeers(ctx).toMutableList()
    list.removeAll { it.deviceId == peer.deviceId }
    list.add(peer)
    prefs.edit().putString(TRUSTED_PEERS_KEY, Gson().toJson(list)).apply()
    writeRustTrustStore(ctx, list)
}

fun loadTrustedPeers(ctx: Context): List<TrustedPeer> {
    return try {
        val saved = securePrefs(ctx)
            .getString(TRUSTED_PEERS_KEY, "[]") ?: "[]"
        val type = object : TypeToken<List<TrustedPeer>>() {}.type
        Gson().fromJson<List<TrustedPeer>>(saved, type) ?: emptyList()
    } catch (_: Exception) {
        emptyList()
    }
}

fun updatePeerAddress(ctx: Context, deviceId: String, address: String) {
    val list = loadTrustedPeers(ctx).map {
        if (it.deviceId == deviceId) it.copy(address = address) else it
    }
    securePrefs(ctx)
        .edit()
        .putString(TRUSTED_PEERS_KEY, Gson().toJson(list))
        .apply()
    writeRustTrustStore(ctx, list)
}

fun trustStorePath(ctx: Context): String {
    return File(ctx.filesDir, TRUST_STORE_FILE).absolutePath
}

fun ensureRustTrustStore(ctx: Context): String {
    writeRustTrustStore(ctx, loadTrustedPeers(ctx))
    return trustStorePath(ctx)
}

fun defaultReceiveDir(ctx: Context): String {
    val base = ctx.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS) ?: ctx.filesDir
    return File(base, "LinkHub").apply { mkdirs() }.absolutePath
}

private fun trustedPeerFromPayload(
    deviceId: String,
    deviceName: String,
    fingerprint: String,
    payload: String
): TrustedPeer? {
    val fields = payload.trim().split("|")
    if (fields.size != 7 || fields[0] != "linkhub-pair-v2") return null

    val publicKey = fields[3]
    val dhPublicKey = fields[4]
    val issuedAtSecs = fields[5].toLongOrNull() ?: return null
    val ttlSecs = fields[6].toLongOrNull() ?: return null
    if (publicKey.isBlank() || dhPublicKey.isBlank() || issuedAtSecs <= 0 || ttlSecs <= 0) return null

    return TrustedPeer(
        deviceId = deviceId,
        deviceName = deviceName,
        fingerprint = fingerprint,
        publicKey = publicKey,
        dhPublicKey = dhPublicKey
    )
}

private fun writeRustTrustStore(ctx: Context, peers: List<TrustedPeer>) {
    val lines = mutableListOf("linkhub_trust_store_v1")
    peers.forEach { peer ->
        if (peer.deviceId.isBlank() || peer.publicKey.isBlank() || peer.dhPublicKey.isBlank()) {
            return@forEach
        }
        lines.add(
            "device=${hexUtf8(peer.deviceId)}|${hexUtf8(peer.deviceName)}|" +
                "${hexUtf8(peer.publicKey)}|${hexUtf8(peer.dhPublicKey)}|${peer.pairedAtSecs}"
        )
    }
    lines.add("")
    File(trustStorePath(ctx)).writeText(lines.joinToString("\n"))
}

private fun securePrefs(ctx: Context): SharedPreferences {
    val masterKey = MasterKey.Builder(ctx)
        .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
        .build()
    val prefs = EncryptedSharedPreferences.create(
        ctx,
        SECURE_PREFS_NAME,
        masterKey,
        EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
        EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
    )
    migrateLegacyPrefs(ctx, prefs)
    return prefs
}

private fun migrateLegacyPrefs(ctx: Context, securePrefs: SharedPreferences) {
    val legacyPrefs = ctx.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    val identity = legacyPrefs.getString(IDENTITY_KEY, null)
    val trustedPeers = legacyPrefs.getString(TRUSTED_PEERS_KEY, null)
    if (identity == null && trustedPeers == null) return

    securePrefs.edit().apply {
        if (identity != null && !securePrefs.contains(IDENTITY_KEY)) {
            putString(IDENTITY_KEY, identity)
        }
        if (trustedPeers != null && !securePrefs.contains(TRUSTED_PEERS_KEY)) {
            putString(TRUSTED_PEERS_KEY, trustedPeers)
        }
        apply()
    }
    legacyPrefs.edit()
        .remove(IDENTITY_KEY)
        .remove(TRUSTED_PEERS_KEY)
        .apply()
}

private fun hexUtf8(value: String): String {
    return value.toByteArray(Charsets.UTF_8).joinToString("") { "%02x".format(it) }
}
