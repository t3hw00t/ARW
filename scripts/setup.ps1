#!powershell
param(
  [switch]$Yes,
  [switch]$RunTests
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:warnings = @()
function Title($t){ Write-Host "`n=== $t ===" -ForegroundColor Cyan }
function Info($m){ Write-Host "[setup] $m" -ForegroundColor DarkCyan }
function Warn($m){ $script:warnings += $m }
function Pause($m){ if(-not $Yes){ Read-Host $m | Out-Null } }

Title 'Prerequisites'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Warn 'Rust `cargo` not found.'
  Write-Host 'Install Rust via rustup:' -ForegroundColor Yellow
  Write-Host '  https://rustup.rs' -ForegroundColor Yellow
  Pause 'Press Enter after installing Rust (or Ctrl+C to abort)'
}

Title 'Build workspace (release)'
& cargo build --workspace --release --locked

if ($RunTests) {
  Title 'Run tests (workspace)'
  & cargo nextest run --workspace --locked
}

Title 'Generate workspace status page'
try { & (Join-Path $PSScriptRoot 'docgen.ps1') } catch { Warn "docgen failed: $($_.Exception.Message)" }

Title 'Package portable bundle'
try {
  & (Join-Path $PSScriptRoot 'package.ps1') -NoBuild
} catch {
  Warn "package.ps1 blocked by execution policy; retrying via child PowerShell with Bypass"
  & powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'package.ps1') -NoBuild
}

Pop-Location
if ($warnings.Count -gt 0) {
  Title 'Warnings'
  foreach ($w in $warnings) { Write-Host "- $w" -ForegroundColor Yellow }
}
Info 'Done. See dist/ for portable bundle.'
