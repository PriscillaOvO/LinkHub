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
}
