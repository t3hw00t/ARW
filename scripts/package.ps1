#!powershell
param(
  [switch]$NoBuild
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Info($msg){ Write-Host "[package] $msg" -ForegroundColor Cyan }
function Die($msg){ Write-Error $msg; exit 1 }

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Die 'Rust `cargo` not found in PATH. Install Rust: https://rustup.rs' }

if (-not $NoBuild) {
  Info 'Building workspace (release)'
  cargo build --workspace --release --locked
}

# Workspace version from root Cargo.toml
$rootToml = Join-Path $PSScriptRoot '..' 'Cargo.toml' | Resolve-Path
$version = (Get-Content $rootToml | Select-String -Pattern '^version\s*=\s*"([^"]+)"' -Context 0,0 | Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) { $version = '0.0.0' }

$os   = if ($IsWindows) { 'windows' } elseif ($IsMacOS) { 'macos' } else { 'linux' }
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -match 'ARM') { $arch = 'arm64' } else { $arch = 'x64' }
$name = "arw-$version-$os-$arch"

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$dist = Join-Path $root 'dist'
$out  = Join-Path $dist $name

New-Item -ItemType Directory -Force $dist | Out-Null
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $out
New-Item -ItemType Directory -Force $out | Out-Null

# Binaries
$binDir = Join-Path $out 'bin'
New-Item -ItemType Directory -Force $binDir | Out-Null

$exe = $IsWindows ? '.exe' : ''
$svcSrc = Join-Path $root "target/release/arw-svc$exe"
$cliSrc = Join-Path $root "target/release/arw-cli$exe"
if (-not (Test-Path $svcSrc)) { Die "Missing binary: $svcSrc (did the build succeed?)" }
if (-not (Test-Path $cliSrc)) { Die "Missing binary: $cliSrc (did the build succeed?)" }
Copy-Item $svcSrc -Destination (Join-Path $binDir ("arw-svc$exe"))
Copy-Item $cliSrc -Destination (Join-Path $binDir ("arw-cli$exe"))

# Configs
$cfgOut = Join-Path $out 'configs'
New-Item -ItemType Directory -Force $cfgOut | Out-Null
Copy-Item (Join-Path $root 'configs/default.toml') -Destination (Join-Path $cfgOut 'default.toml') -Force

# Docs
Copy-Item (Join-Path $root 'docs') -Destination (Join-Path $out 'docs') -Recurse -Force

# Sandbox (Windows only)
if ($IsWindows -and (Test-Path (Join-Path $root 'sandbox/ARW.wsb'))) {
  New-Item -ItemType Directory -Force (Join-Path $out 'sandbox') | Out-Null
  Copy-Item (Join-Path $root 'sandbox/ARW.wsb') -Destination (Join-Path $out 'sandbox/ARW.wsb') -Force
}

# Top-level README for the bundle
$readme = @"
ARW portable bundle ($name)

Contents
- bin/        arw-svc, arw-cli
- configs/    default.toml (portable state paths)
- docs/       project docs
- sandbox/    Windows Sandbox config (Windows only)

Usage
- Run service: bin/arw-svc$exe
- Debug UI:    http://127.0.0.1:8090/debug
- CLI sanity:  bin/arw-cli$exe

Notes
- To force portable mode: set environment variable ARW_PORTABLE=1
"@
Set-Content -Path (Join-Path $out 'README.txt') -Value $readme -Encoding utf8

# Zip
$zip = Join-Path $dist ("$name.zip")
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $out '*') -DestinationPath $zip

Info "Wrote $zip"
