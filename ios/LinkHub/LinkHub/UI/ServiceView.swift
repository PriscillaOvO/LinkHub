import SwiftUI

/// Bonjour advertise/browse controls. Stub wired to `BonjourService`; the
/// listener loop + transfer plumbing over the Rust core FFI is still TODO
/// (tracked in ios/README.md and docs/spec/设计-iOS-端.md).
struct ServiceView: View {
    @State private var advertising = false
    private let bonjour = BonjourService()

    var body: some View {
        NavigationView {
            Form {
                Section("Discovery") {
                    Toggle("Advertise on local network", isOn: $advertising)
                        .onChange(of: advertising) { on in
                            if on {
                                bonjour.startBrowsing()
                            } else {
                                bonjour.stopAll()
                            }
                        }
                    Text("Requires Local Network permission (Settings → LinkHub).")
                        .font(.footnote)
                        .foregroundColor(.secondary)
                }
                Section("Core") {
                    LabeledContent("Bridge", value: "linkhub_* C FFI")
                }
            }
            .navigationTitle("Service")
        }
    }
}
