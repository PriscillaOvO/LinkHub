package com.linkhub.app.ui

import android.content.Context
import android.os.Build
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
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
    val address: String
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
        val serviceInfo = NsdServiceInfo().apply {
            this.serviceName = serviceName
            serviceType = LINKHUB_SERVICE_TYPE
            this.port = port
            setAttribute("lh", "1")
            setAttribute("id", identity.deviceId)
            setAttribute("name", identity.deviceName)
            setAttribute("fp", identity.fingerprint)
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
): List<DiscoveredPeerAddress> = withContext(Dispatchers.IO) {
    val trustedIds = loadTrustedPeers(ctx).map { it.deviceId }.toSet()
    if (trustedIds.isEmpty()) return@withContext emptyList()

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
                val peer = discoveredPeerFrom(info) ?: return@resolveNsdService
                if (peer.deviceId in trustedIds) {
                    discovered.removeAll {
                        it.deviceId == peer.deviceId && it.address == peer.address
                    }
                    discovered.add(peer)
                }
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

private fun discoveredPeerFrom(info: NsdServiceInfo): DiscoveredPeerAddress? {
    val deviceId = nsdAttribute(info, "id") ?: return null
    val deviceName = nsdAttribute(info, "name") ?: info.serviceName
    val fingerprint = nsdAttribute(info, "fp") ?: ""
    val port = info.port.takeIf { it > 0 } ?: nsdAttribute(info, "port")?.toIntOrNull() ?: return null
    val host = nsdHostAddress(info) ?: return null
    return DiscoveredPeerAddress(
        deviceId = deviceId,
        deviceName = deviceName,
        fingerprint = fingerprint,
        address = "$host:$port"
    )
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
