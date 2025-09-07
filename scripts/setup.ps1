#!powershell
param(
  [switch]$Yes,
  [switch]$NoDocs,
  [switch]$RunTests
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Title($t){ Write-Host "`n=== $t ===" -ForegroundColor Cyan }
function Info($m){ Write-Host "[setup] $m" -ForegroundColor DarkCyan }
function Warn($m){ Write-Warning $m }
function Pause($m){ if(-not $Yes){ Read-Host $m | Out-Null } }

Title 'Prerequisites'
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
    }
  }
}

Title 'Build workspace (release)'
& cargo build --workspace --release --locked

if ($RunTests) {
  Title 'Run tests (workspace)'
  & cargo test --workspace --locked
}

Title 'Generate workspace status page'
try { & ./scripts/docgen.ps1 } catch { Warn "docgen failed: $($_.Exception.Message)" }

if (-not $NoDocs -and (Get-Command mkdocs -ErrorAction SilentlyContinue)) {
  Title 'Build docs site (MkDocs)'
  & mkdocs build --strict
} else { Info 'Skipping docs site build.' }

Title 'Package portable bundle'
& ./scripts/package.ps1 -NoBuild

Info 'Done. See dist/ for portable bundle.'

