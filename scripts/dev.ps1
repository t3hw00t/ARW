#!powershell
[CmdletBinding(PositionalBinding = $false)]
param(
  [Parameter(Position = 0)]
  [string]$Command = 'help',
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$Args
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path (Join-Path $ScriptRoot '..')).Path

function Show-Help {
  Write-Host @'
ARW Dev Utility (scripts/dev.ps1)

Usage:
  pwsh -NoLogo -NoProfile -File scripts/dev.ps1 <command> [options]

Commands:
  help               Show this message.
  setup              Run repo setup (defaults: -Headless -Yes unless overridden).
  setup-agent        Headless/minimal setup tuned for autonomous agents.
  build              Build workspace (headless by default).
  build-launcher     Build workspace including the launcher.
  clean              Remove cargo/venv artifacts via scripts/clean.ps1.
  fmt                Run cargo fmt --all.
  fmt-check          Run cargo fmt --all -- --check.
  lint               Run cargo clippy with -D warnings.
  lint-fix           Run cargo clippy --fix (best-effort).
  lint-events        Run event-name linter (python).
  test               Run workspace tests (prefers cargo-nextest).
  test-fast          Alias for cargo nextest run --workspace.
  docs               Regenerate docs (docgen + mkdocs build --strict when available).
  docs-check         Run docs checks (uses scripts/docgen.ps1 and mkdocs when available).
  docs-cache         Build offline docs wheel cache (writes dist/docs-wheels.tar.gz).
  verify             Run the standard fmt → clippy → tests → docs guardrail sequence.
                     Flags: -Fast (skip docs/UI), -WithLauncher (include Tauri crate), -Ci (CI parity: registries, docgens --check, env-guard, smokes)
                     Tip: new to the repo? Start with -Fast and follow docs/guide/quick_smoke.md.
  hooks              Install git hooks (cross-platform wrapper).
  status             Generate workspace status page (docgen).

Pass additional options after the command; they are forwarded to the underlying script.
'@
}

function Resolve-Tool {
  param([string[]]$Names)
  foreach ($name in $Names) {
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd }
  }
  return $null
}

# Preflight: summarize environment mode for users and agents
try {
  $bash = Resolve-Tool @((Join-Path ${env:ProgramFiles} 'Git\bin\bash.exe'), 'bash')
  if ($bash) {
    & $bash.Source (Join-Path $RepoRoot 'scripts\env\status.sh')
  } else {
    $arwEnvPath = Join-Path $RepoRoot '.arw-env'
    $mode = 'unknown'
    if (Test-Path $arwEnvPath) {
      $line = (Get-Content $arwEnvPath | Where-Object { $_ -like 'MODE=*' } | Select-Object -First 1)
      if ($line) { $mode = $line.Split('=')[1] }
    }
    Write-Host ('[env] mode={0} (bash unavailable; install Git Bash for full checks)' -f $mode)
  }
} catch {
  Write-Warning ('[env] status unavailable: {0}' -f $_.Exception.Message)
}

function Contains-Switch {
  param([string[]]$Values, [string[]]$Switches)
  if (-not $Values) { return $false }
  foreach ($value in $Values) {
    if ([string]::IsNullOrWhiteSpace($value)) { continue }
    $trimmed = $value.TrimStart('-', '/')
    $trimmed = ($trimmed.Split('=')[0]).ToLowerInvariant()
    foreach ($switch in $Switches) {
      if ($trimmed -eq $switch.ToLowerInvariant()) { return $true }
    }
  }
  return $false
}

function Invoke-Program {
  param(
    [Parameter(Mandatory = $true)][System.Management.Automation.CommandInfo]$Executable,
    [Parameter()][string[]]$Arguments
  )
  & $Executable.Source @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "Command '$($Executable.Name)' exited with code $LASTEXITCODE"
  }
}

