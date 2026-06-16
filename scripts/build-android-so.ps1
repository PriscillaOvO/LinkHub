param(
    [string]$NdkVersion = "28.2.13676358"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$coreRoot = Join-Path $repoRoot "core-rs"
$jniLibsRoot = Join-Path $repoRoot "android\app\src\main\jniLibs"

if (-not $env:ANDROID_NDK_HOME) {
    if (-not $env:LOCALAPPDATA) {
        throw "LOCALAPPDATA is not set. Set ANDROID_NDK_HOME to your Android NDK path first."
    }

    $defaultNdk = Join-Path $env:LOCALAPPDATA "Android\Sdk\ndk\$NdkVersion"
    $env:ANDROID_NDK_HOME = $defaultNdk
}

if (-not (Test-Path -LiteralPath $env:ANDROID_NDK_HOME)) {
    throw "ANDROID_NDK_HOME does not exist: $env:ANDROID_NDK_HOME"
}

Push-Location $coreRoot
try {
    cargo ndk `
        -t arm64-v8a `
        -t x86_64 `
        -o $jniLibsRoot `
        build --release --lib
}
finally {
    Pop-Location
}
