#!powershell
param(
  [string[]]$Args
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Find-Python {
  foreach ($candidate in @(
      $env:PYTHON,
      $env:PYTHON3,
      (Get-Command python3 -ErrorAction SilentlyContinue),
      (Get-Command python -ErrorAction SilentlyContinue)
    )) {
    if ($null -eq $candidate) { continue }
    if ($candidate -is [System.Management.Automation.CommandInfo]) {
      return $candidate.Source
    }
    if ($candidate -is [string] -and $candidate.Trim() -ne '' -and (Test-Path $candidate)) {
      return (Resolve-Path $candidate).Path
    }
  }
  return $null
}

$python = Find-Python
if (-not $python) {
  Write-Warning '[docgen] python not found; skipping doc generation.'
  exit 0
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$core = Join-Path $scriptRoot 'docgen_core.py'

& $python $core @Args
exit $LASTEXITCODE
