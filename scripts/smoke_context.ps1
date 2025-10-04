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
    return Invoke-SmokeProcess -FilePath $cli.Source -ArgumentList $Args -TimeoutSeconds $TimeoutSeconds -Tag 'smoke-context'
  }
  $root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
  $exe = if ($env:OS -eq 'Windows_NT') { 'arw-cli.exe' } else { 'arw-cli' }
  $candidates = @(
    Join-Path $root "target/release/$exe",
    Join-Path $root "target/debug/$exe"
  )
  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) {
      return Invoke-SmokeProcess -FilePath $candidate -ArgumentList $Args -TimeoutSeconds $TimeoutSeconds -Tag 'smoke-context'
    }
  }
  $cargo = Get-Command cargo -ErrorAction SilentlyContinue
  if ($cargo) {
    $argList = @('run','--quiet','--release','-p','arw-cli','--') + $Args
    return Invoke-SmokeProcess -FilePath $cargo.Source -ArgumentList $argList -TimeoutSeconds $TimeoutSeconds -WorkingDirectory $root -Tag 'smoke-context'
  }
  Write-Error 'Unable to locate arw-cli binary; install it or run cargo build -p arw-cli.'
  return 1
}

$cliArgs = @('smoke','context')
if ($Rest) {
  $cliArgs += $Rest
}
$timeout = Get-SmokeTimeoutValue -SpecificEnvName 'SMOKE_CONTEXT_TIMEOUT_SECS' -DefaultSeconds 600 -Tag 'smoke-context'
$exit = Invoke-ArwCli $cliArgs $timeout
exit $exit
