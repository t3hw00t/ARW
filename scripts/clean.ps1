#!powershell
param(
  [switch]$Hard
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

Write-Host '[clean] Done.' -ForegroundColor Cyan
