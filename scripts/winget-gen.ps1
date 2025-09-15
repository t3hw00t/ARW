#!powershell
[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][string]$Version,
  [Parameter(Mandatory=$true)][string]$InstallerUrl,
  [Parameter(Mandatory=$true)][string]$InstallerSha256,
  [string]$OutDir = 'out-winget'
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

New-Item -ItemType Directory -Force $OutDir | Out-Null

$id = 'ARW.Launcher'
$publisher = 'ARW'
$packageName = 'Agent Hub (ARW) Launcher'

$installer = @"
PackageIdentifier: $id
PackageVersion: $Version
InstallerType: msi
Installers:
  - Architecture: x64
    InstallerUrl: $InstallerUrl
    InstallerSha256: $InstallerSha256
    InstallerLocale: en-US
    Scope: user
"@

$defaultLocale = @"
PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: en-US
Publisher: $publisher
PackageName: $packageName
ShortDescription: Desktop launcher and tray for Agent Hub (ARW)
"@

$versionYaml = @"
PackageIdentifier: $id
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
"@

Set-Content -Path (Join-Path $OutDir 'installer.yaml') -Value $installer -Encoding UTF8
Set-Content -Path (Join-Path $OutDir 'defaultLocale.yaml') -Value $defaultLocale -Encoding UTF8
Set-Content -Path (Join-Path $OutDir 'version.yaml') -Value $versionYaml -Encoding UTF8
Write-Host "[winget-gen] Wrote manifests to $OutDir" -ForegroundColor Cyan

