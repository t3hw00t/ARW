#!powershell
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Write-Error 'Rust `cargo` not found in PATH.'; exit 1 }

Write-Host '[test] Running cargo nextest (workspace)' -ForegroundColor Cyan
cargo nextest run --workspace --locked
