#!powershell
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Info($msg){ Write-Host "[docgen] $msg" -ForegroundColor Cyan }
function Die($msg){ Write-Error $msg; exit 1 }

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Die 'Rust `cargo` not found in PATH.' }

Info 'Collecting cargo metadata'
$json = cargo metadata --no-deps --locked --format-version 1 | Out-String | ConvertFrom-Json

# Derive GitHub blob base for links
function Get-GitHubBlobBase {
  param([string]$Default = 'https://github.com/t3hw00t/ARW/blob/main/')
  try {
    $remote = git config --get remote.origin.url 2>$null
    if ($null -ne $env:REPO_BLOB_BASE -and $env:REPO_BLOB_BASE.Trim() -ne '') { return $env:REPO_BLOB_BASE }
    if ($remote -match 'github\.com[:/]{1,2}([^/]+)/([^/.]+)') {
      $owner = $Matches[1]
      $repo  = $Matches[2]
      return "https://github.com/$owner/$repo/blob/main/"
    }
  } catch { }
  return $Default
}
$RepoBlobBase = Get-GitHubBlobBase

$pkgs = @()
foreach ($p in $json.packages) {
  $kinds = @()
  foreach ($t in $p.targets) {
    foreach ($k in $t.kind) { $kinds += $k }
  }
  $pkgs += [pscustomobject]@{
    name    = $p.name
    version = $p.version
    kind    = ($kinds | Select-Object -Unique) -join ','
    path    = $p.manifest_path
  }
}

# Group
$libs = $pkgs | Where-Object { $_.kind -like '*lib*' } | Sort-Object name
$bins = $pkgs | Where-Object { $_.kind -like '*bin*' } | Sort-Object name

$out = @()
$out += '---'
$out += 'title: Workspace Status'
$out += '---'
$out += ''
$out += '# Workspace Status'
$out += ''
$out += ("Generated: {0:yyyy-MM-dd HH:mm} UTC" -f (Get-Date).ToUniversalTime())
$out += ''
$out += '## Libraries'
if ($libs.Count -eq 0) {
  $out += '_none_'
} else {
  foreach ($l in $libs) {
    $abs = Resolve-Path $l.path | ForEach-Object { $_.Path }
    $repoRoot = Resolve-Path . | ForEach-Object { $_.Path }
    $rel = $abs -replace [regex]::Escape($repoRoot + [IO.Path]::DirectorySeparatorChar), ''
    $rel = $rel -replace '\\','/'
    $out += "- **$($l.name)**: $($l.version) — [$rel]($RepoBlobBase$rel)"
  }
}
$out += ''
$out += '## Binaries'
if ($bins.Count -eq 0) {
  $out += '_none_'
} else {
  foreach ($b in $bins) {
    $abs = Resolve-Path $b.path | ForEach-Object { $_.Path }
    $repoRoot = Resolve-Path . | ForEach-Object { $_.Path }
    $rel = $abs -replace [regex]::Escape($repoRoot + [IO.Path]::DirectorySeparatorChar), ''
    $rel = $rel -replace '\\','/'
    $out += "- **$($b.name)**: $($b.version) — [$rel]($RepoBlobBase$rel)"
  }
}

$dest = Join-Path $PSScriptRoot '..' 'docs' 'developer' 'status.md'
Info "Writing $dest"
# Avoid pipeline-binding quirks across PowerShell versions by setting -Value explicitly
Set-Content -Path $dest -Value ($out -join "`n") -Encoding utf8
Info 'Done.'
