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
$svc = if ($UseDist) {
  $zipBase = Get-ChildItem -Path (Join-Path $root 'dist') -Filter 'arw-*-windows-*' -Directory -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if ($zipBase) { Join-Path $zipBase.FullName (Join-Path 'bin' $exe) } else { $null }
} else { Join-Path (Join-Path $root 'target\release') $exe }

if (-not $svc -or -not (Test-Path $svc)) {
  Write-Warning "Service binary not found ($svc). Building release..."
  Push-Location $root
  cargo build --release -p arw-svc
  Pop-Location
  $svc = Join-Path (Join-Path $root 'target\release') $exe
}

Info "Launching $svc on http://127.0.0.1:$Port"
& $svc
