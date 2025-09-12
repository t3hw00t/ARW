#!powershell
<#
  Windows Advanced Gate (opt-in)
  - SignTool verification for EXE/MSI artifacts (warn by default; fail with -Strict or ARW_STRICT_SIGN_VERIFY=1)
  - MSI ICE validation via MsiVal2 (if available) using Darice.cub (skips if tools not present)
  - AppVerifier presence + suggested commands (no-op unless you enable manually)
  - WACK (App Certification Kit) presence + suggested commands
#>

param(
  [switch]$Strict
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Info($m){ Write-Host "[gate] $m" -ForegroundColor DarkCyan }
function Warn($m){ Write-Host "[gate] $m" -ForegroundColor Yellow }
function Err($m){ Write-Host "[gate] $m" -ForegroundColor Red }

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root

$strictSign = $Strict -or ($env:ARW_STRICT_SIGN_VERIFY -eq '1')

function Find-Tool($names){
  foreach ($n in $names) {
    $p = (Get-Command $n -ErrorAction SilentlyContinue | Select-Object -First 1).Path
    if ($p) { return $p }
  }
  return $null
}

# Collect candidate artifacts
$distDir = Join-Path $root 'dist'
$targetExe = Join-Path $root 'target\release\arw-svc.exe'
$targetExeArm = Join-Path $root 'target\aarch64-pc-windows-msvc\release\arw-svc.exe'
$artifacts = @()
if (Test-Path $distDir) { $artifacts += Get-ChildItem -Path $distDir -Recurse -Include *.exe,*.msi -File -ErrorAction SilentlyContinue }
if (Test-Path $targetExe) { $artifacts += Get-Item $targetExe }
if (Test-Path $targetExeArm) { $artifacts += Get-Item $targetExeArm }
$artifacts = $artifacts | Sort-Object FullName -Unique

if (-not $artifacts -or $artifacts.Count -eq 0) { Warn 'No artifacts found (dist/ or target/release). Sign/ICE checks may be skipped.' }

# 1) SignTool verification
$signtool = Find-Tool @('signtool.exe','signtool')
if ($signtool) {
  Info ("SignTool: $signtool")
  $unsigned = @()
  foreach ($f in $artifacts) {
    if ($f.Extension -in @('.exe','.msi','.dll')) {
      Write-Host ("[gate] Verify: " + $f.FullName)
      $ok = $true
      try {
        & $signtool verify /pa /q "$($f.FullName)" *> $null
        if ($LASTEXITCODE -ne 0) { $ok = $false }
      } catch { $ok = $false }
      if (-not $ok) { $unsigned += $f.FullName; Warn ("Unsigned or invalid signature: " + $f.FullName) }
    }
  }
  if ($unsigned.Count -gt 0) {
    if ($strictSign) { Err ("Signature verification failed for " + $unsigned.Count + " file(s)"); exit 1 }
    else { Warn ("Signature verification issues (non-strict). Set ARW_STRICT_SIGN_VERIFY=1 to fail.") }
  } else { Info 'SignTool verification passed for available artifacts.' }
} else {
  Warn 'signtool.exe not found; skipping signature verification.'
}

# 2) MSI ICE validation (MsiVal2)
$msival = Find-Tool @('MsiVal2.exe','msival2.exe')
if ($msival) {
  Info ("MsiVal2: $msival")
  # Try to find Darice.cub (ICE rules)
  $cube = $null
  $sdkBase = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits'
  if (Test-Path $sdkBase) {
    $cube = Get-ChildItem -Path $sdkBase -Recurse -Filter 'darice.cub' -ErrorAction SilentlyContinue | Select-Object -First 1
  }
  if (-not $cube) { Warn 'Darice.cub not found; ICE rules unavailable. Install Windows Installer SDK (Orca).' }
  $msis = @($artifacts | Where-Object { $_.Extension -eq '.msi' })
  foreach ($msi in $msis) {
    if (-not $cube) { break }
    $log = [System.IO.Path]::ChangeExtension($msi.FullName, '.msi.ice.log')
    Info ("ICE validate: " + $msi.FullName)
    try {
      & $msival "$($msi.FullName)" "$($cube.FullName)" "$log"
    } catch {
      Warn ("MsiVal2 failed: " + $_.Exception.Message)
    }
    if (Test-Path $log) {
      Write-Host ("[gate] ICE log: " + $log)
    }
  }
} else {
  Warn 'MsiVal2.exe not found; skipping ICE validation.'
}

# 3) AppVerifier presence + hints
$appverif = Find-Tool @('appverif.exe','appverifier.exe')
if ($appverif) {
  Info ("AppVerifier: $appverif")
  $svc = $targetExe
  if (Test-Path $svc) {
    Write-Host '[gate] Suggested AppVerifier commands:' -ForegroundColor DarkCyan
    Write-Host ("  `"$appverif`" -verify `"$svc`"")
    Write-Host ("  `"$appverif`" -save log.xml -xml")
    Write-Host 'After enabling, run the app briefly to collect issues.'
  } else {
    Warn 'arw-svc.exe not built; skip AppVerifier hints.'
  }
} else {
  Warn 'AppVerifier not found; install Windows SDK App Verifier to enable dynamic checks.'
}

# 4) WACK presence + hints
$appcert = Find-Tool @('appcert.exe','WindowsAppCertificationKit.exe','WindowsAppCertKit.exe')
if ($appcert) {
  Info ("WACK: $appcert")
  Write-Host '[gate] Suggested WACK command (desktop app):' -ForegroundColor DarkCyan
  Write-Host '  Run GUI to validate desktop installer/app, or use CLI with a package path when applicable.'
} else {
  Warn 'Windows App Certification Kit not found; install via Windows SDK to run certification tests.'
}

Info 'Windows Advanced Gate finished.'
exit 0
