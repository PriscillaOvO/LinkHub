package com.linkhub.app.ui

import android.content.Context
import android.os.Build
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import com.google.gson.Gson
import com.google.gson.annotations.SerializedName
import com.linkhub.app.bridge.RustBridge
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import java.util.Collections
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

private const val LINKHUB_SERVICE_TYPE = "_linkhub._tcp."

data class DiscoveredPeerAddress(
    val deviceId: String,
    val deviceName: String,
    val fingerprint: String,
    val address: String,
    val publicKey: String = "",
    val dhPublicKey: String = "",
    val bindingSig: String = "",
    val trusted: Boolean = false
)

private data class VerifyIdentityResult(
    val success: Boolean = false,
    @SerializedName("device_id") val deviceId: String = "",
    @SerializedName("device_name") val deviceName: String = "",
    val fingerprint: String = "",
    val error: String = ""
)

private object AndroidMdnsAdvertiser {
    private var nsdManager: NsdManager? = null
    private var registrationListener: NsdManager.RegistrationListener? = null
    private var multicastLock: WifiManager.MulticastLock? = null

    fun start(ctx: Context, identity: IdentityJson, port: Int): String {
        stop(ctx)
        val appCtx = ctx.applicationContext
        val manager = appCtx.getSystemService(Context.NSD_SERVICE) as NsdManager
        val serviceName = "${sanitizeMdnsLabel(identity.deviceName)}-${sanitizeMdnsLabel(identity.deviceId)}"
        val identityJson = Gson().toJson(identity)
        val bindingSig = RustBridge.signIdentityBinding(identityJson)
            .takeUnless { it.startsWith("{\"error\"") }
            ?: ""
        val serviceInfo = NsdServiceInfo().apply {
            this.serviceName = serviceName
            serviceType = LINKHUB_SERVICE_TYPE
            this.port = port
            setAttribute("lh", "1")
            setAttribute("id", identity.deviceId)
            setAttribute("name", identity.deviceName)
            setAttribute("fp", identity.fingerprint)
            setAttribute("pk", identity.publicKey)
            setAttribute("dh", identity.dhPublicKey)
            setAttribute("sig", bindingSig)
            setAttribute("port", port.toString())
        }
        val lock = acquireMulticastLock(appCtx, "linkhub-mdns-advertise")
        val listener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) = Unit
            override fun onRegistrationFailed(info: NsdServiceInfo, errorCode: Int) = Unit
            override fun onServiceUnregistered(info: NsdServiceInfo) = Unit
            override fun onUnregistrationFailed(info: NsdServiceInfo, errorCode: Int) = Unit
        }
        manager.registerService(serviceInfo, NsdManager.PROTOCOL_DNS_SD, listener)
        nsdManager = manager
        registrationListener = listener
        multicastLock = lock
        return "$serviceName.$LINKHUB_SERVICE_TYPE"
    }

    fun stop(ctx: Context) {
        val manager = nsdManager ?: ctx.applicationContext.getSystemService(Context.NSD_SERVICE) as NsdManager
        registrationListener?.let {
            try {
                manager.unregisterService(it)
            } catch (_: Exception) {
            }
        }
        registrationListener = null
        nsdManager = null
        multicastLock?.let {
            if (it.isHeld) it.release()
        }
        multicastLock = null
    }
}

fun startAndroidMdnsAdvertise(ctx: Context, identity: IdentityJson, port: Int): String {
    return AndroidMdnsAdvertiser.start(ctx, identity, port)
}

fun stopAndroidMdnsAdvertise(ctx: Context) {
    AndroidMdnsAdvertiser.stop(ctx)
}

suspend fun scanTrustedMdnsPeers(
    ctx: Context,
    timeoutMillis: Long = 4_000
): List<DiscoveredPeerAddress> {
    return scanAndroidMdnsPeers(ctx, timeoutMillis).filter { it.trusted }
}

suspend fun scanAndroidMdnsPeers(
    ctx: Context,
    timeoutMillis: Long = 4_000
): List<DiscoveredPeerAddress> = withContext(Dispatchers.IO) {
    val trustedById = loadTrustedPeers(ctx).associateBy { it.deviceId }

    val appCtx = ctx.applicationContext
    val manager = appCtx.getSystemService(Context.NSD_SERVICE) as NsdManager
    val lock = acquireMulticastLock(appCtx, "linkhub-mdns-scan")
    val discovered = Collections.synchronizedList(mutableListOf<DiscoveredPeerAddress>())
    // NsdManager.registerServiceInfoCallback keeps posting to this executor on its
    // own ServiceHandler thread until the async unregister is confirmed — which can
    // arrive after we tear the scan down. A plain Executors.newSingleThreadExecutor()
    // uses AbortPolicy, so those late tasks throw RejectedExecutionException on a
    // framework thread and crash the whole app. DiscardPolicy silently drops any task
    // submitted after shutdown, making teardown race-safe.
    val executor = ThreadPoolExecutor(
        1, 1, 0L, TimeUnit.MILLISECONDS,
        LinkedBlockingQueue(),
        ThreadPoolExecutor.DiscardPolicy()
    )
    val serviceInfoCallbacks = Collections.synchronizedList(mutableListOf<NsdManager.ServiceInfoCallback>())

    val discoveryListener = object : NsdManager.DiscoveryListener {
        override fun onDiscoveryStarted(serviceType: String) = Unit
        override fun onDiscoveryStopped(serviceType: String) = Unit
        override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) = Unit
        override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) = Unit
        override fun onServiceLost(serviceInfo: NsdServiceInfo) = Unit

        override fun onServiceFound(serviceInfo: NsdServiceInfo) {
            if (serviceInfo.serviceType != LINKHUB_SERVICE_TYPE) return
            resolveNsdService(manager, executor, serviceInfo, serviceInfoCallbacks) { info ->
                val peer = discoveredPeerFrom(info, trustedById) ?: return@resolveNsdService
                discovered.removeAll {
                    it.deviceId == peer.deviceId && it.address == peer.address
                }
                discovered.add(peer)
            }
        }
    }

    try {
        manager.discoverServices(LINKHUB_SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, discoveryListener)
        delay(timeoutMillis.coerceIn(1_000, 15_000))
    } finally {
        try {
            manager.stopServiceDiscovery(discoveryListener)
        } catch (_: Exception) {
        }
        serviceInfoCallbacks.toList().forEach { callback ->
            try {
                manager.unregisterServiceInfoCallback(callback)
            } catch (_: Exception) {
            }
        }
        executor.shutdownNow()
        if (lock.isHeld) lock.release()
    }

    discovered.sortedWith(
        compareBy<DiscoveredPeerAddress> { it.deviceName }
            .thenBy { it.deviceId }
            .thenBy { it.address }
    )
}

