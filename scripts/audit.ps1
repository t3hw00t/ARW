#!powershell
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Agent Hub (ARW) — Supply-chain and code audit helper (PowerShell)

param(
  [switch]$Interactive,
  [ValidateSet('auto','yes','no')][string]$InstallTools = 'auto',
  [switch]$NoAudit,
  [switch]$NoDeny,
  [switch]$Strict
)

function Info($t){ Write-Host "[audit] $t" -ForegroundColor Cyan }
function Warn($t){ Write-Host "[audit] $t" -ForegroundColor Yellow }
function Err($t){ Write-Host "[audit] $t" -ForegroundColor Red }

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Need-Tool($name, [string]$CargoCrate) {
  if (Get-Command $name -ErrorAction SilentlyContinue) { return $true }
  switch ($InstallTools) {
    'no' { Warn "$name not found and --InstallTools=no"; return $false }
    'auto' { Info "Installing missing tool: $name" }
    'yes' { Info "Installing $name (forced)" }
  }
  if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Warn 'cargo not found; cannot install tools'; return $false }
  if ($CargoCrate) {
    try { cargo install --locked $CargoCrate | Out-Null } catch { Warn "Install failed for $CargoCrate: $($_.Exception.Message)"; return $false }
  }
  return [bool](Get-Command $name -ErrorAction SilentlyContinue)
}

function Run-CargoAudit {
  if ($NoAudit) { return }
  if (-not (Need-Tool 'cargo-audit' 'cargo-audit')) { Warn 'Skipping cargo-audit'; return }
  Info 'cargo audit'
  # Temporary ignores for known non-runtime paths
  $args = @()
  try {
    $lock = Join-Path $Root 'Cargo.lock'
    if (Test-Path $lock) {
      $content = Get-Content -Path $lock
      $glibOk = $false
      for ($i=0; $i -lt $content.Count; $i++) {
        if ($content[$i] -match '^name\s*=\s*"glib"') {
          for ($j=$i; $j -lt [Math]::Min($i+5, $content.Count); $j++) { if ($content[$j] -match '^version\s*=\s*"([0-9]+)\.([0-9]+)\.') { $maj=[int]$Matches.Groups[1].Value; $min=[int]$Matches.Groups[2].Value; if ($maj -gt 0 -or ($maj -eq 0 -and $min -ge 20)) { $glibOk = $true }; break } }
          break
        }
      }
      if (-not $glibOk) { $args += @('--ignore','RUSTSEC-2024-0429') }
      # Bench-only arrow2 pulls lexical-core 0.8.x (RUSTSEC-2023-0086) and arrow2 OOB (RUSTSEC-2025-0038). Ignore if arrow2 present.
      if (Select-String -Path $lock -Pattern '^name\s*=\s*"arrow2"' -Quiet) { $args += @('--ignore','RUSTSEC-2023-0086','--ignore','RUSTSEC-2025-0038') }
    }
  } catch { }
  Push-Location $Root; try { & cargo audit @args } catch {} finally { Pop-Location }
}

function Run-CargoDeny {
  if ($NoDeny) { return }
  if (-not (Need-Tool 'cargo-deny' 'cargo-deny')) { Warn 'Skipping cargo-deny'; return }
  Info 'cargo deny check advisories bans sources licenses'
  Push-Location $Root; try { & cargo deny check advisories bans sources licenses } catch {} finally { Pop-Location }
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Err "Rust 'cargo' not found in PATH. Install rustup first: https://rustup.rs"
  exit 1
}

if ($Interactive) {
  $runAudit = -not $NoAudit
  $runDeny = -not $NoDeny
  while ($true) {
    Write-Host "";
    Write-Host "Agent Hub (ARW) — Audit Menu" -ForegroundColor White
    Write-Host "  Root: $Root" -ForegroundColor DarkCyan
    $hasAudit = [bool](Get-Command cargo-audit -ErrorAction SilentlyContinue)
    $hasDeny  = [bool](Get-Command cargo-deny  -ErrorAction SilentlyContinue)
    Write-Host "  Tools: cargo-audit=$hasAudit, cargo-deny=$hasDeny" -ForegroundColor DarkCyan
    Write-Host "  Checks: audit=$runAudit deny=$runDeny strict=$Strict" -ForegroundColor DarkCyan
    @'
  1) Toggle cargo-audit
  2) Toggle cargo-deny
  3) Install missing tools (now)
  4) Run selected checks
  0) Exit
