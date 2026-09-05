# Verifies that every startup subsystem actually came up.
#
# The smoke test proves capture works end to end, but a failure in the tray,
# the global shortcuts or the pre-created launcher window is logged and then
# tolerated by design - the app keeps running. This script reads those logs so
# a silent degradation cannot pass unnoticed.

$ErrorActionPreference = "Stop"

$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root "src-tauri\target\release\nexus-clipboard.exe"
$log = Join-Path $env:TEMP "nexus-startup.log"
$out = Join-Path $env:TEMP "nexus-startup.out"

if (-not (Test-Path $exe)) {
    Write-Host "FAIL: no binary at $exe" -ForegroundColor Red
    exit 1
}

if (Test-Path $log) { Remove-Item $log -Force }
if (Test-Path $out) { Remove-Item $out -Force }

Write-Host "-> launching with NEXUS_LOG=info"
$env:NEXUS_LOG = "info"

# The release binary uses the "windows" subsystem, so it has no console - but
# stderr redirection still captures the tracing output.
$proc = Start-Process -FilePath $exe -PassThru -RedirectStandardError $log -RedirectStandardOutput $out

Start-Sleep -Seconds 7

if ($proc.HasExited) {
    Write-Host "FAIL: exited immediately (code $($proc.ExitCode))" -ForegroundColor Red
    if (Test-Path $log) { Get-Content $log }
    exit 1
}

Stop-Process -Id $proc.Id -Force
Start-Sleep -Milliseconds 800

# tracing's fmt subscriber writes to stdout by default, but a panic or a
# runtime failure lands on stderr - read both so neither can be missed.
$lines = @()
foreach ($stream in @($out, $log)) {
    if (Test-Path $stream) { $lines += Get-Content $stream }
}

if ($lines.Count -eq 0) {
    Write-Host "FAIL: no log was produced" -ForegroundColor Red
    exit 1
}

Write-Host "`n-> startup log"
$lines | ForEach-Object { Write-Host "   $_" }

# Each subsystem logs on success and warns on failure. Assert on both
# directions so a missing line is a failure rather than a silent pass.
$expected = @(
    @{ name = "clipboard watcher"; pattern = "clipboard watcher started" },
    @{ name = "app ready";         pattern = "Nexus Clipboard is ready" }
)

$problems = 0
Write-Host ""
foreach ($check in $expected) {
    if ($lines -match $check.pattern) {
        Write-Host ("  ok    " + $check.name) -ForegroundColor Green
    } else {
        Write-Host ("  FAIL  " + $check.name + " did not start") -ForegroundColor Red
        $problems++
    }
}

# Any warning means a subsystem degraded instead of failing outright.
$warnings = $lines | Where-Object { $_ -match "WARN|ERROR" }
if ($warnings) {
    Write-Host "`n-> degraded subsystems:" -ForegroundColor Yellow
    $warnings | ForEach-Object { Write-Host "   $_" -ForegroundColor Yellow }
    $problems++
} else {
    Write-Host "  ok    no warnings" -ForegroundColor Green
}

if ($problems -gt 0) {
    Write-Host "`nFAIL - $problems problem(s)" -ForegroundColor Red
    exit 1
}

Write-Host "`nPASS - every subsystem started cleanly" -ForegroundColor Green
