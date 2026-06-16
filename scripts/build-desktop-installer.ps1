# Builds the LinkHub Windows desktop installer (NSIS + MSI) via the Tauri CLI.
#
# Prerequisites (install once):
#   - Rust toolchain (https://rustup.rs)
#   - WebView2 runtime (preinstalled on Windows 11)
#   - Tauri CLI v2:  cargo install tauri-cli --version "^2"
#   - For MSI target: WiX Toolset v3 (Tauri downloads it on first build)
#   - For NSIS target: NSIS (Tauri downloads it on first build)
#
# Output: desktop\src-tauri\target\release\bundle\{nsis,msi}\
#
# Code signing is NOT performed here. To sign, set
# bundle.windows.certificateThumbprint in tauri.conf.json and have signtool
# on PATH; Tauri will sign during the build.

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $repoRoot "desktop\src-tauri"

# Verify the Tauri CLI is available.
$tauriVersion = $null
try {
    $tauriVersion = (cargo tauri --version) 2>$null
} catch {
    $tauriVersion = $null
}
if (-not $tauriVersion) {
    throw "Tauri CLI not found. Install it with: cargo install tauri-cli --version `"^2`""
}
Write-Host "Using $tauriVersion"

Push-Location $tauriRoot
try {
    cargo tauri build
} finally {
    Pop-Location
}

$bundleRoot = Join-Path $tauriRoot "target\release\bundle"
Write-Host ""
Write-Host "Build complete. Installers should be under:"
Write-Host "  $bundleRoot\nsis\"
Write-Host "  $bundleRoot\msi\"

if (Test-Path -LiteralPath $bundleRoot) {
    Get-ChildItem -LiteralPath $bundleRoot -Recurse -Include *.exe, *.msi -ErrorAction SilentlyContinue |
        ForEach-Object { Write-Host "  -> $($_.FullName)" }
}
