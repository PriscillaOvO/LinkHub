# Handoff Clock

This file is the source of truth for "which handoff note is newest".

## Rule

At the end of every AI work session, update the top entry with:

- absolute timestamp with timezone
- AI name
- latest commit SHA
- branch
- workspace status summary
- `next_read` file for the next AI

Do not rely on filesystem modified time.

## Latest

timestamp: 2026-06-23 ~18:50 +08:00
ai: Claude
branch: feat/transport-abstraction-m1
latest_commit: 9be6517 (`identity show` prints dh_public_key) — **9 unpushed commits** on top of pushed 2b8e568: 30bb5f2, 9030a06, f513107, 5a157af, 50a22a9, 014dd6a (pre-session) + a5827ac, 019898d, 9be6517 (this session).
prev_note: |
  2026-06-21 ~05:30 — device-free night: 6 commits (Tor Phase 4 slice, mDNS onion, desktop+Android UI, I2P scaffolding, docs). See History.
workspace_status: GREEN, clean tree, 159 default tests (`--frozen`). **REAL-DEVICE TRANSFER VALIDATED on home WiFi (vivo V2454A, Android 16, arm64).** Computer→phone over real WiFi (phone=listener, computer=sender; phone→computer is blocked by Windows Firewall/ProtonVPN inbound, so only the computer-initiated direction works): cross-compiled `linkhub-cli` for aarch64-linux-android (`cargo ndk -p 26`; bin link needs API≥24 for getifaddrs; cargo-ndk's post-link panic is a benign report-gen quirk, binary is produced), pushed to /data/local/tmp. Results: (a) PLAIN `send-file` 1MB → SHA256 match; (b) AUTHENTICATED `send-file-auth` 1MB with **Noise KK handshake + encrypted session** → SHA256 match (this is the path the app ships). Trust stores hand-built from `identity show` (now incl. dh via `9be6517`). This session's 3 commits: `a5827ac` desktop UI 2nd polish (CSS-only: ambient mesh backdrop, gradient logo mark + shimmering wordmark, card gradient top-edge + staggered cascade, primary-button sheen; reduced-motion safe) — verified via headless-Edge static preview; `019898d` Android UI 2nd polish (emoji nav → Material vector icons, surface top bar + indigo→violet brand mark, Pair screen flat sections → rounded SectionCards) — assembleDebug OK, installed + screenshotted on real phone; `9be6517` `identity show` prints dh_public_key (was the missing field for hand-building a trust store). Phone has the LATEST app installed; **computer does NOT have the desktop (Tauri) app built/installed — only the CLI**.
next_read: `docs/ai-handoff/worklog/claude.md` top + `docs/spec/项目状态.md`
NOTE: 9 unpushed commits — user uses "push 吧" as the checkpoint, so HOLD push until asked. BUILD/TEST TIP: use `--frozen` (offline+locked) for core-rs after `target/` cleanup (optional Arti index refresh is curl-flaky on the China net; all 3 index mirrors have entries cached). Desktop checks: run from `desktop/src-tauri`. adb via PowerShell (Git Bash MSYS mangles `/data/...` paths). NOT YET DONE: full app-UI end-to-end on real devices (QR pair → tap-send) — needs the desktop app built+running on the computer + a Windows Firewall inbound rule (phone→computer) and/or ProtonVPN allow-LAN, OR phone hotspot. Real mDNS auto-discovery likely blocked by the inbound firewall. GATED (real device / Phase 0 spike): Tor Phase 4 shells + onion-over-Tor data path; I2P B0 spike; BitTorrent A0 spike (`mainline`). Spike at C:\Dev\tor-spike.

## History

- 2026-06-21 ~03:30 +08:00 | Claude | 5cad6c4, 8008468 | Tor Phase 2 (Arti transport behind `tor` feature: tor_transport.rs OnionStreamDuplex + TorContext bootstrap/connect/host + OnionListener; hosts at identity-derived address; dual-ABI `--features tor` check green, default stays lean) + Phase 3 (TransportKind/ConnectionPath::Onion + plan ordering, onion_hs_seed, CLI listen-tor/connect-tor). All matrices green. Phases 4/5 gated on real-device onion data-path validation. Not pushed.
- 2026-06-21 ~01:30 +08:00 | Claude | 4acf27f, d1c106b | Tor onion spike (Phase 0/0.5): Arti compiles + dual-ABI links (rusqlite/bundled), .so ~7.6-8.6MB/ABI; plain Tor blocked on China net but obfs4 bridges → Bootstrapped 100% (Arti pt-client too, 6.8s); onion data round-trip didn't complete over China+few bridges (documented). Phase 1 committed: pure-Rust v3 onion address derivation (identity/onion.rs, matches Arti vector, +sha3). Phases 2-5 gated on real-device onion validation. Not pushed.
- 2026-06-20 ~04:00 +08:00 | Claude | 22c184e | Reviewed Codex's seamless B–E (C1 accept cb / C2 nearby send / B advanced settings / D share target), security-OK (fail-closed), full matrix green; committed Codex's uncommitted E (desktop accept + core mDNS full-identity broadcast). AirDrop-style A–E complete.

- 2026-06-20 ~02:30 +08:00 | Claude | 9a4b6d4 | A: first-contact handshake (AirDrop-style, no pairing codes). Ed25519-over-DH binding signature defeats MITM DH-swap; AcceptPeerCallback/IncomingPeer/run_authenticated_responder_over_with_accept. +4 tests incl MITM rejection. B–E (Android JNI/UI, share-sheet, desktop) handed to Codex.
- 2026-06-20 ~01:00 +08:00 | Claude | bbdbc04 | Added `turn-dev-server` dev fixture + validated forced-TURN-relay locally via two host CLI peers + standalone TURN + relay-only (256KB SHA match, +bin over relay). Documented that same-host emulators can't do relay-only (NAT topology). Own commit, unpushed.
- 2026-06-20 ~00:00 +08:00 | Claude | beeef5f (no new commit) | Two-emulator end-to-end validation of C5 Android cross-network WebRTC: real webrtc-rs Android runtime + host signaling + public STUN, A→B file transfer SHA-256 byte-identical; also proves T8 binary framing on real Android. Validation only. Gotchas captured in worklog.
- 2026-06-20 02:50 +08:00 | Codex | beeef5f | Final acceptance matrix green and final `codex-to-claude/latest.md` prompt written. Branch ahead of origin; only `.claude/settings.local.json` untracked.
- 2026-06-20 02:30 +08:00 | Codex | 27a1d46→beeef5f | Skipped C6 sliding-window optimization per instruction; documented risk/next-task boundary in tracked specs. Next final acceptance matrix and final Claude prompt.
- 2026-06-20 02:23 +08:00 | Codex | cdf5e66→27a1d46 | Did C5: Android WebRTC send UI + foreground-service receiver loop, JNI stop hook, docs. Android assembleDebug, default NDK check, and WebRTC release .so dual ABI build green. Next C6 decision/skip.
- 2026-06-20 01:55 +08:00 | Codex | 35d9b04→cdf5e66 | Did C4: iOS local-network send/listen/status C ABI, header, Swift wrapper, docs. iOS target check and core/default NDK matrix green. Next C5 Android WebRTC UI.
- 2026-06-20 01:47 +08:00 | Codex | b708c0a→35d9b04 | Did C3: desktop WebRTC resident receive loop with start/stop/status commands and frontend controls; core added stoppable receive helper. Desktop default/webrtc validation + core webrtc e2e all green. Next C4 iOS send/listen FFI.
- 2026-06-20 01:38 +08:00 | Codex | 5a1da39→b708c0a | Did C2: synchronous signaling supervisor for heartbeat, disconnect detection, retry/re-auth, presence recovery; CLI `signal-listen` now uses supervisor events. Core fmt/test/clippy, `cargo check --features webrtc`, and default NDK dual ABI check green. Next C3 desktop resident WebRTC receive loop.
- 2026-06-20 01:25 +08:00 | Codex | b001bd2→5a1da39 | Did C1: signaling-server cross-connection limits (global max + per-IP max, pre-handshake enforcement, permit cleanup) with deterministic integration tests for same-IP/global rejection. Signaling-server fmt/clippy/test green. Next C2 supervisor.
- 2026-06-19 ~21:30 +08:00 | Claude | debd005→b001bd2 | Did T8 (binary file framing, version-negotiated via FILE_START `+bin`, halves wire size; webrtc e2e now on binary path) + T9 (iOS buildable scaffold: C header/modulemap, xcframework build script, staticlib, XcodeGen project, Info.plist permission keys, @main+ServiceView, design doc; `cargo check --target aarch64-apple-ios` green from Windows). Each its own commit, full matrix green. Handed Codex a C1–C6 batch.
- 2026-06-19 ~17:50 +08:00 | Claude | 396bcb8→9447143 | Did T3+T4+T5+T6+T7 in one ultra-effort session, each its own commit, all green. T3 signed SDP; T4 TURN relay (+real in-process TURN e2e); T5 server limits + client backoff/ping; T6 desktop integration + extracted shared core webrtc_session (CLI delegates); T7 Android JNI bridge + .so size delta (strip +8.4 MiB/ABI, webrtc stays opt-in). Not pushed.
- 2026-06-19 ~06:30 +08:00 | Claude | b8ef2e8 (+docs/spec uncommitted) | Finished Codex's interrupted T2: real two-emulator cross-network WebRTC validation. Built Android x86_64 webrtc CLI (minSdk 24), ran bidirectional file transfer over real webrtc-rs Android runtime + emulator NAT via host signaling-server + public STUN; both directions SHA-256 matched source. Validation only (no code). Backlog now T1✅ T2✅, next T3.
- 2026-06-19 01:52 +08:00 | Codex | b8ef2e8 | Completed T1 WebRTC CLI wiring: `listen-webrtc`/`connect-webrtc` behind `--features webrtc`; real signaling-server + two CLI child-process e2e transfers 40KB over webrtc-rs DataChannel with byte match. Full verification matrix green. Started T2 (A→B verified in-emulator) but hit quota before capturing B→A.
- 2026-06-18 07:45 +08:00 | Claude | e108a47 | Completed cross-network main line: sync signaling_client + Ed25519 login + CLI; M3 DataChannelDuplex bridging async webrtc-rs DataChannel to sync Read/Write, ran existing Noise KK file transfer over it (40KB, SHA matched, in-process loopback); connection_plan orchestration. All green incl. `--features webrtc` and cargo ndk default. webrtc-rs/tokio feature-gated off by default. Handed off backlog T1-T9 to Codex.
- 2026-06-18 06:00 +08:00 | Claude | 6909c47 | Committed pipe-selection spike (libp2p + webrtc-rs both cross-compile Win+Android dual-ABI; chose webrtc-rs) + M2-step1 thin signaling-server crate (Ed25519 auth, presence, store-and-forward; 7+4 tests).
- 2026-06-18 03:30 +08:00 | Claude | 3793cb5 (+uncommitted M1) | Wrote cross-network transport design doc and implemented M1: decoupled the authenticated Noise session from `TcpStream` into transport-agnostic generics.
- 2026-06-18 02:58 +08:00 | Claude | 1438ed2 (+docs/spec uncommitted) | Two-emulator UI re-validation of task b on the release APK.
- 2026-06-18 01:58:17 +08:00 | Codex | b3810c9 (+uncommitted) | Implemented task b pairing hardening: `linkhub-pair-v2`, TTL enforcement, 40-bit confirmation code, Android panic hook, and stale service-status reconciliation.
- 2026-06-18 00:20:00 +08:00 | Claude | b3810c9 (+uncommitted) | Two-emulator Android-to-Android validation found and fixed listener stale-status deadlock, NSD crash, R8 trusted-peer TypeToken, confirmation-code nonce mismatch, and listener catch_unwind.
- 2026-06-17 11:05:00 +08:00 | Claude | acd5fdd (uncommitted) | Completed Android listener stop/start fix and ProGuard Gson keep rules.
- 2026-06-17 00:30:00 +08:00 | Claude | acd5fdd | Built and verified signed Android release APK with dev keystore.
- 2026-06-16 22:54:41 +08:00 | Codex | 9b00bd1 | Installed Tauri CLI v2.11.2 and built Windows NSIS/MSI desktop installers.
- 2026-06-16 23:59:00 +08:00 | Claude | d6d5d8c | Productionization: desktop installer config, tray/single-instance, Android release signing/proguard, GUI real-device checklist.
- 2026-06-16 23:30:00 +08:00 | Claude | 52f4587 | Split identity/net modules and added in-process e2e tests.
- 2026-06-16 21:11:25 +08:00 | Codex | cb1b545 | Created directional handoff structure.
