#!powershell
param(
  [switch]$Headless,
  [switch]$Package,
  [switch]$NoDocs,
  [switch]$DryRun
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
## Compatibility: PowerShell 5 vs 7 Invoke-WebRequest
$script:IwrArgs = @{}
try { if ($PSVersionTable.PSVersion.Major -lt 6) { $script:IwrArgs = @{ UseBasicParsing = $true } } } catch {}

function Banner($title, $subtitle){
  $cols = [Console]::WindowWidth
  if (-not $cols -or $cols -lt 40) { $cols = 80 }
  $line = ''.PadLeft($cols, '━')
  Write-Host "`n$line" -ForegroundColor DarkCyan
  Write-Host " $title" -ForegroundColor White
  if ($subtitle) { Write-Host " $subtitle" -ForegroundColor DarkCyan }
  Write-Host $line -ForegroundColor DarkCyan
}
function Section($t){ Write-Host "> $t" -ForegroundColor Magenta }
function Info($t){ Write-Host "[info] $t" -ForegroundColor DarkCyan }
function Warn($t){ Write-Host "[warn] $t" -ForegroundColor Yellow }

function New-AdminToken {
  try {
    $bytes = New-Object byte[] 16
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    return ([System.BitConverter]::ToString($bytes) -replace '-').ToLower()
  } catch {
    return ([Guid]::NewGuid().ToString('N'))
  }
}

function Show-GeneratedToken {
  param(
    [string]$Token,
    [string]$Label = 'admin token'
  )
  $dir = Join-Path $root '.arw'
  New-Item -ItemType Directory -Force $dir | Out-Null
  if (-not [Console]::IsOutputRedirected) {
    Info ("Generated $Label: $Token")
    Warn 'Store this value securely; remove it from scrollback if copied.'
  } else {
    $fileLabel = ($Label.ToLower() -replace '[^a-z0-9._-]', '_')
    $path = Join-Path $dir ("last_$fileLabel.txt")
    if (Test-Path $path) { Remove-Item -Force $path }
    $Token | Set-Content -Path $path -Encoding utf8
    Info ("Generated $Label stored at $path")
    Warn 'Delete this file after saving the value securely.'
  }
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$localBin = Join-Path $root '.arw\bin'
New-Item -ItemType Directory -Force $localBin | Out-Null
if (-not ($env:Path -split ';' | Where-Object { $_ -eq $localBin })) { $env:Path = "$localBin;" + $env:Path }
# Add project-local Rust cargo bin to PATH if present
$localRustCargoBin = Join-Path $root '.arw\rust\cargo\bin'
New-Item -ItemType Directory -Force $localRustCargoBin | Out-Null
if (-not ($env:Path -split ';' | Where-Object { $_ -eq $localRustCargoBin })) { $env:Path = "$localRustCargoBin;" + $env:Path }

# Load saved preferences if present
$envFile = Join-Path $root '.arw\env.ps1'
if (Test-Path $envFile) { . $envFile }

# Setup dry-run mode (session-wide for setup)
$script:SetupDryRun = $false
if ($DryRun) { $script:SetupDryRun = $true }

# Install log helper
function Write-InstallLogDir($rel){
  $log = Join-Path $root '.install.log'
  if (-not (Test-Path $log)) { "# Install log - $(Get-Date -Format o)" | Set-Content -Path $log -Encoding utf8 }
  $entry = "DIR $rel"
  $exists = Get-Content $log | Where-Object { $_ -eq $entry } | Select-Object -First 1
  if (-not $exists) { Add-Content -Path $log -Value $entry }
}
Write-InstallLogDir '.arw\bin'
Write-InstallLogDir '.arw\rust'

Banner 'Agent Hub (ARW) — Interactive Setup (Windows)' 'Portable, local-first agent workspace'
Section 'Project'
Write-Host '  Agent Hub (ARW) — local-first Rust workspace for personal AI agents.'
Write-Host '  Highlights: user-mode HTTP service + debug UI; macro-driven tools; event stream; portable packaging.'

Section 'Host / Hardware'
$os = Get-CimInstance Win32_OperatingSystem
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$gpu = Get-CimInstance Win32_VideoController | Select-Object -First 1
$disk = Get-PSDrive -PSProvider FileSystem | Where-Object { $_.Name -eq (Get-Location).Path.Substring(0,1) }
Write-Host "  • OS:        $($os.Caption) ($($os.Version))"
Write-Host "  • CPU:       $($cpu.Name) ($($cpu.NumberOfLogicalProcessors) cores)"
Write-Host "  • Memory:    $([Math]::Round($os.TotalVisibleMemorySize/1MB,1)) GB"
if ($disk) { Write-Host "  • Disk:      $([Math]::Round($disk.Free/1GB,1)) GB free" }
if ($gpu) { Write-Host "  • GPU:       $($gpu.Name)" }

$Port = $env:ARW_PORT; if (-not $Port) { $Port = 8091 }
$DocsUrl = $env:ARW_DOCS_URL
$AdminToken = $env:ARW_ADMIN_TOKEN
$RunTests = $false
$BuildDocs = $true
$DoPackage = $false

# Headless flags
if ($Headless) { }
if ($Package) { $DoPackage = $true }
if ($NoDocs) { $BuildDocs = $false }

function Ensure-Prereqs {
  Section 'Prerequisites'
  $hasCargo = Get-Command cargo -ErrorAction SilentlyContinue
  if ($hasCargo) { Info (cargo --version) } else { Warn "Rust 'cargo' not found. Install via https://rustup.rs" }
  $py = Get-Command python -ErrorAction SilentlyContinue
  if (-not $py) { $py = Get-Command python3 -ErrorAction SilentlyContinue }
  if ($py) { Info "python: $($py.Path)" } else { Warn 'python not found (docs optional)' }
  $mk = Get-Command mkdocs -ErrorAction SilentlyContinue
  if ($mk) { Info (mkdocs --version) } else { Warn 'mkdocs not found (docs optional)' }
  Read-Host 'Press Enter to continue…' | Out-Null
}

function Install-MkDocs {
  Section 'Install MkDocs (optional)'
  $py = Get-Command python -ErrorAction SilentlyContinue
  if (-not $py) { $py = Get-Command python3 -ErrorAction SilentlyContinue }
  if (-not $py) { Warn 'python not found'; return }
  $venv = Join-Path $root '.venv'
  if ($script:SetupDryRun) {
    Info "[dryrun] Would create venv at $venv and install mkdocs + plugins"
  } else {
    & $py.Path -m venv $venv | Out-Null
    & "$venv\Scripts\python.exe" -m pip install --upgrade pip | Out-Null
    & "$venv\Scripts\python.exe" -m pip install mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin | Out-Null
    if (Test-Path "$venv\Scripts\mkdocs.exe") { $env:Path = "$($venv)\Scripts;" + $env:Path; Info 'MkDocs installed in local venv' } else { Warn 'MkDocs install may have failed' }
    Write-InstallLogDir '.venv'
  }
}

function Configure-Settings {
  Section 'Configure Settings'
  $ans = Read-Host "HTTP port [$Port]"; if ($ans) { $Port = [int]$ans }
  $ans = Read-Host "Docs URL (optional) [$DocsUrl]"; if ($ans -ne '') { $DocsUrl = $ans }
  if (-not $AdminToken) {
    $ans = Read-Host 'Generate admin token now? (Y/n)'
    if ($ans -notmatch '^[nN]') {
      $AdminToken = New-AdminToken
      Show-GeneratedToken -Token $AdminToken -Label 'admin token'
      Warn 'Store this token securely (password manager or encrypted secret).'
    }
  }
  $displayToken = if ($AdminToken) { $AdminToken } else { 'auto' }
  $ans = Read-Host "Admin token [$displayToken]"; if ($ans -ne '') { $AdminToken = $ans }
  $ans = Read-Host 'Run tests after build? (y/N)'; $RunTests = ($ans -match '^[yY]')
  $ans = Read-Host 'Build docs site with MkDocs? (Y/n)'; $BuildDocs = -not ($ans -match '^[nN]')
  $ans = Read-Host 'Create portable bundle in dist/? (y/N)'; $DoPackage = ($ans -match '^[yY]')
}

function Configure-Cluster {
  Section 'Clustering (optional)'
  $ans = Read-Host 'Enable cluster with NATS? (y/N)'
  if ($ans -match '^[yY]') {
    $url = Read-Host 'NATS URL [nats://127.0.0.1:4222]'; if (-not $url) { $url = 'nats://127.0.0.1:4222' }
    $node = Read-Host ("Node ID [$env:COMPUTERNAME]"); if (-not $node) { $node = $env:COMPUTERNAME }
    New-Item -ItemType Directory -Force (Join-Path $root 'configs') | Out-Null
    @"
[runtime]
portable = true

[cluster]
enabled = true
bus = "nats"
queue = "nats"
nats_url = "$url"
node_id = "$node"
"@ | Set-Content -Path (Join-Path $root 'configs\local.toml') -Encoding utf8
    Info 'Wrote configs/local.toml (set ARW_CONFIG=configs/local.toml when starting)'
  } else {
    Info 'Keeping single-node defaults (configs/default.toml)'
  }
}

function Do-Build {
  Section 'Build (release)'
  if ($script:SetupDryRun) {
    Info '[dryrun] Would run: cargo build --workspace --release'
    if ($RunTests) { Info '[dryrun] Would run: cargo nextest run --workspace' }
  } else {
    Push-Location $root
    cargo build --workspace --release
    if ($RunTests) {
      try { cargo nextest --version *> $null; if ($LASTEXITCODE -eq 0) { cargo nextest run --workspace } else { cargo test --workspace } }
      catch { cargo test --workspace }
    }
    Pop-Location
  }
}

function Do-Docs {
  if ($BuildDocs) {
    Section 'Docs generation'
    $venvMk = Join-Path $root '.venv\Scripts'
    if (Test-Path (Join-Path $venvMk 'mkdocs.exe')) { $env:Path = "$venvMk;" + $env:Path }
    if ($script:SetupDryRun) {
      Info '[dryrun] Would run: scripts/docgen.ps1'
    } else {
      & (Join-Path $PSScriptRoot 'docgen.ps1')
    }
  }
}

# Dependencies helpers
function Install-Rustup {
  if (Get-Command cargo -ErrorAction SilentlyContinue) { Info 'Rust already installed'; return }
  Section 'Install Rust (rustup)'
  $rustup = Join-Path $localBin 'rustup-init.exe'
  if ($script:SetupDryRun) {
    Info ("[dryrun] Would download rustup-init.exe to " + $rustup)
    Info '[dryrun] Would set RUSTUP_HOME/CARGO_HOME under .arw\rust and install toolchain'
  } else {
  try {
    Invoke-WebRequest @IwrArgs 'https://win.rustup.rs/' -OutFile $rustup
    $env:RUSTUP_HOME = (Join-Path $root '.arw\rust\rustup')
    $env:CARGO_HOME  = (Join-Path $root '.arw\rust\cargo')
    & $rustup -y --default-toolchain stable --no-modify-path | Out-Null
    if (-not ($env:Path -split ';' | Where-Object { $_ -eq $localRustCargoBin })) { $env:Path = "$localRustCargoBin;" + $env:Path }
    Info 'Rust installed (restart shell if cargo not found)'
  } catch { Warn 'Rustup install failed' }
  }
}

function Install-Python {
  if (Get-Command python -ErrorAction SilentlyContinue) { Info 'Python already installed'; return }
  if (Get-Command winget -ErrorAction SilentlyContinue) {
    Banner 'Install Python via winget' ''
    winget install -e --id Python.Python.3 --silent
  } elseif (Get-Command choco -ErrorAction SilentlyContinue) {
    Banner 'Install Python via Chocolatey' ''
    choco install -y python
  } else {
    Warn 'No package manager found (winget/choco). Install Python from https://www.python.org/downloads/'
  }
}

function Install-Jq {
  if (Get-Command jq -ErrorAction SilentlyContinue) { Info 'jq already installed'; return }
  if (Get-Command winget -ErrorAction SilentlyContinue) { winget install -e --id jqlang.jq --silent; return }
  $jq = Join-Path $localBin 'jq.exe'
  if ($script:SetupDryRun) {
    Info ("[dryrun] Would download jq.exe to " + $jq)
  } else {
  try {
    Invoke-WebRequest @IwrArgs 'https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-windows-amd64.exe' -OutFile $jq
    Info "Downloaded jq to $jq"
    try { $h = Get-FileHash -Path $jq -Algorithm SHA256; Info ("jq sha256: " + $h.Hash) } catch {}
  } catch { Warn 'jq download failed' }
  }
}

function Install-Nextest { if (Get-Command cargo -ErrorAction SilentlyContinue) { cargo install cargo-nextest } else { Warn 'cargo not found' } }

function Install-NatsLocal {
  $ver = '2.10.19'
  $os = 'windows'
  $arch = if ($env:PROCESSOR_ARCHITECTURE -match 'ARM') { 'arm64' } else { 'amd64' }
  $asset = "nats-server-v$ver-$os-$arch.zip"
  $url = "https://github.com/nats-io/nats-server/releases/download/v$ver/$asset"
  $dir = Join-Path $root '.arw\nats'
  New-Item -ItemType Directory -Force (Join-Path $dir 'tmp') | Out-Null
  $zip = Join-Path (Join-Path $dir 'tmp') $asset
  try {
    Invoke-WebRequest @IwrArgs $url -OutFile $zip
  } catch {
    Warn "Download failed: $url"; Write-Host "Fallback: docker run -p 4222:4222 nats:latest"; return
  }
  try {
    Expand-Archive -Path $zip -DestinationPath (Join-Path $dir 'tmp') -Force
    $exe = Get-ChildItem -Path (Join-Path $dir 'tmp') -Recurse -Filter 'nats-server.exe' | Select-Object -First 1
    if ($exe) { Copy-Item $exe.FullName -Destination (Join-Path $dir 'nats-server.exe') -Force; Info "Installed nats-server to $dir" } else { Warn 'nats-server.exe not found in archive' }
  } catch { Warn 'Extraction failed' }
}

function Dependencies-Menu {
  while ($true) {
    Banner 'Dependencies' 'Install/fix common requirements'
    Write-Host @'
  1) Install Rust toolchain (rustup)
  2) Install Python (winget/choco)
  3) Install MkDocs into local venv (.venv)
  4) Install jq (winget or local)
  5) Install cargo-nextest (tests)
  6) Install local NATS (no admin)
  7) Configure HTTP(S) proxy
  8) NATS server (instructions)
  9) Install WSL (requires admin)
  0) Back
