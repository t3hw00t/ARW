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

function Use-GhCli {
  return [bool](Get-Command gh -ErrorAction SilentlyContinue)
}

function Invoke-GhApiJson {
  param([string]$Method, [string]$Url, [Object]$Body)
  $token = $env:GH_TOKEN; if (-not $token) { $token = $env:GITHUB_TOKEN }
  if (-not $token) { throw "GH_TOKEN/GITHUB_TOKEN not set and gh CLI not available." }
  $headers = @{ Authorization = "Bearer $token"; 'User-Agent' = 'arw-ci/1.0'; Accept = 'application/vnd.github+json' }
  if ($Body) {
    $json = $Body | ConvertTo-Json -Depth 6
    return Invoke-RestMethod -Method $Method -Uri $Url -Headers $headers -Body $json -ContentType 'application/json'
  } else {
    return Invoke-RestMethod -Method $Method -Uri $Url -Headers $headers
  }
}

function Get-WorkflowRunsApi {
  param([string]$Repo, [string]$Workflow, [string]$Branch)
  $url = "https://api.github.com/repos/$Repo/actions/workflows/$Workflow/runs?branch=$Branch&event=workflow_dispatch&per_page=5"
  $resp = Invoke-GhApiJson -Method GET -Url $url -Body $null
  return $resp.workflow_runs
}

function Dispatch-And-Wait-WithGh {
  param([string]$Repo, [string]$Workflow, [string]$Branch, [string]$Soft, [string]$TailSecs, [int]$PollSeconds, [int]$TimeoutSeconds)
  Write-Host "[gh] dispatching $Workflow on $Repo@$Branch (soft=$Soft, tail_secs=$TailSecs)" -ForegroundColor Cyan
  # Baseline latest run id
  $before = gh run list -R $Repo --workflow $Workflow --branch $Branch -L 1 --json databaseId | ConvertFrom-Json
  $beforeId = if ($before -and $before.Count -gt 0) { $before[0].databaseId } else { 0 }
  # Dispatch
  gh workflow run $Workflow -R $Repo -r $Branch -f soft=$Soft -f tail_secs=$TailSecs | Out-Null
  # Poll for new run
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  $runId = $null
  do {
    Start-Sleep -Seconds $PollSeconds
    $runs = gh run list -R $Repo --workflow $Workflow --branch $Branch -L 1 --json databaseId,status,conclusion | ConvertFrom-Json
    if ($runs -and $runs.Count -gt 0) {
      if ($runs[0].databaseId -ne $beforeId) { $runId = $runs[0].databaseId; break }
    }
  } while ((Get-Date) -lt $deadline)
  if (-not $runId) { throw "Timed out waiting for workflow run to start." }
  Write-Host ("[gh] run id: {0}" -f $runId)
  # Wait for completion (watch returns nonzero on failure); suppress output
  gh run watch $runId -R $Repo --exit-status | Out-Null
  return [long]$runId
}

function Download-And-Summarize-WithGh {
  param([string]$Repo, [long]$RunId)
  $outDir = Join-Path $env:TEMP "manual-deep-linux-$RunId"
  if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir }
  New-Item -ItemType Directory -Force -Path $outDir | Out-Null
  gh run download $RunId -R $Repo -n manual-deep-linux -D $outDir | Out-Null
  function Resolve-FirstFile([string]$dir,[string]$name) {
    $f = Get-ChildItem -Path $dir -Recurse -File -Filter $name -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($f) { return $f.FullName } else { return $null }
  }
  $econFile   = Resolve-FirstFile $outDir 'economy.json'
  $rsFile     = Resolve-FirstFile $outDir 'route_stats.json'
  $eventsFile = Resolve-FirstFile $outDir 'events_tail.json'
  $metricsFile= Resolve-FirstFile $outDir 'metrics.txt'
  if (-not $econFile) { throw "economy.json missing in artifact" }
  $econ = Get-Content $econFile -Raw | ConvertFrom-Json
  $rs = $null; if ($rsFile) { $rs = Get-Content $rsFile -Raw | ConvertFrom-Json }
  $first = $null; if ($eventsFile) { $first = Get-Content $eventsFile -TotalCount 1 }
  $metrics = ''; if ($metricsFile) { $metrics = Get-Content $metricsFile -Raw }
  $hasSSE = ($metrics -match '^arw_events_sse_(clients|connections_total|sent_total)')
  Write-Host ("[artifact] ECON version: {0} entries={1}" -f $econ.version, ($econ.entries | Measure-Object).Count)
  if ($rs) { Write-Host ("[artifact] ROUTE_STATS keys: {0}" -f ($rs.PSObject.Properties.Name -join ', ')) } else { Write-Warning "[artifact] route_stats.json missing" }
  Write-Host ("[artifact] EVENTS first line present: {0}" -f ([bool]$first))
  Write-Host ("[artifact] METRICS SSE present: {0}" -f $hasSSE)
  Write-Host ("[artifact] saved: {0}" -f $outDir) -ForegroundColor Green
}

