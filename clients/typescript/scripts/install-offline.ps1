[CmdletBinding()]
param(
    [switch]$Refresh
)

$ErrorActionPreference = "Stop"
$clientRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$cacheDir = Join-Path $clientRoot ".npm-cache"
$cacheTar = Join-Path $clientRoot "npm-cache.tgz"

if ($Refresh.IsPresent) {
    if (Test-Path $cacheDir) { Remove-Item -Recurse -Force $cacheDir }
    if (Test-Path $cacheTar) { Remove-Item -Force $cacheTar }
}

if (Test-Path $cacheTar) {
    tar -xzf $cacheTar -C $clientRoot
}

Push-Location $clientRoot
try {
    if (Test-Path $cacheDir) {
        npm ci --offline --prefer-offline --cache $cacheDir
    } else {
        npm ci --prefer-offline --cache $cacheDir
    }
} finally {
    Pop-Location
}

if (Test-Path $cacheDir) {
    tar -czf $cacheTar -C $clientRoot (Split-Path $cacheDir -Leaf)
}