'@
    $pick = Read-Host 'Select'
    switch ($pick) {
      '1' { Install-Rustup }
      '2' { Install-Python }
      '3' { Install-MkDocs }
      '4' { Install-Jq }
      '5' { Install-Nextest }
      '6' { Install-NatsLocal }
      '7' { Configure-Proxies }
      '8' { Write-Host "NATS options: winget/choco (if available), WSL: install or run nats-server in your Linux distro, Docker (fallback): docker run -p 4222:4222 nats:latest, Manual: https://github.com/nats-io/nats-server/releases"; Read-Host 'Continue' | Out-Null }
      '9' { Install-WSL-Elevated }
      '0' { break }
      default { }
    }
  }
}

function Install-WSL-Elevated {
  Section 'Install WSL (admin)'
  try {
    $cmd = "wsl --install -d Ubuntu; Write-Host 'If prompted, restart your PC to complete installation.'; Read-Host 'Press Enter to close'"
    Start-Process -FilePath "powershell.exe" -Verb RunAs -ArgumentList "-NoExit","-Command",$cmd | Out-Null
    Info 'Opened elevated PowerShell to run: wsl --install -d Ubuntu'
  } catch {
    Warn 'Unable to launch elevated PowerShell. Please run as Administrator: wsl --install -d Ubuntu'
  }
}

