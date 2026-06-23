# Codex Decisions

Last updated: 2026-06-16 21:11:25 +08:00

## Decisions

- Keep Android `jniLibs.srcDirs("src/main/jniLibs")` unchanged. Native `.so` files are generated into that directory by script instead of changing Gradle loading.
- Keep Android QR pairing UI-only. It reuses existing pairing payload parsing and trust confirmation flow; no RustBridge/core wire format changes.
- Keep desktop trusted devices UI on existing Tauri/core APIs. It reuses `get_local_status`, `scan_trusted_mdns`, and frontend address cache.
- Do not commit `docs/ai-handoff/` unless the user explicitly asks. This directory is currently intended as local handoff state.

## Commit Discipline

- Commit directly to `main` only when the user asks for implementation completion.
- Keep author email as `PriscillaOvO@users.noreply.github.com`.
- End commits with `Co-Authored-By: Codex <codex@openai.com>`.
- Do not submit `.apk`, `.so`, or real Windows user paths.
