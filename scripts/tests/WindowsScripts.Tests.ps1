param(
  [switch]$VerboseOutput
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Ensure Pester is available when executed ad-hoc
if (-not (Get-Module -ListAvailable -Name Pester)) {
  try { Install-Module Pester -Scope CurrentUser -Force -SkipPublisherCheck -ErrorAction Stop } catch { Write-Warning "Pester not installed and install failed: $($_.Exception.Message)" }
}
Import-Module Pester -ErrorAction SilentlyContinue

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path
$scriptsDir = Join-Path $root 'scripts'

Describe 'Windows PowerShell Scripts - Parse and Structure' {
  It 'parses all top-level scripts without diagnostics' {
    $files = Get-ChildItem -Path $scriptsDir -File -Filter '*.ps1' | Where-Object { $_.FullName -notmatch '\\tests\\' -and $_.Name -ne 'WindowsScripts.Tests.ps1' }
    $errs = @()
    foreach ($f in $files) {
      $tokens = $null; $ast = $null; $diags = $null
      [System.Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$tokens, [ref]$diags) | Out-Null
      if ($diags -and $diags.Count -gt 0) {
        $errs += @([pscustomobject]@{ File = $f.Name; Diagnostics = $diags })
      }
    }
    if ($VerboseOutput) { $errs | ForEach-Object { Write-Host ("Parse issue in {0}: {1}" -f $_.File, ($_.Diagnostics | Select-Object -First 1)) -ForegroundColor Yellow } }
    $errs | Should -BeNullOrEmpty
  }

  Context 'interactive-start-windows.ps1' {
    $startInteractive = Join-Path $scriptsDir 'interactive-start-windows.ps1'
    It 'defines all expected menu functions' {
      $content = Get-Content -Path $startInteractive -Raw
      $funcs = Select-String -Path $startInteractive -Pattern '^function\s+([A-Za-z0-9\-]+)\b' -AllMatches | ForEach-Object { $_.Matches } | ForEach-Object { $_.Groups[1].Value }
      $required = @(
        'Configure-Runtime','Pick-Config','Start-ServiceOnly','Start-TrayPlusService','Start-Connector',
        'Open-ProbeMenu','Build-TestMenu','Cli-ToolsMenu','Main-Menu','Force-Stop','Logs-Menu',
        'Save-Prefs-From-Start','Open-Terminal-Here','Security-Preflight','Reverse-Proxy-Templates',
        'Reverse-Proxy-Caddy-Start','Reverse-Proxy-Caddy-Stop','Reverse-Proxy-Caddy-Choose-Start',
        'Session-Summary','Stop-All','TLS-Wizard','Configure-Http-Port','Spec-Sync','Docs-Build-Open',
        'Tray-Build-Check','Doctor','Install-NatsLocal','Nats-Menu','Wsl-Select-Distro','Wsl-Run',
        'Wsl-Install-Nats','Wsl-Start-Nats','Wsl-Stop-Nats','Wsl-Show-Info','Wsl-Open-Terminal','Wsl-Set-Default',
        'Security-Tips','Start-DryRun','Reverse-Proxy-Caddy-Start-Preview','Reverse-Proxy-Caddy-Stop-Preview','Reverse-Proxy-Templates-Preview','TLS-Wizard-Preview'
      )
      foreach ($r in $required) { $funcs | Should -Contain $r }
    }

    It 'uses IwrArgs for web requests' {
      $lines = Get-Content -Path $startInteractive
      ($lines -match 'Invoke-WebRequest\s+@IwrArgs').Count | Should -BeGreaterThan 0
    }

    It 'opens wt.exe with new-tab before -p' {
      (Get-Content -Path $startInteractive -Raw) -match "new-tab','-p'" | Should -BeTrue
    }

    It 'Force-Stop also kills tray and connector if present' {
      $raw = Get-Content -Path $startInteractive -Raw
      $raw -match "Stop-Process -Name 'arw-tray'" | Should -BeTrue
      $raw -match "Stop-Process -Name 'arw-connector'" | Should -BeTrue
    }

    It 'has a Start (dry-run) menu item and invokes -DryRun' {
      $raw = Get-Content -Path $startInteractive -Raw
      $raw -match 'Start \(dry-run preview\)' | Should -BeTrue
      $raw -match '-DryRun' | Should -BeTrue
    }

    It 'NATS menu has a dry-run option' {
      $raw = Get-Content -Path $startInteractive -Raw
      $raw -match 'Dry-run start plan \(Windows\)' | Should -BeTrue
    }
  }

  Context 'start.ps1' {
    $start = Join-Path $scriptsDir 'start.ps1'
    It 'uses IwrArgs for health check' {
      (Get-Content -Path $start -Raw) -match 'Invoke-WebRequest\s+@IwrArgs' | Should -BeTrue
    }

    It 'exposes a DryRun switch and prints dryrun markers' {
      $raw = Get-Content -Path $start -Raw
      $raw -match '\[switch\]\$DryRun' | Should -BeTrue
      $raw -match '\[dryrun\]' | Should -BeTrue
    }
  }

  Context 'interactive-setup-windows.ps1' {
    $setup = Join-Path $scriptsDir 'interactive-setup-windows.ps1'
    It 'uses IwrArgs for downloads' {
      $raw = Get-Content -Path $setup -Raw
      ($raw -match 'Invoke-WebRequest\s+@IwrArgs').Count | Should -BeGreaterThan 0
    }

    It 'advertises Setup dry-run and includes preview messages' {
      $raw = Get-Content -Path $setup -Raw
      $raw -match 'Toggle dry-run mode' | Should -BeTrue
      $raw -match '\[dryrun\] Would run: scripts/docgen.ps1' | Should -BeTrue
      $raw -match '\[dryrun\] Would run: scripts/package.ps1' | Should -BeTrue
    }
  }
}

# Additional checks for setup dry-run
Describe 'Windows Setup Script — DryRun extras' {
  It 'contains cargo build and download dry-run markers' {
    $setup = Join-Path (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path 'scripts') 'interactive-setup-windows.ps1'
    $raw = Get-Content -Path $setup -Raw
    $raw -match '\\[dryrun\\] Would run: cargo build --workspace --release' | Should -BeTrue
    $raw -match 'download rustup-init.exe' | Should -BeTrue
    $raw -match 'download jq.exe' | Should -BeTrue
  }
}