function Configure-Proxies {
  Section 'Configure HTTP(S) proxy'
  $hp0 = if ($env:HTTP_PROXY) { $env:HTTP_PROXY } else { '' }
  $sp0 = if ($env:HTTPS_PROXY) { $env:HTTPS_PROXY } else { '' }
  $np0 = if ($env:NO_PROXY) { $env:NO_PROXY } else { '' }
  $hp = Read-Host ("HTTP_PROXY [" + $hp0 + "]"); if (-not $hp) { $hp = $env:HTTP_PROXY }
  $sp = Read-Host ("HTTPS_PROXY [" + $sp0 + "]"); if (-not $sp) { $sp = $env:HTTPS_PROXY }
  $np = Read-Host ("NO_PROXY [" + $np0 + "]"); if (-not $np) { $np = $env:NO_PROXY }
  if ($hp) { $env:HTTP_PROXY = $hp; $env:http_proxy = $hp }
  if ($sp) { $env:HTTPS_PROXY = $sp; $env:https_proxy = $sp }
  if ($np) { $env:NO_PROXY = $np; $env:no_proxy = $np }
  Info 'Proxy env updated for this session.'
  $ans = Read-Host 'Persist to .arw\env.ps1? (Y/n)'
  if (-not ($ans -match '^[nN]')) {
    $envDir = Join-Path $root '.arw'; New-Item -ItemType Directory -Force $envDir | Out-Null
    $f = Join-Path $envDir 'env.ps1'
    @(
      "# Proxies",
      "`$env:HTTP_PROXY = '$hp'",
      "`$env:HTTPS_PROXY = '$sp'",
      "`$env:NO_PROXY = '$np'"
    ) | Add-Content -Path $f -Encoding utf8
    Info ("Updated proxies in " + $f)
  }
}

