#!powershell
param(
  [switch]$Yes,
  [switch]$RunTests,
  [switch]$NoDocs,
  [switch]$Minimal,
  [switch]$Headless,
  [switch]$SkipBuild,
  [switch]$SkipCli,
  [switch]$WithCli,
  [switch]$MaxPerf,
  [switch]$StrictReleaseGate,
  [switch]$SkipReleaseGate,
  [switch]$Clean
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:warnings = @()
function Title($t){ Write-Host "`n=== $t ===" -ForegroundColor Cyan }
function Info($m){ Write-Host "[setup] $m" -ForegroundColor DarkCyan }
function Warn($m){ $script:warnings += $m }
function Pause($m){ if(-not $Yes){ Read-Host $m | Out-Null } }

function Ensure-PythonModule {
  param(
    [Parameter(Mandatory=$true)][string]$PythonPath,
    [Parameter(Mandatory=$true)][string]$Module,
    [Parameter()][string]$Package
  )
  if (-not $Package) { $Package = $Module }
  try {
    & $PythonPath -c "import importlib, sys; sys.exit(0 if importlib.util.find_spec('$Module') else 1)" | Out-Null
    if ($LASTEXITCODE -eq 0) { return $true }
  } catch {}
  Info "Installing Python module $Package (pip --user)"
  try {
    & $PythonPath -m pip --version | Out-Null
  } catch {
    Info 'Bootstrapping pip (ensurepip --user)'
    try {
      & $PythonPath -m ensurepip --upgrade --user | Out-Null
    } catch {
      Warn "Unable to bootstrap pip; install $Package manually via `$PythonPath -m pip install --user $Package`."
      return $false
    }
  }
  try {
    $prevBreak = $env:PIP_BREAK_SYSTEM_PACKAGES
    $env:PIP_BREAK_SYSTEM_PACKAGES = '1'
    try {
      & $PythonPath -m pip install --user $Package | Out-Null
    } finally {
      if ([string]::IsNullOrEmpty($prevBreak)) {
        Remove-Item Env:PIP_BREAK_SYSTEM_PACKAGES -ErrorAction SilentlyContinue
      } else {
        $env:PIP_BREAK_SYSTEM_PACKAGES = $prevBreak
      }
    }
    Add-Content $installLog "PIP $Package"
    return $true
  } catch {
    Warn "Failed to install Python module $Package; run `$PythonPath -m pip install --user $Package` manually."
    return $false
  }
}

$buildCli = $true
if ($WithCli.IsPresent) {
  $buildCli = $true
} elseif ($SkipCli.IsPresent) {
  $buildCli = $false
} elseif ($env:ARW_SETUP_AGENT -eq '1') {
  $buildCli = $false
}

$buildMode = $env:ARW_BUILD_MODE
if ([string]::IsNullOrWhiteSpace($buildMode)) { $buildMode = 'release' }
$buildMode = $buildMode.ToLowerInvariant()
if ($buildMode -ne 'release' -and $buildMode -ne 'debug') { $buildMode = 'release' }
$buildLabel = $buildMode

if ($Minimal) {
  Info 'Minimal mode enabled: skipping docs and release packaging.'
  $NoDocs = $true
}
if ($Headless) {
  Info 'Headless mode enabled: launcher build will be skipped.'
}
if ($SkipBuild) {
  Info 'Skip-build enabled: workspace compile/test steps will be bypassed.'
}
if ($buildMode -eq 'debug' -and -not $MaxPerf) {
  Info 'Debug build mode enabled: using cargo build --locked (no --release) for faster iteration.'
}

Title 'Prerequisites'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
# Record install actions for uninstall.ps1
$installLog = Join-Path $root '.install.log'
"# Install log - $(Get-Date)" | Out-File $installLog -Encoding UTF8
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Warn 'Rust `cargo` not found.'
  Write-Host 'Install Rust via rustup:' -ForegroundColor Yellow
  Write-Host '  https://rustup.rs' -ForegroundColor Yellow
  Pause 'Press Enter after installing Rust (or Ctrl+C to abort)'
}

$rustc = Get-Command rustc -ErrorAction SilentlyContinue
if ($rustc) {
  try {
    $info = & $rustc.Source --version
    if ($LASTEXITCODE -eq 0 -and $info -match 'rustc\s+([0-9]+\.[0-9]+\.[0-9]+)') {
      $parsed = [version]$Matches[1]
      if ($parsed -lt [version]'1.90.0') {
        Warn "Rust 1.90.0 or newer required (detected $($Matches[1])). Run `rustup update 1.90.0`."
      } else {
        Info "rustc $($Matches[1])"
      }
    }
  } catch {
    Warn "Unable to query rustc version: $($_.Exception.Message)"
  }
} else {
  Warn 'Rust `rustc` not found on PATH.'
}

