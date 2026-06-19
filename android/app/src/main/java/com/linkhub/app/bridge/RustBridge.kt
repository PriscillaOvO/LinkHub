package com.linkhub.app.bridge

object RustBridge {
    init { System.loadLibrary("linkhub_core") }

    /**
     * Invoked from the native listener thread when an authenticated peer
     * finishes sending a file. Registered handler runs off the main thread.
     */
    @Volatile
    var onFileReceivedListener: ((
        peerDeviceId: String,
        peerDeviceName: String,
        fileName: String,
        filePath: String,
        sizeBytes: Long
    ) -> Unit)? = null

    @JvmStatic
    fun onFileReceived(
        peerDeviceId: String,
        peerDeviceName: String,
        fileName: String,
        filePath: String,
        sizeBytes: Long
    ) {
        try {
            onFileReceivedListener?.invoke(peerDeviceId, peerDeviceName, fileName, filePath, sizeBytes)
        } catch (_: Throwable) {
            // Never let an exception cross back into native code.
        }
    }

    // Identity
    external fun generateIdentity(deviceName: String): String
    external fun restoreIdentity(signingKeyHex: String, staticDhKeyHex: String, deviceName: String): String

    // Pairing
    external fun generatePairingPayload(identityJson: String, ttlSeconds: Long): String
    external fun parsePairingPayload(identityJson: String, payload: String): String
    external fun confirmPairing(identityJson: String, payload: String, confirmationCode: String): String

    // Send
    external fun sendText(identityJson: String, peerAddr: String, peerDeviceId: String, peerDhHex: String, text: String): String
    external fun sendFile(identityJson: String, peerAddr: String, peerDeviceId: String, peerDhHex: String, filePath: String): String

    // Listener
    external fun startListener(identityJson: String, bindAddr: String, trustStorePath: String, receiveDir: String): String
    external fun stopListener(): String
    external fun listenerStatus(): String

    // Cross-network (WebRTC). These only do real work when the native library is
    // built with the `webrtc` feature (`cargo ndk -P 24 ... build --features webrtc`,
    // minSdk 24); the default `.so` returns a JSON error so the symbols still link.
    // Both calls BLOCK (they run their own runtime + signaling) — invoke off the
    // main thread, and on the foreground service so Doze doesn't suspend them.
    // iceConfigJson: {"ice_urls":[...],"turn_username":"","turn_credential":"","relay_only":false}
    external fun webrtcSendFile(identityJson: String, trustStorePath: String, peerDeviceId: String, signalingUrl: String, iceConfigJson: String, filePath: String): String
    external fun webrtcReceiveFile(identityJson: String, trustStorePath: String, signalingUrl: String, iceConfigJson: String, receiveDir: String): String
}