function Invoke-Step {
  param(
    [string]$Name,
    [ScriptBlock]$Action,
    [bool]$Required = $true,
    [ScriptBlock]$ShouldRun = $null,
    [string]$SkipReason = ''
  )
  if ($ShouldRun -ne $null -and -not (& $ShouldRun)) {
    return [pscustomobject]@{
      Name    = $Name
      Status  = 'skipped'
      Message = if ($SkipReason) { $SkipReason } else { 'prerequisite unavailable' }
    }
  }
  try {
    & $Action | Out-Null
    return [pscustomobject]@{
      Name    = $Name
      Status  = 'ok'
      Message = ''
    }
  } catch {
    return [pscustomobject]@{
      Name    = $Name
      Status  = if ($Required) { 'failed' } else { 'warn' }
      Message = $_.Exception.Message
    }
  }
}

function Invoke-Verify {
  param(
    [switch]$Fast,
    [switch]$SkipDocs,
    [switch]$SkipUI,
    [switch]$SkipDocPython,
    [switch]$WithLauncher,
    [switch]$Ci
  )

  $fastMode = $Fast.IsPresent -and -not $Ci.IsPresent
  if ($Fast.IsPresent -and $Ci.IsPresent) {
    Write-Host "[verify] -Ci overrides -Fast; running full suite."
  }
  if ($Ci.IsPresent) {
    $SkipDocs = $false
    $SkipUI = $false
    $SkipDocPython = $false
  } elseif ($fastMode) {
    $SkipDocs = $true
    $SkipUI = $true
    $SkipDocPython = $true
  }

  $results = @()
  $cargo = Resolve-Tool @('cargo')
  $nextest = Resolve-Tool @('cargo-nextest')
  $node = Resolve-Tool @(
    'node',
    (Join-Path ${env:ProgramFiles} 'nodejs\node.exe')
  )
  $mkdocs = Resolve-Tool @(
    (Join-Path $RepoRoot '.venv\Scripts\mkdocs.exe'),
    (Join-Path $RepoRoot '.venv/bin/mkdocs'),
    'mkdocs'
  )
  $python = Resolve-Tool @(
    (Join-Path $RepoRoot '.venv\Scripts\python.exe'),
    (Join-Path $RepoRoot '.venv/bin/python'),
    'python','python3'
  )
  $pythonHasYaml = $false
  if ($python) {
    try {
      & $python -c "import yaml" | Out-Null
      if ($LASTEXITCODE -eq 0) { $pythonHasYaml = $true }
    } catch {
      $pythonHasYaml = $false
    }
  }
  $bash = Resolve-Tool @((Join-Path ${env:ProgramFiles} 'Git\bin\bash.exe'), 'bash')

  if (-not $cargo) {
    throw "cargo not found in PATH. Install Rust toolchain via https://rustup.rs"
  }

  # Include launcher by default; allow opt-out via env
  $includeLauncher = $true
  if ($env:ARW_VERIFY_EXCLUDE_LAUNCHER -match '^(1|true|yes)$') { $includeLauncher = $false }
  if ($WithLauncher.IsPresent) { $includeLauncher = $true }
  if ($includeLauncher) {
    Write-Host "[verify] including arw-launcher targets (default)"
  } else {
    Write-Host "[verify] excluding arw-launcher crate (set ARW_VERIFY_EXCLUDE_LAUNCHER=1 to disable)"
  }
  $requireDocs = $Ci.IsPresent
  if ($env:ARW_VERIFY_REQUIRE_DOCS -match '^(1|true|yes)$') {
    $requireDocs = $true
  }
  if ($fastMode) {
    Write-Host "[verify] fast mode enabled (skipping doc sync, docs lint, launcher UI tests)."
  }

  $results += Invoke-Step -Name 'cargo fmt --all -- --check' -Action {
    Invoke-Program -Executable $cargo -Arguments @('fmt','--all','--','--check')
  }

  $clippyArgs = @('clippy','--workspace','--all-targets')
  if (-not $includeLauncher) { $clippyArgs += @('--exclude','arw-launcher') }
  $clippyArgs += @('--','-D','warnings')
  $results += Invoke-Step -Name ("cargo " + ($clippyArgs -join ' ')) -Action {
    Invoke-Program -Executable $cargo -Arguments $clippyArgs
  }

  $testArgs = $null
  $testStepName = $null
  if ($nextest) {
    $testArgs = @('nextest','run','--workspace')
    if (-not $includeLauncher) { $testArgs += @('--exclude','arw-launcher') }
    if ($Ci.IsPresent) { $testArgs += @('--no-fail-fast') }
    $testStepName = "cargo $($testArgs -join ' ')"
  } else {
    Write-Warning 'cargo-nextest not found; falling back to cargo test --workspace --locked.'
    $testArgs = @('test','--workspace','--locked')
    if (-not $includeLauncher) { $testArgs += @('--exclude','arw-launcher') }
    # Limit test binary threads to reduce flakiness in fallback mode
    $testArgs += @('--','--test-threads=1')
    $testStepName = "cargo $($testArgs -join ' ')"
  }
  $results += Invoke-Step -Name $testStepName -Action {
    Invoke-Program -Executable $cargo -Arguments $testArgs
  }

  if ($SkipUI.IsPresent) {
    $results += [pscustomobject]@{
      Name    = 'launcher UI smoke tests'
      Status  = 'skipped'
      Message = 'launcher UI checks disabled (--fast/-SkipUI)'
    }
  } elseif (-not $includeLauncher) {
    $results += [pscustomobject]@{
      Name    = 'launcher UI smoke tests'
      Status  = 'skipped'
      Message = 'headless default; pass -WithLauncher to include'
    }
  } elseif ($null -eq $node) {
    $results += [pscustomobject]@{
      Name    = 'launcher UI smoke tests'
      Status  = 'skipped'
      Message = 'Node.js 18+ not found; install Node.js or pass -SkipUI/-Fast to suppress'
    }
  } else {
    $results += Invoke-Step -Name 'node read_store.test.js' -Action {
      $testPath = Join-Path $RepoRoot 'apps\arw-launcher\src-tauri\ui\read_store.test.js'
      Invoke-Program -Executable $node -Arguments @($testPath)
    }
    $results += Invoke-Step -Name 'node persona_preview.test.js' -Action {
      $testPath = Join-Path $RepoRoot 'apps\arw-launcher\src-tauri\ui\persona_preview.test.js'
      Invoke-Program -Executable $node -Arguments @($testPath)
    }
  }

  # TypeScript client build (optional; skips in -Fast)
  if ($Fast.IsPresent) {
    $results += [pscustomobject]@{
      Name    = 'clients/typescript build'
      Status  = 'skipped'
      Message = 'skipped in -Fast mode'
    }
  } else {
    $npm = Resolve-Tool @('npm')
    $node = if ($node) { $node } else { Resolve-Tool @('node') }
    if ($npm -and $node) {
      $results += Invoke-Step -Name 'clients/typescript build' -Action {
        Push-Location (Join-Path $RepoRoot 'clients/typescript')
        try {
          $lockPath = Join-Path (Get-Location) 'package-lock.json'
          if (Test-Path $lockPath) {
            Invoke-Program -Executable $npm -Arguments @('ci','--no-audit','--no-fund')
          } else {
            Invoke-Program -Executable $npm -Arguments @('install','--no-audit','--no-fund')
          }
          Invoke-Program -Executable $npm -Arguments @('run','build')
        } finally {
          Pop-Location
        }
      }
    } else {
      $results += [pscustomobject]@{
        Name    = 'clients/typescript build'
        Status  = 'skipped'
        Message = 'Node.js/npm not found'
      }
    }
  }

  $docStepName1 = 'python check_operation_docs_sync.py'
  $docStepName2 = 'python gen_topics_doc.py --check'
  if ($SkipDocPython.IsPresent) {
    $message = 'doc sync checks disabled (--fast/-SkipDocPython)'
    $results += [pscustomobject]@{ Name = $docStepName1; Status = 'skipped'; Message = $message }
    $results += [pscustomobject]@{ Name = $docStepName2; Status = 'skipped'; Message = $message }
  } elseif ($null -eq $python) {
    if ($requireDocs) {
      $message = 'Python 3.11+ not found in PATH'
      $results += [pscustomobject]@{ Name = $docStepName1; Status = 'failed'; Message = $message }
      $results += [pscustomobject]@{ Name = $docStepName2; Status = 'failed'; Message = $message }
    } else {
      $message = 'python not found; set ARW_VERIFY_REQUIRE_DOCS=1 to require doc tooling'
      $results += [pscustomobject]@{ Name = $docStepName1; Status = 'skipped'; Message = $message }
      $results += [pscustomobject]@{ Name = $docStepName2; Status = 'skipped'; Message = $message }
    }
  } elseif (-not $pythonHasYaml) {
    if ($requireDocs) {
      $message = ('PyYAML missing for {0}; install with `python3 -m pip install --user --break-system-packages pyyaml`' -f $python.Name)
      $results += [pscustomobject]@{ Name = $docStepName1; Status = 'failed'; Message = $message }
      $results += [pscustomobject]@{ Name = $docStepName2; Status = 'failed'; Message = $message }
    } else {
      $message = 'PyYAML missing; set ARW_VERIFY_REQUIRE_DOCS=1 to require doc tooling'
      $results += [pscustomobject]@{ Name = $docStepName1; Status = 'skipped'; Message = $message }
      $results += [pscustomobject]@{ Name = $docStepName2; Status = 'skipped'; Message = $message }
    }
  } else {
    $results += Invoke-Step -Name $docStepName1 -Action {
      $previousEncoding = $env:PYTHONIOENCODING
      try {
        $env:PYTHONIOENCODING = 'utf-8'
        $scriptPath = Join-Path $RepoRoot 'scripts\check_operation_docs_sync.py'
        Invoke-Program -Executable $python -Arguments @($scriptPath)
      } finally {
        if ($null -eq $previousEncoding) {
          Remove-Item Env:PYTHONIOENCODING -ErrorAction SilentlyContinue
        } else {
          $env:PYTHONIOENCODING = $previousEncoding
        }
      }
    } -Required:$requireDocs

    $results += Invoke-Step -Name $docStepName2 -Action {
      $scriptPath = Join-Path $RepoRoot 'scripts\gen_topics_doc.py'
      Invoke-Program -Executable $python -Arguments @($scriptPath,'--check')
    } -Required:$requireDocs
  }

  if ($null -eq $python) {
    if ($requireDocs) {
      $results += [pscustomobject]@{
        Name    = 'python lint_event_names.py'
        Status  = 'failed'
        Message = 'Python 3.11+ not found in PATH'
      }
    } else {
      $results += [pscustomobject]@{
        Name    = 'python lint_event_names.py'
        Status  = 'skipped'
        Message = 'python not found; set ARW_VERIFY_REQUIRE_DOCS=1 to require doc tooling'
      }
    }
  } else {
    $results += Invoke-Step -Name 'python lint_event_names.py' -Action {
      $scriptPath = Join-Path $RepoRoot 'scripts\lint_event_names.py'
      Invoke-Program -Executable $python -Arguments @($scriptPath)
    }
  }

  # Run doc generation before mkdocs when docs are enabled
  $docgenArgs = @()
  if (-not $Ci.IsPresent) { $docgenArgs += @('--skip-builds') }
  $docgenSkipReason = if ($SkipDocs) { 'doc generation disabled (--fast/--skip-docs)' } else { 'Docs toolchain missing; pass -SkipDocs/-Fast to skip' }
  $results += Invoke-Step -Name ('docgen ' + ($docgenArgs -join ' ')) -Action {
    & (Join-Path $ScriptRoot 'docgen.ps1') @docgenArgs
  } -Required:$requireDocs -ShouldRun { -not $SkipDocs } -SkipReason $docgenSkipReason

  $docsSkipReason = if ($SkipDocs) { 'docs lint disabled (--fast/--skip-docs)' } else { 'Docs toolchain missing; install via venv then pass -SkipDocs/-Fast to skip' }
  $results += Invoke-Step -Name 'docs lint' -Action {
    $docsScript = Join-Path $RepoRoot 'scripts\docs_check.py'
    if (Test-Path $docsScript) {
      $pythonLocal = Resolve-Tool @(
        (Join-Path $RepoRoot '.venv\Scripts\python.exe'),
        (Join-Path $RepoRoot '.venv/bin/python'),
        'python3','python'
      )
      if ($pythonLocal) {
        Invoke-Program -Executable $pythonLocal -Arguments @($docsScript)
        return
      }
    }
    if ($mkdocs) {
      Invoke-Program -Executable $mkdocs -Arguments @('build','--strict','-f', (Join-Path $RepoRoot 'mkdocs.yml'))
      return
    }
    throw 'docs_check.py and mkdocs not available'
  } -Required:$true -ShouldRun { -not $SkipDocs } -SkipReason $docsSkipReason

  if ($Ci.IsPresent) {
    Write-Host '[verify] CI mode enabled (running extended guardrails).'
    if ($null -eq $python) {
      $results += [pscustomobject]@{
        Name    = 'python check_feature_integrity.py'
        Status  = 'failed'
        Message = 'Python 3.11+ not found in PATH'
      }
    } else {
      $results += Invoke-Step -Name 'python check_feature_integrity.py' -Action {
        $scriptPath = Join-Path $RepoRoot 'scripts\check_feature_integrity.py'
        Invoke-Program -Executable $python -Arguments @($scriptPath)
      }

      $results += Invoke-Step -Name 'python check_system_components_integrity.py' -Action {
        $scriptPath = Join-Path $RepoRoot 'scripts\check_system_components_integrity.py'
        Invoke-Program -Executable $python -Arguments @($scriptPath)
      }

      foreach ($regen in @(
          'scripts\gen_feature_matrix.py',
          'scripts\gen_feature_catalog.py',
          'scripts\gen_system_components.py'
        )) {
        $results += Invoke-Step -Name ('python {0} --check' -f $regen) -Action {
          $scriptPath = Join-Path $RepoRoot $regen
          Invoke-Program -Executable $python -Arguments @($scriptPath,'--check')
        }
      }
    }

    if ($null -eq $bash) {
      $results += [pscustomobject]@{
        Name    = 'bash check_env_guard.sh'
        Status  = 'failed'
        Message = 'Git Bash not found; install Git Bash to run CI guardrails'
      }
    } else {
      $results += Invoke-Step -Name 'bash check_env_guard.sh' -Action {
        $previous = $env:ENFORCE_ENV_GUARD
        try {
          $env:ENFORCE_ENV_GUARD = '1'
          & $bash.Source (Join-Path $RepoRoot 'scripts\check_env_guard.sh')
          if ($LASTEXITCODE -ne 0) {
            throw ('check_env_guard.sh exited with {0}' -f $LASTEXITCODE)
          }
        } finally {
          if ($null -eq $previous) {
            Remove-Item Env:ENFORCE_ENV_GUARD -ErrorAction SilentlyContinue
          } else {
            $env:ENFORCE_ENV_GUARD = $previous
          }
        }
      }

      $results += Invoke-Step -Name 'ci snappy bench' -Action {
        $python = Resolve-Tool @('python3', 'python')
        if (-not $python) {
          throw 'python not found; install Python 3 to run ci_snappy_bench.'
        }
        & $python.Source (Join-Path $RepoRoot 'scripts\ci_snappy_bench.py')
        if ($LASTEXITCODE -ne 0) {
          throw ('ci_snappy_bench.py exited with {0}' -f $LASTEXITCODE)
        }
      }

      $results += Invoke-Step -Name 'triad smoke' -Action {
        if ($env:OS -eq 'Windows_NT' -and (Test-Path (Join-Path $RepoRoot 'scripts\smoke_triad.ps1'))) {
          & (Join-Path $RepoRoot 'scripts\smoke_triad.ps1')
        } else {
          & $bash.Source (Join-Path $RepoRoot 'scripts\triad_smoke.sh')
          if ($LASTEXITCODE -ne 0) { throw ('triad_smoke.sh exited with {0}' -f $LASTEXITCODE) }
        }
      }

      $results += Invoke-Step -Name 'context telemetry check' -Action {
        $python = Resolve-Tool @('python3', 'python')
        if ($python) {
          & $python.Source (Join-Path $RepoRoot 'scripts\context_ci.py')
          if ($LASTEXITCODE -ne 0) { throw ('context_ci.py exited with {0}' -f $LASTEXITCODE) }
        } elseif ($env:OS -eq 'Windows_NT' -and (Test-Path (Join-Path $RepoRoot 'scripts\smoke_context.ps1'))) {
          & (Join-Path $RepoRoot 'scripts\smoke_context.ps1')
        } else {
          throw 'python not found; install Python 3 to run context_ci.'
        }
      }

      $results += Invoke-Step -Name 'bash runtime_llama_smoke.sh (MODE=stub)' -Action {
        $previousMode = $env:MODE
        try {
          $env:MODE = 'stub'
          & $bash.Source (Join-Path $RepoRoot 'scripts\runtime_llama_smoke.sh')
          if ($LASTEXITCODE -ne 0) {
            throw ('runtime_llama_smoke.sh exited with {0}' -f $LASTEXITCODE)
          }
        } finally {
          if ($null -eq $previousMode) {
            Remove-Item Env:MODE -ErrorAction SilentlyContinue
          } else {
            $env:MODE = $previousMode
          }
        }
      }

      $results += Invoke-Step -Name 'bash check_legacy_surface.sh' -Action {
        $previousWait = $env:ARW_LEGACY_CHECK_WAIT_SECS
        try {
          $env:ARW_LEGACY_CHECK_WAIT_SECS = '30'
          & $bash.Source (Join-Path $RepoRoot 'scripts\check_legacy_surface.sh')
          if ($LASTEXITCODE -ne 0) {
            throw ('check_legacy_surface.sh exited with {0}' -f $LASTEXITCODE)
          }
        } finally {
          if ($null -eq $previousWait) {
            Remove-Item Env:ARW_LEGACY_CHECK_WAIT_SECS -ErrorAction SilentlyContinue
          } else {
            $env:ARW_LEGACY_CHECK_WAIT_SECS = $previousWait
          }
        }
      }
    }
  }

  $hasFailure = $false
  foreach ($result in $results) {
    if ($null -eq $result) { continue }
    if (-not ($result | Get-Member -Name Status -ErrorAction SilentlyContinue)) {
      Write-Host ('[warn] Unexpected step result type: {0}' -f $result.GetType().FullName) -ForegroundColor Yellow
      Write-Host ($result | Out-String)
      continue
    }
    switch ($result.Status) {
      'ok' {
        Write-Host ('[ok] {0}' -f $result.Name) -ForegroundColor Green
      }
      'skipped' {
        Write-Host ('[skip] {0} - {1}' -f $result.Name, $result.Message) -ForegroundColor Yellow
      }
      'warn' {
        Write-Host ('[warn] {0} - {1}' -f $result.Name, $result.Message) -ForegroundColor Yellow
      }
      'failed' {
        Write-Host ('[fail] {0} - {1}' -f $result.Name, $result.Message) -ForegroundColor Red
        $hasFailure = $true
      }
    }
  }

  if ($hasFailure) {
    throw 'Verification failed. Review the [fail] entries above.'
  }
}

