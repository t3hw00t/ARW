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
  [int]$WaitHealthTimeoutSecs = 30
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Info($m){ Write-Host "[start] $m" -ForegroundColor DarkCyan }

if ($Debug) { $env:ARW_DEBUG = '1' }
if ($DocsUrl) { $env:ARW_DOCS_URL = $DocsUrl }
if ($AdminToken) { $env:ARW_ADMIN_TOKEN = $AdminToken }
if ($TimeoutSecs) { $env:ARW_HTTP_TIMEOUT_SECS = "$TimeoutSecs" }
if ($Port) { $env:ARW_PORT = "$Port" }

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe = 'arw-svc.exe'
$trayExe = 'arw-tray.exe'
$svc = if ($UseDist) {
  $zipBase = Get-ChildItem -Path (Join-Path $root 'dist') -Filter 'arw-*-windows-*' -Directory -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if ($zipBase) { Join-Path $zipBase.FullName (Join-Path 'bin' $exe) } else { $null }
} else { Join-Path (Join-Path $root 'target\release') $exe }
$tray = if ($UseDist) {
  if ($zipBase) { Join-Path $zipBase.FullName (Join-Path 'bin' $trayExe) } else { $null }
} else { Join-Path (Join-Path $root 'target\release') $trayExe }

if (-not $svc -or -not (Test-Path $svc)) {
  if ($NoBuild) {
    Write-Error "Service binary not found and -NoBuild specified. Build first or remove -NoBuild."
    exit 1
  }
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

if (-not $tray -or -not (Test-Path $tray)) {
  if (-not $NoBuild) {
    Write-Warning "Tray binary not found ($tray). Attempting build..."
    try {
      Push-Location $root
      if (Get-Command cargo -ErrorAction SilentlyContinue) {
        cargo build --release -p arw-tray
      } else {
        Write-Warning "Rust 'cargo' not found; skipping tray build."
      }
    } catch {
      Write-Warning "Tray build attempt failed: $($_.Exception.Message)"
    } finally {
      try { Pop-Location } catch {}
    }
  }
  $tray = Join-Path (Join-Path $root 'target\release') $trayExe
}

# Respect ARW_NO_TRAY=1 for CLI-only environments
$skipTray = $false
if ($env:ARW_NO_TRAY -and $env:ARW_NO_TRAY -eq '1') { $skipTray = $true }

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
      $resp = Invoke-WebRequest -UseBasicParsing -TimeoutSec 5 -Uri ("$base/healthz")
      if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 500) { $ok = $true; break }
    } catch {}
    Start-Sleep -Milliseconds 500
  }
  if ($ok) { Info ("Health OK after " + $attempts + " checks → $base/healthz") } else { Write-Warning ("Health not reachable within $timeoutSecs seconds → $base/healthz") }
}

if (-not $skipTray -and (Test-Path $tray)) {
  Info "Launching $svc on http://127.0.0.1:$Port"
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
  Info "Launching tray $tray"
  & $tray
} else {
  $msg = if ($skipTray) { '(ARW_NO_TRAY=1)' } else { '(tray not found)' }
  Info "Launching $svc on http://127.0.0.1:$Port $msg"
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