private fun discoveredPeerFrom(
    info: NsdServiceInfo,
    trustedById: Map<String, TrustedPeer>
): DiscoveredPeerAddress? {
    val deviceId = nsdAttribute(info, "id") ?: return null
    val deviceName = nsdAttribute(info, "name") ?: info.serviceName
    val fingerprint = nsdAttribute(info, "fp") ?: ""
    val port = info.port.takeIf { it > 0 } ?: nsdAttribute(info, "port")?.toIntOrNull() ?: return null
    val host = nsdHostAddress(info) ?: return null
    val publicKey = nsdAttribute(info, "pk") ?: ""
    val dhPublicKey = nsdAttribute(info, "dh") ?: ""
    val bindingSig = nsdAttribute(info, "sig") ?: ""
    val trusted = trustedById[deviceId]
    val verified = verifyDiscoveredIdentity(deviceId, deviceName, publicKey, dhPublicKey, bindingSig)

    if (verified == null && trusted == null) return null

    return DiscoveredPeerAddress(
        deviceId = deviceId,
        deviceName = verified?.deviceName?.ifBlank { deviceName }
            ?: trusted?.deviceName?.ifBlank { deviceName }
            ?: deviceName,
        fingerprint = verified?.fingerprint?.ifBlank { fingerprint }
            ?: trusted?.fingerprint
            ?: fingerprint,
        address = "$host:$port",
        publicKey = verified?.let { publicKey } ?: trusted?.publicKey.orEmpty(),
        dhPublicKey = verified?.let { dhPublicKey } ?: trusted?.dhPublicKey.orEmpty(),
        bindingSig = verified?.let { bindingSig }.orEmpty(),
        trusted = trusted != null
    )
}

private fun verifyDiscoveredIdentity(
    deviceId: String,
    deviceName: String,
    publicKey: String,
    dhPublicKey: String,
    bindingSig: String
): VerifyIdentityResult? {
    if (publicKey.isBlank() || dhPublicKey.isBlank() || bindingSig.isBlank()) return null
    return try {
        val json = RustBridge.verifyIdentityBinding(deviceId, deviceName, publicKey, dhPublicKey, bindingSig)
        Gson().fromJson(json, VerifyIdentityResult::class.java).takeIf { it.success }
    } catch (_: Exception) {
        null
    }
}

private fun resolveNsdService(
    manager: NsdManager,
    executor: java.util.concurrent.Executor,
    serviceInfo: NsdServiceInfo,
    callbacks: MutableList<NsdManager.ServiceInfoCallback>,
    onResolved: (NsdServiceInfo) -> Unit
) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
        val callback = object : NsdManager.ServiceInfoCallback {
            override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {
                callbacks.remove(this)
            }

            override fun onServiceUpdated(info: NsdServiceInfo) {
                onResolved(info)
            }

            override fun onServiceLost() {
                callbacks.remove(this)
            }

            override fun onServiceInfoCallbackUnregistered() {
                callbacks.remove(this)
            }
        }
        callbacks.add(callback)
        try {
            manager.registerServiceInfoCallback(serviceInfo, executor, callback)
        } catch (_: Exception) {
            callbacks.remove(callback)
        }
    } else {
        resolveNsdServiceLegacy(manager, serviceInfo, onResolved)
    }
}

@Suppress("DEPRECATION")
private fun resolveNsdServiceLegacy(
    manager: NsdManager,
    serviceInfo: NsdServiceInfo,
    onResolved: (NsdServiceInfo) -> Unit
) {
    manager.resolveService(serviceInfo, object : NsdManager.ResolveListener {
        override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) = Unit
        override fun onServiceResolved(info: NsdServiceInfo) = onResolved(info)
    })
}

private fun nsdHostAddress(info: NsdServiceInfo): String? {
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
        info.hostAddresses.firstOrNull()?.hostAddress
    } else {
        @Suppress("DEPRECATION")
        info.host?.hostAddress
    }
}

private fun nsdAttribute(info: NsdServiceInfo, key: String): String? {
    return info.attributes[key]?.toString(Charsets.UTF_8)?.takeIf { it.isNotBlank() }
}

private fun acquireMulticastLock(ctx: Context, tag: String): WifiManager.MulticastLock {
    val wifi = ctx.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
    return wifi.createMulticastLock(tag).apply {
        setReferenceCounted(false)
        acquire()
    }
}

private fun sanitizeMdnsLabel(value: String): String {
    val sanitized = value
        .map { if (it.isLetterOrDigit() || it == '-') it else '-' }
        .joinToString("")
        .trim('-')
    return sanitized.ifBlank { "device" }
}
