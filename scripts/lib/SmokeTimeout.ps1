if (Get-Variable -Name SmokeTimeoutLibLoaded -Scope Script -ErrorAction SilentlyContinue) {
  return
}
$script:SmokeTimeoutLibLoaded = $true

function Get-SmokeTimeoutValue {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory = $false)][string]$SpecificEnvName,
    [Parameter(Mandatory = $false)][int]$DefaultSeconds = 600,
    [Parameter(Mandatory = $false)][string]$Tag = 'smoke'
  )

  $fallback = $DefaultSeconds
  if ($env:SMOKE_TIMEOUT_SECS) {
    $parsed = 0
    if ([int]::TryParse($env:SMOKE_TIMEOUT_SECS, [ref]$parsed)) {
      $fallback = $parsed
    }
    elseif ($env:SMOKE_TIMEOUT_SECS.Trim()) {
      Write-Warning "[$Tag] SMOKE_TIMEOUT_SECS='${env:SMOKE_TIMEOUT_SECS}' is not a valid integer; using $fallback"
    }
  }

  if ($SpecificEnvName) {
    $specificValue = (Get-Item -Path "env:$SpecificEnvName" -ErrorAction SilentlyContinue)?.Value
    if ($specificValue) {
      $specific = 0
      if ([int]::TryParse($specificValue, [ref]$specific)) {
        return $specific
      }
      Write-Warning "[$Tag] $SpecificEnvName='${specificValue}' is not a valid integer; using fallback $fallback"
    }
  }

  return $fallback
}

function Invoke-SmokeProcess {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [Parameter()][string[]]$ArgumentList,
    [Parameter()][int]$TimeoutSeconds = 600,
    [Parameter()][string]$WorkingDirectory,
    [Parameter()][string]$Tag = 'smoke'
  )

  $startParams = @{ FilePath = $FilePath; ArgumentList = $ArgumentList; PassThru = $true; NoNewWindow = $true }
  if ($WorkingDirectory) {
    $startParams.WorkingDirectory = $WorkingDirectory
  }

  $proc = Start-Process @startParams
  try {
    if ($TimeoutSeconds -gt 0) {
      Wait-Process -Id $proc.Id -Timeout $TimeoutSeconds | Out-Null
    }
    else {
      $proc.WaitForExit()
    }
  }
  catch [System.TimeoutException] {
    Write-Warning "[$Tag] timed out after ${TimeoutSeconds}s; terminating process (PID $($proc.Id))"
    try { $proc.CloseMainWindow() | Out-Null } catch { }
    try { $proc.Kill() } catch { }
    $proc.WaitForExit()
    return 124
  }

  $proc.WaitForExit()
  return $proc.ExitCode
}
