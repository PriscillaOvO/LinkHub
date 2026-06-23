# Codex Known Risks

Last updated: 2026-06-16 21:11:25 +08:00

## Risks And Gaps

- The Android emulator environment may not support real QR recognition without a configured camera feed. Use a physical Android device for final QR validation.
- Emulator NAT/mDNS limitations still apply. Prefer adb forward or real LAN devices for cross-device authenticated transfer tests.
- Desktop UI click flow needs a manual Tauri smoke test after the Devices page changes.
- `docs/ai-handoff/README.md` may display mojibake in some PowerShell reads. Avoid rewriting it unless deliberately normalizing file encoding.
