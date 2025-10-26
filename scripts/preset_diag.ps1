$ErrorActionPreference = 'Stop'

param(
  [string]$Base = 'http://127.0.0.1:8091',
  [string]$Token = $env:ARW_ADMIN_TOKEN
)

function Get-About {
  param([string]$Url, [string]$Auth)
  $headers = @{}
  if ($Auth) { $headers['Authorization'] = "Bearer $Auth" }
  try {
    $resp = Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri "$Url/about" -TimeoutSec 5
    return $resp.Content | ConvertFrom-Json
  } catch {
    Write-Error "[preset-diag] cannot fetch $Url/about: $($_.Exception.Message)"; exit 1
  }
}

$about = Get-About -Url $Base -Auth $Token
$tier = $about.perf_preset.tier
if (-not $tier) { $tier = 'unknown' }
Write-Output ("Preset tier: {0}" -f $tier)
if ($about.perf_preset.http_max_conc -ne $null) { Write-Output ("HTTP concurrency (about): {0}" -f $about.perf_preset.http_max_conc) }
if ($about.perf_preset.actions_queue_max -ne $null) { Write-Output ("Actions queue max (about): {0}" -f $about.perf_preset.actions_queue_max) }

Write-Output 'Local env overrides (if set):'
$keys = @(
  'ARW_HTTP_MAX_CONC','ARW_WORKERS','ARW_WORKERS_MAX','ARW_ACTIONS_QUEUE_MAX',
  'ARW_TOOLS_CACHE_TTL_SECS','ARW_TOOLS_CACHE_CAP',
  'ARW_PREFER_LOW_POWER','ARW_LOW_POWER','ARW_OCR_PREFER_LOW_POWER','ARW_OCR_LOW_POWER',
  'ARW_ACCESS_LOG','ARW_ACCESS_UA','ARW_ACCESS_UA_HASH','ARW_ACCESS_REF',
  'ARW_EVENTS_SSE_DECORATE','ARW_RUNTIME_WATCHER_COOLDOWN_MS',
  'ARW_MEMORY_EMBED_BACKFILL_BATCH','ARW_MEMORY_EMBED_BACKFILL_IDLE_SEC'
)
$any = $false
foreach ($k in $keys) { if ($env:$k) { Write-Output ("  {0}={1}" -f $k, (Get-Item -Path Env:$k).Value); $any = $true } }
if (-not $any) { Write-Output '  (none)' }

exit 0

