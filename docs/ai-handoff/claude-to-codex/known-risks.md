# Claude Known Risks

Last updated: 2026-06-16 23:59:00 +08:00

## Risks And Gaps (2026-06-16 productionization)

- Desktop installer (`cargo tauri build`) was never run here — the Tauri CLI and WiX/NSIS tooling may or may not be installed on this machine. The bundle config is plausible but unproven; first real `tauri build` may surface missing tooling or icon/target issues.
- Signed Android release was never run: no keystore exists and `assembleRelease` needs the release `.so` first. Both the signed path AND the unsigned-fallback path are untested end-to-end. `isShrinkResources = true` + minify can strip resources/classes that only reflection/JNI reaches — watch for runtime ClassNotFound/missing-resource on first real release APK, and extend `proguard-rules.pro` if so.
- `tray-icon` uses `app.default_window_icon()` and `.expect("missing window icon")` — if a future config drops the window icon, the desktop app will panic at startup. Acceptable now (icon is configured) but note the hard unwrap.
- The real-device GUI checklist is unexecuted; treat every "Expected" in it as a hypothesis until a real run confirms.

## Risks And Gaps

- Android target not recompiled this session. The split kept the public API byte-identical so `cargo ndk ... check --lib` is expected to still pass, but it was not run. Re-run it (or trust the unchanged API) before shipping an Android build.
- `secure_store.rs` macOS/Linux branches (`security-framework` / `secret-service` + `tokio`) cannot be compiled on Windows (`#[cfg]`-gated). Their code paths were moved verbatim, not modified — but they remain unverified on their target platforms, same as before the split.
- `core-rs/tests/e2e.rs` `plain_file_round_trips` uses a probe-then-bind free-port pattern (no pre-bound-listener API exists for the cleartext listener), leaving a small TOCTOU window. Stable in runs so far; if it ever flakes under parallel load, switch it to a serialized port or add a plain pre-bound-listener entry point.
- Carry-over from Codex (still open): emulator NAT/mDNS limits, real-device QR decode unverified, Windows↔Android authenticated transfer not re-run, Tauri Devices/Send click flow not manually smoke-tested. None of these are affected by this refactor.
- `docs/ai-handoff/README.md` may render mojibake in some PowerShell reads — avoid rewriting it unless deliberately normalizing encoding.