function Do-Package {
  if ($DoPackage) {
    Section 'Packaging portable bundle'
    if ($script:SetupDryRun) {
      Info '[dryrun] Would run: scripts/package.ps1'
    } else {
      & (Join-Path $PSScriptRoot 'package.ps1')
    }
  }
}

function Save-Preferences {
  $envDir = Join-Path $root '.arw'
  New-Item -ItemType Directory -Force $envDir | Out-Null
  $f = Join-Path $envDir 'env.ps1'
  @(
    "# ARW env (project-local)",
    "# dot-source this file to apply preferences",
    "`$env:ARW_USE_NIX = '$($env:ARW_USE_NIX)'",
    "`$env:ARW_ALLOW_SYSTEM_PKGS = '$($env:ARW_ALLOW_SYSTEM_PKGS)'",
    "`$env:ARW_PORT = '$Port'",
    "`$env:ARW_DOCS_URL = '$DocsUrl'",
    "`$env:ARW_ADMIN_TOKEN = '$AdminToken'"
  ) | Set-Content -Path $f -Encoding utf8
  Info "Saved preferences to $f"
}

function First-Run-Wizard {
  Banner 'First-Run Wizard' 'Guided setup for ARW'
  Write-Host 'Choose a setup profile:'
  Write-Host '  1) Local only (no launcher)'
  Write-Host '  2) Local with launcher (preferred)'
  Write-Host '  3) Cluster (NATS)'
  $prof = Read-Host 'Select [1/2/3]'; if (-not $prof) { $prof = '1' }
  $p = Read-Host ("HTTP port [" + $Port + "]"); if (-not $p) { $p = $Port }
  $ans = Read-Host 'Generate admin token? (Y/n)'
  if (-not ($ans -match '^[nN]')) {
    $tok = New-AdminToken
    $env:ARW_ADMIN_TOKEN = $tok
    Show-GeneratedToken -Token $tok -Label 'admin token'
    Warn 'Store this token securely.'
  }
  $cfgDir = Join-Path $root 'configs'; New-Item -ItemType Directory -Force $cfgDir | Out-Null
  $cfgPath = Join-Path $cfgDir 'local.toml'
  if ($prof -eq '3') {
    $nurl = Read-Host 'NATS URL [nats://127.0.0.1:4222]'; if (-not $nurl) { $nurl = 'nats://127.0.0.1:4222' }
    @"
[runtime]
portable = true
port = $p

[cluster]
enabled = true
bus = "nats"
queue = "nats"
nats_url = "$nurl"
"@ | Set-Content -Path $cfgPath -Encoding utf8
  } else {
    @"
[runtime]
portable = true
port = $p

[cluster]
enabled = false
bus = "local"
queue = "local"
"@ | Set-Content -Path $cfgPath -Encoding utf8
  }
  $env:ARW_CONFIG = $cfgPath; $script:CfgPath = $cfgPath; $script:Port = [int]$p
  Info ("Wrote " + $cfgPath + ' and set ARW_CONFIG')
  if ($prof -eq '3') { Install-NatsLocal }
  Do-Build
  $go = Read-Host 'Start service now? (Y/n)'; if (-not ($go -match '^[nN]')) { & (Join-Path $PSScriptRoot 'start.ps1') -Port $Port -LauncherDebug -WaitHealth -WaitHealthTimeoutSecs 20; Start-Process -FilePath ("http://127.0.0.1:" + $Port + "/spec") | Out-Null }
  $sv = Read-Host 'Save preferences to .arw\env.ps1? (Y/n)'; if (-not ($sv -match '^[nN]')) { Save-Preferences }
}

