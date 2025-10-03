#!powershell
[CmdletBinding()]
param(
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$Rest
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-ArwCli([string[]]$Args) {
  $cli = Get-Command arw-cli -ErrorAction SilentlyContinue
  if ($cli) {
    & $cli.Source @Args
    return $LASTEXITCODE
  }
  $root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
  $exe = if ($env:OS -eq 'Windows_NT') { 'arw-cli.exe' } else { 'arw-cli' }
  $candidates = @(
    Join-Path $root "target/release/$exe",
    Join-Path $root "target/debug/$exe"
  )
  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) {
      & $candidate @Args
      return $LASTEXITCODE
    }
  }
  $cargo = Get-Command cargo -ErrorAction SilentlyContinue
  if ($cargo) {
    & $cargo.Source run --quiet --release -p arw-cli -- @Args
    return $LASTEXITCODE
  }
  Write-Error 'Unable to locate arw-cli binary; install it or run cargo build -p arw-cli.'
  return 1
}

$cliArgs = @('smoke','context')
if ($Rest) {
  $cliArgs += $Rest
}
$exit = Invoke-ArwCli $cliArgs
exit $exit
