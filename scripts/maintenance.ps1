#requires -Version 7.0
param(
    [switch] $DryRun,
    [int] $KeepLogsDays = 7,
    [string] $StateDir,
    [ValidateSet('private','shared','public')]
    [string] $PointerConsent = 'private',
    [string[]] $Tasks
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:RepoRoot = Split-Path -Path $PSScriptRoot -Parent
Set-Location $RepoRoot

function Write-Info($Message) {
    Write-Host "[maintenance] $Message"
}

function Invoke-Step {
    param (
        [scriptblock] $Action,
        [string] $Description
    )
    if ($DryRun.IsPresent) {
        Write-Info "dry-run: $Description"
    } else {
        Write-Info "running: $Description"
        & $Action
    }
}

function Remove-ItemSafe([string] $Target) {
    if (-not (Test-Path -LiteralPath $Target)) {
        return
    }
    if ($DryRun.IsPresent) {
        Write-Info "dry-run: Remove-Item $Target"
    } else {
        Remove-Item -LiteralPath $Target -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Get-PythonCommand {
    foreach ($candidate in 'python', 'python3') {
        if (Get-Command $candidate -ErrorAction SilentlyContinue) {
            return $candidate
        }
    }
    return $null
}

function Invoke-External {
    param(
        [string] $Command,
        [string[]] $Arguments
    )
    if ($DryRun.IsPresent) {
        Write-Info "dry-run: $Command $($Arguments -join ' ')"
    } else {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $Command
        $psi.WorkingDirectory = $RepoRoot
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.UseShellExecute = $false
        foreach ($arg in $Arguments) {
            $null = $psi.ArgumentList.Add($arg)
        }
        $process = [System.Diagnostics.Process]::Start($psi)
        $stdout = $process.StandardOutput.ReadToEnd()
        $stderr = $process.StandardError.ReadToEnd()
        $process.WaitForExit()
        if ($stdout) { Write-Host $stdout.TrimEnd() }
        if ($stderr) { Write-Error $stderr.TrimEnd() }
        if ($process.ExitCode -ne 0) {
            throw "Command `$Command` failed with exit code $($process.ExitCode)"
        }
    }
}

$defaultTasks = @(
    'clean',
    'prune-logs',
    'prune-tokens',
    'docs',
    'cargo-check',
    'audit-summary',
    'pointer-migrate'
)

if (-not $Tasks -or $Tasks.Count -eq 0) {
    $Tasks = $defaultTasks
}

if (-not $StateDir) {
    $StateDir = Join-Path $RepoRoot 'apps/arw-server/state'
}

function Invoke-Clean {
    $paths = @(
        'target',
        'dist',
        'site',
        'apps/arw-launcher/src-tauri/bin',
        'apps/arw-launcher/src-tauri/gen',
        'apps/arw-server/state/tmp',
        'target/tmp',
        'target/nextest'
    )
    foreach ($relative in $paths) {
        Remove-ItemSafe (Join-Path $RepoRoot $relative)
    }
    Get-ChildItem -Path $RepoRoot -Directory -Filter '__pycache__' -Recurse -ErrorAction SilentlyContinue |
        ForEach-Object { Remove-ItemSafe $_.FullName }
}

function Invoke-PruneLogs {
    $cutoff = (Get-Date).AddDays(-1 * [double]$KeepLogsDays)
    foreach ($relative in @('.arw/logs', 'target/logs')) {
        $dir = Join-Path $RepoRoot $relative
        if (-not (Test-Path -LiteralPath $dir)) { continue }
        Get-ChildItem -Path $dir -File -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTime -lt $cutoff } |
            ForEach-Object {
                if ($DryRun.IsPresent) {
                    Write-Info ("dry-run: delete old log {0}" -f $_.FullName)
                } else {
                    Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue
                    Write-Info ("deleted log {0}" -f $_.FullName)
                }
            }
    }
}

function Invoke-PruneTokens {
    $tokenDir = Join-Path $RepoRoot '.arw'
    if (-not (Test-Path -LiteralPath $tokenDir)) { return }
    Get-ChildItem -Path $tokenDir -Filter 'last_*token*.txt' -File -ErrorAction SilentlyContinue |
        ForEach-Object {
            if ($DryRun.IsPresent) {
                Write-Info ("dry-run: delete token file {0}" -f $_.FullName)
            } else {
                Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue
                Write-Info ("deleted token file {0}" -f $_.FullName)
            }
        }
}

function Invoke-Docs {
    Remove-ItemSafe (Join-Path $RepoRoot 'site')
    $python = Get-PythonCommand
    if ($python) {
        $scriptPath = Join-Path $PSScriptRoot 'stamp_docs_updated.py'
        Invoke-External -Command $python -Arguments @($scriptPath)
    } else {
        Write-Info "python not available; skipping docs stamp"
    }
}

function Invoke-CargoCheck {
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        Invoke-External -Command 'cargo' -Arguments @('check','--workspace')
    } else {
        Write-Info "cargo not available; skipping cargo-check"
    }
}

function Invoke-AuditSummary {
    $auditScript = Join-Path $PSScriptRoot 'audit.sh'
    if ((Test-Path -LiteralPath $auditScript) -and (Get-Command bash -ErrorAction SilentlyContinue)) {
        Invoke-External -Command 'bash' -Arguments @($auditScript,'--summary')
    } else {
        Write-Info "audit summary skipped (bash or audit.sh unavailable)"
    }
}

function Invoke-PointerMigrate {
    if (-not (Test-Path -LiteralPath $StateDir)) {
        Write-Info "pointer-migrate: state dir $StateDir not found; skipping"
        return
    }
    $python = Get-PythonCommand
    if (-not $python) {
        Write-Info "python not available; skipping pointer-migrate"
        return
    }
    $scriptPath = Join-Path $PSScriptRoot 'migrate_pointer_tokens.py'
    $args = @($scriptPath, '--state-dir', $StateDir, '--default-consent', $PointerConsent)
    if ($DryRun.IsPresent) {
        $args += '--dry-run'
    }
    Invoke-External -Command $python -Arguments $args
}

$taskHandlers = @{
    'clean'           = { Invoke-Step -Description 'clean workspace' -Action { Invoke-Clean } }
    'prune-logs'      = { Invoke-Step -Description 'prune logs' -Action { Invoke-PruneLogs } }
    'prune-tokens'    = { Invoke-Step -Description 'prune token files' -Action { Invoke-PruneTokens } }
    'docs'            = { Invoke-Step -Description 'refresh doc stamps' -Action { Invoke-Docs } }
    'cargo-check'     = { Invoke-Step -Description 'cargo check' -Action { Invoke-CargoCheck } }
    'audit-summary'   = { Invoke-Step -Description 'audit summary' -Action { Invoke-AuditSummary } }
    'pointer-migrate' = { Invoke-Step -Description 'pointer migrate' -Action { Invoke-PointerMigrate } }
}

foreach ($task in $Tasks) {
    if (-not $taskHandlers.ContainsKey($task)) {
        throw "Unknown task '$task'"
    }
    &$($taskHandlers[$task])
}

Write-Info 'completed'
