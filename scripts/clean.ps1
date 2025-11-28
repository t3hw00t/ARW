#!powershell
param(
  [switch]$Hard,
  [switch]$State,
  [switch]$LauncherCache
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function RmPath($p){ if (Test-Path $p) { Remove-Item -Force -Recurse -ErrorAction SilentlyContinue $p } }

Write-Host '[clean] Removing target/ and dist/' -ForegroundColor Cyan
RmPath 'target'
RmPath 'dist'

if ($Hard) {
  Write-Host '[clean] Hard mode: removing backups (*.bak_*, .backups/)' -ForegroundColor Yellow
  Get-ChildItem -Recurse -File -Filter '*.bak_*' | ForEach-Object { Remove-Item -Force $_.FullName }
  if (Test-Path '.backups') { Remove-Item -Recurse -Force '.backups' }
}

if ($State -or $LauncherCache) {
  Write-Host '[clean] Removing local state/caches under AppData (user-scoped)' -ForegroundColor Yellow
}

if ($State) {
  # Removes runtime state and logs (clears local DB/cache). Safe for “offline/offline” resets.
  $localState = Join-Path $env:LOCALAPPDATA 'arw'
  $roamingState = Join-Path $env:APPDATA 'arw'
  Write-Host "  - Removing $localState" -ForegroundColor Yellow
  Write-Host "  - Removing $roamingState" -ForegroundColor Yellow
  RmPath $localState
  RmPath $roamingState
}

if ($LauncherCache) {
  # Clears the WebView2 profile used by the launcher UI (fixes stuck/offline states).
  $launcherCachePath = Join-Path $env:LOCALAPPDATA 'org.arw.launcher'
  Write-Host "  - Removing $launcherCachePath" -ForegroundColor Yellow
  RmPath $launcherCachePath
}

Write-Host '[clean] Done.' -ForegroundColor Cyan
