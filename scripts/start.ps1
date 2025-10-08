#!powershell
[CmdletBinding()]
param(
  [int]$Port = 8091,
  [switch]$LauncherDebug,
  [string]$DocsUrl,
  [string]$AdminToken,
  [int]$TimeoutSecs = 20,
  [switch]$UseDist,
  [switch]$NoBuild,
  [switch]$WaitHealth,
  [int]$WaitHealthTimeoutSecs = 30,
  [switch]$DryRun,
  [switch]$HideWindow,
  [switch]$ServiceOnly,
  [switch]$LauncherOnly,
  [switch]$InstallWebView2,
  [switch]$NoSummary
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($ServiceOnly -and $LauncherOnly) {
  Write-Error '-ServiceOnly and -LauncherOnly cannot be combined.'
  exit 1
}

function Get-LauncherConfigDir {
  if ($env:ARW_CONFIG_HOME -and -not [string]::IsNullOrWhiteSpace($env:ARW_CONFIG_HOME)) {
    return $env:ARW_CONFIG_HOME.TrimEnd('\')
  }
  if ($env:XDG_CONFIG_HOME -and -not [string]::IsNullOrWhiteSpace($env:XDG_CONFIG_HOME)) {
    return (Join-Path $env:XDG_CONFIG_HOME 'arw')
  }
  $appData = [Environment]::GetFolderPath('ApplicationData')
  if ([string]::IsNullOrWhiteSpace($appData)) { return $null }
  return (Join-Path $appData 'arw\config')
}

function ConvertTo-LauncherHashtable {
  param([Parameter(Mandatory = $true)][object]$InputObject)
  if ($null -eq $InputObject) { return @{} }
  if ($InputObject -is [System.Collections.IDictionary]) {
    $table = @{}
    foreach ($key in $InputObject.Keys) { $table[$key] = $InputObject[$key] }
    return $table
  }
  $table = @{}
  foreach ($prop in ($InputObject | Get-Member -MemberType NoteProperty -ErrorAction SilentlyContinue)) {
    $name = $prop.Name
    $table[$name] = $InputObject.$name
  }
  return $table
}


function Load-LauncherSettings {
  try {
    $configDir = Get-LauncherConfigDir
    if (-not $configDir) { return $null }
    $prefsPath = Join-Path $configDir 'prefs-launcher.json'
    if (-not (Test-Path $prefsPath)) { return $null }
    $raw = Get-Content $prefsPath -Raw
    if ([string]::IsNullOrWhiteSpace($raw)) { return $null }
    $parsed = $raw | ConvertFrom-Json
    return ConvertTo-LauncherHashtable -InputObject $parsed
  } catch {
    return $null
  }
}


function Update-LauncherPrefs {
  param(
    [string]$Token,
    [Nullable[int]]$Port
  )
  if ([string]::IsNullOrWhiteSpace($Token) -and -not $Port.HasValue) { return }
  $configDir = Get-LauncherConfigDir
  if (-not $configDir) { return }
  if (-not (Test-Path $configDir)) {
    try { New-Item -ItemType Directory -Path $configDir -Force | Out-Null } catch { return }
  }
  $prefsPath = Join-Path $configDir 'prefs-launcher.json'
  $data = @{}
  if (Test-Path $prefsPath) {
    try {
      $raw = Get-Content $prefsPath -Raw
      if (-not [string]::IsNullOrWhiteSpace($raw)) {
        $parsed = $raw | ConvertFrom-Json
        $data = ConvertTo-LauncherHashtable -InputObject $parsed
      }
    } catch {
      $data = @{}
    }
  }
  if (-not ($data -is [System.Collections.IDictionary])) { $data = @{} }
  if (-not [string]::IsNullOrWhiteSpace($Token)) { $data['adminToken'] = $Token }
  if ($Port.HasValue) { $data['port'] = [int]$Port.Value }
  try {
    $json = $data | ConvertTo-Json -Depth 8
    Set-Content -Path $prefsPath -Value $json -Encoding UTF8
  } catch {}
}

# Compatibility: PowerShell 5 vs 7 for Invoke-WebRequest
$script:IwrArgs = @{}
try { if ($PSVersionTable.PSVersion.Major -lt 6) { $script:IwrArgs = @{ UseBasicParsing = $true } } } catch {}

# Optional: WebView2 runtime helpers (for Tauri-based launcher)
$script:HasWebView2 = $true
try {
  . (Join-Path $PSScriptRoot 'webview2.ps1')
  try {
    if (Get-Command Test-WebView2Runtime -ErrorAction SilentlyContinue) { $script:HasWebView2 = (Test-WebView2Runtime) }
  } catch { $script:HasWebView2 = $true }
} catch { $script:HasWebView2 = $true }

function Info($m){ Write-Host "[start] $m" -ForegroundColor DarkCyan }
function Dry($m){ if ($DryRun) { Write-Host "[dryrun] $m" -ForegroundColor Yellow } }

$showSummary = -not $NoSummary
$summaryLines = New-Object System.Collections.Generic.List[string]
function Add-SummaryLine {
  param([string]$Text)
  if ($showSummary -and $Text) { [void]$summaryLines.Add($Text) }
}
function Emit-Summary {
  if (-not $showSummary -or $summaryLines.Count -eq 0) { return }
  Write-Host ''
  Write-Host '--- Launcher summary ---' -ForegroundColor DarkCyan
  foreach ($line in $summaryLines) { Write-Host "  $line" }
  Write-Host '------------------------' -ForegroundColor DarkCyan
}
$portWasSpecified = $PSBoundParameters.ContainsKey('Port')

$launcherSettings = Load-LauncherSettings
$settingsPort = $null
$settingAutostartService = $null
$settingNotifyOnStatus = $null
$settingBaseOverride = $null

if ($launcherSettings) {
  if ($launcherSettings.ContainsKey('port')) {
    try {
      $settingsPort = [int]$launcherSettings['port']
    } catch {}
  }
  if ($launcherSettings.ContainsKey('autostart')) {
    $settingAutostartService = [bool]$launcherSettings['autostart']
  }
  if ($launcherSettings.ContainsKey('notifyOnStatus')) {
    $settingNotifyOnStatus = [bool]$launcherSettings['notifyOnStatus']
  }
  if ($launcherSettings.ContainsKey('baseOverride')) {
    $settingBaseOverride = [string]$launcherSettings['baseOverride']
    if ([string]::IsNullOrWhiteSpace($settingBaseOverride)) { $settingBaseOverride = $null }
    else { $settingBaseOverride = $settingBaseOverride.Trim() }
  }
}

if (-not $portWasSpecified -and $settingsPort -and $settingsPort -ge 1 -and $settingsPort -le 65535) {
  $Port = $settingsPort
}

$debugEnabled = $LauncherDebug
if (-not $debugEnabled -and $PSBoundParameters.ContainsKey('Debug')) {
  $debugEnabled = $true
}

if ($debugEnabled) { if (-not $DryRun) { $env:ARW_DEBUG = '1' } else { Dry 'Would set ARW_DEBUG=1' } }
if ($DocsUrl) { if (-not $DryRun) { $env:ARW_DOCS_URL = $DocsUrl } else { Dry "Would set ARW_DOCS_URL=$DocsUrl" } }
if ($AdminToken) { if (-not $DryRun) { $env:ARW_ADMIN_TOKEN = $AdminToken } else { Dry 'Would set ARW_ADMIN_TOKEN=<redacted>' } }
if ($TimeoutSecs) { if (-not $DryRun) { $env:ARW_HTTP_TIMEOUT_SECS = "$TimeoutSecs" } else { Dry "Would set ARW_HTTP_TIMEOUT_SECS=$TimeoutSecs" } }
if ($Port) { if (-not $DryRun) { $env:ARW_PORT = "$Port" } else { Dry "Would set ARW_PORT=$Port" } }
if (-not $DryRun) {
  if (-not $env:ARW_EGRESS_PROXY_ENABLE) { $env:ARW_EGRESS_PROXY_ENABLE = '1' }
  if (-not $env:ARW_DNS_GUARD_ENABLE) { $env:ARW_DNS_GUARD_ENABLE = '1' }
} else {
  Dry 'Would set ARW_EGRESS_PROXY_ENABLE=1 (default)'
  Dry 'Would set ARW_DNS_GUARD_ENABLE=1 (default)'
}

$persistToken = $env:ARW_ADMIN_TOKEN
if (-not [string]::IsNullOrWhiteSpace($persistToken) -or $portWasSpecified) {
  if ($portWasSpecified) {
    Update-LauncherPrefs -Token $persistToken -Port $Port
  } else {
    Update-LauncherPrefs -Token $persistToken
  }
}

$windowStyle = [System.Diagnostics.ProcessWindowStyle]::Minimized
if ($HideWindow) {
  $windowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
} elseif ($debugEnabled) {
  $windowStyle = [System.Diagnostics.ProcessWindowStyle]::Normal
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe = 'arw-server.exe'
$launcherExe = 'arw-launcher.exe'
$svc = if ($UseDist) {
  $zipBase = Get-ChildItem -Path (Join-Path $root 'dist') -Filter 'arw-*-windows-*' -Directory -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if ($zipBase) { Join-Path $zipBase.FullName (Join-Path 'bin' $exe) } else { $null }
} else { Join-Path (Join-Path $root 'target\release') $exe }
$launcher = if ($UseDist) {
  if ($zipBase) { Join-Path $zipBase.FullName (Join-Path 'bin' $launcherExe) } else { $null }
} else { Join-Path (Join-Path $root 'target\release') $launcherExe }

$startService = $true
$startLauncher = $true
if ($ServiceOnly) {
  $startLauncher = $false
  if (-not $DryRun) {
    $env:ARW_NO_LAUNCHER = '1'
    $env:ARW_NO_TRAY = '1'
  } else {
    Dry 'Would set ARW_NO_LAUNCHER=1 (service only)'
    Dry 'Would set ARW_NO_TRAY=1 (service only)'
  }
} elseif ($LauncherOnly) {
  $startService = $false
  if (-not $DryRun) {
    $env:ARW_NO_LAUNCHER = '0'
    $env:ARW_NO_TRAY = '0'
  } else {
    Dry 'Would set ARW_NO_LAUNCHER=0 (launcher only)'
    Dry 'Would set ARW_NO_TRAY=0 (launcher only)'
  }
} elseif (($env:ARW_NO_LAUNCHER -and $env:ARW_NO_LAUNCHER -eq '1') -or ($env:ARW_NO_TRAY -and $env:ARW_NO_TRAY -eq '1')) {
  $startLauncher = $false
}

if (-not $startService -and -not $startLauncher) {
  Info 'Nothing to launch (launcher disabled and service suppressed).'
  exit 0
}

if (-not $startService -and $WaitHealth) {
  Write-Warning '-WaitHealth requested but service launch disabled; skipping health probe.'
}

if ($startLauncher -and -not $DryRun) {
  $webViewReady = $script:HasWebView2
  if (-not $webViewReady -and $InstallWebView2) {
    try {
      if (Get-Command Install-WebView2Runtime -ErrorAction SilentlyContinue) {
        Info 'WebView2 runtime missing. Attempting Evergreen installation (silent)...'
        $installed = Install-WebView2Runtime -Silent
        if ($installed) {
          Info 'WebView2 runtime installed successfully.'
          try {
            if (Get-Command Test-WebView2Runtime -ErrorAction SilentlyContinue) {
              $webViewReady = Test-WebView2Runtime
              $script:HasWebView2 = $webViewReady
            }
          } catch {}
        } else {
          Write-Warning 'WebView2 installation failed or was cancelled. Launcher will be skipped unless the runtime is installed.'
        }
      } else {
        Write-Warning 'Install-WebView2Runtime helper unavailable; please run scripts\webview2.ps1 manually.'
      }
    } catch {
      Write-Warning "WebView2 installation attempt failed: $($_.Exception.Message)"
    }
  }
  if (-not $webViewReady) {
    if ($LauncherOnly) {
      Write-Error 'WebView2 runtime not detected. Install it via scripts\webview2.ps1 or rerun with -InstallWebView2 before using -LauncherOnly.'
      exit 1
    }
    Write-Warning 'WebView2 runtime not detected. Skipping desktop launcher; run scripts\webview2.ps1 or re-run with -InstallWebView2 after installing the Evergreen runtime.'
    $startLauncher = $false
    $ServiceOnly = $true
    if (-not $DryRun) {
      $env:ARW_NO_LAUNCHER = '1'
      $env:ARW_NO_TRAY = '1'
    }
    Add-SummaryLine 'Launcher skipped: WebView2 runtime missing.'
  } else {
    Add-SummaryLine 'WebView2 runtime detected.'
  }
} elseif ($startLauncher -and $DryRun) {
  Dry 'Would verify WebView2 runtime before launching the desktop UI.'
}

if ($startService -and (-not $svc -or -not (Test-Path $svc))) {
  if ($DryRun) {
    Write-Warning "Service binary not found ($svc). [dryrun] would build release (arw-server)."
    $svc = Join-Path (Join-Path $root 'target\release') $exe
  } elseif ($NoBuild) {
    Write-Error "Service binary not found and -NoBuild specified. Build first or remove -NoBuild."
    exit 1
  } else {
    Write-Warning "Service binary not found ($svc). Building release..."
    try {
      Push-Location $root
      if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw "Rust 'cargo' not found in PATH. Install Rust from https://rustup.rs" }
      cargo build --release -p arw-server
    } catch {
      Write-Error ("Failed to build arw-server: " + $_.Exception.Message)
      Pop-Location
      exit 1
    } finally {
      if ((Get-Location).Path -ne $root) { try { Pop-Location } catch {} }
    }
    $svc = Join-Path (Join-Path $root 'target\release') $exe
  }
}

if ($startLauncher -and (-not $launcher -or -not (Test-Path $launcher))) {
  if ($DryRun) {
    Write-Warning "Launcher binary not found ($launcher). [dryrun] would attempt build (arw-launcher)."
  } elseif (-not $NoBuild) {
    Write-Warning "Launcher binary not found ($launcher). Attempting build..."
    try {
      Push-Location $root
      if (Get-Command cargo -ErrorAction SilentlyContinue) {
        cargo build --release -p arw-launcher
      } else {
        Write-Warning "Rust 'cargo' not found; skipping launcher build."
      }
    } catch {
      Write-Warning "Launcher build attempt failed: $($_.Exception.Message)"
    } finally {
      try { Pop-Location } catch {}
    }
  }
  $launcher = Join-Path (Join-Path $root 'target\release') $launcherExe
}

# Helper utilities
function Ensure-ParentDir($path) {
  try {
    $dir = [System.IO.Path]::GetDirectoryName($path)
    if ($dir -and $dir -ne '') { New-Item -ItemType Directory -Force $dir | Out-Null }
  } catch {}
}

function Wait-For-Health($port, $timeoutSecs) {
  $base = "http://127.0.0.1:$port"
  $deadline = (Get-Date).AddSeconds([int]$timeoutSecs)
  $ok = $false
  $attempts = 0
  while ((Get-Date) -lt $deadline) {
    $attempts++
    try {
      $resp = Invoke-WebRequest @IwrArgs -TimeoutSec 5 -Uri ("$base/healthz")
      if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300) { $ok = $true; break }
    } catch {}
    Start-Sleep -Milliseconds 500
  }
  if ($ok) { Info ("Health OK after " + $attempts + " checks → $base/healthz") } else { Write-Warning ("Health not reachable within $timeoutSecs seconds → $base/healthz") }
}

function Start-ServiceBinary([string]$message) {
  Info $message
  if ($DryRun) {
    Dry ("Would start: $svc (cwd=$root, windowStyle=$($windowStyle.ToString()))")
    if ($env:ARW_LOG_FILE) { Dry ("Would redirect output to $env:ARW_LOG_FILE") }
    if ($env:ARW_PID_FILE) { Dry ("Would write PID file to $env:ARW_PID_FILE") }
    if ($WaitHealth) { Dry ("Would wait for health at /healthz (timeout ${WaitHealthTimeoutSecs}s)") }
    return
  }

  $startArgs = @{ FilePath = $svc; WorkingDirectory = $root; PassThru = $true }
  if ($env:ARW_LOG_FILE) {
    Ensure-ParentDir $env:ARW_LOG_FILE
    $startArgs.WindowStyle = $windowStyle
    $startArgs.RedirectStandardOutput = $env:ARW_LOG_FILE
    $startArgs.RedirectStandardError = $env:ARW_LOG_FILE
  } elseif ($HideWindow) {
    $startArgs.WindowStyle = $windowStyle
  } else {
    $startArgs.NoNewWindow = $true
  }

  $p = Start-Process @startArgs

  if ($env:ARW_PID_FILE) {
    Ensure-ParentDir $env:ARW_PID_FILE
    try { $p.Id | Out-File -FilePath $env:ARW_PID_FILE -Encoding ascii -Force } catch {}
  }

  if ($WaitHealth) { Wait-For-Health -port $Port -timeoutSecs $WaitHealthTimeoutSecs }
}

function Start-LauncherBinary {
  Info "Launching launcher $launcher"
  if ($DryRun) {
    Dry ("Would launch launcher: $launcher")
    return
  }
  if (-not $script:HasWebView2) {
    Write-Warning "WebView2 Runtime not detected; the launcher may prompt to install it. You can install it now via: powershell -ExecutionPolicy Bypass -File scripts/webview2.ps1"
  }
  if (-not $LauncherOnly) {
    if ($settingAutostartService -eq $false) {
      try { Remove-Item Env:ARW_AUTOSTART -ErrorAction SilentlyContinue } catch {}
    } else {
      try { $env:ARW_AUTOSTART = '1' } catch {}
    }
  }
  & $launcher
}

if ($startLauncher -and -not (Test-Path $launcher)) {
  if ($LauncherOnly) {
    Write-Error "Launcher binary not found ($launcher). Build it first or rerun without -LauncherOnly."
    exit 1
  }
  Write-Warning "Launcher binary not found ($launcher); falling back to service only."
  $startLauncher = $false
}

$serviceBase = "http://127.0.0.1:$Port"
if ($startService) {
  Add-SummaryLine "Service listening on $serviceBase"
} elseif ($startLauncher) {
  Add-SummaryLine "Launcher-only mode: expecting service at $serviceBase"
}
if ($startLauncher) {
  Add-SummaryLine 'Control Room launching via desktop launcher.'
} elseif ($startService) {
  Add-SummaryLine ("Headless mode: open Control Room in your browser → {0}/admin/ui/control/" -f $serviceBase)
}
if ($launcherSettings) {
  $summaryPieces = @()
  $summaryPieces += "port $Port"
  if ($settingAutostartService -ne $null) {
    $summaryPieces += "autostart " + ($settingAutostartService ? 'on' : 'off')
  }
  if ($settingNotifyOnStatus -ne $null) {
    $summaryPieces += "notifications " + ($settingNotifyOnStatus ? 'on' : 'off')
  }
  Add-SummaryLine ("Launcher defaults → {0}" -f ($summaryPieces -join ', '))
  if ($settingBaseOverride) {
    Add-SummaryLine ("Default base override → {0}" -f $settingBaseOverride)
  }
  Add-SummaryLine 'Adjust via Control Room → Launcher Settings.'
} else {
  Add-SummaryLine 'Launcher settings: use Control Room → Launcher Settings to adjust defaults.'
}
if ($env:ARW_ADMIN_TOKEN -and -not [string]::IsNullOrWhiteSpace($env:ARW_ADMIN_TOKEN)) {
  Add-SummaryLine 'Admin token detected via ARW_ADMIN_TOKEN.'
} else {
  Add-SummaryLine 'Admin token not set; export ARW_ADMIN_TOKEN to secure admin endpoints.'
}

if ($startService) {
  if (-not (Test-Path $svc)) {
    Write-Error "Service binary not found ($svc). Build it first or rerun without -ServiceOnly."
    exit 1
  }
  $context = if ($startLauncher) { "Launching $svc on http://127.0.0.1:$Port" } else { "Launching $svc on http://127.0.0.1:$Port (service only)" }
  Start-ServiceBinary $context
}

Emit-Summary

if ($startLauncher) {
  Start-LauncherBinary
} elseif (-not $startService) {
  Info 'Launcher requested suppression of service; exiting.'
}
