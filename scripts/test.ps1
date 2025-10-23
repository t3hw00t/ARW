#!powershell
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Write-Error 'Rust `cargo` not found in PATH.'; exit 1 }

$nextest = Get-Command cargo-nextest -ErrorAction SilentlyContinue
if ($nextest) {
  Write-Host '[test] Running cargo nextest (workspace)' -ForegroundColor Cyan
  & $nextest.Source run --workspace --locked --test-threads=1
} else {
  Write-Warning "cargo-nextest not found; falling back to 'cargo test --workspace --locked'."
  Write-Warning "Install it with 'cargo install --locked cargo-nextest' for faster runs."
  cargo test --workspace --locked -- --test-threads=1
}
