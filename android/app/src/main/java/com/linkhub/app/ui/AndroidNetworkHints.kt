package com.linkhub.app.ui

import java.net.Inet4Address
import java.net.NetworkInterface

data class AndroidNetworkHint(
    val interfaceName: String,
    val address: String
)

fun localAndroidNetworkHints(port: Int): List<AndroidNetworkHint> {
    return NetworkInterface.getNetworkInterfaces()
        .toList()
        .filter { it.isUp && !it.isLoopback }
        .flatMap { network ->
            network.inetAddresses.toList()
                .filterIsInstance<Inet4Address>()
                .filter { !it.isLoopbackAddress && !it.isLinkLocalAddress }
                .map { addr ->
                    AndroidNetworkHint(
                        interfaceName = network.displayName ?: network.name,
                        address = "${addr.hostAddress}:$port"
                    )
                }
        }
        .distinctBy { it.address }
        .sortedWith(compareBy<AndroidNetworkHint> { it.interfaceName }.thenBy { it.address })
}
