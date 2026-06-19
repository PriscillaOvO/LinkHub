import SwiftUI

/// App entry point. (Previously missing — the source tree had a `ContentView`
/// but no `@main`, so it could not launch.)
@main
struct LinkHubApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