$commandKey = $Command.ToLowerInvariant()
switch ($commandKey) {
  'help' {
    Show-Help
  }
  'setup' {
    $defaults = @()
    $hasYes = Contains-Switch -Values $Args -Switches @('yes')
    $hasHeadless = Contains-Switch -Values $Args -Switches @('headless')
    $hasWithLauncher = Contains-Switch -Values $Args -Switches @('withlauncher')
    if (-not $hasYes) { $defaults += '-Yes' }
    if (-not $hasHeadless -and -not $hasWithLauncher) { $defaults += '-Headless' }
    & (Join-Path $ScriptRoot 'setup.ps1') @defaults @Args
  }
  'setup-agent' {
    $previousDocgen = $env:ARW_DOCGEN_SKIP_BUILDS
    $previousBuildMode = $env:ARW_BUILD_MODE
    $previousAgentFlag = $env:ARW_SETUP_AGENT
    try {
      $env:ARW_DOCGEN_SKIP_BUILDS = '1'
      $env:ARW_BUILD_MODE = 'debug'
      $env:ARW_SETUP_AGENT = '1'
      & (Join-Path $ScriptRoot 'setup.ps1') -Headless -Minimal -NoDocs -SkipCli -Yes @Args
    } finally {
      if ($null -eq $previousDocgen) {
        Remove-Item Env:ARW_DOCGEN_SKIP_BUILDS -ErrorAction SilentlyContinue
      } else {
        $env:ARW_DOCGEN_SKIP_BUILDS = $previousDocgen
      }
      if ($null -eq $previousBuildMode) {
        Remove-Item Env:ARW_BUILD_MODE -ErrorAction SilentlyContinue
      } else {
        $env:ARW_BUILD_MODE = $previousBuildMode
      }
      if ($null -eq $previousAgentFlag) {
        Remove-Item Env:ARW_SETUP_AGENT -ErrorAction SilentlyContinue
      } else {
        $env:ARW_SETUP_AGENT = $previousAgentFlag
      }
    }
  }
  'build' {
    $defaults = @()
    $hasHeadless = Contains-Switch -Values $Args -Switches @('headless')
    $hasWithLauncher = Contains-Switch -Values $Args -Switches @('withlauncher')
    if (-not $hasHeadless -and -not $hasWithLauncher) { $defaults += '-Headless' }
    & (Join-Path $ScriptRoot 'build.ps1') @defaults @Args
  }
  'build-launcher' {
    & (Join-Path $ScriptRoot 'build.ps1') '-WithLauncher' @Args
  }
  'clean' {
    & (Join-Path $ScriptRoot 'clean.ps1') @Args
  }
  'fmt' {
    $cargo = Resolve-Tool @('cargo')
    if (-not $cargo) { throw 'cargo not found in PATH.' }
    Invoke-Program -Executable $cargo -Arguments @('fmt','--all')
  }
  'fmt-check' {
    $cargo = Resolve-Tool @('cargo')
    if (-not $cargo) { throw 'cargo not found in PATH.' }
    Invoke-Program -Executable $cargo -Arguments @('fmt','--all','--','--check')
  }
  'lint' {
    $cargo = Resolve-Tool @('cargo')
    if (-not $cargo) { throw 'cargo not found in PATH.' }
    Invoke-Program -Executable $cargo -Arguments @('clippy','--workspace','--all-targets','--','-D','warnings')
  }
  'lint-fix' {
    $cargo = Resolve-Tool @('cargo')
    if (-not $cargo) { throw 'cargo not found in PATH.' }
    Invoke-Program -Executable $cargo -Arguments @('clippy','--workspace','--all-targets','--fix','-Z','unstable-options','--allow-dirty','--allow-staged')
  }
  'lint-events' {
    $python = Resolve-Tool @(
      (Join-Path $RepoRoot '.venv\Scripts\python.exe'),
      (Join-Path $RepoRoot '.venv/bin/python'),
      'python','python3'
    )
    if (-not $python) { throw 'python not found; install Python 3.11+ to lint events.' }
    Invoke-Program -Executable $python -Arguments @((Join-Path $RepoRoot 'scripts\lint_event_names.py'))
  }
  'test' {
    & (Join-Path $ScriptRoot 'test.ps1') @Args
  }
  'test-fast' {
    $cargo = Resolve-Tool @('cargo')
    $nextest = Resolve-Tool @('cargo-nextest')
    if ($cargo -and $nextest) {
      # Prefer invoking as `cargo nextest run` (official interface)
      Invoke-Program -Executable $cargo -Arguments @('nextest','run','--workspace')
    } else {
      Write-Warning 'cargo-nextest not found; falling back to cargo test --workspace --locked.'
      if (-not $cargo) { throw 'cargo not found in PATH.' }
      Invoke-Program -Executable $cargo -Arguments @('test','--workspace','--locked','--','--test-threads=1')
    }
  }
  'docs' {
    & (Join-Path $ScriptRoot 'docgen.ps1') @Args
    $mkdocs = Resolve-Tool @(
      (Join-Path $RepoRoot '.venv\Scripts\mkdocs.exe'),
      (Join-Path $RepoRoot '.venv/bin/mkdocs'),
      'mkdocs'
    )
    if ($mkdocs) {
      Invoke-Program -Executable $mkdocs -Arguments @('build','--strict','-f',(Join-Path $RepoRoot 'mkdocs.yml'))
    } else {
      Write-Warning 'mkdocs not found; skipping mkdocs build. Install via `pip install mkdocs-material`.'
    }
  }
  'docs-check' {
    & (Join-Path $ScriptRoot 'docgen.ps1') @Args
    $docsScript = Join-Path $RepoRoot 'scripts\docs_check.py'
    if (Test-Path $docsScript) {
      $pythonDocs = Resolve-Tool @(
        (Join-Path $RepoRoot '.venv\Scripts\python.exe'),
        (Join-Path $RepoRoot '.venv/bin/python'),
        'python3','python'
      )
      if ($pythonDocs) {
        Invoke-Program -Executable $pythonDocs -Arguments @($docsScript)
        return
      }
    }
    $mkdocs = Resolve-Tool @(
      (Join-Path $RepoRoot '.venv\Scripts\mkdocs.exe'),
      (Join-Path $RepoRoot '.venv/bin/mkdocs'),
      'mkdocs'
    )
    if ($mkdocs) {
      Write-Warning 'Python not available; running mkdocs build --strict as a lightweight docs check.'
      Invoke-Program -Executable $mkdocs -Arguments @('build','--strict','-f',(Join-Path $RepoRoot 'mkdocs.yml'))
    } else {
      Write-Warning 'Docs checks skipped (missing docs_check.py & mkdocs). Install the docs toolchain or run with -SkipDocs.'
    }
  }
  'docs-cache' {
    $bashCandidates = @('bash')
    if (-not [string]::IsNullOrEmpty($env:ProgramFiles)) {
      $bashCandidates += (Join-Path $env:ProgramFiles 'Git\bin\bash.exe')
    }
    $bash = Resolve-Tool $bashCandidates
    if (-not $bash) {
      throw 'bash not found; install Git for Windows (provides bash) or run this command from a Unix-like shell.'
    }
    $archive = Join-Path $RepoRoot 'dist/docs-wheels.tar.gz'
    $script = (Join-Path $ScriptRoot 'build_docs_wheels.sh').Replace('\\','/')
    & $bash.Source $script '--archive' $archive @Args
  }
  'verify' {
    $recognized = @('fast','skip-docs','skip-ui','skip-doc-python','with-launcher','ci')
    $fast = Contains-Switch -Values $Args -Switches @('fast')
    $skipDocs = Contains-Switch -Values $Args -Switches @('skip-docs')
    $skipUI = Contains-Switch -Values $Args -Switches @('skip-ui')
    $skipDocPython = Contains-Switch -Values $Args -Switches @('skip-doc-python')
    $withLauncher = Contains-Switch -Values $Args -Switches @('with-launcher')
    $ci = Contains-Switch -Values $Args -Switches @('ci')
    $unknown = @()
    foreach ($arg in $Args) {
      if ($arg -like '-*') {
        $trimmed = $arg.TrimStart('-', '/')
        $trimmed = ($trimmed.Split('=')[0]).ToLowerInvariant()
        if (-not $recognized.Contains($trimmed)) {
          $unknown += $arg
        }
      } else {
        $unknown += $arg
      }
    }
    if ($unknown.Count -gt 0) {
      throw ('Unknown verify option(s): {0}' -f ($unknown -join ', '))
    }
    Invoke-Verify -Fast:$fast -SkipDocs:$skipDocs -SkipUI:$skipUI -SkipDocPython:$skipDocPython -WithLauncher:$withLauncher -Ci:$ci
  }
  'hooks' {
    & (Join-Path $ScriptRoot 'hooks' 'install_hooks.ps1') @Args
  }
  'status' {
    & (Join-Path $ScriptRoot 'docgen.ps1') @Args
  }
  default {
    Write-Error ('Unknown command ''{0}''. Run ''pwsh -File scripts/dev.ps1 help'' for usage.' -f $Command)
    exit 1
  }
}
