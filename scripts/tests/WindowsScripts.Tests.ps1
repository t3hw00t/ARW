param(
  [switch]$VerboseOutput
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Ensure Pester is available when executed ad-hoc
$minimumPesterVersion = [Version]'5.4.0'
$availablePester = Get-Module -ListAvailable -Name Pester | Where-Object { $_.Version -ge $minimumPesterVersion }
if (-not $availablePester) {
  try { Install-Module Pester -Scope CurrentUser -Force -SkipPublisherCheck -MinimumVersion $minimumPesterVersion -ErrorAction Stop } catch { Write-Warning "Pester not installed and install failed: $($_.Exception.Message)" }
  $availablePester = Get-Module -ListAvailable -Name Pester | Where-Object { $_.Version -ge $minimumPesterVersion }
}
Import-Module Pester -MinimumVersion $minimumPesterVersion -ErrorAction SilentlyContinue

Describe 'Windows PowerShell Scripts - Parse and Structure' {
  BeforeAll {
    $script:root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path
    $script:scriptsDir = Join-Path $script:root 'scripts'
  }

  It 'parses all top-level scripts without diagnostics' {
    $files = Get-ChildItem -Path $script:scriptsDir -File -Filter '*.ps1' | Where-Object { $_.FullName -notmatch '\\tests\\' -and $_.Name -ne 'WindowsScripts.Tests.ps1' }
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
    BeforeAll {
      $script:startInteractive = Join-Path $script:scriptsDir 'interactive-start-windows.ps1'
    }
    It 'defines all expected menu functions' {
      $content = Get-Content -Path $script:startInteractive -Raw
      $funcs = Select-String -Path $script:startInteractive -Pattern '^function\s+([A-Za-z0-9\-]+)\b' -AllMatches | ForEach-Object { $_.Matches } | ForEach-Object { $_.Groups[1].Value }
      $required = @(
        'Configure-Runtime','Pick-Config','Start-ServiceOnly','Start-LauncherPlusService','Start-Connector',
        'Open-ProbeMenu','Build-TestMenu','Cli-ToolsMenu','Main-Menu','Force-Stop','Logs-Menu',
        'Save-Prefs-From-Start','Open-Terminal-Here','Security-Preflight','Reverse-Proxy-Templates',
        'Reverse-Proxy-Caddy-Start','Reverse-Proxy-Caddy-Stop','Reverse-Proxy-Caddy-Choose-Start',
        'Session-Summary','Stop-All','TLS-Wizard','Configure-Http-Port','Spec-Sync','Docs-Build-Open',
        'Launcher-Build-Check','Doctor','Install-NatsLocal','Nats-Menu','Wsl-Select-Distro','Wsl-Run',
        'Wsl-Install-Nats','Wsl-Start-Nats','Wsl-Stop-Nats','Wsl-Show-Info','Wsl-Open-Terminal','Wsl-Set-Default',
        'Security-Tips','Start-DryRun','Reverse-Proxy-Caddy-Start-Preview','Reverse-Proxy-Caddy-Stop-Preview','Reverse-Proxy-Templates-Preview','TLS-Wizard-Preview'
      )
      foreach ($r in $required) { $funcs | Should -Contain $r }
    }

    It 'uses IwrArgs for web requests' {
      $lines = Get-Content -Path $script:startInteractive
      ($lines -match 'Invoke-WebRequest\s+@IwrArgs').Count | Should -BeGreaterThan 0
    }

    It 'opens wt.exe with new-tab before -p' {
      (Get-Content -Path $script:startInteractive -Raw) -match "new-tab','-p'" | Should -BeTrue
    }

    It 'Force-Stop also kills launcher and connector if present' {
      $raw = Get-Content -Path $script:startInteractive -Raw
      $raw -match "Stop-Process -Name 'arw-launcher'" | Should -BeTrue
      $raw -match "Stop-Process -Name 'arw-connector'" | Should -BeTrue
    }

    It 'has a Start (dry-run) menu item and invokes -DryRun' {
      $raw = Get-Content -Path $script:startInteractive -Raw
      $raw -match 'Start \(dry-run preview\)' | Should -BeTrue
      $raw -match '-DryRun' | Should -BeTrue
    }

    It 'NATS menu has a dry-run option' {
      $raw = Get-Content -Path $startInteractive -Raw
      $raw -match 'Dry-run start plan \(Windows\)' | Should -BeTrue
    }
  }

  Context 'start.ps1' {
    BeforeAll {
      $script:startScript = Join-Path $script:scriptsDir 'start.ps1'
    }
    It 'uses IwrArgs for health check' {
      (Get-Content -Path $script:startScript -Raw) -match 'Invoke-WebRequest\s+@IwrArgs' | Should -BeTrue
    }

    It 'exposes a DryRun switch and prints dryrun markers' {
      $raw = Get-Content -Path $script:startScript -Raw
      $raw -match '\[switch\]\$DryRun' | Should -BeTrue
      $raw -match '\[dryrun\]' | Should -BeTrue
    }
  }

  Context 'interactive-setup-windows.ps1' {
    BeforeAll {
      $script:setupScript = Join-Path $script:scriptsDir 'interactive-setup-windows.ps1'
    }
    It 'uses IwrArgs for downloads' {
      $raw = Get-Content -Path $script:setupScript -Raw
      ([regex]::Matches($raw, 'Invoke-WebRequest\s+@IwrArgs').Count) | Should -BeGreaterThan 0
    }

    It 'advertises Setup dry-run and includes preview messages' {
      $raw = Get-Content -Path $script:setupScript -Raw
      $raw -match 'Toggle dry-run mode' | Should -BeTrue
      $raw -match '\[dryrun\] Would run: scripts/docgen.ps1' | Should -BeTrue
      $raw -match '\[dryrun\] Would run: scripts/package.ps1' | Should -BeTrue
    }
  }
}

# Additional checks for setup dry-run
Describe 'Windows Setup Script — DryRun extras' {
  BeforeAll {
    if (-not $script:scriptsDir) {
      $script:root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path
      $script:scriptsDir = Join-Path $script:root 'scripts'
    }
  }
  It 'contains cargo build and download dry-run markers' {
    $setup = Join-Path $script:scriptsDir 'interactive-setup-windows.ps1'
    $raw = Get-Content -Path $setup -Raw
    $raw -match '\[dryrun\] Would run: cargo build --workspace --release' | Should -BeTrue
    $raw -match 'download rustup-init.exe' | Should -BeTrue
    $raw -match 'download jq.exe' | Should -BeTrue
  }
}

Context 'stamp_docs_updated.py' {
  BeforeAll {
    $script:pythonCmd = Get-Command python3 -ErrorAction SilentlyContinue
    if (-not $script:pythonCmd) {
      $script:pythonCmd = Get-Command python -ErrorAction Stop
    }
  }

  It 'git_status_has_changes detects pending changes and cleans up' {
    $code = @"
import sys, pathlib, subprocess

root = pathlib.Path.cwd()
sys.path.insert(0, str(root / "scripts"))
import stamp_docs_updated as sdu  # type: ignore

tmp_path = root / "docs" / "__stamp_tmp_test.md"
rel = None
if tmp_path.exists():
    tmp_path.unlink()

try:
    tmp_path.write_text("temporary\n", encoding="utf-8")
    if not sdu.git_status_has_changes(str(tmp_path)):
        print("expected detection for untracked file", file=sys.stderr)
        sys.exit(1)
    rel = tmp_path.relative_to(root).as_posix()
    subprocess.run(["git", "add", rel], cwd=root, check=True)
    if not sdu.git_status_has_changes(str(tmp_path)):
        print("expected detection for staged file", file=sys.stderr)
        sys.exit(2)
finally:
    if rel is not None:
        subprocess.run(["git", "reset", "--quiet", "HEAD", rel], cwd=root, check=False)
    if tmp_path.exists():
        tmp_path.unlink()

if sdu.git_status_has_changes(str(tmp_path)):
    print("expected no detection after cleanup", file=sys.stderr)
    sys.exit(3)
"@

    & $script:pythonCmd.Path -c $code
    $LASTEXITCODE | Should -Be 0
  }

  It 'uses Generated timestamp to drive Updated metadata and stays idempotent' {
    $code = @'
import pathlib, sys

root = pathlib.Path.cwd()
sys.path.insert(0, str(root / "scripts"))
import stamp_docs_updated as sdu  # type: ignore

tmp_path = root / "docs" / "__stamp_tmp_generated.md"
if tmp_path.exists():
    tmp_path.unlink()

try:
    tmp_path.write_text(
        """---
title: Temp Doc
---

# Temp Doc
Generated: 2030-01-02 04:05 UTC
Type: Reference
""",
        encoding="utf-8",
    )

    changed = sdu.process(str(tmp_path))
    if not changed:
        print("expected process() to insert Updated line", file=sys.stderr)
        sys.exit(10)

    lines = tmp_path.read_text(encoding='utf-8').splitlines()
    expected = "Updated: 2030-01-02"
    if expected not in lines:
        print("updated line missing or incorrect: " + repr(lines), file=sys.stderr)
        sys.exit(11)

    # Running again should be a no-op
    changed_again = sdu.process(str(tmp_path))
    if changed_again:
        print("process() should be idempotent when timestamps already aligned", file=sys.stderr)
        sys.exit(12)
finally:
    if tmp_path.exists():
        tmp_path.unlink()
'@

    & $script:pythonCmd.Path -c $code
    $LASTEXITCODE | Should -Be 0
  }
}
