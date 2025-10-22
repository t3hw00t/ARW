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
$baseUrl = $env:SMOKE_CONTEXT_BASE_URL
if ([string]::IsNullOrWhiteSpace($baseUrl)) {
  $baseUrl = $env:CONTEXT_SMOKE_BASE_URL
}
if ([string]::IsNullOrWhiteSpace($baseUrl)) {
  $baseUrl = $env:TRIAD_SMOKE_BASE_URL
}
if (-not [string]::IsNullOrWhiteSpace($baseUrl)) {
  $baseArgPresent = $false
  if ($Rest) {
    foreach ($arg in $Rest) {
      if ($arg -eq '--base-url' -or $arg -like '--base-url=*') {
        $baseArgPresent = $true
        break
      }
    }
  }
  if (-not $baseArgPresent) {
    $cliArgs += @('--base-url', $baseUrl)
  }
}

$persona = $env:SMOKE_CONTEXT_PERSONA
if ([string]::IsNullOrWhiteSpace($persona)) {
  $persona = $env:CONTEXT_SMOKE_PERSONA
}
if ([string]::IsNullOrWhiteSpace($persona)) {
  $persona = $env:ARW_PERSONA_ID
}
$personaArgPresent = $false
if ($Rest) {
  foreach ($arg in $Rest) {
    if ($arg -eq '--persona-id' -or $arg -like '--persona-id=*') {
      $personaArgPresent = $true
      break
    }
  }
}
if (-not $personaArgPresent -and -not [string]::IsNullOrWhiteSpace($persona)) {
  $cliArgs += @('--persona-id', $persona)
}

if ($Rest) {
  $cliArgs += $Rest
}
$timeout = Get-SmokeTimeoutValue -SpecificEnvName 'SMOKE_CONTEXT_TIMEOUT_SECS' -DefaultSeconds 600 -Tag 'smoke-context'
$exit = Invoke-ArwCli $cliArgs $timeout
exit $exit
