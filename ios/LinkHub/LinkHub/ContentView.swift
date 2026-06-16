import SwiftUI

/// Main tabbed interface for LinkHub iOS.
struct ContentView: View {
    @State private var selectedTab = 0

    var body: some View {
        TabView(selection: $selectedTab) {
            PairView()
                .tabItem { Label("Pair", systemImage: "qrcode") }
                .tag(0)

            DevicesView()
                .tabItem { Label("Devices", systemImage: "laptopcomputer") }
                .tag(1)

            SendView()
                .tabItem { Label("Send", systemImage: "paperplane") }
                .tag(2)

            ServiceView()
                .tabItem { Label("Service", systemImage: "antenna.radiowaves.left.and.right") }
                .tag(3)
        }
    }
}
