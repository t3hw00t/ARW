#requires -Version 7.0
param(
    [string] $RepoRoot = 'C:\ARW',
    [string] $StateDir = 'C:\ARW\apps\arw-server\state',
    [ValidateSet('private','shared','public')]
    [string] $PointerConsent = 'private',
    [string] $ServiceName = 'arw-server',
    [switch] $DryRun,
    [string[]] $Tasks,
    [switch] $SkipServiceStop
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Info($Message) {
    Write-Host "[arw-maintenance] $Message"
}

if (-not (Test-Path -LiteralPath $RepoRoot)) {
    throw "RepoRoot '$RepoRoot' not found"
}

$maintenance = Join-Path $RepoRoot 'scripts\maintenance.ps1'
if (-not (Test-Path -LiteralPath $maintenance)) {
    throw "maintenance.ps1 not found at $maintenance"
}

$service = $null
try {
    $service = Get-Service -Name $ServiceName -ErrorAction Stop
} catch {
    Write-Info "service '$ServiceName' not found; continuing without service control"
}

if ($service -and -not $SkipServiceStop.IsPresent) {
    if ($DryRun.IsPresent) {
        Write-Info "dry-run: would stop service $ServiceName"
    } else {
        Write-Info "stopping service $ServiceName"
        Stop-Service -Name $ServiceName -Force -ErrorAction Continue
    }
}

$args = @{
    StateDir       = $StateDir
    PointerConsent = $PointerConsent
    DryRun         = $DryRun.IsPresent
}
if ($Tasks) {
    $args['Tasks'] = $Tasks
}

Write-Info "invoking maintenance.ps1 (dryRun=$($DryRun.IsPresent))"
& $maintenance @args

if ($service -and -not $SkipServiceStop.IsPresent) {
    if ($DryRun.IsPresent) {
        Write-Info "dry-run: would start service $ServiceName"
    } else {
        Write-Info "starting service $ServiceName"
        Start-Service -Name $ServiceName -ErrorAction Continue
    }
}

Write-Info "complete"
