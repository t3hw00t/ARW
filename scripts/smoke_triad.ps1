#!powershell
[CmdletBinding()]
param(
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$Rest
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'lib/SmokeTimeout.ps1')

function Invoke-ArwCli([string[]]$Args, [int]$TimeoutSeconds) {
  $cli = Get-Command arw-cli -ErrorAction SilentlyContinue
  if ($cli) {
    return Invoke-SmokeProcess -FilePath $cli.Source -ArgumentList $Args -TimeoutSeconds $TimeoutSeconds -Tag 'smoke-triad'
  }
  $root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
  $exe = if ($env:OS -eq 'Windows_NT') { 'arw-cli.exe' } else { 'arw-cli' }
  $candidates = @(
    Join-Path $root "target/release/$exe",
    Join-Path $root "target/debug/$exe"
  )
  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) {
      return Invoke-SmokeProcess -FilePath $candidate -ArgumentList $Args -TimeoutSeconds $TimeoutSeconds -Tag 'smoke-triad'
    }
  }
  $cargo = Get-Command cargo -ErrorAction SilentlyContinue
  if ($cargo) {
    $argList = @('run','--quiet','--release','-p','arw-cli','--') + $Args
    return Invoke-SmokeProcess -FilePath $cargo.Source -ArgumentList $argList -TimeoutSeconds $TimeoutSeconds -WorkingDirectory $root -Tag 'smoke-triad'
  }
  Write-Error 'Unable to locate arw-cli binary; install it or run cargo build -p arw-cli.'
  return 1
}

$cliArgs = @('smoke','triad')
if ($Rest) {
  $cliArgs += $Rest
}
$timeout = Get-SmokeTimeoutValue -SpecificEnvName 'SMOKE_TRIAD_TIMEOUT_SECS' -DefaultSeconds 600 -Tag 'smoke-triad'
$exit = Invoke-ArwCli $cliArgs $timeout
exit $exit
