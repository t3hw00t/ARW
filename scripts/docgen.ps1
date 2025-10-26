#!powershell
param(
  [string[]]$Args
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Prefer repository-local virtual environment Python if available
$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path (Join-Path $ScriptRoot '..')).Path

function Find-Python {
  # Candidate order: repo venv (Windows/Posix), env overrides, then PATH tools
  $winVenv = Join-Path $RepoRoot '.venv\Scripts\python.exe'
  $posixVenv = Join-Path $RepoRoot '.venv/bin/python'
  foreach ($candidate in @(
      $winVenv,
      $posixVenv,
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

$core = Join-Path $ScriptRoot 'docgen_core.py'

& $python $core @Args
exit $LASTEXITCODE
