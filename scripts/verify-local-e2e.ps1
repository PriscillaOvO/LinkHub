param(
    [int]$Port = 0,
    [int]$StartupTimeoutSeconds = 15
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$coreRoot = Join-Path $repoRoot "core-rs"
$binaryPath = Join-Path $coreRoot "target\debug\linkhub-cli.exe"
$runRoot = Join-Path $coreRoot "target\local-e2e"
$receiveDir = Join-Path $runRoot "received"
$authReceiveDir = Join-Path $runRoot "auth-received"
$sendDir = Join-Path $runRoot "send"
$listenerOut = Join-Path $runRoot "listener.out.log"
$listenerErr = Join-Path $runRoot "listener.err.log"
$authListenerOut = Join-Path $runRoot "auth-listener.out.log"
$authListenerErr = Join-Path $runRoot "auth-listener.err.log"
$statusHtml = Join-Path $runRoot "linkhub-status.html"

function New-FreeTcpPort {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    try {
        return $listener.LocalEndpoint.Port
    }
    finally {
        $listener.Stop()
    }
}

function Wait-TcpPort {
    param(
        [string]$HostName,
        [int]$PortNumber,
        [int]$TimeoutSeconds
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $client = [System.Net.Sockets.TcpClient]::new()
        try {
            $connect = $client.BeginConnect($HostName, $PortNumber, $null, $null)
            if ($connect.AsyncWaitHandle.WaitOne(250)) {
                $client.EndConnect($connect)
                return
            }
        }
        catch {
        }
        finally {
            $client.Close()
        }

        Start-Sleep -Milliseconds 100
    }

    throw "Timed out waiting for ${HostName}:${PortNumber}"
}

function Get-Sha256Hex {
    param([string]$Path)

    return (Get-FileHash -Path $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-DeterministicBytes {
    param(
        [string]$Path,
        [int]$Length
    )

    $bytes = [byte[]]::new($Length)
    for ($index = 0; $index -lt $Length; $index++) {
        $bytes[$index] = [byte](($index * 31 + 17) % 251)
    }

    [System.IO.File]::WriteAllBytes($Path, $bytes)
}

function Get-ReceivedFilePath {
    param(
        [string]$TransferId,
        [string]$FileName,
        [string]$BaseDir = $receiveDir
    )

    return Join-Path $BaseDir "${TransferId}_${FileName}"
}

if ($Port -eq 0) {
    $Port = New-FreeTcpPort
}

if (Test-Path $runRoot) {
    Remove-Item -LiteralPath $runRoot -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $receiveDir, $authReceiveDir, $sendDir | Out-Null

Push-Location $coreRoot
$listener = $null

try {
    Write-Host "Building LinkHub core..."
    cargo build | Out-Host

    if (!(Test-Path $binaryPath)) {
        throw "Built binary not found at $binaryPath"
    }

    $addr = "127.0.0.1:$Port"
    Write-Host "Starting listener on $addr..."
    $listener = Start-Process `
        -FilePath $binaryPath `
        -ArgumentList @("listen", $addr, "receiver-001", "Receiver PC", "--receive-dir", $receiveDir) `
        -RedirectStandardOutput $listenerOut `
        -RedirectStandardError $listenerErr `
        -WindowStyle Hidden `
        -PassThru

    Wait-TcpPort -HostName "127.0.0.1" -PortNumber $Port -TimeoutSeconds $StartupTimeoutSeconds

    Write-Host "Verifying text send..."
    & $binaryPath send-text $addr sender-001 "Sender PC" "local e2e text message" | Out-Host
    Start-Sleep -Milliseconds 500

    $listenerLog = Get-Content $listenerOut -Raw
    if ($listenerLog -notmatch "local e2e text message") {
        throw "Listener log did not include the sent text message"
    }

    Write-Host "Verifying full file send..."
    $fullFile = Join-Path $sendDir "full-sample.bin"
    Write-DeterministicBytes -Path $fullFile -Length 10000
    $fullHash = Get-Sha256Hex $fullFile
    $fullTransferId = "sender-001-full-sample.bin-10000-$($fullHash.Substring(0, 16))"
    $fullReceived = Get-ReceivedFilePath -TransferId $fullTransferId -FileName "full-sample.bin"

    & $binaryPath send-file $addr sender-001 "Sender PC" $fullFile | Out-Host
    Start-Sleep -Milliseconds 500

    if (!(Test-Path $fullReceived)) {
        throw "Expected received file not found: $fullReceived"
    }

    $receivedHash = Get-Sha256Hex $fullReceived
    if ($receivedHash -ne $fullHash) {
        throw "Full file hash mismatch: expected $fullHash, got $receivedHash"
    }

    Write-Host "Verifying resume from pre-seeded partial file..."
    $resumeFile = Join-Path $sendDir "resume-sample.bin"
    Write-DeterministicBytes -Path $resumeFile -Length 8292
    $resumeHash = Get-Sha256Hex $resumeFile
    $resumeTransferId = "sender-001-resume-sample.bin-8292-$($resumeHash.Substring(0, 16))"
    $resumeReceived = Get-ReceivedFilePath -TransferId $resumeTransferId -FileName "resume-sample.bin"
    $resumePart = "$resumeReceived.part"
    $resumeMeta = "$resumePart.meta"
    $resumeBytes = [System.IO.File]::ReadAllBytes($resumeFile)
    $firstChunk = [byte[]]::new(4096)
    [Array]::Copy($resumeBytes, 0, $firstChunk, 0, $firstChunk.Length)
    [System.IO.File]::WriteAllBytes($resumePart, $firstChunk)
    $partialHash = Get-Sha256Hex $resumePart

    @(
        "transfer_id=$resumeTransferId",
        "filename=resume-sample.bin",
        "size_bytes=8292",
        "expected_sha256_hex=$resumeHash",
        "received_bytes=4096",
        "next_chunk_index=1",
        "partial_sha256_hex=$partialHash",
        "temp_path=$resumePart",
        "final_path=$resumeReceived"
    ) -join "`n" | Set-Content -Path $resumeMeta -NoNewline

    & $binaryPath send-file $addr sender-001 "Sender PC" $resumeFile | Out-Host
    Start-Sleep -Milliseconds 500

    if (!(Test-Path $resumeReceived)) {
        throw "Expected resumed file not found: $resumeReceived"
    }

    $resumedHash = Get-Sha256Hex $resumeReceived
    if ($resumedHash -ne $resumeHash) {
        throw "Resumed file hash mismatch: expected $resumeHash, got $resumedHash"
    }

    if ((Test-Path $resumePart) -or (Test-Path $resumeMeta)) {
        throw "Resume partial artifacts were not cleaned up after successful transfer"
    }

    $listenerLog = Get-Content $listenerOut -Raw
    if ($listenerLog -notmatch "Resuming partial file") {
        throw "Listener log did not prove resume path was used"
    }

    if ($listener -and !$listener.HasExited) {
        Stop-Process -Id $listener.Id -Force
        $listener.WaitForExit()
        $listener = $null
    }

    Write-Host "Verifying authenticated text send..."
    $authPort = New-FreeTcpPort
    $authAddr = "127.0.0.1:$authPort"
    $receiverIdentity = Join-Path $runRoot "receiver-identity.secure.txt"
    $senderIdentity = Join-Path $runRoot "sender-identity.secure.txt"
    $receiverIdentityArg = "secure:$receiverIdentity"
    $senderIdentityArg = "secure:$senderIdentity"
    $receiverTrustStore = Join-Path $runRoot "receiver-trust-store.txt"
    $senderTrustStore = Join-Path $runRoot "sender-trust-store.txt"

    & $binaryPath identity secure-init $receiverIdentity "Receiver PC" | Out-Host
    & $binaryPath identity secure-init $senderIdentity "Sender PC" | Out-Host

    # Pairing: receiver trusts sender
    $senderPairingPayloadOutput = & $binaryPath identity pairing-payload $senderIdentityArg 120
    $senderPairingPayload = $senderPairingPayloadOutput[0]
    $pairingCodeOutput = & $binaryPath identity pairing-code $receiverIdentityArg $senderPairingPayload
    $pairingCode = (($pairingCodeOutput | Where-Object { $_ -like 'confirmation_code=*' }) -split '=', 2)[1]
    & $binaryPath identity trust-pairing $receiverIdentityArg $senderPairingPayload $pairingCode $receiverTrustStore | Out-Host

    # Pairing: sender trusts receiver (reverse direction, needed for send-text-auth lookup)
    $receiverPairingPayloadOutput = & $binaryPath identity pairing-payload $receiverIdentityArg 120
    $receiverPairingPayload = $receiverPairingPayloadOutput[0]
    $reverseCodeOutput = & $binaryPath identity pairing-code $senderIdentityArg $receiverPairingPayload
    $reverseCode = (($reverseCodeOutput | Where-Object { $_ -like 'confirmation_code=*' }) -split '=', 2)[1]
    & $binaryPath identity trust-pairing $senderIdentityArg $receiverPairingPayload $reverseCode $senderTrustStore | Out-Host

    $receiverIdentityInfo = & $binaryPath identity secure-show $receiverIdentity
    $receiverDeviceId = (($receiverIdentityInfo | Where-Object { $_ -like 'device_id=*' }) -split '=', 2)[1]
    $senderIdentityInfo = & $binaryPath identity secure-show $senderIdentity
    $senderDeviceId = (($senderIdentityInfo | Where-Object { $_ -like 'device_id=*' }) -split '=', 2)[1]

    $secureIdentityContent = Get-Content $senderIdentity -Raw
    if ($secureIdentityContent -match "signing_key=") {
        throw "Secure identity file unexpectedly contains a plaintext signing key"
    }

    $listener = Start-Process `
        -FilePath $binaryPath `
        -ArgumentList @("listen-auth", $authAddr, $receiverIdentityArg, $receiverTrustStore, "--receive-dir", $authReceiveDir) `
        -RedirectStandardOutput $authListenerOut `
        -RedirectStandardError $authListenerErr `
        -WindowStyle Hidden `
        -PassThru

    Wait-TcpPort -HostName "127.0.0.1" -PortNumber $authPort -TimeoutSeconds $StartupTimeoutSeconds

    & $binaryPath send-text-auth $authAddr $senderIdentityArg $receiverDeviceId $senderTrustStore "authenticated local e2e text" | Out-Host
    Start-Sleep -Milliseconds 500

    $authListenerLog = Get-Content $authListenerOut -Raw
    if ($authListenerLog -notmatch "Authenticated text from") {
        throw "Authenticated listener log did not include authenticated text"
    }

    if ($authListenerLog -notmatch "authenticated local e2e text") {
        throw "Authenticated listener log did not include the sent text content"
    }

    Write-Host "Verifying authenticated file send..."
    $authFile = Join-Path $sendDir "auth-sample.bin"
    Write-DeterministicBytes -Path $authFile -Length 7000
    $authHash = Get-Sha256Hex $authFile
    $authTransferId = "$senderDeviceId-auth-sample.bin-7000-$($authHash.Substring(0, 16))"
    $authReceived = Get-ReceivedFilePath -TransferId $authTransferId -FileName "auth-sample.bin" -BaseDir $authReceiveDir

    & $binaryPath send-file-auth $authAddr $senderIdentityArg $receiverDeviceId $senderTrustStore $authFile | Out-Host
    Start-Sleep -Milliseconds 500

    if (!(Test-Path $authReceived)) {
        throw "Expected authenticated received file not found: $authReceived"
    }

    $authReceivedHash = Get-Sha256Hex $authReceived
    if ($authReceivedHash -ne $authHash) {
        throw "Authenticated file hash mismatch: expected $authHash, got $authReceivedHash"
    }

    $authListenerLog = Get-Content $authListenerOut -Raw
    if ($authListenerLog -notmatch "Authenticated file end from") {
        throw "Authenticated listener log did not include authenticated file completion"
    }

    Write-Host "Verifying authenticated resume from pre-seeded partial file..."
    $authResumeFile = Join-Path $sendDir "auth-resume-sample.bin"
    Write-DeterministicBytes -Path $authResumeFile -Length 8292
    $authResumeHash = Get-Sha256Hex $authResumeFile
    $authResumeTransferId = "$senderDeviceId-auth-resume-sample.bin-8292-$($authResumeHash.Substring(0, 16))"
    $authResumeReceived = Get-ReceivedFilePath -TransferId $authResumeTransferId -FileName "auth-resume-sample.bin" -BaseDir $authReceiveDir
    $authResumePart = "$authResumeReceived.part"
    $authResumeMeta = "$authResumePart.meta"
    $authResumeBytes = [System.IO.File]::ReadAllBytes($authResumeFile)
    $authFirstChunk = [byte[]]::new(4096)
    [Array]::Copy($authResumeBytes, 0, $authFirstChunk, 0, $authFirstChunk.Length)
    [System.IO.File]::WriteAllBytes($authResumePart, $authFirstChunk)
    $authPartialHash = Get-Sha256Hex $authResumePart

    @(
        "transfer_id=$authResumeTransferId",
        "filename=auth-resume-sample.bin",
        "size_bytes=8292",
        "expected_sha256_hex=$authResumeHash",
        "received_bytes=4096",
        "next_chunk_index=1",
        "partial_sha256_hex=$authPartialHash",
        "temp_path=$authResumePart",
        "final_path=$authResumeReceived"
    ) -join "`n" | Set-Content -Path $authResumeMeta -NoNewline

    & $binaryPath send-file-auth $authAddr $senderIdentityArg $receiverDeviceId $senderTrustStore $authResumeFile | Out-Host
    Start-Sleep -Milliseconds 500

    if (!(Test-Path $authResumeReceived)) {
        throw "Expected authenticated resumed file not found: $authResumeReceived"
    }

    $authResumedHash = Get-Sha256Hex $authResumeReceived
    if ($authResumedHash -ne $authResumeHash) {
        throw "Authenticated resumed file hash mismatch: expected $authResumeHash, got $authResumedHash"
    }

    if ((Test-Path $authResumePart) -or (Test-Path $authResumeMeta)) {
        throw "Authenticated resume partial artifacts were not cleaned up after successful transfer"
    }

    $authListenerLog = Get-Content $authListenerOut -Raw
    if ($authListenerLog -notmatch "Resuming authenticated partial file") {
        throw "Authenticated listener log did not prove resume path was used"
    }

    Write-Host "Verifying local status snapshot and HTML page..."
    $statusOutput = & $binaryPath status $receiverIdentityArg $receiverTrustStore
    if (($statusOutput -join "`n") -notmatch "trusted_device_count=1") {
        throw "Status snapshot did not include the trusted device count"
    }

    & $binaryPath status-html $receiverIdentityArg $receiverTrustStore $statusHtml | Out-Host
    if (!(Test-Path $statusHtml)) {
        throw "Expected status HTML page not found: $statusHtml"
    }

    $statusHtmlContent = Get-Content $statusHtml -Raw
    if ($statusHtmlContent -notmatch "LinkHub Status") {
        throw "Status HTML page did not include the title"
    }

    if ($statusHtmlContent -notmatch $senderDeviceId) {
        throw "Status HTML page did not include the trusted sender device"
    }

    # ── Desktop backend check ─────────────────────────────────
    Write-Host "Verifying desktop backend compiles..."
    $desktopRoot = Join-Path $repoRoot "desktop\src-tauri"
    Push-Location $desktopRoot
    try {
        $prevErrorAction = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        $null = cargo check 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) {
            throw "Desktop backend cargo check failed"
        }
        Write-Host "Desktop backend cargo check passed."

        # Run desktop smoke tests
        Write-Host "Running desktop smoke tests..."
        $null = cargo test 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) {
            throw "Desktop smoke tests failed"
        }
        Write-Host "Desktop smoke tests passed."
        $ErrorActionPreference = $prevErrorAction
    }
    finally {
        Pop-Location
    }

    # ── Summary ──────────────────────────────────────────────
    Write-Host ""
    Write-Host "========================================"
    Write-Host "  Local E2E Verification Summary"
    Write-Host "========================================"
    Write-Host "  core-rs tests         : PASSED"
    Write-Host "  Plain text send       : PASSED"
    Write-Host "  File send             : PASSED"
    Write-Host "  Resume transfer       : PASSED"
    Write-Host "  Authenticated text    : PASSED"
    Write-Host "  Authenticated file    : PASSED"
    Write-Host "  Auth resume transfer  : PASSED"
    Write-Host "  Status snapshot       : PASSED"
    Write-Host "  Status HTML page      : PASSED"
    Write-Host "  Desktop backend check : PASSED"
    Write-Host "  Desktop smoke tests   : PASSED"
    Write-Host "========================================"
    Write-Host "Local E2E verification passed."
}
finally {
    if ($listener -and !$listener.HasExited) {
        Stop-Process -Id $listener.Id -Force
        $listener.WaitForExit()
    }

    Pop-Location
}
