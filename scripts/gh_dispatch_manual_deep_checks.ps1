Param(
  [string]$Repo = "t3hw00t/ARW",
  [string]$Workflow = "manual-deep-checks.yml",
  [string]$Branch = "main",
  [string]$Soft = "false",
  [string]$TailSecs = "12",
  [int]$PollSeconds = 10,
  [int]$TimeoutSeconds = 900
)

$ErrorActionPreference = 'Stop'
$token = $env:GH_TOKEN
if (-not $token) { $token = $env:GITHUB_TOKEN }
if (-not $token) {
  Write-Error "GH_TOKEN/GITHUB_TOKEN not set. Export a token with 'actions:read' and 'actions:write' (repo scope) and retry."
}

function Invoke-GhApiJson {
  param([string]$Method, [string]$Url, [Object]$Body)
  $headers = @{ Authorization = "Bearer $token"; 'User-Agent' = 'arw-ci/1.0'; Accept = 'application/vnd.github+json' }
  if ($Body) {
    $json = $Body | ConvertTo-Json -Depth 6
    return Invoke-RestMethod -Method $Method -Uri $Url -Headers $headers -Body $json -ContentType 'application/json'
  } else {
    return Invoke-RestMethod -Method $Method -Uri $Url -Headers $headers
  }
}

function Get-WorkflowRuns {
  param([string]$Repo, [string]$Workflow, [string]$Branch)
  $url = "https://api.github.com/repos/$Repo/actions/workflows/$Workflow/runs?branch=$Branch&event=workflow_dispatch&per_page=5"
  $resp = Invoke-GhApiJson -Method GET -Url $url -Body $null
  return $resp.workflow_runs
}

Write-Host "[gh] dispatching $Workflow on $Repo@$Branch (soft=$Soft, tail_secs=$TailSecs)" -ForegroundColor Cyan

# Baseline runs before dispatch
$beforeRuns = Get-WorkflowRuns -Repo $Repo -Workflow $Workflow -Branch $Branch
$beforeTopId = if ($beforeRuns -and $beforeRuns.Count -gt 0) { $beforeRuns[0].id } else { 0 }

# Dispatch
$dispatchUrl = "https://api.github.com/repos/$Repo/actions/workflows/$Workflow/dispatches"
[void](Invoke-GhApiJson -Method POST -Url $dispatchUrl -Body @{ ref = $Branch; inputs = @{ soft = $Soft; tail_secs = $TailSecs } })

# Poll for new run
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$run = $null
do {
  Start-Sleep -Seconds $PollSeconds
  $runs = Get-WorkflowRuns -Repo $Repo -Workflow $Workflow -Branch $Branch
  if ($runs -and $runs.Count -gt 0) {
    if ($runs[0].id -ne $beforeTopId) { $run = $runs[0]; break }
  }
} while ((Get-Date) -lt $deadline)

if (-not $run) { throw "Timed out waiting for workflow run to start." }
Write-Host ("[gh] run id: {0} status: {1}" -f $run.id, $run.status)

# Wait for completion
do {
  Start-Sleep -Seconds $PollSeconds
  $runUrl = "https://api.github.com/repos/$Repo/actions/runs/$($run.id)"
  $run = Invoke-GhApiJson -Method GET -Url $runUrl -Body $null
  Write-Host ("[gh] status: {0} conclusion: {1}" -f $run.status, $run.conclusion)
} while ($run.status -ne 'completed' -and (Get-Date) -lt $deadline)

if ($run.status -ne 'completed') { throw "Run did not complete in time." }
if ($run.conclusion -ne 'success') { Write-Warning "Run conclusion: $($run.conclusion)" }

# List artifacts
$artsUrl = "https://api.github.com/repos/$Repo/actions/runs/$($run.id)/artifacts"
$arts = Invoke-GhApiJson -Method GET -Url $artsUrl -Body $null
$art = $arts.artifacts | Where-Object { $_.name -eq 'manual-deep-linux' } | Select-Object -First 1
if (-not $art) {
  Write-Warning "manual-deep-linux artifact not found; listing available:"
  $arts.artifacts | ForEach-Object { Write-Host (" - {0}" -f $_.name) }
  throw "Artifact missing"
}
Write-Host ("[gh] downloading artifact id={0} size={1}" -f $art.id, $art.size_in_bytes)

# Download artifact zip
$zipPath = Join-Path $env:TEMP "manual-deep-linux-$($run.id).zip"
$wc = New-Object System.Net.WebClient
$wc.Headers.Add('Authorization', "Bearer $token")
$wc.Headers.Add('User-Agent', 'arw-ci/1.0')
$wc.DownloadFile($art.archive_download_url, $zipPath)

# Extract
$outDir = Join-Path $env:TEMP "manual-deep-linux-$($run.id)"
if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir }
Expand-Archive -Path $zipPath -DestinationPath $outDir

# Parse and summarize
$econ = Get-Content (Join-Path $outDir 'economy.json') -Raw | ConvertFrom-Json
$rs = Get-Content (Join-Path $outDir 'route_stats.json') -Raw | ConvertFrom-Json
$first = (Get-Content (Join-Path $outDir 'events_tail.json') -TotalCount 1)
$metrics = Get-Content (Join-Path $outDir 'metrics.txt') -Raw
$hasSSE = ($metrics -match '^arw_events_sse_(clients|connections_total|sent_total)')

Write-Host ("[artifact] ECON version: {0} entries={1}" -f $econ.version, ($econ.entries | Measure-Object).Count)
Write-Host ("[artifact] ROUTE_STATS keys: {0}" -f ($rs.PSObject.Properties.Name -join ', '))
Write-Host ("[artifact] EVENTS first line present: {0}" -f ([bool]$first))
Write-Host ("[artifact] METRICS SSE present: {0}" -f $hasSSE)

Write-Host ("[artifact] saved: {0}" -f $outDir) -ForegroundColor Green
