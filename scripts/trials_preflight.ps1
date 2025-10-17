#!powershell
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Log {
  param([string]$Message)
  Write-Host "-> $Message"
}

function Warn {
  param([string]$Message)
  Write-Warning $Message
}

function Invoke-JustTarget {
  param(
    [string]$Target,
    [string]$Label
  )
  $just = Get-Command just -ErrorAction SilentlyContinue
  if (-not $just) {
    Warn "Skipping $Label: 'just' not found."
    return $false
  }
  & $just.Source --show $Target *> $null
  if ($LASTEXITCODE -ne 0) {
    Warn "Skipping $Label: just target '$Target' not defined."
    return $false
  }
  Log "Running $Label ($Target)"
  & $just.Source $Target
  if ($LASTEXITCODE -ne 0) {
    throw "just $Target failed with exit code $LASTEXITCODE"
  }
  return $true
}

function Invoke-ArwCli {
  param(
    [string[]]$Args,
    [string]$Label,
    [string]$Root
  )
  $cliPath = $null
  $cli = Get-Command arw-cli -ErrorAction SilentlyContinue
  if ($cli) {
    $cliPath = $cli.Source
  }
  if (-not $cliPath) {
    $exe = if ($env:OS -eq 'Windows_NT') { 'arw-cli.exe' } else { 'arw-cli' }
    $candidates = @(
      (Join-Path $Root 'target' 'release' $exe),
      (Join-Path $Root 'target' 'debug' $exe)
    )
    foreach ($candidate in $candidates) {
      if (Test-Path $candidate) {
        $cliPath = $candidate
        break
      }
    }
  }
  if (-not $cliPath) {
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargo) {
      Warn "Skipping $Label: arw-cli not found and cargo unavailable."
      return $false
    }
    Log "Building arw-cli (missing binary)"
    Push-Location $Root
    try {
      & $cargo.Source build -p arw-cli *> $null 2>&1
    } finally {
      Pop-Location
    }
    if ($LASTEXITCODE -ne 0) {
      Warn "cargo build -p arw-cli failed (exit $LASTEXITCODE)"
      return $false
    }
    $cli = Get-Command arw-cli -ErrorAction SilentlyContinue
    if ($cli) {
      $cliPath = $cli.Source
    } else {
      $exe = if ($env:OS -eq 'Windows_NT') { 'arw-cli.exe' } else { 'arw-cli' }
      $releasePath = Join-Path $Root 'target' 'release' $exe
      $debugPath = Join-Path $Root 'target' 'debug' $exe
      if (Test-Path $releasePath) { $cliPath = $releasePath }
      elseif (Test-Path $debugPath) { $cliPath = $debugPath }
    }
  }
  if (-not $cliPath) {
    Warn "Skipping $Label: arw-cli still unavailable after build."
    return $false
  }
  Log "Running $Label (arw-cli)"
  & $cliPath @Args
  if ($LASTEXITCODE -ne 0) {
    throw "$Label failed with exit code $LASTEXITCODE"
  }
  return $true
}

function Invoke-BashScript {
  param(
    [string]$Script,
    [string]$Label
  )
  $bash = Get-Command bash -ErrorAction SilentlyContinue
  if (-not $bash) {
    Warn "Skipping $Label: bash not available."
    return $false
  }
  if (-not (Test-Path $Script)) {
    Warn "Skipping $Label: script '$Script' not found."
    return $false
  }
  Log "Running $Label (bash)"
  & $bash.Source $Script
  if ($LASTEXITCODE -ne 0) {
    throw "$Label failed with exit code $LASTEXITCODE"
  }
  return $true
}

function Run-TriadPreflight {
  param([string]$Root)
  if (Invoke-JustTarget -Target 'triad-smoke' -Label 'kernel triad smoke check') {
    return
  }
  if (Invoke-ArwCli -Args @('smoke','triad','--wait-timeout-secs','45') -Label 'kernel triad smoke check' -Root $Root) {
    return
  }
  $triadSh = Join-Path $Root 'scripts' 'triad_smoke.sh'
  if (Invoke-BashScript -Script $triadSh -Label 'kernel triad smoke check') {
    return
  }
  throw 'Triad smoke helpers unavailable (install `just`, build arw-cli, or provide bash)'
}

function Run-ContextPreflight {
  param([string]$Root)
  if (Invoke-JustTarget -Target 'context-ci' -Label 'context telemetry checks') {
    return
  }
  if (Invoke-ArwCli -Args @('smoke','context','--wait-timeout-secs','45') -Label 'context telemetry checks' -Root $Root) {
    return
  }
    $python = Get-Command python3 -ErrorAction SilentlyContinue
    if (-not $python) {
      $python = Get-Command python -ErrorAction SilentlyContinue
    }
    if ($python) {
      Log 'context telemetry checks (python)'
      & $python.Source (Join-Path $Root 'scripts' 'context_ci.py')
      if ($LASTEXITCODE -eq 0) { return }
    }
    throw 'Context telemetry helpers unavailable (install `just`, build arw-cli, or provide Python 3)'
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
try {
  Log 'Trial preflight starting'

  Run-TriadPreflight -Root $root
  Log 'Triad endpoints verified (actions/state/events)'
  Run-ContextPreflight -Root $root
  Log 'Context telemetry checks'

  $legacyPs = Join-Path $root 'scripts' 'check_legacy_surface.ps1'
  $legacySh = Join-Path $root 'scripts' 'check_legacy_surface.sh'
  if (Test-Path $legacyPs) {
    Log 'Running legacy surface check (PowerShell)'
    & $legacyPs
    if ($LASTEXITCODE -ne 0) {
      Warn "Legacy surface check reported issues (exit $LASTEXITCODE)"
    }
  } elseif (Test-Path $legacySh) {
    if (Get-Command bash -ErrorAction SilentlyContinue) {
      Log 'Running legacy surface check (bash)'
      & bash $legacySh
      if ($LASTEXITCODE -ne 0) {
        Warn "Legacy surface check reported issues (exit $LASTEXITCODE)"
      }
    } else {
      Warn "Skipping legacy surface check: bash not available"
    }
  } else {
    Warn 'Skipping legacy surface check: script missing'
  }

  Log 'Trial preflight complete'
}
finally {
  Pop-Location
}