# Main
if (Use-GhCli) {
  $rid = Dispatch-And-Wait-WithGh -Repo $Repo -Workflow $Workflow -Branch $Branch -Soft $Soft -TailSecs $TailSecs -PollSeconds $PollSeconds -TimeoutSeconds $TimeoutSeconds
  Download-And-Summarize-WithGh -Repo $Repo -RunId $rid
  return
}

# Fallback: REST API using GH_TOKEN/GITHUB_TOKEN
$token = $env:GH_TOKEN; if (-not $token) { $token = $env:GITHUB_TOKEN }
if (-not $token) {
  Write-Error "gh CLI not available and no GH_TOKEN/GITHUB_TOKEN set. Run 'gh auth login' or export a token."
}

Write-Host "[api] dispatching $Workflow on $Repo@$Branch (soft=$Soft, tail_secs=$TailSecs)" -ForegroundColor Cyan
$beforeRuns = Get-WorkflowRunsApi -Repo $Repo -Workflow $Workflow -Branch $Branch
$beforeTopId = if ($beforeRuns -and $beforeRuns.Count -gt 0) { $beforeRuns[0].id } else { 0 }
$dispatchUrl = "https://api.github.com/repos/$Repo/actions/workflows/$Workflow/dispatches"
[void](Invoke-GhApiJson -Method POST -Url $dispatchUrl -Body @{ ref = $Branch; inputs = @{ soft = $Soft; tail_secs = $TailSecs } })
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$run = $null
do {
  Start-Sleep -Seconds $PollSeconds
  $runs = Get-WorkflowRunsApi -Repo $Repo -Workflow $Workflow -Branch $Branch
  if ($runs -and $runs.Count -gt 0) {
    if ($runs[0].id -ne $beforeTopId) { $run = $runs[0]; break }
  }
} while ((Get-Date) -lt $deadline)
if (-not $run) { throw "Timed out waiting for workflow run to start." }
do {
  Start-Sleep -Seconds $PollSeconds
  $runUrl = "https://api.github.com/repos/$Repo/actions/runs/$($run.id)"
  $run = Invoke-GhApiJson -Method GET -Url $runUrl -Body $null
  Write-Host ("[api] status: {0} conclusion: {1}" -f $run.status, $run.conclusion)
} while ($run.status -ne 'completed' -and (Get-Date) -lt $deadline)
if ($run.status -ne 'completed') { throw "Run did not complete in time." }
if ($run.conclusion -ne 'success') { Write-Warning "Run conclusion: $($run.conclusion)" }
$artsUrl = "https://api.github.com/repos/$Repo/actions/runs/$($run.id)/artifacts"
$arts = Invoke-GhApiJson -Method GET -Url $artsUrl -Body $null
$art = $arts.artifacts | Where-Object { $_.name -eq 'manual-deep-linux' } | Select-Object -First 1
if (-not $art) { throw "Artifact manual-deep-linux missing" }
$zipPath = Join-Path $env:TEMP "manual-deep-linux-$($run.id).zip"
$wc = New-Object System.Net.WebClient
$wc.Headers.Add('Authorization', "Bearer $token")
$wc.Headers.Add('User-Agent', 'arw-ci/1.0')
$wc.DownloadFile($art.archive_download_url, $zipPath)
$outDir = Join-Path $env:TEMP "manual-deep-linux-$($run.id)"
if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir }
Expand-Archive -Path $zipPath -DestinationPath $outDir
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
