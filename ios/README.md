# LinkHub iOS

SwiftUI client for LinkHub. Full design + rationale: [docs/spec/设计-iOS-端.md](../docs/spec/设计-iOS-端.md).

> **Building iOS requires macOS + Xcode** (iOS SDK, `lipo`, `xcodebuild`). The
> repo is currently developed on Windows, so this directory is a **buildable
> scaffold**: everything below is committed so a Mac can go from clone → running
> app without reverse-engineering the layout.

## Layout

```
ios/
├── project.yml                 # XcodeGen project definition (text, not .pbxproj)
├── include/
│   ├── linkhub_core.h          # C ABI of the Rust core (core-rs/src/ios_bridge.rs)
│   └── module.modulemap        # exposes it to Swift as `import LinkHubCoreFFI`
├── scripts/build-core-ios.sh   # cross-compile core → Frameworks/LinkHubCore.xcframework
├── Frameworks/                 # (generated) LinkHubCore.xcframework — git-ignored
├── LinkHub/
│   ├── Info.plist              # local-network + Bonjour permission keys
│   └── LinkHub/                # Swift sources (App, ContentView, UI/, Service/, Bridge/)
```

## Build on a Mac

```sh
# 1. Rust core → static xcframework (installs the apple targets itself)
./ios/scripts/build-core-ios.sh release

# 2. Generate the Xcode project from project.yml
brew install xcodegen        # once
cd ios && xcodegen generate

# 3. Open and run
open LinkHub.xcodeproj
```

The app links `LinkHubCore.xcframework` statically and calls the `linkhub_*`
C functions through the `LinkHubCoreFFI` Clang module. The JSON contract across
the FFI is identical to the Android JNI bridge.

## Status (T9 scaffold)

Done this round: FFI module wired (`core-rs/src/ios_bridge.rs`, iOS-gated), C
header + module map, cross-compile script, `staticlib` crate-type, XcodeGen
project, Info.plist permission keys, `@main` entry + `ServiceView` (the source
tree previously had neither).

Not done (needs a Mac): generate/build the project, on-device local-network
permission test, background-transfer behaviour. The Swift `RustBridge` currently
covers identity + pairing only — send/listen FFI (mirroring the Android JNI
`sendText`/`sendFile`/`startListener`) is still to be added. iOS cross-network
(webrtc-rs) is deferred (C++ deps, unverified on iOS), opt-in like Android.

### Rust-for-iOS type-check

`ios_bridge` is `#[cfg(target_os = "ios")]`, so the default desktop/Android build
and test matrix are unaffected; the only core change is the extra `staticlib`
crate-type (produces a `.a`, leaves the `.so`/rlib unchanged). A host-side
`cargo check --target aarch64-apple-ios --lib` **passes from the Windows dev box**
(exit 0): with the `aarch64-apple-ios` std installed, the whole core — including
the now-compiled `ios_bridge` cfg path, plus `mdns-sd`, `tungstenite`, and all
crypto deps — type-checks for iOS without a Mac (no linking). The authoritative
compile/link/run still happens on macOS via the steps above.
