import SwiftUI

struct SendView: View {
    @State private var peerAddr = "192.168.1.100:8787"
    @State private var textInput = ""
    @State private var statusMsg = ""

    var body: some View {
        NavigationView {
            Form {
                Section("Peer") {
                    TextField("Address (IP:port)", text: $peerAddr)
                }
                Section("Text") {
                    TextEditor(text: $textInput).frame(minHeight: 80)
                    Button("Send Encrypted Text") {
                        statusMsg = "Text sent (via Rust core)"
                    }
                }
                Section("File") {
                    Button("Choose File & Send") {
                        statusMsg = "File picker TBD"
                    }
                }
                Text(statusMsg).foregroundColor(.blue)
            }
            .navigationTitle("Send")
        }
    }
}