function Support-Bundle {
  $outdir = Join-Path $root '.arw\support'; New-Item -ItemType Directory -Force $outdir | Out-Null
  $ts = Get-Date -Format yyyyMMdd_HHmmss
  $tmp = Join-Path $outdir ("tmp_" + $ts); New-Item -ItemType Directory -Force $tmp | Out-Null
  $logs = Join-Path $root '.arw\logs'; if (Test-Path $logs) { Copy-Item $logs -Recurse -Destination (Join-Path $tmp 'logs') }
  $cfgs = Join-Path $root 'configs'; if (Test-Path $cfgs) { Copy-Item $cfgs -Recurse -Destination (Join-Path $tmp 'configs') }
  $envFile = Join-Path $tmp 'env_redacted.txt'
  Get-ChildItem Env: | Where-Object { $_.Name -like 'ARW_*' } | ForEach-Object {
    $val = if ($_.Name -eq 'ARW_ADMIN_TOKEN') { '***REDACTED***' } else { $_.Value }
    ("{0}={1}" -f $_.Name, $val) | Add-Content -Path $envFile -Encoding utf8
  }
  $zip = Join-Path $outdir ("arw_support_" + $ts + ".zip")
  Compress-Archive -Path (Join-Path $tmp '*') -DestinationPath $zip -Force
  Remove-Item -Recurse -Force $tmp
  Info ("Support bundle: " + $zip)
}

