#!powershell
[CmdletBinding()]
param(
  [int]$Port = 8090,
  [switch]$Debug,
  [string]$DocsUrl,
  [string]$AdminToken,
  [int]$TimeoutSecs = 20,
  [switch]$UseDist,
  [switch]$NoBuild,
  [switch]$WaitHealth,
  [int]$WaitHealthTimeoutSecs = 30,
  [switch]$DryRun
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Compatibility: PowerShell 5 vs 7 for Invoke-WebRequest
$script:IwrArgs = @{}
try { if ($PSVersionTable.PSVersion.Major -lt 6) { $script:IwrArgs = @{ UseBasicParsing = $true } } } catch {}

# Optional: WebView2 runtime helpers (for Tauri-based launcher)
$script:HasWebView2 = $true
try {
  . (Join-Path $PSScriptRoot 'webview2.ps1')
  try {
    if (Get-Command Test-WebView2Runtime -ErrorAction SilentlyContinue) { $script:HasWebView2 = (Test-WebView2Runtime) }
  } catch { $script:HasWebView2 = $true }
} catch { $script:HasWebView2 = $true }

function Info($m){ Write-Host "[start] $m" -ForegroundColor DarkCyan }
function Dry($m){ if ($DryRun) { Write-Host "[dryrun] $m" -ForegroundColor Yellow } }

if ($Debug) { if (-not $DryRun) { $env:ARW_DEBUG = '1' } else { Dry 'Would set ARW_DEBUG=1' } }
if ($DocsUrl) { if (-not $DryRun) { $env:ARW_DOCS_URL = $DocsUrl } else { Dry "Would set ARW_DOCS_URL=$DocsUrl" } }
if ($AdminToken) { if (-not $DryRun) { $env:ARW_ADMIN_TOKEN = $AdminToken } else { Dry 'Would set ARW_ADMIN_TOKEN=<redacted>' } }
if ($TimeoutSecs) { if (-not $DryRun) { $env:ARW_HTTP_TIMEOUT_SECS = "$TimeoutSecs" } else { Dry "Would set ARW_HTTP_TIMEOUT_SECS=$TimeoutSecs" } }
if ($Port) { if (-not $DryRun) { $env:ARW_PORT = "$Port" } else { Dry "Would set ARW_PORT=$Port" } }

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe = 'arw-svc.exe'
$launcherExe = 'arw-launcher.exe'
$svc = if ($UseDist) {
  $zipBase = Get-ChildItem -Path (Join-Path $root 'dist') -Filter 'arw-*-windows-*' -Directory -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if ($zipBase) { Join-Path $zipBase.FullName (Join-Path 'bin' $exe) } else { $null }
} else { Join-Path (Join-Path $root 'target\release') $exe }
$launcher = if ($UseDist) {
  if ($zipBase) { Join-Path $zipBase.FullName (Join-Path 'bin' $launcherExe) } else { $null }
} else { Join-Path (Join-Path $root 'target\release') $launcherExe }

if (-not $svc -or -not (Test-Path $svc)) {
  if ($DryRun) {
    Write-Warning "Service binary not found ($svc). [dryrun] would build release (arw-svc)."
    $svc = Join-Path (Join-Path $root 'target\release') $exe
  } elseif ($NoBuild) {
    Write-Error "Service binary not found and -NoBuild specified. Build first or remove -NoBuild."
    exit 1
  } else {
    Write-Warning "Service binary not found ($svc). Building release..."
    try {
      Push-Location $root
      if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw "Rust 'cargo' not found in PATH. Install Rust from https://rustup.rs" }
      cargo build --release -p arw-svc
    } catch {
      Write-Error ("Failed to build arw-svc: " + $_.Exception.Message)
      Pop-Location
      exit 1
    } finally {
      if ((Get-Location).Path -ne $root) { try { Pop-Location } catch {} }
    }
    $svc = Join-Path (Join-Path $root 'target\release') $exe
  }
}

if (-not $launcher -or -not (Test-Path $launcher)) {
  if ($DryRun) {
    Write-Warning "Launcher binary not found ($launcher). [dryrun] would attempt build (arw-launcher)."
  } elseif (-not $NoBuild) {
    Write-Warning "Launcher binary not found ($launcher). Attempting build..."
    try {
      Push-Location $root
      if (Get-Command cargo -ErrorAction SilentlyContinue) {
        cargo build --release -p arw-launcher
      } else {
        Write-Warning "Rust 'cargo' not found; skipping launcher build."
      }
    } catch {
      Write-Warning "Launcher build attempt failed: $($_.Exception.Message)"
    } finally {
      try { Pop-Location } catch {}
    }
  }
  $launcher = Join-Path (Join-Path $root 'target\release') $launcherExe
}

