import SwiftUI

struct DevicesView: View {
    @State private var identity: String?
    @State private var devices: [TrustedDevice] = []

    var body: some View {
        NavigationView {
            List {
                Section("This Device") {
                    if let id = identity {
                        Text(id).font(.caption).monospaced()
                    } else {
                        Button("Load Identity") { /* load from UserDefaults */ }
                    }
                }
                Section("Trusted (\(devices.count))") {
                    if devices.isEmpty {
                        Text("No trusted devices").foregroundColor(.secondary)
                    } else {
                        ForEach(devices, id: \.id) { device in
                            VStack(alignment: .leading) {
                                Text(device.name).font(.headline)
                                Text(device.id).font(.caption).monospaced()
                            }
                        }
                    }
                }
            }
            .navigationTitle("Devices")
        }
    }
}

struct TrustedDevice {
    let id: String
    let name: String
    let fingerprint: String
}
