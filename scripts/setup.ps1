#!powershell
param(
  [switch]$Yes,
  [switch]$RunTests,
  [switch]$NoDocs
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
# Record install actions for uninstall.ps1
$installLog = Join-Path $root '.install.log'
"# Install log - $(Get-Date)" | Out-File $installLog -Encoding UTF8
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Warn 'Rust `cargo` not found.'
  Write-Host 'Install Rust via rustup:' -ForegroundColor Yellow
  Write-Host '  https://rustup.rs' -ForegroundColor Yellow
  Pause 'Press Enter after installing Rust (or Ctrl+C to abort)'
}

$py = (Get-Command python -ErrorAction SilentlyContinue) ?? (Get-Command python3 -ErrorAction SilentlyContinue)
if (-not $py) {
  Warn 'Python not found. Docs/site build and docgen extras may be skipped.'
} else {
  if (-not (Get-Command mkdocs -ErrorAction SilentlyContinue)) {
    if ($NoDocs) { Warn 'Skipping MkDocs install because -NoDocs was provided.' }
    else {
      Info 'MkDocs not found. Attempting to install via pip...'
      try { & $py.Path -m pip install --upgrade pip | Out-Null } catch { Warn 'pip upgrade failed (continuing).'}
      try { & $py.Path -m pip install mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin } catch { Warn 'pip install for mkdocs failed. Docs site will be skipped.' }
      if (Get-Command mkdocs -ErrorAction SilentlyContinue) {
        foreach($pkg in 'mkdocs','mkdocs-material','mkdocs-git-revision-date-localized-plugin') { Add-Content $installLog "PIP $pkg" }
      } else {
        Warn 'MkDocs install failed. Docs site will be skipped.'
      }
    }
  }
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
'DIR target','DIR dist' | ForEach-Object { Add-Content $installLog $_ }
if (Test-Path (Join-Path $root 'site')) { Add-Content $installLog 'DIR site' }

Pop-Location
if ($warnings.Count -gt 0) {
  Title 'Warnings'
  foreach ($w in $warnings) { Write-Host "- $w" -ForegroundColor Yellow }
}
Info 'Done. See dist/ for portable bundle.'