# Respect ARW_NO_LAUNCHER/ARW_NO_TRAY=1 for CLI-only environments
$skipLauncher = $false
if (($env:ARW_NO_LAUNCHER -and $env:ARW_NO_LAUNCHER -eq '1') -or ($env:ARW_NO_TRAY -and $env:ARW_NO_TRAY -eq '1')) { $skipLauncher = $true }

function Ensure-ParentDir($path) {
  try {
    $dir = [System.IO.Path]::GetDirectoryName($path)
    if ($dir -and $dir -ne '') { New-Item -ItemType Directory -Force $dir | Out-Null }
  } catch {}
}

function Wait-For-Health($port, $timeoutSecs) {
  $base = "http://127.0.0.1:$port"
  $deadline = (Get-Date).AddSeconds([int]$timeoutSecs)
  $ok = $false
  $attempts = 0
  while ((Get-Date) -lt $deadline) {
    $attempts++
    try {
      $resp = Invoke-WebRequest @IwrArgs -TimeoutSec 5 -Uri ("$base/healthz")
      if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300) { $ok = $true; break }
    } catch {}
    Start-Sleep -Milliseconds 500
  }
  if ($ok) { Info ("Health OK after " + $attempts + " checks → $base/healthz") } else { Write-Warning ("Health not reachable within $timeoutSecs seconds → $base/healthz") }
}

if (-not $skipLauncher -and (Test-Path $launcher)) {
  Info "Launching $svc on http://127.0.0.1:$Port"
  if ($DryRun) {
    Dry ("Would start: $svc (cwd=$root)")
    if ($env:ARW_LOG_FILE) { Dry ("Would redirect output to $env:ARW_LOG_FILE") }
    if ($env:ARW_PID_FILE) { Dry ("Would write PID file to $env:ARW_PID_FILE") }
    if ($WaitHealth) { Dry ("Would wait for health at /healthz (timeout ${WaitHealthTimeoutSecs}s)") }
    Dry ("Would launch launcher: $launcher")
  } else {
    if ($env:ARW_LOG_FILE) {
      Ensure-ParentDir $env:ARW_LOG_FILE
      $p = Start-Process -FilePath $svc -WorkingDirectory $root -WindowStyle Hidden -RedirectStandardOutput $env:ARW_LOG_FILE -RedirectStandardError $env:ARW_LOG_FILE -PassThru
    } else {
      $p = Start-Process -FilePath $svc -WorkingDirectory $root -WindowStyle Hidden -PassThru
    }
    if ($env:ARW_PID_FILE) {
      Ensure-ParentDir $env:ARW_PID_FILE
      try { $p.Id | Out-File -FilePath $env:ARW_PID_FILE -Encoding ascii -Force } catch {}
    }
    if ($WaitHealth) { Wait-For-Health -port $Port -timeoutSecs $WaitHealthTimeoutSecs }
    if (-not $script:HasWebView2) {
      Write-Warning "WebView2 Runtime not detected; the launcher may prompt to install it. You can install it now via: powershell -ExecutionPolicy Bypass -File scripts/webview2.ps1"
    }
    Info "Launching launcher $launcher"
    # Hint the launcher to auto-start the service if not already running
    try { $env:ARW_AUTOSTART = '1' } catch {}
    & $launcher
  }
} else {
  $msg = if ($skipLauncher) { '(headless env: ARW_NO_LAUNCHER/ARW_NO_TRAY)' } else { '(launcher not found)' }
  Info "Launching $svc on http://127.0.0.1:$Port $msg"
  if ($DryRun) {
    Dry ("Would start: $svc (cwd=$root)")
    if ($env:ARW_LOG_FILE) { Dry ("Would redirect output to $env:ARW_LOG_FILE") }
    if ($env:ARW_PID_FILE) { Dry ("Would write PID file to $env:ARW_PID_FILE") }
    if ($WaitHealth) { Dry ("Would wait for health at /healthz (timeout ${WaitHealthTimeoutSecs}s)") }
  } else {
    if ($env:ARW_PID_FILE) {
      if ($env:ARW_LOG_FILE) {
        Ensure-ParentDir $env:ARW_LOG_FILE
        $p = Start-Process -FilePath $svc -WorkingDirectory $root -WindowStyle Hidden -RedirectStandardOutput $env:ARW_LOG_FILE -RedirectStandardError $env:ARW_LOG_FILE -PassThru
      } else {
        $p = Start-Process -FilePath $svc -WorkingDirectory $root -WindowStyle Hidden -PassThru
      }
      Ensure-ParentDir $env:ARW_PID_FILE
      try { $p.Id | Out-File -FilePath $env:ARW_PID_FILE -Encoding ascii -Force } catch {}
      if ($WaitHealth) { Wait-For-Health -port $Port -timeoutSecs $WaitHealthTimeoutSecs }
    } else {
      if ($env:ARW_LOG_FILE) {
        & $svc *> $env:ARW_LOG_FILE
      } else {
        & $svc
      }
    }
  }
}
