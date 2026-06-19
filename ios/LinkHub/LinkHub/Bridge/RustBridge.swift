import Foundation

/// Swift wrapper around the Rust core library C FFI.
/// These call the `linkhub_*` functions exported from core-rs/src/ios_bridge.rs.
enum RustBridge {

    // ── Identity ──

    static func generateIdentity(deviceName: String) -> String? {
        guard let ptr = linkhub_generate_identity(deviceName.cString(using: .utf8)) else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }

    static func restoreIdentity(signingKeyHex: String, staticDhKeyHex: String, deviceName: String) -> String? {
        guard let ptr = linkhub_restore_identity(
            signingKeyHex.cString(using: .utf8),
            staticDhKeyHex.cString(using: .utf8),
            deviceName.cString(using: .utf8)
        ) else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }

    // ── Pairing ──

    static func generatePairingPayload(deviceId: String, deviceName: String,
                                        publicKey: String, dhPublicKey: String,
                                        ttlSeconds: UInt64) -> String? {
        guard let ptr = linkhub_generate_pairing_payload(
            deviceId.cString(using: .utf8), deviceName.cString(using: .utf8),
            publicKey.cString(using: .utf8), dhPublicKey.cString(using: .utf8),
            ttlSeconds
        ) else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }

    static func parsePairingPayload(identityJson: String, payload: String) -> String? {
        guard let ptr = linkhub_parse_pairing_payload(
            identityJson.cString(using: .utf8), payload.cString(using: .utf8)
        ) else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }

    static func confirmPairing(identityJson: String, payload: String,
                                confirmationCode: String) -> String? {
        guard let ptr = linkhub_confirm_pairing(
            identityJson.cString(using: .utf8), payload.cString(using: .utf8),
            confirmationCode.cString(using: .utf8)
        ) else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }

    // ── Local-network send/listen ──

    static func sendText(identityJson: String, peerAddr: String,
                         peerDeviceId: String, peerDhHex: String,
                         text: String) -> String? {
        guard let ptr = linkhub_send_text(
            identityJson.cString(using: .utf8), peerAddr.cString(using: .utf8),
            peerDeviceId.cString(using: .utf8), peerDhHex.cString(using: .utf8),
            text.cString(using: .utf8)
        ) else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }

    static func sendFile(identityJson: String, peerAddr: String,
                         peerDeviceId: String, peerDhHex: String,
                         filePath: String) -> String? {
        guard let ptr = linkhub_send_file(
            identityJson.cString(using: .utf8), peerAddr.cString(using: .utf8),
            peerDeviceId.cString(using: .utf8), peerDhHex.cString(using: .utf8),
            filePath.cString(using: .utf8)
        ) else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }

    static func startListener(identityJson: String, bindAddr: String,
                              trustStorePath: String, receiveDir: String) -> String? {
        guard let ptr = linkhub_start_listener(
            identityJson.cString(using: .utf8), bindAddr.cString(using: .utf8),
            trustStorePath.cString(using: .utf8), receiveDir.cString(using: .utf8)
        ) else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }

    static func stopListener() -> String? {
        guard let ptr = linkhub_stop_listener() else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }

    static func listenerStatus() -> String? {
        guard let ptr = linkhub_listener_status() else { return nil }
        defer { linkhub_free_string(ptr) }
        return String(cString: ptr)
    }
}
