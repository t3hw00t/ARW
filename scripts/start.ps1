#!powershell
param(
  [int]$Port = 8090,
  [switch]$Debug,
  [string]$DocsUrl,
  [string]$AdminToken,
  [int]$TimeoutSecs = 20,
  [switch]$UseDist
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
  Write-Warning "Service binary not found ($svc). Building release..."
  Push-Location $root
  cargo build --release -p arw-svc
  Pop-Location
  $svc = Join-Path (Join-Path $root 'target\release') $exe
}

if (-not $tray -or -not (Test-Path $tray)) {
  Write-Warning "Tray binary not found ($tray). Attempting build..."
  try {
    Push-Location $root
    cargo build --release -p arw-tray
    Pop-Location
  } catch {
    Pop-Location
  }
  $tray = Join-Path (Join-Path $root 'target\release') $trayExe
}

# Respect ARW_NO_TRAY=1 for CLI-only environments
$skipTray = $false
if ($env:ARW_NO_TRAY -and $env:ARW_NO_TRAY -eq '1') { $skipTray = $true }

if (-not $skipTray -and (Test-Path $tray)) {
  Info "Launching $svc on http://127.0.0.1:$Port"
  if ($env:ARW_LOG_FILE) {
    try { New-Item -ItemType Directory -Force ([System.IO.Path]::GetDirectoryName($env:ARW_LOG_FILE)) | Out-Null } catch {}
    $p = Start-Process -FilePath $svc -RedirectStandardOutput $env:ARW_LOG_FILE -RedirectStandardError $env:ARW_LOG_FILE -PassThru
  } else {
    $p = Start-Process -FilePath $svc -PassThru
  }
  if ($env:ARW_PID_FILE) {
    try { New-Item -ItemType Directory -Force ([System.IO.Path]::GetDirectoryName($env:ARW_PID_FILE)) | Out-Null } catch {}
    try { $p.Id | Out-File -FilePath $env:ARW_PID_FILE -Encoding ascii -Force } catch {}
  }
  Info "Launching tray $tray"
  & $tray
} else {
  $msg = if ($skipTray) { '(ARW_NO_TRAY=1)' } else { '(tray not found)' }
  Info "Launching $svc on http://127.0.0.1:$Port $msg"
  if ($env:ARW_PID_FILE) {
    if ($env:ARW_LOG_FILE) {
      try { New-Item -ItemType Directory -Force ([System.IO.Path]::GetDirectoryName($env:ARW_LOG_FILE)) | Out-Null } catch {}
      $p = Start-Process -FilePath $svc -RedirectStandardOutput $env:ARW_LOG_FILE -RedirectStandardError $env:ARW_LOG_FILE -PassThru
    } else {
      $p = Start-Process -FilePath $svc -PassThru
    }
    try { New-Item -ItemType Directory -Force ([System.IO.Path]::GetDirectoryName($env:ARW_PID_FILE)) | Out-Null } catch {}
    try { $p.Id | Out-File -FilePath $env:ARW_PID_FILE -Encoding ascii -Force } catch {}
  } else {
    if ($env:ARW_LOG_FILE) {
      & $svc *> $env:ARW_LOG_FILE
    } else {
      & $svc
    }
  }
}
