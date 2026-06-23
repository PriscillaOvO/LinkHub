# Claude Decisions

Last updated: 2026-06-16 23:59:00 +08:00

## Decisions (2026-06-16 productionization)

- Desktop `bundle.targets` was narrowed from `"all"` to `["nsis","msi"]` on purpose: `"all"` tries macOS/Linux targets that cannot build on a Windows-only toolchain. If cross-platform installers are wanted later, set per-OS targets in CI, not back to `"all"` blindly.
- `tauri-plugin-single-instance` is registered as the FIRST plugin — Tauri requires this so a second launch is intercepted before other state initializes. Do not reorder it below other `.plugin(...)` calls.
- Android release signing is intentionally credential-free in git: it reads `keystore.properties` (git-ignored) or `LINKHUB_RELEASE_*` env vars. When neither is present the release build is left UNSIGNED (warns, does not fail) so CI/contributors without the keystore can still produce an APK. Do not hardcode a keystore path or password.
- `proguard-rules.pro` MUST keep `com.linkhub.app.bridge.RustBridge` and all `native <methods>` — R8 renaming them breaks JNI symbol lookup against the Rust `.so` and the `onFileReceived` native→Kotlin callback. Extend keeps additively if new JNI surface is added.
- A new GUI-level checklist `scripts/two-device-app-test.md` was added ALONGSIDE the CLI `two-device-test.md`, not replacing it — they validate different layers (final installed apps vs core/CLI). Pairing direction in it assumes desktop has no camera: desktop shows QR → phone scans; reverse uses phone payload text → desktop paste.

## Decisions

- The `identity` and `net` splits are internal-only. `lib.rs` re-exports are unchanged on purpose — do not relocate or rename public symbols when extending these modules; add new functions or submodules instead.
- Child submodules reach shared helpers through the module root: identity submodules call parent helpers via `super::` (Rust lets descendants see ancestor private items); cross-sibling functions are `pub(super)` (e.g. `secure_store`'s protect/unprotect, `auth_session`'s `open_authenticated_stream` / `send_encrypted_*`). Keep this pattern rather than promoting things to `pub`/`pub(crate)`.
- `secure_store.rs` now isolates the per-platform key protection. Task B (macOS Keychain / Linux Secret Service) should be implemented inside the existing `#[cfg(target_os = ...)]` `protect_local_identity_bytes` / `unprotect_local_identity_bytes` stubs there.
- Integration tests live in `core-rs/tests/e2e.rs` and drive the public API in-process over real TCP loopback (no child processes). They rely on the invariant that a sender returns `Ok` only after the receiver's final ACK, and the receiver writes/renames the file (and fires its callback) before sending that ACK — so no polling is needed. Preserve that ordering in the session loops if you touch them.
- `sha2` was added to `[dev-dependencies]` (it is also a normal dependency) so integration tests can hash; integration tests cannot see normal `[dependencies]` directly.
- Did not re-run `cargo ndk` or `verify-local-e2e.ps1` this session; documented as not-verified rather than claimed.

## Commit Discipline

- Keep author email as `PriscillaOvO@users.noreply.github.com`.
- Do not submit `.apk`, `.so`, or real Windows user paths.
- End commits with `Co-Authored-By: Claude <noreply@anthropic.com>`.
- Commit code + tracked docs directly to `main` and push (done: `52f4587`). Keep `docs/ai-handoff/` untracked unless the user explicitly asks to commit it.