$cl = Get-Command cl.exe -ErrorAction SilentlyContinue
if ($cl) {
  Info "MSVC Build Tools detected: $($cl.Source)"
} else {
  $vsInstall = $null
  $vswherePath = $null
  $vswhereBase = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
  if (-not $vswhereBase) { $vswhereBase = [Environment]::GetEnvironmentVariable('ProgramFiles') }
  if ($vswhereBase) {
    $vswherePath = Join-Path $vswhereBase 'Microsoft Visual Studio\\Installer\\vswhere.exe'
  }
  if ($vswherePath -and (Test-Path $vswherePath)) {
    try {
      $vsInstall = & $vswherePath -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
      if ($LASTEXITCODE -ne 0) { $vsInstall = $null }
    } catch {}
  }
  if ($vsInstall) {
    Warn "Microsoft C++ Build Tools detected at $vsInstall, but developer command environment is not active (cl.exe missing from PATH)."
    Write-Host 'Open a "x64 Native Tools Command Prompt for VS 2022" or run:' -ForegroundColor Yellow
    Write-Host "  `"$vsInstall\VC\Auxiliary\Build\vcvars64.bat`"" -ForegroundColor Yellow
    Write-Host 'Then re-run setup in the same shell so native builds succeed.' -ForegroundColor Yellow
  } else {
    Warn 'Microsoft C++ Build Tools (cl.exe) not found. Required for crates with native code (e.g., ring).'
    Write-Host 'Install Build Tools (per-user) via winget:' -ForegroundColor Yellow
    Write-Host '  winget install --id Microsoft.VisualStudio.2022.BuildTools --source winget' -ForegroundColor Yellow
    Write-Host 'When prompted, select the "Desktop development with C++" workload (MSVC, SDK, CMake).' -ForegroundColor Yellow
    Pause 'Press Enter after installing Build Tools (or Ctrl+C to abort)'
  }
}

$py = Get-Command python -ErrorAction SilentlyContinue
if (-not $py) { $py = Get-Command python3 -ErrorAction SilentlyContinue }
if (-not $py) {
  Warn 'Python not found. Docs/site build and docgen extras may be skipped.'
} else {
  if (-not (Get-Command mkdocs -ErrorAction SilentlyContinue)) {
    if ($NoDocs) { Warn 'Skipping MkDocs install because -NoDocs was provided.' }
    else {
      Info 'MkDocs not found. Attempting to install via pip...'
      try { & $py.Path -m pip install --upgrade --user pip | Out-Null } catch { Warn 'pip upgrade failed (continuing).'}
      try { & $py.Path -m pip install --user mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin } catch { Warn 'pip install for mkdocs failed. Docs site will be skipped.' }
      $mkOnPath = Get-Command mkdocs -ErrorAction SilentlyContinue
      $mkViaPy = $false
      try { & $py.Path -m mkdocs --version | Out-Null; $mkViaPy = $true } catch { $mkViaPy = $false }
      if ($mkOnPath -or $mkViaPy) {
        foreach($pkg in 'mkdocs','mkdocs-material','mkdocs-git-revision-date-localized-plugin') { Add-Content $installLog "PIP $pkg" }
      } else {
        Warn 'MkDocs install failed. Docs site will be skipped.'
      }
    }
  }
  Ensure-PythonModule -PythonPath $py.Path -Module 'yaml' -Package 'pyyaml' | Out-Null
}
Title 'Clean previous build artifacts'
if ($Clean) {
  try { & cargo clean } catch {}
} else {
  Info 'Skipping cargo clean (pass -Clean to force a fresh build).'
}

$buildFlags = @('--locked')
if ($buildMode -eq 'release') { $buildFlags += '--release' }
Title ("Build ({0}): core binaries" -f $buildLabel)
# Build only the essential binaries first to keep memory usage low on all platforms.
Title "Build workspace ($buildLabel)"
if ($SkipBuild) {
  Info 'Skipping workspace build (--SkipBuild).'
} else {
  if ($MaxPerf) {
    Info 'Opt-in: maxperf profile enabled'
    # Override global jobs=1 to allow parallel builds for maxperf
    try { $env:CARGO_BUILD_JOBS = [Environment]::ProcessorCount } catch {}
    Info ("Building arw-server ({0})" -f $buildLabel)
    & cargo build --profile maxperf --locked -p arw-server
    if ($buildCli) {
      Info ("Building arw-cli ({0})" -f $buildLabel)
      & cargo build --profile maxperf --locked -p arw-cli
    } else {
      Info 'Skipping arw-cli build (requested)'
    }
  } else {
    Info ("Building arw-server ({0})" -f $buildLabel)
    $serverArgs = @('build') + $buildFlags + @('-p','arw-server')
    & cargo @serverArgs
    if ($buildCli) {
      Info ("Building arw-cli ({0})" -f $buildLabel)
      $cliArgs = @('build') + $buildFlags + @('-p','arw-cli')
      & cargo @cliArgs
    } else {
      Info 'Skipping arw-cli build (requested)'
    }
  }

  # Try to build the optional Desktop Launcher (Tauri) best-effort.
  if (-not $Headless) {
      try {
        Write-Host "[setup] Attempting optional build: arw-launcher" -ForegroundColor DarkCyan
        if ($MaxPerf) {
          & cargo build --profile maxperf --locked -p arw-launcher --features launcher-linux-ui
        } else {
          $launcherArgs = @('build') + $buildFlags + @('-p','arw-launcher','--features','launcher-linux-ui')
          & cargo @launcherArgs
        }
    } catch {
      Warn "arw-launcher build skipped (optional): $($_.Exception.Message)"
    }
  } else {
    Info 'Skipping arw-launcher build (headless).'
  }

  Add-Content $installLog 'DIR target'
}

if ($RunTests) {
  if ($SkipBuild) {
    Warn '-RunTests requested but build step was skipped; not running tests.'
  } else {
  Title 'Run tests (workspace)'
  $nextest = Get-Command cargo-nextest -ErrorAction SilentlyContinue
  $useCargoTest = $false
  if (-not $nextest) {
    $cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cargoCmd) {
      $install = $Yes
      if (-not $Yes) {
        $resp = Read-Host 'cargo-nextest not found. Install now? (Y/n)'
        $install = -not ($resp -match '^[nN]')
      }
      if ($install) {
        Info 'Installing cargo-nextest (cargo install --locked cargo-nextest)'
        & $cargoCmd.Source install --locked cargo-nextest
        if ($LASTEXITCODE -ne 0) {
          Warn 'cargo-nextest install failed; falling back to cargo test.'
          $useCargoTest = $true
        } else {
          $nextest = Get-Command cargo-nextest -ErrorAction SilentlyContinue
        }
      } else {
        Warn 'Skipping cargo-nextest install; falling back to cargo test.'
        $useCargoTest = $true
      }
    } else {
      Warn 'cargo-nextest not found and cargo unavailable; falling back to cargo test.'
      $useCargoTest = $true
    }
  }
  if (-not $useCargoTest -and -not $nextest) {
    Warn 'cargo-nextest unavailable after install attempt; using cargo test.'
    $useCargoTest = $true
  }
  if ($useCargoTest) {
    & cargo test --workspace --locked
  } else {
    & $nextest.Source run --workspace --locked
  }
  }
}

if (-not $Minimal) {
  Title 'Generate workspace status page'
  try { & (Join-Path $PSScriptRoot 'docgen.ps1') } catch { Warn "docgen failed: $($_.Exception.Message)" }

  Title 'Package portable bundle'
  try {
    $packageScript = Join-Path $PSScriptRoot 'package.ps1'
    $packageParams = @{ NoBuild = $true }
    if ($StrictReleaseGate) { $packageParams['StrictReleaseGate'] = $true }
    if ($SkipReleaseGate) { $packageParams['SkipReleaseGate'] = $true }
    & $packageScript @packageParams
  } catch {
    Warn "package.ps1 blocked by execution policy; retrying via child PowerShell with Bypass"
    $fallback = @('-ExecutionPolicy','Bypass','-File',(Join-Path $PSScriptRoot 'package.ps1'),'-NoBuild')
    if ($StrictReleaseGate) { $fallback += '-StrictReleaseGate' }
    if ($SkipReleaseGate) { $fallback += '-SkipReleaseGate' }
    & powershell @fallback
  }
  Add-Content $installLog 'DIR dist'
  if (Test-Path (Join-Path $root 'site')) { Add-Content $installLog 'DIR site' }
}

Title 'Windows runtime check (WebView2 for Launcher)'
try {
  . (Join-Path $PSScriptRoot 'webview2.ps1')
  $hasWV2 = Test-WebView2Runtime
  if ($hasWV2) {
    Info 'WebView2 Evergreen Runtime detected.'
  } else {
    Write-Host 'WebView2 Runtime not found. Required for the Tauri-based Desktop Launcher on Windows 10/Server.' -ForegroundColor Yellow
    Write-Host 'On Windows 11 it is in-box. You can install the Evergreen Runtime now.' -ForegroundColor Yellow
    if ($Yes) {
      $ok = Install-WebView2Runtime -Silent
      if ($ok) { Info 'WebView2 installed.' } else { Warn 'WebView2 install failed or was canceled.' }
    } else {
      $ans = Read-Host 'Install WebView2 Runtime now? (y/N)'
      if ($ans -match '^[yY]') {
        $ok = Install-WebView2Runtime
        if ($ok) { Info 'WebView2 installed.' } else { Warn 'WebView2 install failed or was canceled.' }
      }
    }
  }
} catch {
  Warn "WebView2 check failed: $($_.Exception.Message)"
}

Pop-Location
if ($warnings.Count -gt 0) {
  Title 'Warnings'
  foreach ($w in $warnings) { Write-Host "- $w" -ForegroundColor Yellow }
}
if ($Minimal) {
  Info 'Done. Core binaries are available under target\release\'
} else {
  Info 'Done. See dist\ for portable bundle.'
}

