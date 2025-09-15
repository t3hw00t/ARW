#!powershell
param(
  [switch]$NoBuild,
  [string]$Target
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Info($msg){ Write-Host "[package] $msg" -ForegroundColor Cyan }
function Die($msg){ Write-Error $msg; exit 1 }

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Die 'Rust `cargo` not found in PATH. Install Rust: https://rustup.rs' }

if (-not $NoBuild) {
  if ($Target) {
    Info "Building (release) for target $Target"
    cargo build --release --locked --target $Target -p arw-svc -p arw-cli | Out-Null
    # Try launcher too; ignore failures
    try { cargo build --release --locked --target $Target -p arw-launcher | Out-Null } catch {}
  } else {
    Info 'Building workspace (release)'
    cargo build --workspace --release --locked | Out-Null
  }
}

# Workspace version from root Cargo.toml
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$rootToml = Join-Path $root 'Cargo.toml'
$version = (Get-Content -Path $rootToml | Select-String -Pattern '^version\s*=\s*"([^"]+)"' -Context 0,0 | Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) { $version = '0.0.0' }

$isWindows = $env:OS -eq 'Windows_NT'
if ($Target) {
  if ($Target -like '*-pc-windows-msvc') { $os = 'windows' }
  elseif ($Target -like '*-apple-darwin') { $os = 'macos' }
  elseif ($Target -like '*-unknown-linux-gnu') { $os = 'linux' }
  else { $os = if ($isWindows) { 'windows' } elseif ($IsMacOS) { 'macos' } else { 'linux' } }
  if ($Target -like 'aarch64-*') { $arch = 'arm64' }
  elseif ($Target -like 'x86_64-*') { $arch = 'x64' }
  else { $arch = if ($env:PROCESSOR_ARCHITECTURE -match 'ARM') { 'arm64' } else { 'x64' } }
  # Detect profile dir: prefer 'release', fallback to 'maxperf'
  $relDir = "target/$Target/release"
  $altDir = "target/$Target/maxperf"
  $binRoot = Join-Path $root $relDir
  if (-not (Test-Path $binRoot)) { $binRoot = Join-Path $root $altDir }
} else {
  $os   = if ($isWindows) { 'windows' } elseif ($IsMacOS) { 'macos' } else { 'linux' }
  $arch = if ($env:PROCESSOR_ARCHITECTURE -match 'ARM') { 'arm64' } else { 'x64' }
  # Detect profile dir: prefer 'release', fallback to 'maxperf'
  $binRoot = Join-Path $root 'target/release'
  if (-not (Test-Path $binRoot)) { $binRoot = Join-Path $root 'target/maxperf' }
}
$name = "arw-$version-$os-$arch"

$dist = Join-Path $root 'dist'
$out  = Join-Path $dist $name

New-Item -ItemType Directory -Force $dist | Out-Null
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $out
New-Item -ItemType Directory -Force $out | Out-Null

# Binaries
$binDir = Join-Path $out 'bin'
New-Item -ItemType Directory -Force $binDir | Out-Null

$exe = ''
if ($isWindows) { $exe = '.exe' }
$svcSrc = Join-Path $binRoot "arw-svc$exe"
$cliSrc = Join-Path $binRoot "arw-cli$exe"
$launcherSrc = Join-Path $binRoot "arw-launcher$exe"
if (-not (Test-Path $svcSrc)) { Die "Missing binary: $svcSrc (did the build succeed?)" }
if (-not (Test-Path $cliSrc)) { Die "Missing binary: $cliSrc (did the build succeed?)" }
Copy-Item $svcSrc -Destination (Join-Path $binDir ("arw-svc$exe"))
Copy-Item $cliSrc -Destination (Join-Path $binDir ("arw-cli$exe"))
if (Test-Path $launcherSrc) { Copy-Item $launcherSrc -Destination (Join-Path $binDir ("arw-launcher$exe")) }

# Configs
$cfgOut = Join-Path $out 'configs'
New-Item -ItemType Directory -Force $cfgOut | Out-Null
Copy-Item (Join-Path $root 'configs/default.toml') -Destination (Join-Path $cfgOut 'default.toml') -Force

# Docs
Copy-Item (Join-Path $root 'docs') -Destination (Join-Path $out 'docs') -Recurse -Force
if (Test-Path (Join-Path $root 'site')) {
  Copy-Item (Join-Path $root 'site') -Destination (Join-Path $out 'docs-site') -Recurse -Force
}

# Sandbox (Windows only)
if ($env:OS -eq 'Windows_NT' -and (Test-Path (Join-Path $root 'sandbox/ARW.wsb'))) {
  New-Item -ItemType Directory -Force (Join-Path $out 'sandbox') | Out-Null
  Copy-Item (Join-Path $root 'sandbox/ARW.wsb') -Destination (Join-Path $out 'sandbox/ARW.wsb') -Force
}

# Top-level README for the bundle
$readme = @"
ARW portable bundle ($name)

Contents
- bin/        arw-svc, arw-cli, (optional) arw-launcher
- configs/    default.toml (portable state paths)
- docs/       project docs
- sandbox/    Windows Sandbox config (Windows only)

Usage
- Run service: bin/arw-svc$exe
- Debug UI:    http://127.0.0.1:8090/debug
- CLI sanity:  bin/arw-cli$exe
- Launcher:    bin/arw-launcher$exe (tray + windows UI)

Notes
- To force portable mode: set environment variable ARW_PORTABLE=1
"@
Set-Content -Path (Join-Path $out 'README.txt') -Value $readme -Encoding utf8

# Zip
$zip = Join-Path $dist ("$name.zip")
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $out '*') -DestinationPath $zip

Info "Wrote $zip"
