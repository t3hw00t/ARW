#!powershell
param(
  [switch]$DebugBuild,
  [switch]$NoTests,
  [switch]$WithLauncher,
  [switch]$Headless,
  [switch]$Help
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Info($msg){ Write-Host "[build] $msg" -ForegroundColor Cyan }
function Warn($msg){ Write-Warning $msg }
function Die($msg){ Write-Error $msg; exit 1 }

if ($Help) {
@'
Usage: scripts/build.ps1 [-DebugBuild] [-NoTests] [-WithLauncher] [-Headless]

Options:
  -DebugBuild    Build without --release (faster iterative debug profile)
  -NoTests       Skip workspace tests after building
  -WithLauncher  Opt in to building the Tauri launcher (requires platform deps)
  -Headless      Force headless build (default; skips arw-launcher package)
  -Help          Show this message
'@ | Write-Host
  exit 0
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Die 'Rust `cargo` not found in PATH. Install Rust: https://rustup.rs' }

$mode = if ($DebugBuild) { 'debug' } else { 'release' }
$includeLauncher = $false
if ($env:ARW_BUILD_LAUNCHER -match '^(1|true|yes)$') {
  $includeLauncher = $true
}
if ($WithLauncher -and $Headless) {
  Die 'Specify only one of -WithLauncher or -Headless.'
}
if ($WithLauncher) { $includeLauncher = $true }
if ($Headless) { $includeLauncher = $false }
$flavour = if ($includeLauncher) { 'full (includes arw-launcher)' } else { 'headless (skips arw-launcher)' }
Info "Building workspace ($mode, $flavour)"

$cargoArgs = @('build','--workspace','--locked')
if (-not $includeLauncher) {
  $cargoArgs += @('--exclude','arw-launcher')
}
if (-not $DebugBuild) { $cargoArgs += '--release' }
& cargo @cargoArgs

if (-not $NoTests) {
  $nextest = Get-Command cargo-nextest -ErrorAction SilentlyContinue
  if ($nextest) {
    Info 'Running tests (workspace via nextest)'
    & $nextest.Source run --workspace --locked --test-threads=1
  } else {
    Warn "cargo-nextest not found; falling back to 'cargo test --workspace --locked'."
    Warn "Install it with 'cargo install --locked cargo-nextest' for faster runs."
    & cargo test --workspace --locked -- --test-threads=1
  }
}

Info 'Done.'
