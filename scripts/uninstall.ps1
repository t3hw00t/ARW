#!powershell
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$log = Join-Path $root '.install.log'
if (-not (Test-Path $log)) {
  Write-Host '[uninstall] No install log found; nothing to do.'
  exit 0
}
$removed = @()
$left = @()
$py = Get-Command python -ErrorAction SilentlyContinue
if (-not $py) { $py = Get-Command python3 -ErrorAction SilentlyContinue }
Get-Content $log | ForEach-Object {
  if ($_ -match '^(#|\s*$)') { return }
  $parts = $_.Split(' ',2)
  $type = $parts[0]
  $item = $parts[1]
  switch ($type) {
    'DIR' {
      $path = Join-Path $root $item
      if (Test-Path $path) {
        Remove-Item -Recurse -Force $path
        $removed += $item
      } else {
        $left += "$item (missing)"
      }
    }
    'PIP' {
      if ($py) {
        try {
          & $py.Path -m pip uninstall -y $item | Out-Null
          $removed += "pip package $item"
        } catch {
          $left += "pip package $item"
        }
      } else {
        $left += "pip package $item (python not found)"
      }
    }
  }
}
Remove-Item -Force $log
Write-Host '[uninstall] Removed:'
foreach ($r in $removed) { Write-Host "  - $r" }
if ($left.Count -gt 0) {
  Write-Host '[uninstall] Left on system:'
  foreach ($k in $left) { Write-Host "  - $k" }
}
Write-Host '[uninstall] Done.'
