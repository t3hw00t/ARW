#!powershell
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Test-WebView2Runtime {
  try {
    $keys = @(
      'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
      'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
      'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    )
    foreach ($k in $keys) { if (Test-Path $k) { return $true } }
  } catch {}
  return $false
}

function Install-WebView2Runtime {
  param(
    [switch]$Silent
  )
  $url = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703'
  $tmp = Join-Path $env:TEMP 'MicrosoftEdgeWebView2Setup.exe'
  Write-Host "[webview2] Downloading Evergreen Runtime bootstrapper..." -ForegroundColor DarkCyan
  try {
    # PS5 compatibility: -UseBasicParsing if available
    $iwrArgs = @{}
    try { if ($PSVersionTable.PSVersion.Major -lt 6) { $iwrArgs = @{ UseBasicParsing = $true } } } catch {}
    Invoke-WebRequest @iwrArgs -Uri $url -OutFile $tmp
  } catch {
    Write-Warning "Failed to download WebView2 bootstrapper: $($_.Exception.Message)"
    return $false
  }
  Write-Host "[webview2] Running bootstrapper..." -ForegroundColor DarkCyan
  try {
    if ($Silent) {
      Start-Process -FilePath $tmp -ArgumentList '/silent','/install' -Wait | Out-Null
    } else {
      Start-Process -FilePath $tmp -ArgumentList '/install' -Wait | Out-Null
    }
  } catch {
    Write-Warning "WebView2 bootstrapper failed: $($_.Exception.Message)"
    return $false
  }
  return (Test-WebView2Runtime)
}

function WebView2-Menu {
  Banner 'WebView2 Runtime' 'Required for Tauri Launcher (Win10/Server)'
  try {
    $has = if (Get-Command Test-WebView2Runtime -ErrorAction SilentlyContinue) { Test-WebView2Runtime } else { $false }
  } catch { $has = $false }
  if ($has) { Write-Host ' Status: installed' -ForegroundColor DarkCyan } else { Write-Host ' Status: not installed' -ForegroundColor Yellow }
  Write-Host @'
  1) Install Evergreen Runtime (user/machine)
  0) Back
'@
  $pick = Read-Host 'Select'
  switch ($pick) {
    '1' {
      try {
        $ok = Install-WebView2Runtime
        if ($ok) { Write-Host '[webview2] Installed.' -ForegroundColor DarkCyan } else { Write-Warning 'Install failed or cancelled.' }
      } catch { Write-Warning 'Install failed' }
    }
    default { }
  }
}
