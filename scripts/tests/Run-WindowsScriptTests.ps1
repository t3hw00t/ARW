#!powershell
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

param(
  [switch]$VerboseOutput
)

Write-Host '[tests] Preparing Pester' -ForegroundColor Cyan
if (-not (Get-Module -ListAvailable -Name Pester)) {
  try { Install-Module Pester -Scope CurrentUser -Force -SkipPublisherCheck -ErrorAction Stop } catch { Write-Warning "Pester install failed: $($_.Exception.Message)" }
}
Import-Module Pester -ErrorAction SilentlyContinue

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

