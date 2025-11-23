#!powershell
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path (Join-Path $ScriptRoot '..\\..')).Path
$EnvFile = Join-Path $RepoRoot '.arw-env'
$AllowedModes = @('linux','windows-host','windows-wsl','mac')

function Get-HostMode {
  $isWin = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)
  $isMac = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::OSX)
  $isLinux = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Linux)

  if ($isWin) {
    try {
      if (Test-Path '/proc/version' -ErrorAction Stop) {
        $content = Get-Content '/proc/version' -ErrorAction Stop -TotalCount 1
        if ($content -match 'microsoft') { return 'windows-wsl' }
      }
    } catch { }
    return 'windows-host'
  }

  if ($isMac) { return 'mac' }
  if ($isLinux) { return 'linux' }

  return 'unknown'
}

function Read-ModeFile {
  param([string]$Path)
  if (-not (Test-Path $Path)) { return $null }
  foreach ($line in Get-Content $Path) {
    if ($line -like 'MODE=*') {
      $mode = $line.Split('=')[1].Trim()
      if ($mode) { return $mode }
    }
  }
  return $null
}

$hostMode = Get-HostMode
$fileMode = Read-ModeFile -Path $EnvFile
$modeSource = if ($fileMode) { '.arw-env' } else { 'implicit' }
$mode = if ($fileMode) { $fileMode } else { $hostMode }

if (-not ($AllowedModes -contains $mode)) {
  Write-Warning ("[env] Invalid MODE `{0}`; falling back to host `{1}`." -f $mode, $hostMode)
  $mode = $hostMode
  $modeSource = 'host-fallback'
}

if ($mode -ne $hostMode) {
  Write-Warning ("[env] MODE `{0}` differs from host `{1}`. Run `bash scripts/env/switch.sh {1}` to realign." -f $mode, $hostMode)
}

$exeSuffix = if ($mode -eq 'windows-host') { '.exe' } else { '' }

Write-Host ("Mode: {0}" -f $mode)
Write-Host ("Source: {0}" -f $modeSource)
Write-Host ("Host: {0}" -f $hostMode)
Write-Host ("Repo: {0}" -f $RepoRoot)
Write-Host ("Target dir: {0}" -f (Join-Path $RepoRoot 'target'))
Write-Host ("Virtualenv: {0}" -f (Join-Path $RepoRoot '.venv'))
Write-Host ("Binary suffix: {0}" -f $exeSuffix)
