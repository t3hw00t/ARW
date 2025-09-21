#!powershell
Set-StrictMode -Version Latest
param(
  [switch]$Interactive,
  [int]$Port = 0,
  [string]$DocsUrl,
  [string]$AdminToken,
  [switch]$Dist,
  [switch]$NoBuild,
  [switch]$Open,
  [switch]$NoOpen,
  [switch]$NoHealth,
  [int]$HealthTimeout = 20
)

function Info($t){ Write-Host "[debug] $t" -ForegroundColor Cyan }
function Warn($t){ Write-Host "[debug] $t" -ForegroundColor Yellow }

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$PORT_DEFAULT = 8091
$portWasSpecified = $PSBoundParameters.ContainsKey('Port') -and $Port -ne 0
if (-not $portWasSpecified -and $env:ARW_PORT) {
  $parsedPort = 0
  if ([int]::TryParse($env:ARW_PORT, [ref]$parsedPort)) {
    $Port = $parsedPort
    $portWasSpecified = $true
  }
}
$openUi = $false
if ($Open) { $openUi = $true }
if ($NoOpen) { $openUi = $false }

if (-not $portWasSpecified) { $Port = $PORT_DEFAULT }

if ($Interactive) {
  Write-Host 'Agent Hub (ARW) — Debug (interactive)' -ForegroundColor White
  $ans = Read-Host "HTTP port [$Port]"; if ($ans) { $Port = [int]$ans; $portWasSpecified = $true }
  $ans = Read-Host "Docs URL (optional) [$DocsUrl]"; if ($ans -ne '') { $DocsUrl = $ans }
  if (-not $AdminToken) {
    $yn = Read-Host 'Generate admin token? (Y/n)'
    if ($yn -notmatch '^[nN]') {
      $bytes = New-Object byte[] 24; (New-Object System.Security.Cryptography.RNGCryptoServiceProvider).GetBytes($bytes)
      $AdminToken = [Convert]::ToBase64String($bytes).Replace('=','').Replace('+','').Replace('/','').Substring(0,32)
      Info "Token: $AdminToken"
    }
  }
  $yn = Read-Host 'Use dist/ if available? (y/N)'; if ($yn -match '^[yY]') { $Dist = $true } else { $Dist = $false }
  $yn = Read-Host 'Open admin debug UI after start? (y/N)'; if ($yn -match '^[yY]') { $openUi = $true } else { $openUi = $false }
}

if (-not $portWasSpecified) { $Port = $PORT_DEFAULT }

$env:ARW_DEBUG = '1'
$env:ARW_PORT = "$Port"
if ($DocsUrl) { $env:ARW_DOCS_URL = $DocsUrl }
if ($AdminToken) { $env:ARW_ADMIN_TOKEN = $AdminToken }

$argsList = @('--port', "$Port", '--debug')
if ($DocsUrl) { $argsList += @('--docs-url', $DocsUrl) }
if ($AdminToken) { $argsList += @('--admin-token', $AdminToken) }
if ($Dist) { $argsList += '--dist' }
if ($NoBuild) { $argsList += '--no-build' }
if (-not $NoHealth) { $argsList += @('--wait-health', '--wait-health-timeout-secs', "$HealthTimeout") }

& (Join-Path $PSScriptRoot 'start.ps1') @argsList

if ($openUi) {
  $base = "http://127.0.0.1:$Port/admin/debug"
  try { Start-Process $base | Out-Null } catch {}
}
