param(
  [string]$Label = $env:ARW_RELEASE_BLOCKER_LABEL
)

if ($env:ARW_SKIP_RELEASE_BLOCKER_CHECK -eq '1') {
  return
}

if (-not $Label) {
  $Label = 'release-blocker:restructure'
}

$repo = if ($env:GITHUB_REPOSITORY) { $env:GITHUB_REPOSITORY } else {
  try {
    $remote = git remote get-url origin 2>$null
    if ($remote -match 'github\.com[:/](?<repo>[^/]+/[A-Za-z0-9._-]+)(\.git)?$') {
      $Matches['repo']
    } else {
      $null
    }
  } catch {
    $null
  }
}

if (-not $repo) {
  throw '[release-gate] Unable to determine GitHub repository. Set GITHUB_REPOSITORY or configure origin to point at github.com.'
}

$encodedLabel = [System.Net.WebUtility]::UrlEncode($Label)
$perPage = 100
$baseUri = "https://api.github.com/repos/$repo/issues?state=open&labels=$encodedLabel&per_page=$perPage"

$headers = @{ 'Accept' = 'application/vnd.github+json' }
$token = $env:GH_TOKEN
if (-not $token -and $env:GITHUB_TOKEN) { $token = $env:GITHUB_TOKEN }
if ($token) { $headers['Authorization'] = "Bearer $token" }

$issues = New-Object System.Collections.Generic.List[string]
$remaining = $null
$page = 1

while ($true) {
  $uri = "$baseUri&page=$page"
  try {
    $response = Invoke-RestMethod -Uri $uri -Headers $headers -Method Get -ErrorAction Stop -ResponseHeadersVariable respHeaders
  } catch {
    $status = $null
    $detail = $null
    if ($_.Exception.Response) {
      $status = $_.Exception.Response.StatusCode.value__
      try {
        $reader = New-Object System.IO.StreamReader $_.Exception.Response.GetResponseStream()
        $detail = $reader.ReadToEnd()
        $reader.Close()
      } catch {}
    }
    $msg = if ($detail) { $detail } else { $_.Exception.Message }
    if ($status -eq 403 -and -not $token) {
      $msg += ' (provide GH_TOKEN or GITHUB_TOKEN to increase rate limits)'
    }
    throw "[release-gate] Failed to query GitHub issues. Status: $status. $msg"
  }

  if ($respHeaders -and $respHeaders.'X-RateLimit-Remaining') {
    $remaining = $respHeaders.'X-RateLimit-Remaining'
  }

  if ($null -eq $response) {
    break
  }

  if ($response -is [System.Collections.IDictionary] -and $response.ContainsKey('message')) {
    throw "[release-gate] GitHub API error: $($response.message)"
  }

  if ($response -isnot [System.Collections.IEnumerable] -or $response -is [string]) {
    $response = @($response)
  }

  $countThisPage = 0
  foreach ($item in $response) {
    $countThisPage++
    if ($item.PSObject.Properties.Name -contains 'pull_request') { continue }
    $issues.Add("#$($item.number) ($($item.title))")
  }

  if ($countThisPage -lt $perPage) { break }
  $page++
}

if ($issues.Count -gt 0) {
  $joined = [string]::Join(', ', $issues)
  throw "[release-gate] Open $Label issues: $joined"
}

$suffix = if (-not $token) { ' (unauthenticated request)' } elseif ($remaining) { " (remaining rate limit: $remaining)" } else { '' }
Write-Host ("[release-gate] No open $Label issues.$suffix") -ForegroundColor Green
