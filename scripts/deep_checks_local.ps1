#!/usr/bin/env pwsh
$ErrorActionPreference = 'Stop'

# Local deep checks harness (Windows)
# - Builds arw-server, arw-cli, arw-mini-dashboard (release)
# - Starts arw-server on ARW_PORT (default 8099)
# - Verifies daily brief/economy snapshots, route_stats (mini-dashboard once),
#   events tail structured output, and SSE metrics presence

$BASE = $env:BASE; if (-not $BASE) { $BASE = 'http://127.0.0.1:8099' }
$env:ARW_ADMIN_TOKEN = if ($env:ARW_ADMIN_TOKEN) { $env:ARW_ADMIN_TOKEN } else { 'test-admin-token' }
$env:ARW_PORT = if ($env:ARW_PORT) { $env:ARW_PORT } else { '8099' }
$env:ARW_DEBUG = '1'

Write-Host '[deep-checks] building (release)'
cargo build -p arw-server -p arw-cli -p arw-mini-dashboard --release

Write-Host "[deep-checks] starting arw-server on port $($env:ARW_PORT)"
$p = Start-Process -FilePath target/release/arw-server.exe -PassThru -WindowStyle Hidden -RedirectStandardOutput server.out -RedirectStandardError server.err
Set-Content -Path $env:TEMP\arw-svc-local.pid -Value $p.Id

for ($i=0; $i -lt 60; $i++) {
  try { Invoke-WebRequest -UseBasicParsing "$BASE/healthz" -TimeoutSec 2 | Out-Null; break } catch { Start-Sleep -Seconds 1 }
}
Invoke-WebRequest -UseBasicParsing "$BASE/about" | Out-Null

Write-Host '[deep-checks] daily brief + economy'
$headers = @{ Authorization = "Bearer $env:ARW_ADMIN_TOKEN" }
(Invoke-WebRequest -UseBasicParsing -Headers $headers "$BASE/state/briefs/daily").Content | Out-File -FilePath $env:TEMP\brief.json -Encoding ascii
target/release/arw-cli.exe state economy-ledger --base $BASE --limit 5 --json | Out-File -FilePath $env:TEMP\economy.json -Encoding ascii
$brief = Get-Content $env:TEMP\brief.json -Raw | ConvertFrom-Json
$econ = Get-Content $env:TEMP\economy.json -Raw | ConvertFrom-Json
if (-not $econ.PSObject.Properties.Name.Contains('version')) { throw 'economy has no version' }

Write-Host '[deep-checks] route_stats once (mini-dashboard)'
target/release/arw-mini-dashboard.exe --base $BASE --id route_stats --json --once | Out-File -FilePath $env:TEMP\route_stats.json -Encoding ascii
$rs = Get-Content $env:TEMP\route_stats.json -Raw | ConvertFrom-Json
if ($null -eq $rs) { throw 'route_stats parse failed' }

Write-Host '[deep-checks] events tail (structured, ~7s)'
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = "target/release/arw-cli.exe"
$psi.Arguments = "events tail --base $BASE --prefix service.,state. --structured --replay 1 --store $env:TEMP/last-event-id"
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$pt = [System.Diagnostics.Process]::Start($psi)
Start-Sleep -Seconds 7
if (-not $pt.HasExited) { $pt.Kill() }
$out = $pt.StandardOutput.ReadToEnd()
$first = ($out -split "`n")[0]
if (-not $first) {
  if ($env:DEEP_SOFT -eq '1' -or $env:DEEP_SOFT -eq 'true') {
    Write-Host '[deep-checks][soft] no events output; continuing due to DEEP_SOFT'
  } else {
    throw 'no events output'
  }
} else {
  $null = $first | ConvertFrom-Json
}
$out | Out-File -FilePath $env:TEMP\events_tail.json -Encoding ascii

Write-Host '[deep-checks] metrics (SSE counters present)'
$m = Invoke-WebRequest -UseBasicParsing "$BASE/metrics"
if (-not ($m.Content -match '^arw_events_sse_connections_total' -or $m.Content -match '^arw_events_sse_sent_total')) {
  if ($env:DEEP_SOFT -eq '1' -or $env:DEEP_SOFT -eq 'true') {
    Write-Host '[deep-checks][soft] sse metrics missing; continuing due to DEEP_SOFT'
  } else {
    throw 'sse metrics missing'
  }
} else {
  Write-Host 'metrics ok'
}

Write-Host '[deep-checks] stopping server'
if (Test-Path $env:TEMP\arw-svc-local.pid) { $pid = Get-Content $env:TEMP\arw-svc-local.pid; Stop-Process -Id $pid -ErrorAction SilentlyContinue }
Write-Host "[deep-checks] logs: server.out/server.err"

Write-Host '[deep-checks] OK'
