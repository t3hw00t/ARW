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

$py = Get-Command python -ErrorAction SilentlyContinue
if (-not $py) { $py = Get-Command python3 -ErrorAction SilentlyContinue }
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

Title 'Windows runtime check (WebView2 for Launcher)'
try {
  . (Join-Path $PSScriptRoot 'webview2.ps1')
  $hasWV2 = Test-WebView2Runtime
  if ($hasWV2) {
    Info 'WebView2 Evergreen Runtime detected.'
  } else {
    Write-Host 'WebView2 Runtime not found. Required for the Tauri-based Desktop Launcher on Windows 10/Server.' -ForegroundColor Yellow
    Write-Host 'On Windows 11 it is in-box. You can install the Evergreen Runtime now.' -ForegroundColor Yellow
    if ($Yes) {
      $ok = Install-WebView2Runtime -Silent
      if ($ok) { Info 'WebView2 installed.' } else { Warn 'WebView2 install failed or was cancelled.' }
    } else {
      $ans = Read-Host 'Install WebView2 Runtime now? (y/N)'
      if ($ans -match '^[yY]') {
        $ok = Install-WebView2Runtime
        if ($ok) { Info 'WebView2 installed.' } else { Warn 'WebView2 install failed or was cancelled.' }
      }
    }
  }
} catch {
  Warn "WebView2 check failed: $($_.Exception.Message)"
}

Pop-Location
if ($warnings.Count -gt 0) {
  Title 'Warnings'
  foreach ($w in $warnings) { Write-Host "- $w" -ForegroundColor Yellow }
}
Info 'Done. See dist/ for portable bundle.'
