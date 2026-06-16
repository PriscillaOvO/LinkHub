import SwiftUI

/// QR code pairing screen.
struct PairView: View {
    @State private var deviceName = "iPhone"
    @State private var identityJson: String?
    @State private var payload = ""
    @State private var peerPayload = ""
    @State private var confirmationCode = ""
    @State private var inputCode = ""
    @State private var statusMsg = ""

    var body: some View {
        NavigationView {
            Form {
                // Identity
                Section("My Identity") {
                    TextField("Device Name", text: $deviceName)
                    Button("Generate Identity") {
                        if let json = RustBridge.generateIdentity(deviceName: deviceName) {
                            identityJson = json
                            statusMsg = "Identity created"
                        }
                    }
                    if let id = identityJson { Text(id).font(.caption) }
                }

                // Generate payload
                Section("My QR Payload") {
                    Button("Generate Payload") {
                        // Generate payload using stored identity
                        statusMsg = "Payload (scan from other device)"
                    }
                    if !payload.isEmpty {
                        Text(payload).font(.system(size: 10, design: .monospaced))
                    }
                }

                // Scan peer
                Section("Scan Peer") {
                    TextField("Paste peer payload", text: $peerPayload)
                    Button("Inspect") {
                        if let id = identityJson,
                           let result = RustBridge.parsePairingPayload(identityJson: id, payload: peerPayload) {
                            statusMsg = result
                        }
                    }
                    if !confirmationCode.isEmpty {
                        Text("Code: \(confirmationCode)").font(.largeTitle).monospaced()
                        TextField("Enter code", text: $inputCode)
                        Button("Confirm") {
                            if let id = identityJson,
                               let result = RustBridge.confirmPairing(
                                identityJson: id, payload: peerPayload,
                                confirmationCode: inputCode) {
                                statusMsg = result
                            }
                        }
                    }
                }

                Text(statusMsg).foregroundColor(.blue)
            }
            .navigationTitle("Pair")
        }
    }
}