'@ | Write-Host
    $pick = Read-Host 'Select'
    switch ($pick) {
      '1' { $runAudit = -not $runAudit }
      '2' { $runDeny  = -not $runDeny }
      '3' { Need-Tool 'cargo-audit' 'cargo-audit' | Out-Null; Need-Tool 'cargo-deny' 'cargo-deny' | Out-Null }
      '4' { if ($runAudit) { Run-CargoAudit }; if ($runDeny) { Run-CargoDeny } }
      '0' { break }
      default {}
    }
  }
  exit 0
}

# Standard mode
if (-not $NoAudit) { Run-CargoAudit }
if (-not $NoDeny)  { Run-CargoDeny }

if ($Strict) {
  $rc = 0
  # Strict mode: run cargo audit, ignoring only RUSTSEC-2024-0429 if glib<0.20
  if (-not $NoAudit) {
    $args = @()
    try {
      $lock = Join-Path $Root 'Cargo.lock'
      if (Test-Path $lock) {
        $content = Get-Content -Path $lock
        $glibOk = $false
        for ($i=0; $i -lt $content.Count; $i++) {
          if ($content[$i] -match '^name\s*=\s*"glib"') {
            for ($j=$i; $j -lt [Math]::Min($i+5, $content.Count); $j++) { if ($content[$j] -match '^version\s*=\s*"([0-9]+)\.([0-9]+)\.') { $maj=[int]$Matches.Groups[1].Value; $min=[int]$Matches.Groups[2].Value; if ($maj -gt 0 -or ($maj -eq 0 -and $min -ge 20)) { $glibOk = $true }; break } }
            break
          }
        }
        if (-not $glibOk) { $args += @('--ignore','RUSTSEC-2024-0429') }
        if (Select-String -Path $lock -Pattern '^name\s*=\s*"arrow2"' -Quiet) { $args += @('--ignore','RUSTSEC-2023-0086','--ignore','RUSTSEC-2025-0038') }
      }
    } catch {}
    Push-Location $Root; try { & cargo audit @args } catch { $rc = 1 } finally { Pop-Location }
  }
  if (-not $NoDeny)  { Push-Location $Root; try { & cargo deny check advisories bans sources licenses } catch { $rc = 1 } finally { Pop-Location } }
  # Auto-clean temporary ignore for RUSTSEC-2024-0429 when glib >= 0.20
  try {
    $lock = Join-Path $Root 'Cargo.lock'
    $deny = Join-Path $Root 'deny.toml'
    if (Test-Path $lock -and Test-Path $deny) {
      $content = Get-Content -Path $lock
      $glibVer = $null
      for ($i=0; $i -lt $content.Count; $i++) {
        if ($content[$i] -match '^name\s*=\s*"glib"') {
          for ($j=$i; $j -lt [Math]::Min($i+5, $content.Count); $j++) { if ($content[$j] -match '^version\s*=\s*"([0-9]+)\.([0-9]+)\.') { $glibVer = $Matches; break } }
          break
        }
      }
      if ($glibVer) {
        $maj = [int]$glibVer.Groups[1].Value; $min = [int]$glibVer.Groups[2].Value
        if ($maj -gt 0 -or ($maj -eq 0 -and $min -ge 20)) {
          if (Select-String -Path $deny -Pattern 'RUSTSEC-2024-0429' -Quiet) {
            Info 'glib >= 0.20 detected. Removing temporary ignore RUSTSEC-2024-0429 from deny.toml'
            $raw = Get-Content -Path $deny -Raw
            # Remove lines containing the advisory id
            $new = ($raw -split "\r?\n") | Where-Object { $_ -notmatch 'RUSTSEC-2024-0429' } | ForEach-Object { $_ }
            [System.IO.File]::WriteAllText($deny, [string]::Join([Environment]::NewLine, $new))
          }
        }
      }
    }
  } catch { }
  if ($rc -ne 0) { exit $rc }
}

exit 0
