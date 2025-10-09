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
  verify             Run the standard fmt → clippy → tests → docs guardrail sequence.
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
  $results = @()
  $cargo = Resolve-Tool @('cargo')
  $nextest = Resolve-Tool @('cargo-nextest')
  $node = Resolve-Tool @(
    'node',
    (Join-Path ${env:ProgramFiles} 'nodejs\node.exe')
  )
  $mkdocs = Resolve-Tool @('mkdocs', (Join-Path $RepoRoot '.venv\Scripts\mkdocs.exe'))
  $python = Resolve-Tool @('python','python3')
  $bash = Resolve-Tool @((Join-Path ${env:ProgramFiles} 'Git\bin\bash.exe'), 'bash')

  if (-not $cargo) {
    throw "cargo not found in PATH. Install Rust toolchain via https://rustup.rs"
  }

  $results += Invoke-Step -Name 'cargo fmt --all -- --check' -Action {
    Invoke-Program -Executable $cargo -Arguments @('fmt','--all','--','--check')
  }

  $results += Invoke-Step -Name 'cargo clippy --workspace --all-targets -- -D warnings' -Action {
    Invoke-Program -Executable $cargo -Arguments @('clippy','--workspace','--all-targets','--','-D','warnings')
  }

  $testStepName = if ($nextest) { 'cargo nextest run --workspace' } else { 'cargo test --workspace --locked' }
  $results += Invoke-Step -Name $testStepName -Action {
    if ($nextest) {
      Invoke-Program -Executable $cargo -Arguments @('nextest','run','--workspace')
    } else {
      Invoke-Program -Executable $cargo -Arguments @('test','--workspace','--locked')
    }
  }

  $results += Invoke-Step -Name 'node read_store.test.js' -Action {
    $testPath = Join-Path $RepoRoot 'apps\arw-launcher\src-tauri\ui\read_store.test.js'
    Invoke-Program -Executable $node -Arguments @($testPath)
  } -Required:$false -ShouldRun { $null -ne $node } -SkipReason 'node not found; skipping UI store test'

  $results += Invoke-Step -Name 'python check_operation_docs_sync.py' -Action {
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
  } -Required:$false -ShouldRun { $null -ne $python } -SkipReason 'python not found; skipping operation docs sync check'

  $results += Invoke-Step -Name 'python gen_topics_doc.py --check' -Action {
    $scriptPath = Join-Path $RepoRoot 'scripts\gen_topics_doc.py'
    Invoke-Program -Executable $python -Arguments @($scriptPath,'--check')
  } -Required:$false -ShouldRun { $null -ne $python } -SkipReason 'python not found; skipping topics doc check'

  $results += Invoke-Step -Name 'python lint_event_names.py' -Action {
    $scriptPath = Join-Path $RepoRoot 'scripts\lint_event_names.py'
    Invoke-Program -Executable $python -Arguments @($scriptPath)
  } -Required:$false -ShouldRun { $null -ne $python } -SkipReason 'python not found; skipping event-name lint'

  $results += Invoke-Step -Name 'docs_check.sh' -Action {
    $scriptPath = (Join-Path $RepoRoot 'scripts/docs_check.sh').Replace('\','/')
    & $bash.Source $scriptPath
  } -Required:$false -ShouldRun {
    ($null -ne $bash) -and (Test-Path (Join-Path $RepoRoot 'scripts\docs_check.sh'))
  } -SkipReason 'bash or docs_check.sh unavailable; skipping docs lint'

  $results += Invoke-Step -Name 'mkdocs build --strict' -Action {
    Invoke-Program -Executable $mkdocs -Arguments @('build','--strict','-f',(Join-Path $RepoRoot 'mkdocs.yml'))
  } -Required:$false -ShouldRun { $null -ne $mkdocs } -SkipReason 'mkdocs not found; install via pip install mkdocs-material'

  $hasFailure = $false
  foreach ($result in $results) {
    if ($null -eq $result) { continue }
    if (-not ($result | Get-Member -Name Status -ErrorAction SilentlyContinue)) {
      Write-Host "[warn] Unexpected step result type: $($result.GetType().FullName)" -ForegroundColor Yellow
      Write-Host ($result | Out-String)
      continue
    }
    switch ($result.Status) {
      'ok' {
        Write-Host "[ok] $($result.Name)" -ForegroundColor Green
      }
      'skipped' {
        Write-Host "[skip] $($result.Name) — $($result.Message)" -ForegroundColor Yellow
      }
      'warn' {
        Write-Host "[warn] $($result.Name) — $($result.Message)" -ForegroundColor Yellow
      }
      'failed' {
        Write-Host "[fail] $($result.Name) — $($result.Message)" -ForegroundColor Red
        $hasFailure = $true
      }
    }
  }

  if ($hasFailure) {
    throw "Verification failed. Review the [fail] entries above."
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
    $previous = $env:ARW_DOCGEN_SKIP_BUILDS
    try {
      $env:ARW_DOCGEN_SKIP_BUILDS = '1'
      & (Join-Path $ScriptRoot 'setup.ps1') -Headless -Minimal -NoDocs -Yes @Args
    } finally {
      if ($null -eq $previous) {
        Remove-Item Env:ARW_DOCGEN_SKIP_BUILDS -ErrorAction SilentlyContinue
      } else {
        $env:ARW_DOCGEN_SKIP_BUILDS = $previous
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
    $python = Resolve-Tool @('python','python3')
    if (-not $python) { throw 'python not found; install Python 3.11+ to lint events.' }
    Invoke-Program -Executable $python -Arguments @((Join-Path $RepoRoot 'scripts\lint_event_names.py'))
  }
  'test' {
    & (Join-Path $ScriptRoot 'test.ps1') @Args
  }
  'test-fast' {
    $nextest = Resolve-Tool @('cargo-nextest')
    if ($nextest) {
      Invoke-Program -Executable $nextest -Arguments @('run','--workspace')
    } else {
      Write-Warning 'cargo-nextest not found; falling back to cargo test --workspace --locked.'
      $cargo = Resolve-Tool @('cargo')
      if (-not $cargo) { throw 'cargo not found in PATH.' }
      Invoke-Program -Executable $cargo -Arguments @('test','--workspace','--locked')
    }
  }
  'docs' {
    & (Join-Path $ScriptRoot 'docgen.ps1') @Args
    $mkdocs = Resolve-Tool @('mkdocs', (Join-Path $RepoRoot '.venv\Scripts\mkdocs.exe'))
    if ($mkdocs) {
      Invoke-Program -Executable $mkdocs -Arguments @('build','--strict','-f',(Join-Path $RepoRoot 'mkdocs.yml'))
    } else {
      Write-Warning 'mkdocs not found; skipping mkdocs build. Install via `pip install mkdocs-material`.'
    }
  }
  'docs-check' {
    & (Join-Path $ScriptRoot 'docgen.ps1') @Args
    $bash = Resolve-Tool @('bash')
    if ($bash -and (Test-Path (Join-Path $RepoRoot 'scripts\docs_check.sh'))) {
      $scriptPath = (Join-Path $RepoRoot 'scripts/docs_check.sh').Replace('\\','/')
      & $bash.Source $scriptPath
    } else {
      $mkdocs = Resolve-Tool @('mkdocs', (Join-Path $RepoRoot '.venv\Scripts\mkdocs.exe'))
      if ($mkdocs) {
        Write-Warning 'bash not available; running mkdocs build --strict as a lightweight docs check.'
        Invoke-Program -Executable $mkdocs -Arguments @('build','--strict','-f',(Join-Path $RepoRoot 'mkdocs.yml'))
      } else {
        Write-Warning 'Docs checks skipped (missing bash/mkdocs). Install Git Bash or MkDocs to enable full validation.'
      }
    }
  }
  'verify' {
    Invoke-Verify
  }
  'hooks' {
    & (Join-Path $ScriptRoot 'hooks' 'install_hooks.ps1') @Args
  }
  'status' {
    & (Join-Path $ScriptRoot 'docgen.ps1') @Args
  }
  default {
    Write-Error "Unknown command '$Command'. Run 'pwsh -File scripts/dev.ps1 help' for usage."
    exit 1
  }
}
