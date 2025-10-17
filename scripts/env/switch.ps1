#!powershell
[CmdletBinding()]
param(
  [Parameter(Position = 0, Mandatory = $true)]
  [ValidateSet('linux', 'windows-host', 'windows-wsl', 'mac')]
  [string]$Mode
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

$candidates = @()
$programFiles = [Environment]::GetEnvironmentVariable('ProgramFiles')
if ($programFiles) {
  $candidates += (Join-Path $programFiles 'Git\bin\bash.exe')
}
$programFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
if ($programFilesX86) {
  $candidates += (Join-Path $programFilesX86 'Git\bin\bash.exe')
}
$bashCmd = Get-Command bash -ErrorAction SilentlyContinue
if ($bashCmd) {
  $candidates += $bashCmd.Source
}

$candidateBash = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $candidateBash) {
  throw "bash executable not found; install Git for Windows or add bash to PATH."
}

$repoUnix = & $candidateBash -lc "cygpath -u '$repoRoot'" 2>$null
if (-not $?) {
  throw "failed to translate path '$repoRoot' via cygpath (is Git Bash installed?)."
}
$repoUnix = $repoUnix.Trim()

$command = "cd '$repoUnix' && bash scripts/env/switch.sh $Mode"
& $candidateBash -lc $command
if ($LASTEXITCODE -ne 0) {
  throw "env switch failed with exit code $LASTEXITCODE"
}