function Main-Menu {
  while ($true) {
    Banner 'Setup Menu' ("Choose an action — DryRun=" + $script:SetupDryRun)
    Write-Host @'
  1) Check prerequisites
  2) Dependencies (install/fix common requirements)
  3) Install MkDocs toolchain (optional)
  4) Configure settings (port/docs/token)
  5) Configure clustering (NATS)
  6) Build now (release)
  7) Generate docs (if enabled)
  8) Package portable bundle (dist/)
  9) Run everything (build → docs → package)
  10) Open project README
  11) Save preferences (./.arw/env.ps1)
  12) First-run wizard (guided)
  13) Create support bundle
  14) Toggle dry-run mode (now: On/Off)
  15) Audit supply-chain (cargo-audit/deny)
  0) Exit
'@
    $pick = Read-Host 'Select'
    switch ($pick) {
      '1' { Ensure-Prereqs }
      '2' { Dependencies-Menu }
      '3' { Install-MkDocs }
      '4' { Configure-Settings }
      '5' { Configure-Cluster }
      '6' { Do-Build }
      '7' { Do-Docs }
      '8' { Do-Package }
      '9' { Do-Build; Do-Docs; Do-Package }
      '10' { Start-Process -FilePath (Join-Path $root 'README.md') | Out-Null }
      '11' { Save-Preferences }
      '12' { First-Run-Wizard }
      '13' { Support-Bundle }
      '14' { $script:SetupDryRun = -not $script:SetupDryRun; if ($script:SetupDryRun) { Info 'Dry-run mode enabled' } else { Info 'Dry-run mode disabled' } }
      '15' { try { & (Join-Path $PSScriptRoot 'audit.ps1') -Interactive } catch { Warn $_.Exception.Message } }
      '0' { break }
      default { }
    }
  }

  Section 'Next'
  Info ("Start the service with: scripts\interactive-start-windows.ps1")
}

if ($Headless) {
  Ensure-Prereqs
  Do-Build
  if ($BuildDocs) { Do-Docs }
  if (-not $Package) { $DoPackage = $true }
  Do-Package
  exit 0
}

Configure-Settings
Main-Menu
