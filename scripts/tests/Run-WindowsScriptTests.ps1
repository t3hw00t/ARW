#!powershell
param(
  [switch]$VerboseOutput
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host '[tests] Preparing Pester' -ForegroundColor Cyan
$minimumPesterVersion = [Version]'5.4.0'
$availablePester = Get-Module -ListAvailable -Name Pester | Where-Object { $_.Version -ge $minimumPesterVersion }
if (-not $availablePester) {
  Write-Host "[tests] Installing Pester >= $minimumPesterVersion (CurrentUser scope)" -ForegroundColor Cyan
  try {
    Install-Module Pester -Scope CurrentUser -Force -SkipPublisherCheck -MinimumVersion $minimumPesterVersion -ErrorAction Stop
  } catch {
    Write-Error "Failed to install required Pester version: $($_.Exception.Message)"
    exit 1
  }
  $availablePester = Get-Module -ListAvailable -Name Pester | Where-Object { $_.Version -ge $minimumPesterVersion }
  if (-not $availablePester) {
    Write-Error "Pester >= $minimumPesterVersion not available after install attempt."
    exit 1
  }
}
Import-Module Pester -MinimumVersion $minimumPesterVersion -ErrorAction Stop

$tests = Join-Path $PSScriptRoot 'WindowsScripts.Tests.ps1'
if (-not (Test-Path $tests)) { Write-Error "Missing tests file: $tests"; exit 1 }

$config = New-PesterConfiguration
$config.Run.Path = $tests
$config.Run.PassThru = $true
$config.Run.Exit = $false
$config.Output.Verbosity = if ($VerboseOutput) { 'Detailed' } else { 'Normal' }

Write-Host '[tests] Running Pester' -ForegroundColor Cyan
$result = Invoke-Pester -Configuration $config
if ($result.FailedCount -gt 0) { exit 1 } else { exit 0 }

