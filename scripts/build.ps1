#!powershell
param(
  [switch]$DebugBuild,
  [switch]$NoTests
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Info($msg){ Write-Host "[build] $msg" -ForegroundColor Cyan }
function Warn($msg){ Write-Warning $msg }
function Die($msg){ Write-Error $msg; exit 1 }

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Die 'Rust `cargo` not found in PATH. Install Rust: https://rustup.rs' }

$mode = if ($DebugBuild) { 'debug' } else { 'release' }
Info "Building workspace ($mode)"

$cargoArgs = @('build','--workspace','--locked')
if (-not $DebugBuild) { $cargoArgs += '--release' }
& cargo @cargoArgs

if (-not $NoTests) {
  $nextest = Get-Command cargo-nextest -ErrorAction SilentlyContinue
  if ($nextest) {
    Info 'Running tests (workspace via nextest)'
    & $nextest.Source run --workspace --locked
  } else {
    Warn "cargo-nextest not found; falling back to 'cargo test --workspace --locked'."
    Warn "Install it with 'cargo install --locked cargo-nextest' for faster runs."
    & cargo test --workspace --locked
  }
}

Info 'Done.'
