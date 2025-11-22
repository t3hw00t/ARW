Param(
  [string]$Base = "http://127.0.0.1:8099",
  [int]$TailSeconds = 10,
  [switch]$Soft
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$docsVenvPy = Join-Path $root '.venv\docs\Scripts\python.exe'
if (-not (Test-Path $docsVenvPy)) { $docsVenvPy = Join-Path $root '.venv/docs/bin/python' }
if (-not $env:LLMLINGUA_PYTHON -and (Test-Path $docsVenvPy)) {
  $env:LLMLINGUA_PYTHON = $docsVenvPy
}

Write-Host "[deep-checks] building (release)" -ForegroundColor Cyan
cargo build -p arw-server -p arw-cli -p arw-mini-dashboard --release

# Isolated state/cache/logs under %TEMP% to avoid profile paths and lock conflicts
$baseDir = Join-Path $env:TEMP 'arw-local'
$dataDir = Join-Path $baseDir 'data'
$cacheDir = Join-Path $baseDir 'cache'
$logsDir = Join-Path $baseDir 'logs'
New-Item -ItemType Directory -Force -Path $dataDir,$cacheDir,$logsDir | Out-Null

$env:ARW_ADMIN_TOKEN = $env:ARW_ADMIN_TOKEN -as [string]
if (-not $env:ARW_ADMIN_TOKEN) { $env:ARW_ADMIN_TOKEN = 'test-admin-token' }
$env:ARW_DEBUG = '1'
$env:ARW_PORT  = ([uri]$Base).Port
if (-not $env:ARW_PORT) { $env:ARW_PORT = '8099' }
$env:ARW_DATA_DIR  = $dataDir
$env:ARW_CACHE_DIR = $cacheDir
$env:ARW_LOGS_DIR  = $logsDir

Write-Host "[deep-checks] starting arw-server on $Base" -ForegroundColor Cyan
$p = Start-Process -FilePath target/release/arw-server.exe -PassThru -WindowStyle Hidden -RedirectStandardOutput server.out -RedirectStandardError server.err
$pidFile = Join-Path $env:TEMP 'arw-svc.pid'
Set-Content -Path $pidFile -Value $p.Id

# Wait for healthz
$ok = $false
for ($i=0; $i -lt 120; $i++) {
  try {
    (Invoke-WebRequest -UseBasicParsing "$Base/healthz" -TimeoutSec 2) | Out-Null
    $ok = $true; break
  } catch { Start-Sleep -Seconds 1 }
}
if (-not $ok) {
  Write-Warning "[deep-checks] server did not become healthy"
  if (Test-Path server.err) { Get-Content server.err | Select-Object -Last 120 }
  throw "server not healthy"
}
Invoke-WebRequest -UseBasicParsing "$Base/about" | Out-Null

# Economy snapshot JSON
$econPath = Join-Path $env:TEMP 'economy.json'
& target/release/arw-cli.exe state economy-ledger --base $Base --limit 5 --json | Out-File -FilePath $econPath -Encoding ascii
$econ = Get-Content $econPath -Raw | ConvertFrom-Json
if ($null -eq $econ -or -not ($econ.PSObject.Properties.Name -contains 'version')) {
  throw "economy.json missing version"
}

# Route stats snapshot JSON
$rsPath = Join-Path $env:TEMP 'route_stats.json'
$headers = @{ Authorization = "Bearer $env:ARW_ADMIN_TOKEN" }
(Invoke-WebRequest -UseBasicParsing -Headers $headers "$Base/state/route_stats").Content | Out-File -FilePath $rsPath -Encoding ascii
$rs = Get-Content $rsPath -Raw | ConvertFrom-Json
if ($null -eq $rs -and -not $Soft) { throw "route_stats parse failed" }

# Events tail (structured)
Write-Host "[deep-checks] events tail (structured) ${TailSeconds}s" -ForegroundColor Cyan
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = "target/release/arw-cli.exe"
$psi.Arguments = "events tail --base $Base --prefix service.,state. --structured --replay 1 --store $env:TEMP/last-event-id"
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$p2 = [System.Diagnostics.Process]::Start($psi)
Start-Sleep -Seconds $TailSeconds
if (-not $p2.HasExited) { try { $p2.Kill() } catch {} }
$eventsPath = Join-Path $env:TEMP 'events_tail.json'
$out = $p2.StandardOutput.ReadToEnd()
$out | Out-File -FilePath $eventsPath -Encoding ascii
$first = (Get-Content $eventsPath -TotalCount 1)
if (-not $first -and -not $Soft) { throw "no events output" }

# Metrics scrape with fallback
$metricsPath = Join-Path $env:TEMP 'metrics.txt'
Write-Host "[deep-checks] metrics scrape" -ForegroundColor Cyan
Invoke-WebRequest -UseBasicParsing "$Base/metrics" | Select-Object -ExpandProperty Content | Out-File -FilePath $metricsPath -Encoding ascii
$metrics = Get-Content $metricsPath -Raw
$hasSSE = ($metrics -match '^arw_events_sse_(clients|connections_total|sent_total)')
if (-not $hasSSE) {
  $hasPatchCounter = ($metrics -match '^arw_events_total\{kind="state.read.model.patch"\}')
  if (-not $hasPatchCounter -and -not $Soft) { throw "missing SSE metrics" }
}

# Summaries
Write-Host ("ECON version: {0} entries: {1} totals: {2}" -f $econ.version, ($econ.entries | Measure-Object).Count, ($econ.totals | Measure-Object).Count)
Write-Host ("ROUTE_STATS keys: {0}" -f ($rs.PSObject.Properties.Name -join ', '))
Write-Host ("EVENTS first line present: {0} len={1}" -f ([bool]$first), ($first | ForEach-Object { $_.Length }))
Write-Host ("METRICS SSE present: {0}" -f $hasSSE)

# Stop server
Write-Host "[deep-checks] stopping server" -ForegroundColor Cyan
try {
  if (Test-Path $pidFile) { $pid = Get-Content $pidFile; Stop-Process -Id $pid -ErrorAction SilentlyContinue }
} catch {}
Write-Host "[deep-checks] logs: $(Resolve-Path server.out), $(Resolve-Path server.err)" -ForegroundColor DarkGray

Write-Host "[deep-checks] OK" -ForegroundColor Green
