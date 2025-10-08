#!powershell
[CmdletBinding(DefaultParameterSetName='Default', PositionalBinding=$false)]
param(
  [int]$Port,
  [switch]$Launcher,
  [switch]$NewToken,
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$ServerArgs
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Info($message){ Write-Host "[first-run] $message" -ForegroundColor DarkCyan }
function Write-Warn($message){ Write-Host "[first-run] $message" -ForegroundColor Yellow }
function Write-ErrorAndExit($message){ Write-Error $message; exit 1 }

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (Test-Path (Join-Path $scriptDir 'bin')) {
  $root = $scriptDir
  $binRoot = Join-Path $root 'bin'
} elseif (Test-Path (Join-Path $scriptDir '..\bin')) {
  $root = (Resolve-Path (Join-Path $scriptDir '..')).Path
  $binRoot = Join-Path $root 'bin'
} elseif (Test-Path (Join-Path $scriptDir '..\target\release')) {
  $root = (Resolve-Path (Join-Path $scriptDir '..')).Path
  $binRoot = Join-Path $root 'target\release'
} else {
  Write-ErrorAndExit 'Unable to locate portable bundle outputs. Run from the extracted release directory or ensure target\release exists.'
}

$serverExe = Join-Path $binRoot 'arw-server.exe'
if (-not (Test-Path $serverExe)) {
  Write-ErrorAndExit "arw-server binary not found at $serverExe"
}
$launcherExe = Join-Path $binRoot 'arw-launcher.exe'

function New-AdminToken {
  $bytes = New-Object byte[] 32
  [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
  return ([System.Convert]::ToHexString($bytes)).ToLowerInvariant()
}

$stateDir = Join-Path $root 'state'
if (-not (Test-Path $stateDir)) { New-Item -ItemType Directory -Path $stateDir -Force | Out-Null }
$tokenFile = Join-Path $stateDir 'admin-token.txt'

$token = $env:ARW_ADMIN_TOKEN
if ($NewToken -or [string]::IsNullOrWhiteSpace($token)) {
  if (-not $NewToken -and (Test-Path $tokenFile)) {
    try {
      $token = (Get-Content $tokenFile -Raw).Trim()
      if ($token) {
        Write-Info "Reusing saved admin token from $tokenFile"
      }
    } catch {
      $token = $null
    }
  }
  if (-not $token) {
    try {
      $token = New-AdminToken
      Set-Content -Path $tokenFile -Value $token -Encoding ascii
      Write-Info "Generated new admin token and saved it to $tokenFile"
    } catch {
      Write-ErrorAndExit "Unable to generate an admin token automatically. Set ARW_ADMIN_TOKEN manually and re-run. Details: $($_.Exception.Message)"
    }
  }
} else {
  try { Set-Content -Path $tokenFile -Value $token -Encoding ascii } catch {}
}
$env:ARW_ADMIN_TOKEN = $token

$effectivePort = 8091
if ($env:ARW_PORT) {
  try { $effectivePort = [int]$env:ARW_PORT } catch {}
}
if ($PSBoundParameters.ContainsKey('Port')) {
  $effectivePort = $Port
}
$env:ARW_PORT = "$effectivePort"
if (-not $env:ARW_BIND) { $env:ARW_BIND = '127.0.0.1' }

Write-Info "Admin token: $token"
Write-Info "Control Room: http://127.0.0.1:$effectivePort/admin/ui/control/"
Write-Info "Debug panels: http://127.0.0.1:$effectivePort/admin/debug"
Write-Info "Saved token file: $tokenFile"

if (-not $ServerArgs) { $ServerArgs = @() }

if ($Launcher) {
  if (Test-Path $launcherExe) {
    Write-Info "Starting service and launcher..."
    $svcProcess = Start-Process -FilePath $serverExe -ArgumentList $ServerArgs -NoNewWindow -PassThru
    Start-Sleep -Seconds 1
    $launcherProcess = Start-Process -FilePath $launcherExe -NoNewWindow -PassThru
    try {
      $svcProcess.WaitForExit()
    } finally {
      if ($launcherProcess -and -not $launcherProcess.HasExited) {
        try { $launcherProcess.Kill() } catch {}
      }
    }
  } else {
    Write-Warn 'Launcher binary not available; running service only.'
    & $serverExe @ServerArgs
  }
} else {
  Write-Info "Starting service only on http://127.0.0.1:$effectivePort"
  & $serverExe @ServerArgs
}
