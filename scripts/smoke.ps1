# Smoke test: launch the built app, drive the real Windows clipboard, and verify
# that entries land in the database correctly classified.
#
# This covers what unit tests cannot - the Win32 clipboard listener, the capture
# pipeline, classification, encryption and SQLite working together in a real
# desktop session.

$ErrorActionPreference = "Stop"

$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root "src-tauri\target\release\nexus-clipboard.exe"
$dataDir = Join-Path $env:APPDATA "dev.nexus.clipboard"

if (-not (Test-Path $exe)) {
    Write-Host "FAIL: no binary at $exe - run 'pnpm app:build' first" -ForegroundColor Red
    exit 1
}

Write-Host "-> binary:   $exe"
Write-Host "-> data dir: $dataDir"

# Start clean so the assertions cannot pass on stale data.
if (Test-Path $dataDir) {
    Write-Host "-> clearing previous data"
    Remove-Item $dataDir -Recurse -Force
}

# Windows PowerShell 5.1 cannot assign directly from a try/catch block.
$originalClipboard = $null
try { $originalClipboard = Get-Clipboard -Raw } catch { }

function Stop-App($process) {
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
        Start-Sleep -Milliseconds 500
    }
    if ($originalClipboard) { Set-Clipboard -Value $originalClipboard }
}

Write-Host "`n-> launching"
$proc = Start-Process -FilePath $exe -PassThru
Start-Sleep -Seconds 6

if ($proc.HasExited) {
    Write-Host "FAIL: the app exited immediately (code $($proc.ExitCode))" -ForegroundColor Red
    exit 1
}
Write-Host "  running as PID $($proc.Id)"

if (-not (Test-Path (Join-Path $dataDir "history.db"))) {
    Write-Host "FAIL: no database was created" -ForegroundColor Red
    Stop-App $proc
    exit 1
}
Write-Host "  database created"

# Each Set-Clipboard fires WM_CLIPBOARDUPDATE - exactly the path the watcher
# listens on. The last payload is a credential, which must come back encrypted.
Write-Host "`n-> copying test payloads"
$stamp = Get-Random
$nl = [Environment]::NewLine

# Built with single-quoted literals and explicit concatenation so no payload
# character is at the mercy of PowerShell's string escaping.
$codeSample = 'pub fn smoke() {' + $nl + '    let mut n = ' + $stamp + ';' + $nl +
              '    println!("{}", n);' + $nl + '}'

$payloads = @(
    @{ kind = 'text';   value = 'smoke test plain text ' + $stamp },
    @{ kind = 'link';   value = 'https://example.com/smoke-' + $stamp },
    @{ kind = 'json';   value = '{"smoke": true, "n": ' + $stamp + '}' },
    @{ kind = 'code';   value = $codeSample },
    @{ kind = 'secret'; value = 'ghp_smoketest' + ('a' * 30) }
)

foreach ($payload in $payloads) {
    Set-Clipboard -Value $payload.value
    Write-Host ('  copied ' + $payload.kind)
    Start-Sleep -Milliseconds 700
}

Start-Sleep -Seconds 2

Write-Host "`n-> inspecting the database"
$env:NEXUS_DATA_DIR = $dataDir
& node (Join-Path $PSScriptRoot "inspect-db.mjs")
$inspected = $LASTEXITCODE

$mem = [math]::Round((Get-Process -Id $proc.Id).WorkingSet64 / 1MB, 1)
Write-Host "`n-> memory: $mem MB"

Write-Host "`n-> stopping"
Stop-App $proc

if ($inspected -ne 0) {
    Write-Host "`nFAIL: nothing was captured" -ForegroundColor Red
    exit 1
}

Write-Host "`nPASS" -ForegroundColor Green
