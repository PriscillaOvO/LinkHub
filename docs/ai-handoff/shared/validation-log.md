# Validation Log

## 2026-06-19 ~06:30 +08:00 Claude (T2: cross-network WebRTC file transfer on REAL Android emulator runtime — BOTH directions SHA-256 verified)

Finished + verified the T2 run that Codex started but couldn't capture before its quota ran out (Codex had verified A→B in-emulator; B→A had been launched but not pulled/checked). Emulators were still alive; I confirmed B→A landed and SHA-matched on the host.

- **Setup (Codex's, runtime-only — no code):** built Android **x86_64** `linkhub-cli` with `--features webrtc` via `cargo ndk -P 24 -t x86_64 build --bin linkhub-cli --features webrtc` (needs **minSdk 24** — webrtc-rs uses `getifaddrs`/`freeifaddrs` which are absent at API 21; this does NOT affect the default `cargo ndk check --lib` which stays green). Pushed the binary to two AVDs (`emulator-5554`=AVD-A `lh-2f8f7c970a0a54ef`, `emulator-5556`=AVD-B `lh-e0806865cf9e15f8`); ran it via `/system/bin/linker64 /data/local/tmp/linkhub-cli` (the tmp mount is noexec). Generated identities + paired both via CLI (`identity pairing-payload`/`pairing-code`/`trust-pairing`), confirmation codes matched. Started host `linkhub-signaling-server 0.0.0.0:9000`; emulators reach it at `10.0.2.2:9000`. ICE used public `stun:stun.l.google.com:19302`.
- **A→B (verified by Codex in-emulator, source SHA re-confirmed by me):** `connect-webrtc` 5554→5556 of `a-to-b.bin` (40000 B). DataChannel established, Noise KK handshake completed, 10 chunks, received file SHA-256 **`605a4914…1980d7` == source ✓**.
- **B→A (finished + verified by me):** `connect-webrtc` 5556→5554 of `b-to-a.bin` (40000 B). A-side listener log shows DataChannel established, Noise handshake complete, 10 chunks, **saved** (the auth session only renames `.part`→final when size **and** full-SHA verify pass). Pulled the received file to host: SHA-256 **`eb610cdc…bc264c` == source ✓** (`MATCH`).
- **Significance:** this is the first proof that the M3 WebRTC path **runs on real Android (webrtc-rs runtime, not just cross-compile) and traverses real (emulator) NAT via the signaling server + public STUN**, bidirectionally, end-to-end Noise-encrypted, byte-identical. The M3 in-process loopback test (`webrtc_e2e.rs`) is now backed by a real-runtime, real-NAT result.
- **Notes (not bugs):** webrtc Android build requires `-P 24` (minSdk 24); `/data/local/tmp` is noexec so the CLI runs via `linker64`; pairing payloads contain `|` so remote `adb shell` args must be single-quoted (test-harness detail). Default Android `.so` is unaffected (webrtc feature off) — measuring the webrtc-on `.so` size delta is still open (task T7).
- **Not done:** real physical devices (emulators only); arm64 webrtc runtime (only x86_64 emulator exercised; arm64 covered by build/spike); TURN forced-relay (public STUN sufficed here, NAT was permissive). Left the two emulators + signaling-server running at session end (Codex started them) — safe to kill.
- Code state: clean working tree; T1 committed `b8ef2e8`. This session = validation only; updated `docs/spec/*` (项目状态/路线图/设计§9) + this log + handoff-clock.

## 2026-06-18 03:30 +08:00 Claude (cross-network transport M1: decouple authenticated session from TcpStream)

- Wrote design doc `docs/spec/设计-跨网络传输-webrtc.md` (stage-5 cross-network architecture, signaling, pipe selection, milestones, open decisions), then implemented M1.
- M1 (core-rs only, public API unchanged): abstracted the authenticated session off `TcpStream`. New transport-agnostic entry points `run_authenticated_session_over<W: Write, R: BufRead>` (responder) and `perform_initiator_handshake<W, R>` (initiator); `run_authenticated_session(TcpStream)` / `open_authenticated_stream(addr)` kept as thin TCP wrappers. Generic-ized `send_encrypted_with_ack_retries`, `send_encrypted_file_start_with_retries`, `send_encrypted_frame`, `recv_encrypted_frame`, `wait_for_auth_challenge`, `wait_for_ack`, and `ack::write_message`.
- Added in-crate unit test `authenticated_text_round_trips_over_in_memory_transport` that runs the full handshake + encrypted TEXT + ACK over a blocking in-memory duplex (no sockets) — proves transport-independence beyond the type level. The TCP path stays covered by `tests/e2e.rs`.
- Verified: `cargo fmt`; `cargo test` PASS (115 lib + 16 CLI + 1 cross-process CLI e2e + 4 e2e + 1 new transport test); `cargo clippy --all-targets` exit 0 with NO warnings referencing my changed files (pre-existing DPAPI/style lints elsewhere remain); `cargo ndk -t arm64-v8a -t x86_64 check --lib` PASS both ABIs (jni_bridge unaffected); `cargo check` (desktop) PASS. TCP behavior unchanged.
- Decisions locked by user: groundwork-first (M1), pipe tech leans libp2p (confirm via cross-compile spike before M2), Claude implements.
- Not done: M2+ (signaling server, WebRTC); pipe-tech cross-compile spike; `/code-review ultra` on an M1 PR (recommended next).
- Uncommitted: design doc, 开发路线图.md §5 pointer, 项目状态.md, M1 code (auth_session.rs, ack.rs), handoff docs — pending user go-ahead.

## 2026-06-18 02:58 +08:00 Claude (task b linkhub-pair-v2 two-emulator UI re-validation on RELEASE APK)

- Verified clean working tree, `main` == `origin/main` (0/0 ahead/behind).
- `apksigner verify --print-certs android/app/build/outputs/apk/release/app-release.apk`: PASS — V2 signer, `CN=LinkHub Dev`, SHA-256 `ae2bbe03e5fd0bba9d15de812baecf4155f578e37d5d549d9487260a936da6bc` (same cert as prior validated builds; updates installs in place; no "DOES NOT VERIFY").
- Clean-installed the freshly built **release** APK on both AVDs (`LinkHub_API_34`=5554, `LinkHub_API_34_b`=5556). Testing release (not debug) also exercises R8/proguard. Drove Compose UI via `adb` (`uiautomator dump` + coordinate taps + `input text`); helper `.screenshots/drive.sh` (git-ignored, updated its `payload_from_dump` grep from `linkhub-pair-v1`→`linkhub-pair-v[12]`).
- **v2 pairing — VALIDATED end-to-end on release UI:**
  - Generated codes parse as v2: `linkhub-pair-v2|id|name|pub|dh|issued_at|ttl` (7 pipe fields, TTL=120s, no nonce). 5554=`lh-e764558cebc2c1fb` fp `47AF-746E-BD17-47CE`; 5556=`lh-7349419be21291a2` fp `FA8A-81DA-821A-2959`.
  - **Confirmation code 40-bit / 10-hex** displayed `XXXXX-XXXXX`: both sides showed the SAME code `CCBE4-7578F` (symmetric over sorted fingerprints; verified by each viewing the other's payload).
  - **TTL enforcement WORKS**: pasting a payload aged ~180s (>120s TTL) → peer-info fields (`设备/ID/指纹/确认码`) all empty and **确认配对 disabled** (rejected). Fresh payloads (<120s) populate normally. NOTE: the 120s window is tight for slow adb UI driving — `confirmPairing` re-parses the pasted payload string so it must still be fresh at confirm time; succeeded by generating peer code immediately before each confirm.
  - **Mutual trust established**: both devices' Devices page shows `可信设备 (1)` with the peer's id+fingerprint.
- **Bidirectional transfer — VALIDATED on release, SHA-256 matched both ways.** Networking: receiver `adb forward tcp:<hostport> tcp:8787`, sender `adb reverse tcp:<port> tcp:<hostport>` → app dials `127.0.0.1:<port>` (A→B used host:18787, B→A used host:18788).
  - Text A→B: sender 5554 History `↑ 发送 文本 · 我的安卓` 成功 (`hello-v2-A-to-B`).
  - File A→B `lhtest_a.txt`: received on 5556 at `…/files/Download/LinkHub/`, **SHA-256 `cd5d5c…02ab8` identical** to source; sender History `↑ 发送 文件` 成功, receiver History `↓ 接收 文件`.
  - Text B→A: sender 5556 History `↑ 发送 文本` 文本已发送/成功 (`hello-v2-B-to-A`).
  - File B→A `lhtest_b.txt` (54 B): received on 5554, **SHA-256 `f0fc32…cc81` identical**; sender 5556 History `↑ 发送 文件` 成功, receiver 5554 History `↓ 接收 文件`.
- Test-harness notes (NOT product bugs): (a) Android only registers `onFileReceivedListener` (LinkHubService.kt:56) — received **text** is decrypted in core but intentionally not surfaced in UI/history, so a successful text send shows only on the SENDER side. (b) Native `File.exists()`/`std::fs` cannot read shell-written files under `/storage/emulated/0/Android/data/<pkg>/files/` on a release (non-debuggable) build — must use the in-app **选择文件** SAF picker, which copies into internal `cacheDir/linkhub-send/` (native-readable). The one `失败` History entry on 5554 is my initial bad-path attempt before switching to the picker.
- NOT run this session: real-device install smoke (user has one device); arm64 runtime (only x86_64 emulators exercised at runtime; arm64 covered by build).
- No tracked code changed this session (validation only). `docs/spec/*` synced. `.screenshots/` test files/screenshots are git-ignored and removed after use.

## 2026-06-18 Codex (task b linkhub-pair-v2 pairing security hardening)

- Fixed the two confirmed xhigh pairing findings: v2 payload now carries an absolute `issued_at` Unix timestamp and `ttl`, so receivers can enforce expiry with `SystemTime`; confirmation code widened from 24 bits to 40 bits while staying symmetric over sorted fingerprints only.
- Wire format is now `linkhub-pair-v2|device_id_hex|device_name_hex|public_key|dh_public_key|issued_at_unix_seconds|ttl_seconds`; nonce is removed. Existing v1 payloads are rejected with a regenerate/re-pair error. Existing paired devices may need re-pairing where the old payload/trust flow is stale.
- Synced protocol consumers: Rust core, CLI, JNI bridge, iOS bridge, Tauri desktop commands, Android `trustedPeerFromPayload`, Android scanner prefix, desktop/script/NFC hints.
- Added defense/observability: Android JNI installs a process-wide `std::panic::set_hook` with `android_logger` and exposes the last native panic through `listenerStatus.error`; Android UI reconciles stale persisted `service_status.running=true` to false when the fresh process has no live listener.
- Added/updated tests: expired payload rejected, unexpired payload accepted, v2 round-trip, two-sided code equality, distinct pair code inequality, sort symmetry, v2 code length.
- Verified: `cargo test --manifest-path core-rs/Cargo.toml` (115 + 16 + 1 + 4 + doctests) passed with no warnings; `cargo build --manifest-path core-rs/Cargo.toml` passed with no warnings; `cargo ndk -t arm64-v8a -t x86_64 check --lib` passed; `android\gradlew.bat :app:compileDebugKotlin` passed; `scripts/build-android-so.ps1` rebuilt arm64-v8a/x86_64 `.so`; `:app:assembleDebug` and `:app:assembleRelease` passed; `cargo check --manifest-path desktop/src-tauri/Cargo.toml` passed with no warnings; `node --check desktop/src/js/pairing.js` passed.
- Artifact hygiene: `git ls-files "*.so" "*.apk" "*.aab" "*.jks" "*.keystore" "android/keystore.properties"` returned empty.
- Not run: two-emulator UI pairing/transfer replay and real-device install smoke.

## 2026-06-17 Claude (xhigh code-review of b3810c9 + @Volatile fix)

- Ran `/code-review xhigh` (multi-agent) on commit b3810c9. The 5 fixes themselves verified sound; review surfaced deeper, mostly PRE-EXISTING pairing-security weaknesses.
- Findings (severity order): (#1 CONFIRMED) pairing TTL is toothless — `jni_bridge` parse/confirm pass `Instant::now()` as created_at so `is_expired` is always false; payload carries only a TTL duration, no absolute issue-time. (#2 CONFIRMED, nuance) dropping the nonce makes the 24-bit (6-hex) confirmation code offline-precomputable for a short-code MITM collision; the stable-SAS-from-keys design is fine (Signal-style) — fix is to widen the code + add an absolute timestamp, NOT re-add the nonce. (#3 CONFIRMED, FIXED) `LinkHubService.isRunning` lacked `@Volatile` while now the sole cross-thread liveness source. (#4) `catch_unwind` only covers the accept-loop thread; per-connection workers + no `std::panic::set_hook` → native panics invisible. (#5) stale `service_status.running` is still written (latent trap). (#6/#7/#8/#9 LOW/INFO).
- FIXED this session: #3 — added `@Volatile` to `LinkHubService.isRunning` (+ comment). #7 (partial) — nonce is dead weight on the trust path but is the 6th field of the 7-field `linkhub-pair-v1` format that the Kotlin parser requires exactly; removing it is a wire-format break, so only added a clarifying comment on the field and deferred removal to the planned v2.
- Verified: `core-rs cargo check` ok; `:app:compileDebugKotlin` ok. Rust change is comment-only (no `.so` rebuild needed); the `@Volatile` change needs an APK rebuild to ship. Both edits UNCOMMITTED.
- Deferred to next session (task b): payload v2 with absolute timestamp (enforce TTL) + drop nonce; widen confirmation code; one-time panic hook + android_logger; stop persisting `service_status.running` as a liveness source. Recommend a dedicated `/code-review ultra` on the pairing-security design first.

## 2026-06-17 Claude (Android↔Android two-emulator validation — found+fixed 4 bugs)

- Goal: validate Android↔Android bidirectional encrypted text+file using two emulators (user has only one real device). Created a 2nd AVD `LinkHub_API_34_b`; booted both headless (5554=A, 5556=B). Drove the Compose UI entirely via `adb` (`uiautomator dump` + coordinate taps + `input text`); helpers live in git-ignored `.screenshots/drive.sh`.
- Cross-emulator networking (no mDNS under emulator NAT): receiver exposed via `adb forward tcp:<hostport> tcp:8787`, sender reaches it via `adb reverse tcp:<port> tcp:<hostport>` → app dials `127.0.0.1:<port>`. (`10.0.2.2`→host-forward path did NOT work here — timed out.)
- **RESULT — Android↔Android FULLY VALIDATED on debug**: A→B and B→A both text ("文本已发送") and file. Received files SHA-256 **identical** to source on both directions; receive callback fired → receive notification + History "↓ 接收 文件 · <peer> / 成功"; sender History "↑ 发送 …". (Authenticated Noise session: sender only returns success after the receiver's final ACK, so success ⇒ end-to-end decrypt+verify on the peer.)

Four real bugs found and FIXED this session (all reproduced on the emulator, not external-env issues):

1. **Listener never (re)bound after process death — the big one.** After the app process is killed (reinstall / swipe-away / OS restart), the persisted `service_status.running=true` (from a prior session) was still read by `ServiceScreen`, so the UI showed "运行中", **disabled 启动监听**, and 停止 was a no-op on the already-dead service → dead-lock where nothing ever bound `0.0.0.0:8787` (confirmed: `ss`/`/proc/net/tcp` had no 8787, in-guest connect = "Connection refused"), yet UI claimed running. This silently broke ALL inbound transfers. FIX: `ServiceScreen` now derives `isRunning` solely from the process-scoped `LinkHubService.isRunning` static (correctly false in a fresh process), not the persisted flag. Verified: after fix, status resets to "已停止", button enabled, start → `ss` shows `LISTEN 0.0.0.0:8787`, and a real HELLO gets `AUTH_CHALLENGE`.
2. **NSD/mDNS crash (`RejectedExecutionException`) crashing the whole app** on the Send/Devices background scan. `scanTrustedMdnsPeers` passed `Executors.newSingleThreadExecutor()` (AbortPolicy) to `registerServiceInfoCallback`, then `shutdownNow()` in `finally`; NsdManager's async unregister keeps posting to it on a framework thread → uncaught RejectedExecutionException → crash within ~10s of opening Send/Devices whenever a peer exists. FIX: use a `ThreadPoolExecutor` with `DiscardPolicy` so late tasks are silently dropped. Verified: 25s on Send page across multiple scan cycles, 0 FATAL, app stays foreground.
3. **Release (R8) trusted-peer persistence broken.** `loadTrustedPeers` uses `TypeToken<List<TrustedPeer>>`; R8 full mode (AGP 8+ default) strips the anonymous TypeToken's generic arg → list deserializes empty. Pairing reported "已信任" but Devices/Send showed 0 (debug showed 1 — isolating R8). Identity (non-generic `fromJson`) was unaffected. FIX: `-keep class com.google.gson.reflect.TypeToken { *; }` + `-keep class * extends ...TypeToken`. **Verified on a fresh RELEASE build**: pair → Devices "可信设备 (1)" (was 0).
4. **Listener worker panic could leave a fake-running state (defensive).** `jni_bridge::startListener`'s worker now wraps `run_authenticated_listener_on_with_callback` in `catch_unwind`, recording any panic into `last_error` and always clearing `LISTENER_RUNNING` on exit (Android has no panic hook, so such panics were previously invisible AND left RUNNING stuck true).

- Builds/tests after fixes: `core-rs cargo test` green (0 failures); rebuilt both ABIs `.so` via `cargo ndk` (arm64-v8a + x86_64, include catch_unwind); `:app:assembleDebug` and `:app:assembleRelease` BUILD SUCCESSFUL (R8 + signing).
- NOT done: real-device re-run (user's one device) — emulator validation stands in for the two-device loop; arm64 path exercised only by build, runtime transfer verified on x86_64 emulators. Changes UNCOMMITTED (user hasn't asked to commit).
- FIXED (UX/security): the two devices displayed DIFFERENT confirmation codes for the same pair — defeating the cross-device compare. Root cause: the code mixed in the invitation nonce, but each side only holds the PEER's nonce in its session, so the two codes differed. Fix: `confirmation_code` now depends only on the two sorted fingerprints (drop nonce); MITM protection still comes from binding both public keys. Verified on the two emulators: both now show the same code (e.g. 6DE-352). Regression test `pairing_code_is_same_on_both_devices` updated to use DIFFERENT nonces per side and still assert equality. `core-rs cargo test` green; both `.so` + debug APK rebuilt.

## 2026-06-17 Claude (Android listener-restart + release Gson fixes)

- Context: resumed an interrupted real-device bug-fix session. Working tree already had desktop fixes (汉化/CSP `data-act`/卡顿, documented + verified) plus a HALF-DONE Android fix: `jni_bridge.rs` had added `LISTENER_EPOCH`/`LISTENER_HANDLE`/`stop_and_join_listener()` but never wired them in (dead code, no effect), and `proguard-rules.pro` Gson-keep rules.
- Completed the wiring: `startListener` now `stop_and_join_listener()` before bind, claims a new `LISTENER_EPOCH`, stores the `JoinHandle`, and only clears `LISTENER_RUNNING` on exit if still the current generation; `stopListener` now joins the worker before returning.
- `cargo ndk -t arm64-v8a check --lib` (Android target, where `#[cfg(target_os="android")]` jni_bridge actually compiles): passed — only the pre-existing `net/protocol.rs` dead_code warning; NO new dead-code warnings (confirms the new statics/helper are all used now).
- `core-rs cargo test --quiet`: passed (110 + 16 unit, 1 doc, 4 e2e; 0 failures).
- `desktop/src-tauri cargo check`: passed (only pre-existing protocol.rs warning).
- Rebuilt release `.so` both ABIs via `cargo ndk -t arm64-v8a -t x86_64 -o ../android/app/src/main/jniLibs build --release --lib`: ok, copied to jniLibs.
- `gradlew :app:assembleRelease`: BUILD SUCCESSFUL (R8 minify + resource shrink + `validateSigningRelease`). Artifact `app-release.apk` (~23.8 MB), built fresh today.
- `apksigner verify --print-certs`: V2 signed, cert `CN=LinkHub Dev`, SHA-256 `ae2bbe03…a6bc` — SAME cert as the prior validated APK, so it updates the user's existing install in place.
- NOT verified (needs the user's real device): stop→restart of the listener service no longer fails; release-build identity generation no longer hits "创建失败". A truncated-address send failure seen during testing (`172.20.10:8787`, missing last octet) looked like a manual mid-test typo (a correct `172.20.10.5:8787` worked earlier); NOT treated as a code bug — flag for the user to confirm.
- Reinstalled on the user's real device (Vivo V2454A) via `adb`: old build was differently-signed (debug), so `install -r` failed `INSTALL_FAILED_UPDATE_INCOMPATIBLE`; uninstall + fresh `install` of the signed release succeeded (clean install, on-device identity/trust/history wiped — re-pair needed). App launches.
- Real-device test screenshots were transient and have been deleted per user request (screenshots now live in a git-ignored `.screenshots/` folder and are removed once used).
- Changes UNCOMMITTED at time of writing (user hadn't asked to commit).

## 2026-06-16 Claude (Android signed release APK validation)

- `keytool -genkeypair` (dev/test keystore `android/app/linkhub-release.jks`, RSA 2048, validity 10000): passed (exit 0). Credentials in git-ignored `android/keystore.properties`.
- `scripts/build-android-release.ps1`: passed. `build-android-so.ps1` produced arm64-v8a/x86_64 release `.so`; `gradlew.bat :app:assembleRelease` BUILD SUCCESSFUL (incl. `minifyReleaseWithR8`, resource shrink, `validateSigningRelease`). Gradle 8.13.
- Artifact: `android/app/build/outputs/apk/release/app-release.apk` (~24.9 MB).
- `apksigner verify --print-certs`: passed (exit 0). V2 signer, cert `CN=LinkHub Dev`, SHA-256 ae2bbe…a6bc.
- APK native libs: `lib/arm64-v8a/liblinkhub_core.so` and `lib/x86_64/liblinkhub_core.so` present (matches build-android-so.ps1 targets); armeabi-v7a/x86 have only ML Kit libs (no core .so) — fine for arm64 phones.
- NOT verified: no-keystore unsigned-fallback path (needs a separate rebuild); R8-shrunk release runtime (no class/resource loss) — pending real-device run.
- No tracked code changed (gradle config already in d6d5d8c); only docs updated. Keystore/.jks/keystore.properties NOT committed (git-ignored).

## 2026-06-16 Codex (desktop Windows installer validation)

- `cargo tauri --version`: initially failed because Tauri CLI was not installed.
- `cargo install tauri-cli --version "^2"`: passed; installed `tauri-cli 2.11.2`.
- `cargo tauri --version`: passed after install, returned `tauri-cli 2.11.2`.
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-desktop-installer.ps1`: passed. Tauri release build succeeded, NSIS 3.11 and WiX 3.14.1 were downloaded/validated by Tauri, and both bundles were produced.
- Installer artifacts produced locally: `desktop/src-tauri/target/release/bundle/nsis/LinkHub_0.1.0_x64-setup.exe` and `desktop/src-tauri/target/release/bundle/msi/LinkHub_0.1.0_x64_en-US.msi`.
- Manual install smoke NOT run by Codex: user still needs to install and verify second launch focuses existing window, tray menu contains "显示 LinkHub / 退出", and tray left-click focuses the main window.

## 2026-06-16 Claude (productionization: installer/signing/real-device checklist)

- `cargo check --manifest-path desktop/src-tauri/Cargo.toml`: passed WITH new tray + single-instance code (tauri 2.11.2, tauri-plugin-single-instance 2.4.2). Only pre-existing `net/protocol.rs` dead_code warning.
- `android\gradlew.bat :app:help`: passed (Gradle 8.13) — new Kotlin DSL signingConfigs/buildTypes/Properties import configures cleanly.
- `cargo tauri build` (desktop installer): NOT run — Tauri/WiX/NSIS toolchain presence unconfirmed; heavy. Installer artifacts unproven.
- `:app:assembleRelease` / `bundleRelease` (signed APK/AAB): NOT run — no keystore; needs release `.so` first. Signed + unsigned-fallback paths both untested.
- `scripts/two-device-app-test.md` real-device GUI loop: NOT run — pending user's two physical devices.
- Leak checks: `git ls-files "*.so" "*.apk" "*.aab" "*.jks" "*.keystore"` empty; no real Windows username/path committed.
- Committed and pushed to `main`: `52f4587..d6d5d8c`.

## 2026-06-16 Claude (core split + in-process e2e tests)

- `cargo check --manifest-path core-rs/Cargo.toml`: passed (only pre-existing `net/protocol.rs` dead_code warning).
- `cargo test --manifest-path core-rs/Cargo.toml`: passed — 110 + 16 unit tests, 4 new `tests/e2e.rs` integration tests, 0 failures.
- `cargo test --manifest-path core-rs/Cargo.toml --test e2e`: 4 passed (authenticated text, authenticated file, authenticated resume, plain file).
- `cargo check --manifest-path desktop/src-tauri/Cargo.toml`: passed.
- `cargo ndk ... check` (Android target): NOT run this session — public API unchanged, expected to still compile.
- `scripts/verify-local-e2e.ps1`: NOT re-run this session — superseded for cargo coverage by `tests/e2e.rs`.
- Committed and pushed to `main`: `cb1b545..52f4587`.

## 2026-06-16 Codex

- `scripts/build-android-so.ps1`: passed during Android task validation.
- `android\gradlew.bat :app:assembleDebug`: passed during Android task validation.
- Android API 34 AVD install/start smoke: passed.
- Android QR scanner permission and preview smoke: passed.
- Android QR decode from real camera feed: not verified in current emulator.
- Windows CLI to Android authenticated transfer after Task C: not fully re-run; documented as pending manual/real-device verification.
- Desktop JS syntax checks: passed for `app.js`, `devices.js`, `send.js`, `pairing.js`, `history.js`, `service.js`.
- `desktop/src-tauri cargo check`: passed with existing `core-rs/src/net/protocol.rs` dead_code warning.
- `.so` / `.apk` tracking check: `git ls-files "*.so"` and `git ls-files "*.apk"` empty.
