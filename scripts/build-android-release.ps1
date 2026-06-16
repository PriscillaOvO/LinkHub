# Builds a signed Android release APK (and optionally AAB) for LinkHub.
#
# One-time setup
# --------------
# 1. Generate a keystore (keep the .jks OUTSIDE git, e.g. in your user profile):
#
#      keytool -genkeypair -v `
#        -keystore "$env:USERPROFILE\linkhub-release.jks" `
#        -alias linkhub `
#        -keyalg RSA -keysize 2048 -validity 10000
#
#    (keytool will prompt for the store/key passwords — do not hardcode them.)
#
# 2. Create android\keystore.properties (git-ignored) with:
#
#      storeFile=C:/Users/<用户名>/linkhub-release.jks
#      storePassword=<store password>
#      keyAlias=linkhub
#      keyPassword=<key password>
#
#    Or set the equivalent environment variables instead:
#      LINKHUB_RELEASE_STORE_FILE, LINKHUB_RELEASE_STORE_PASSWORD,
#      LINKHUB_RELEASE_KEY_ALIAS, LINKHUB_RELEASE_KEY_PASSWORD
#
# If no signing is configured the gradle build still runs but emits an UNSIGNED
# release APK (see build.gradle.kts warning).
#
# Output:
#   android\app\build\outputs\apk\release\app-release.apk
#   android\app\build\outputs\bundle\release\app-release.aab  (with -Bundle)

param(
    [switch]$Bundle
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$androidRoot = Join-Path $repoRoot "android"

# 1. Build the release native libraries (.so) into jniLibs first.
Write-Host "==> Building release native libraries (.so)..."
& (Join-Path $PSScriptRoot "build-android-so.ps1")

# 2. Assemble the signed release APK (and AAB if requested).
Push-Location $androidRoot
try {
    $tasks = @(":app:assembleRelease")
    if ($Bundle) { $tasks += ":app:bundleRelease" }

    Write-Host "==> Running gradle: $($tasks -join ' ')"
    & .\gradlew.bat @tasks
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "Build complete. Artifacts:"
$apk = Join-Path $androidRoot "app\build\outputs\apk\release\app-release.apk"
if (Test-Path -LiteralPath $apk) { Write-Host "  APK -> $apk" }
$aab = Join-Path $androidRoot "app\build\outputs\bundle\release\app-release.aab"
if (Test-Path -LiteralPath $aab) { Write-Host "  AAB -> $aab" }
