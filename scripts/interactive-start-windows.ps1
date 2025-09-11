#!powershell
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Banner($title, $subtitle){
  $cols = [Console]::WindowWidth
  if (-not $cols -or $cols -lt 40) { $cols = 80 }
  $line = ''.PadLeft($cols, '━')
  Write-Host "`n$line" -ForegroundColor DarkCyan
  Write-Host " $title" -ForegroundColor White
  if ($subtitle) { Write-Host " $subtitle" -ForegroundColor DarkCyan }
  Write-Host $line -ForegroundColor DarkCyan
}
function Section($t){ Write-Host "> $t" -ForegroundColor Magenta }
function Info($t){ Write-Host "[info] $t" -ForegroundColor DarkCyan }
function Warn($t){ Write-Host "[warn] $t" -ForegroundColor Yellow }

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
# Add project-local Rust cargo bin to PATH if present (isolated, no admin)
$localRustCargoBin = Join-Path $root '.arw\rust\cargo\bin'
New-Item -ItemType Directory -Force $localRustCargoBin | Out-Null
if (-not ($env:Path -split ';' | Where-Object { $_ -eq $localRustCargoBin })) { $env:Path = "$localRustCargoBin;" + $env:Path }

# Load saved preferences if present
$envFile = Join-Path $root '.arw\env.ps1'
if (Test-Path $envFile) { . $envFile }

Banner 'ARW — Start Menu (Windows)' 'Start services, tools, and debugging'
Write-Host '  Agents Running Wild (ARW) — local-first Rust workspace for personal AI agents.'
Write-Host '  Highlights: user-mode HTTP service + debug UI; macro-driven tools; event stream; portable packaging.'

$Port = if ($env:ARW_PORT) { [int]$env:ARW_PORT } else { 8090 }
$Debug = if ($env:ARW_DEBUG -eq '1') { $true } else { $false }
$DocsUrl = $env:ARW_DOCS_URL
$AdminToken = $env:ARW_ADMIN_TOKEN
$UseDist = $false
$CfgPath = $env:ARW_CONFIG

function Configure-Runtime {
  Section 'Runtime Settings'
  $ans = Read-Host "HTTP port [$Port]"; if ($ans) { $Port = [int]$ans }
  try {
    $busy = (Test-NetConnection -ComputerName 127.0.0.1 -Port $Port -WarningAction SilentlyContinue).TcpTestSucceeded
    if ($busy) {
      $np = $Port
      for ($i=$Port; $i -lt ($Port+100); $i++) { $t = (Test-NetConnection -ComputerName 127.0.0.1 -Port $i -WarningAction SilentlyContinue).TcpTestSucceeded; if (-not $t) { $np = $i; break } }
      Write-Warning "Port $Port busy. Suggesting $np"
      $ans2 = Read-Host "Use $np instead? (Y/n)"; if (-not ($ans2 -match '^[nN]')) { $Port = $np }
    }
  } catch {}
  $ans = Read-Host 'Enable debug endpoints? (y/N)'; $Debug = ($ans -match '^[yY]')
  $ans = Read-Host "Docs URL (optional) [$DocsUrl]"; if ($ans -ne '') { $DocsUrl = $ans }
  $ans = Read-Host "Admin token (optional) [$AdminToken]"; if ($ans -ne '') { $AdminToken = $ans }
  $ans = Read-Host 'Use packaged dist/ bundle when present? (y/N)'; $UseDist = ($ans -match '^[yY]')
}

function Pick-Config {
  Section 'Config'
  $cfgDisplay = if ($null -ne $CfgPath -and $CfgPath -ne '') { $CfgPath } else { '<default configs/default.toml>' }
  Write-Host ("Current ARW_CONFIG: " + $cfgDisplay)
  $ans = Read-Host 'Enter config path (or empty for default)'
  if ($ans -ne '') { $CfgPath = $ans }
}

function Start-ServiceOnly {
  Section 'Start: service only'
  $env:ARW_NO_TRAY = '1'
  if ($CfgPath) { $env:ARW_CONFIG = $CfgPath }
  $runDir = Join-Path $root '.arw\run'; New-Item -ItemType Directory -Force $runDir | Out-Null
  $env:ARW_PID_FILE = (Join-Path $runDir 'arw-svc.pid')
  $logs = Join-Path $root '.arw\logs'; New-Item -ItemType Directory -Force $logs | Out-Null
  $env:ARW_LOG_FILE = (Join-Path $logs 'arw-svc.out.log')
  $svc = Join-Path $root 'target\release\arw-svc.exe'
  if (-not (Test-Path $svc) -and -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Warn 'Service binary missing and Rust not installed. Run Setup → Dependencies → Install Rust.'
  }
  if (-not (Security-Preflight)) { Warn 'Start cancelled'; return }
  $svcArgs = @('-Port', $Port, '-TimeoutSecs', 20)
  if ($Debug) { $svcArgs += '-Debug' }
  if ($DocsUrl) { $svcArgs += @('-DocsUrl', $DocsUrl) }
  if ($AdminToken) { $svcArgs += @('-AdminToken', $AdminToken) }
  if ($UseDist) { $svcArgs += '-UseDist' }
  & (Join-Path $PSScriptRoot 'start.ps1') @svcArgs
}

function Start-TrayPlusService {
  Section 'Start: tray + service'
  $env:ARW_NO_TRAY = ''
  if ($CfgPath) { $env:ARW_CONFIG = $CfgPath }
  $runDir = Join-Path $root '.arw\run'; New-Item -ItemType Directory -Force $runDir | Out-Null
  $env:ARW_PID_FILE = (Join-Path $runDir 'arw-svc.pid')
  $logs = Join-Path $root '.arw\logs'; New-Item -ItemType Directory -Force $logs | Out-Null
  $env:ARW_LOG_FILE = (Join-Path $logs 'arw-svc.out.log')
  $svcArgs = @('-Port', $Port, '-TimeoutSecs', 20)
  if ($Debug) { $svcArgs += '-Debug' }
  if ($DocsUrl) { $svcArgs += @('-DocsUrl', $DocsUrl) }
  if ($AdminToken) { $svcArgs += @('-AdminToken', $AdminToken) }
  if ($UseDist) { $svcArgs += '-UseDist' }
  if (-not (Security-Preflight)) { Warn 'Start cancelled'; return }
  & (Join-Path $PSScriptRoot 'start.ps1') @svcArgs
  $tray = Join-Path $root 'target\release\arw-tray.exe'
  if (-not (Test-Path $tray)) {
    Warn 'Tray not available. If build failed or toolchains missing, use Setup → Dependencies.'
  }
}

function Start-Connector {
  Section 'Start: connector (if built with NATS)'
  $exe = Join-Path $root 'target\release\arw-connector.exe'
  if (-not (Test-Path $exe)) {
    Warn 'arw-connector not found; build first (with features)'
    return
  }
  Start-Process -FilePath $exe | Out-Null
}

function Open-ProbeMenu {
  $base = "http://127.0.0.1:$Port"
  while ($true) {
    Banner 'Open / Probe' $base
    Write-Host @'
  1) Open Debug UI (/debug)
  2) Open API Spec (/spec)
  3) Open Tools JSON (/introspect/tools)
  4) Invoke health (/healthz)
  5) Trigger test event (/emit/test)
  6) Check NATS connectivity
  7) Copy Debug URL to clipboard
  8) Copy Spec URL to clipboard
  9) Copy admin curl (introspect/tools)
  10) Copy admin curl (shutdown)
  0) Back
'@
    $pick = Read-Host 'Select'
    switch ($pick) {
      '1' { Start-Process -FilePath "$base/debug" | Out-Null }
      '2' { Start-Process -FilePath "$base/spec" | Out-Null }
      '3' { Start-Process -FilePath "$base/introspect/tools" | Out-Null }
      '4' { try { (Invoke-WebRequest -UseBasicParsing "$base/healthz").Content | Write-Host } catch {} ; Read-Host 'Continue' | Out-Null }
      '5' { try { (Invoke-WebRequest -UseBasicParsing "$base/emit/test").Content | Write-Host } catch {} ; Read-Host 'Continue' | Out-Null }
      '6' { $u = Read-Host 'NATS URL [nats://127.0.0.1:4222]'; if (-not $u) { $u = 'nats://127.0.0.1:4222' }; $rest = $u -replace '^.*?://',''; $parts = $rest.Split(':'); $h=$parts[0]; $p=if ($parts.Length -gt 1) { [int]$parts[1] } else { 4222 }; try { $ok = (Test-NetConnection -ComputerName $h -Port $p -WarningAction SilentlyContinue).TcpTestSucceeded; if ($ok) { Info ("NATS reachable at $($h):$p") } else { Warn ("Cannot reach $($h):$p") } } catch { Warn 'Test failed' }; Read-Host 'Continue' | Out-Null }
      '7' { try { Set-Clipboard -Value "$base/debug"; Info 'Copied Debug URL' } catch { } }
      '8' { try { Set-Clipboard -Value "$base/spec"; Info 'Copied Spec URL' } catch { } }
      '9' {
        $tok = $env:ARW_ADMIN_TOKEN
        if (-not $tok) { $ans = Read-Host 'No token set. Generate one now? (Y/n)'; if (-not ($ans -match '^[nN]')) { $tok = [Guid]::NewGuid().ToString('N'); $env:ARW_ADMIN_TOKEN = $tok; $script:AdminToken = $tok; Info 'Generated token for this session.' } }
        if ($tok) { $cmd = "curl -sS -H `"X-ARW-Admin: $tok`" `"$base/introspect/tools`" | jq ." } else { $cmd = "curl -sS -H `"X-ARW-Admin: YOUR_TOKEN`" `"$base/introspect/tools`" | jq ." }
        try { Set-Clipboard -Value $cmd; Info 'Copied admin curl snippet' } catch { }
        Write-Host $cmd
      }
      '10' {
        $tok = $env:ARW_ADMIN_TOKEN
        if (-not $tok) { $ans = Read-Host 'No token set. Generate one now? (Y/n)'; if (-not ($ans -match '^[nN]')) { $tok = [Guid]::NewGuid().ToString('N'); $env:ARW_ADMIN_TOKEN = $tok; $script:AdminToken = $tok; Info 'Generated token for this session.' } }
        if ($tok) { $cmd = "curl -sS -H `"X-ARW-Admin: $tok`" `"$base/shutdown`"" } else { $cmd = "curl -sS -H `"X-ARW-Admin: YOUR_TOKEN`" `"$base/shutdown`"" }
        try { Set-Clipboard -Value $cmd; Info 'Copied admin curl shutdown' } catch { }
        Write-Host $cmd
      }
      '0' { break }
      default { }
    }
  }
}

function Build-TestMenu {
  while ($true) {
    Banner 'Build & Test' 'Workspace targets'
    Write-Host @'
  1) Cargo build (release)
  2) Cargo build with NATS features (release)
  3) Cargo test (nextest)
  4) Generate docs page (docgen)
  5) Package portable bundle (dist/)
  0) Back
'@
    $pick = Read-Host 'Select'
    switch ($pick) {
      '1' { Push-Location $root; cargo build --workspace --release; Pop-Location }
      '2' { Push-Location $root; cargo build --workspace --release --features nats; Pop-Location }
      '3' { Push-Location $root; cargo nextest run --workspace; Pop-Location }
      '4' { & (Join-Path $PSScriptRoot 'docgen.ps1') }
      '5' { & (Join-Path $PSScriptRoot 'package.ps1') }
      '0' { break }
      default { }
    }
  }
}

function Cli-ToolsMenu {
  $exe = Join-Path $root 'target\release\arw-cli.exe'
  if (-not (Test-Path $exe)) { Warn 'arw-cli not found; build first'; Read-Host 'Continue' | Out-Null; return }
  while ($true) {
    Banner 'CLI Tools' $exe
    Write-Host @'
  1) List tools (JSON)
  2) Print capsule template
  3) Generate ed25519 keypair (b64)
  0) Back
'@
    $pick = Read-Host 'Select'
    switch ($pick) {
      '1' { & $exe tools; Read-Host 'Continue' | Out-Null }
      '2' { & $exe capsule template; Read-Host 'Continue' | Out-Null }
      '3' { & $exe capsule gen-ed25519; Read-Host 'Continue' | Out-Null }
      '0' { break }
      default { }
    }
  }
}

function Main-Menu {
  while ($true) {
    Banner 'Start Menu' ("Port=$Port Debug=$Debug Dist=$UseDist")
    Write-Host @'
  1) Configure runtime (port/docs/token)
  2) Select config file (ARW_CONFIG)
  3) Start service only
  4) Start tray + service (if available)
  5) Start connector (NATS)
  6) Open/probe endpoints
  7) Build & test
  8) CLI tools
  9) Stop service (/shutdown)
  10) Force stop (PID/name)
  11) NATS manager (Windows/WSL)
  12) View logs
  13) Save preferences
  14) Doctor (quick checks)
  15) Open Windows Terminal (repo)
  16) Configure HTTP port (write config)
  17) Spec sync (validate /spec)
  18) Docs build + open
  19) Tray build check
  20) Generate reverse proxy templates (Caddy/Nginx)
  21) Security tips
  22) Start Caddy reverse proxy (https://localhost:8443)
  23) Stop Caddy reverse proxy
  24) Disable debug now
  25) TLS wizard (LE/mkcert/self-signed)
  26) Start Caddy with selected Caddyfile
  27) Write session summary (./.arw/support)
  28) Stop all (svc/proxy/nats)
  0) Exit
'@
    $pick = Read-Host 'Select'
    switch ($pick) {
      '1' { Configure-Runtime }
      '2' { Pick-Config }
      '3' { Start-ServiceOnly }
      '4' { Start-TrayPlusService }
      '5' { Start-Connector }
      '6' { Open-ProbeMenu }
      '7' { Build-TestMenu }
      '8' { Cli-ToolsMenu }
      '9' { try { (Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$Port/shutdown").Content | Write-Host } catch {}; Read-Host 'Continue' | Out-Null }
      '10' { Force-Stop }
      '11' { Nats-Menu }
      '12' { Logs-Menu }
      '13' { Save-Prefs-From-Start }
      '14' { Doctor }
      '15' { Open-Terminal-Here }
      '16' { Configure-Http-Port }
      '17' { Spec-Sync }
      '18' { Docs-Build-Open }
      '19' { Tray-Build-Check }
      '20' { Reverse-Proxy-Templates }
      '21' { Security-Tips }
      '22' { Reverse-Proxy-Caddy-Start }
      '23' { Reverse-Proxy-Caddy-Stop }
      '24' { $script:Debug = $false; $env:ARW_DEBUG = ''; Info 'Debug disabled for this session' }
      '25' { TLS-Wizard }
      '26' { Reverse-Proxy-Caddy-Choose-Start }
      '27' { Session-Summary }
      '28' { Stop-All }
      '0' { break }
      default { }
    }
  }
}

function Force-Stop {
  Section 'Force stop'
  $pidFile = Join-Path $root '.arw\run\arw-svc.pid'
  if (Test-Path $pidFile) {
    try {
      $pid = Get-Content -Path $pidFile | Select-Object -First 1
      if ($pid) { Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue; Info "Stopped PID $pid" } else { Warn 'PID file empty' }
    } catch { Warn 'Failed to stop PID from file' }
  } else {
    Stop-Process -Name 'arw-svc' -Force -ErrorAction SilentlyContinue
    Warn 'PID file missing; attempted Stop-Process arw-svc'
  }
}

function Logs-Menu {
  $logs = Join-Path $root '.arw\logs'; New-Item -ItemType Directory -Force $logs | Out-Null
  $svcLog = Join-Path $logs 'arw-svc.out.log'
  $natsOut = Join-Path $logs 'nats-server.out.log'
  $natsErr = Join-Path $logs 'nats-server.err.log'
  Banner 'Logs' $logs
  Write-Host '  1) Tail service log (if available)'
  Write-Host '  2) Tail nats-server out'
  Write-Host '  3) Tail nats-server err'
  Write-Host '  0) Back'
  $pick = Read-Host 'Select'
  switch ($pick) {
    '1' { if (Test-Path $svcLog) { Get-Content -Path $svcLog -Wait -Tail 200 } else { Warn 'No service log yet (will appear after next start with logging).' ; Read-Host 'Continue' | Out-Null } }
    '2' { if (Test-Path $natsOut) { Get-Content -Path $natsOut -Wait -Tail 200 } else { Warn 'No nats out log' ; Read-Host 'Continue' | Out-Null } }
    '3' { if (Test-Path $natsErr) { Get-Content -Path $natsErr -Wait -Tail 200 } else { Warn 'No nats err log' ; Read-Host 'Continue' | Out-Null } }
    default { }
  }
}

function Save-Prefs-From-Start {
  $envDir = Join-Path $root '.arw'
  New-Item -ItemType Directory -Force $envDir | Out-Null
  $f = Join-Path $envDir 'env.ps1'
  @(
    "# ARW env (project-local)",
    "# dot-source this file to apply preferences",
    "$env:ARW_PORT = '$Port'",
    "$env:ARW_DOCS_URL = '$DocsUrl'",
    "$env:ARW_ADMIN_TOKEN = '$AdminToken'",
    "$env:ARW_CONFIG = '$CfgPath'"
  ) | Set-Content -Path $f -Encoding utf8
  Info ("Saved preferences to " + $f)
}

function Open-Terminal-Here {
  $wt = Get-Command wt.exe -ErrorAction SilentlyContinue
  if ($wt) {
    Start-Process -FilePath wt.exe -ArgumentList @('-w','0','new-tab','-d',$root) | Out-Null
    Info 'Opened Windows Terminal in repo root'
  } else {
    Start-Process -FilePath powershell.exe -ArgumentList @('-NoExit','-Command',("cd `"$root`"")) | Out-Null
    Info 'Opened PowerShell in repo root'
  }
}

function Security-Preflight {
  if ($Debug -and -not $AdminToken) {
    Banner 'Security Preflight' 'Admin token recommended'
    Write-Host 'ARW_DEBUG=1 enables admin endpoints without a token.'
    Write-Host 'Recommended: generate a token for this session or disable debug.'
    Write-Host '  1) Generate token and continue'
    Write-Host '  2) Continue without token (development)'
    Write-Host '  3) Cancel start'
    $s = Read-Host 'Select [1/2/3]'; if (-not $s) { $s = '1' }
    switch ($s) {
      '1' { $tok = [Guid]::NewGuid().ToString('N'); $env:ARW_ADMIN_TOKEN = $tok; $script:AdminToken = $tok; Info 'Token set for this session.'; return $true }
      '2' { return $true }
      default { return $false }
    }
  }
  return $true
}

function Reverse-Proxy-Templates {
  Section 'Reverse proxy templates'
  $out = Join-Path $root 'configs\reverse_proxy'
  New-Item -ItemType Directory -Force (Join-Path $out 'caddy') | Out-Null
  New-Item -ItemType Directory -Force (Join-Path $out 'nginx') | Out-Null
  $caddy = @"
localhost:8443 {
  tls internal
  reverse_proxy 127.0.0.1:$Port
}
"@
  $caddy | Set-Content -Path (Join-Path $out 'caddy\Caddyfile') -Encoding utf8
  $ng = @'
upstream arw_upstream { server 127.0.0.1:__PORT__; }
server {
  listen 8080;
  location / { proxy_pass http://arw_upstream; proxy_set_header Host $host; proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for; }
}
# TLS block (requires certs); see notes in Linux/macOS templates for generating self-signed certs
'@
  $ng | Set-Content -Path (Join-Path $out 'nginx\arw.conf') -Encoding utf8
  (Get-Content -Path (Join-Path $out 'nginx\arw.conf')) -replace '__PORT__', "$Port" | Set-Content -Path (Join-Path $out 'nginx\arw.conf')
  Info ("Caddyfile: " + (Join-Path $out 'caddy\Caddyfile'))
  Info ("Nginx:     " + (Join-Path $out 'nginx\arw.conf'))
  Write-Host "Run Caddy (if installed): caddy run --config `"$(Join-Path $out 'caddy\Caddyfile')`""
  Write-Host "Run nginx (if installed): start nginx and include arw.conf; may require manual merge"
}

function Reverse-Proxy-Caddy-Start {
  Section 'Start Caddy reverse proxy'
  $caddy = Get-Command caddy -ErrorAction SilentlyContinue
  if (-not $caddy) { Warn 'caddy.exe not found in PATH'; return }
  $out = Join-Path $root 'configs\reverse_proxy\caddy\Caddyfile'
  if (-not (Test-Path $out)) { Reverse-Proxy-Templates }
  $logs = Join-Path $root '.arw\logs'; New-Item -ItemType Directory -Force $logs | Out-Null
  $run = Join-Path $root '.arw\run'; New-Item -ItemType Directory -Force $run | Out-Null
  $p = Start-Process -FilePath $caddy.Path -ArgumentList @('run','--config',$out) -RedirectStandardOutput (Join-Path $logs 'caddy.out.log') -RedirectStandardError (Join-Path $logs 'caddy.out.log') -PassThru
  ($p.Id) | Out-File -FilePath (Join-Path $run 'caddy.pid') -Encoding ascii -Force
  Info ("Caddy started (pid " + $p.Id + ") — open https://localhost:8443")
  Start-Process -FilePath 'https://localhost:8443' | Out-Null
}

function Reverse-Proxy-Caddy-Stop {
  Section 'Stop Caddy'
  $run = Join-Path $root '.arw\run\caddy.pid'
  if (Test-Path $run) { try { $pid = Get-Content $run | Select-Object -First 1; if ($pid) { Stop-Process -Id $pid -Force } } catch { } Remove-Item $run -Force -ErrorAction SilentlyContinue } else { Get-Process caddy -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue }
  Info 'Stopped Caddy (if running)'
}

function Reverse-Proxy-Caddy-Choose-Start {
  Section 'Start Caddy with selected Caddyfile'
  $dir = Join-Path $root 'configs\reverse_proxy\caddy'
  if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
  $files = Get-ChildItem -Path $dir -Filter 'Caddyfile*' -File -ErrorAction SilentlyContinue
  if (-not $files) { Warn 'No Caddyfiles found; run TLS wizard or generate templates'; return }
  $i = 1; foreach ($f in $files) { Write-Host ("  $i) " + $f.Name); $i++ }
  $sel = Read-Host 'Select'; if (-not $sel) { $sel = 1 }
  $idx = [int]$sel - 1; if ($idx -lt 0 -or $idx -ge $files.Count) { Warn 'Invalid selection'; return }
  $cfg = $files[$idx].FullName
  $logs = Join-Path $root '.arw\logs'; New-Item -ItemType Directory -Force $logs | Out-Null
  $run = Join-Path $root '.arw\run'; New-Item -ItemType Directory -Force $run | Out-Null
  try { & (Get-Command caddy).Path validate --config $cfg | Out-Null; Info 'Caddyfile valid' } catch { Warn 'Caddyfile validation failed'; $c = Read-Host 'Start anyway? (y/N)'; if (-not ($c -match '^[yY]')) { return } }
  $p = Start-Process -FilePath (Get-Command caddy).Path -ArgumentList @('run','--config',$cfg) -RedirectStandardOutput (Join-Path $logs 'caddy.out.log') -RedirectStandardError (Join-Path $logs 'caddy.out.log') -PassThru
  ($p.Id) | Out-File -FilePath (Join-Path $run 'caddy.pid') -Encoding ascii -Force
  Info ("Caddy started with " + (Split-Path $cfg -Leaf) + ' — https://localhost:8443')
  Start-Process -FilePath 'https://localhost:8443' | Out-Null
}

function Session-Summary {
  Section 'Session summary'
  $sup = Join-Path $root '.arw\support'; New-Item -ItemType Directory -Force $sup | Out-Null
  $ts = Get-Date -Format yyyyMMdd_HHmmss
  $out = Join-Path $sup ("session_" + $ts + '.md')
  $nOk = $false; try { $nOk = (Test-NetConnection -ComputerName 127.0.0.1 -Port 4222 -WarningAction SilentlyContinue).TcpTestSucceeded } catch {}
  $svcLog = Join-Path $root '.arw\logs\arw-svc.out.log'
  $caddyPid = Join-Path $root '.arw\run\caddy.pid'
  $txt = @(
    "# ARW Session Summary ($ts)",
    "- Port: $Port",
    "- Debug: $Debug",
    "- ARW_CONFIG: $((if ($null -ne $CfgPath -and $CfgPath -ne '') { $CfgPath } else { '<default>' }))",
    "- Docs URL: $DocsUrl",
    "- Admin token set: $([bool]$env:ARW_ADMIN_TOKEN)",
    "- NATS reachable (nats://127.0.0.1:4222): $nOk",
    "- Service log: $svcLog",
    "- Caddy running: $([bool](Test-Path $caddyPid))",
    '',
    '## Useful URLs',
    ("- Debug: http://127.0.0.1:" + $Port + "/debug"),
    ("- Spec:  http://127.0.0.1:" + $Port + "/spec")
  )
  $txt | Set-Content -Path $out -Encoding utf8
  Info ("Wrote " + $out)
}

function Stop-All {
  Force-Stop
  Reverse-Proxy-Caddy-Stop
  try { Stop-Process -Name 'nats-server' -Force -ErrorAction SilentlyContinue } catch {}
  Info 'Stopped svc/proxy/nats'
}

function TLS-Wizard {
  Banner 'TLS Wizard' 'Choose a TLS strategy'
  Write-Host '  1) Public domain with Let''s Encrypt (Caddy)'
  Write-Host '  2) Local dev TLS via mkcert (Caddy)'
  Write-Host '  3) Self-signed (Caddy internal)'
  $t = Read-Host 'Select [1/2/3]'; if (-not $t) { $t = '3' }
  $outc = Join-Path $root 'configs\reverse_proxy\caddy'; New-Item -ItemType Directory -Force $outc | Out-Null
  switch ($t) {
    '1' {
      $d = Read-Host 'Domain (e.g., arw.example.com)'; $e = Read-Host 'Email for ACME (e.g., you@example.com)'
      if (-not $d -or -not $e) { Warn 'Domain and email required'; return }
      $c = @"
$d {
  tls $e
  reverse_proxy 127.0.0.1:$Port
}
"@
      $cf = Join-Path $outc ("Caddyfile." + $d)
      $c | Set-Content -Path $cf -Encoding utf8
      Info ("Wrote " + $cf)
      Write-Host 'Ensure ports 80/443 are reachable and DNS resolves the domain to this host.'
    }
    '2' {
      $mk = Get-Command mkcert -ErrorAction SilentlyContinue
      if (-not $mk) { Warn 'mkcert not found (install via scoop/choco). Falling back to self-signed.'; return }
      $d = Read-Host 'Dev hostname [localhost]'; if (-not $d) { $d = 'localhost' }
      $cert = Join-Path $outc ($d + '.crt'); $key = Join-Path $outc ($d + '.key')
      try { & $mk.Path -install } catch {}
      & $mk.Path -cert-file $cert -key-file $key $d
      $c = @"
$d {
  tls $cert $key
  reverse_proxy 127.0.0.1:$Port
}
"@
      $cf = Join-Path $outc ("Caddyfile." + $d)
      $c | Set-Content -Path $cf -Encoding utf8
      Info ("Wrote " + $cf)
    }
    default {
      Info 'Self-signed supported via existing Caddyfile with tls internal.'
    }
  }
}
function Configure-Http-Port {
  Section 'Configure HTTP port in configs/local.toml'
  $p = Read-Host ("HTTP port [" + $Port + "]"); if (-not $p) { $p = $Port }
  $cfgDir = Join-Path $root 'configs'; New-Item -ItemType Directory -Force $cfgDir | Out-Null
  $cfg = @"
[runtime]
portable = true
port = $p

[cluster]
enabled = false
bus = "local"
queue = "local"
"@
  $cfgPath = Join-Path $cfgDir 'local.toml'
  $cfg | Set-Content -Path $cfgPath -Encoding utf8
  $env:ARW_CONFIG = $cfgPath
  $script:CfgPath = $cfgPath
  $script:Port = [int]$p
  Info ("Wrote " + $cfgPath + " and set ARW_CONFIG. Port=" + $p)
}

function Spec-Sync {
  Section 'Spec sync'
  $base = "http://127.0.0.1:$Port"
  try { (Invoke-WebRequest -UseBasicParsing "$base/spec").StatusCode | Out-Null; Info '/spec ok' } catch { Warn '/spec not reachable' }
  try { (Invoke-WebRequest -UseBasicParsing "$base/spec/openapi.yaml").StatusCode | Out-Null; Info '/spec/openapi.yaml ok' } catch { Warn 'openapi not found' }
  try { (Invoke-WebRequest -UseBasicParsing "$base/healthz").StatusCode | Out-Null; Info '/healthz ok' } catch { Warn '/healthz failed' }
  Start-Process -FilePath "$base/spec" | Out-Null
}

function Docs-Build-Open {
  Section 'Docs build + open'
  $venvMk = Join-Path $root '.venv\Scripts\mkdocs.exe'
  if (Test-Path $venvMk) { & $venvMk build } elseif (Get-Command mkdocs -ErrorAction SilentlyContinue) { mkdocs build } else { Warn 'mkdocs not found; use Setup → Dependencies to install'; return }
  $idx = Join-Path $root 'site\index.html'
  if (Test-Path $idx) { Start-Process -FilePath $idx | Out-Null } else { Warn 'site/index.html not found' }
}

function Tray-Build-Check {
  Section 'Tray build check'
  $logDir = Join-Path $root '.arw\logs'; New-Item -ItemType Directory -Force $logDir | Out-Null
  $log = Join-Path $logDir 'tray-build.log'
  try { Push-Location $root; cargo build --release -p arw-tray *> $log } catch { } finally { Pop-Location }
  Get-Content -Path $log -Tail 60 | Write-Host
  $exe = Join-Path $root 'target\release\arw-tray.exe'
  if (Test-Path $exe) { Info ("Tray built: " + $exe) } else { Warn 'Tray not built; GTK for Windows builds are non-trivial; consider using service only (tray optional).' }
}

function Doctor {
  Banner 'Doctor' ''
  $cargo = Get-Command cargo -ErrorAction SilentlyContinue
  if ($cargo) { Info (cargo --version) } else { Warn 'cargo not found' }
  $jq = Get-Command jq -ErrorAction SilentlyContinue
  if ($jq) { Info ("jq " + (& $jq.Path --version)) } else { Warn 'jq not found' }
  $mk = Get-Command mkdocs -ErrorAction SilentlyContinue
  if ($mk) { Info (mkdocs --version) } else { Warn 'mkdocs not found (docs optional)' }
  $svc = Join-Path $root 'target\release\arw-svc.exe'; if (Test-Path $svc) { Info ("arw-svc: " + $svc) } else { Warn 'arw-svc not built' }
  $tray = Join-Path $root 'target\release\arw-tray.exe'; if (Test-Path $tray) { Info ("arw-tray: " + $tray) } else { Warn 'tray not built (optional)' }
  try { $ok = (Test-NetConnection -ComputerName 127.0.0.1 -Port 4222 -WarningAction SilentlyContinue).TcpTestSucceeded; if ($ok) { Info 'NATS reachable on 127.0.0.1:4222' } else { Warn 'NATS not reachable' } } catch { }
  Read-Host 'Continue' | Out-Null
}

function Install-NatsLocal {
  $ver = '2.10.19'
  $os = 'windows'
  $arch = if ($env:PROCESSOR_ARCHITECTURE -match 'ARM') { 'arm64' } else { 'amd64' }
  $asset = "nats-server-v$ver-$os-$arch.zip"
  $url = "https://github.com/nats-io/nats-server/releases/download/v$ver/$asset"
  $dir = Join-Path $root '.arw\nats'
  New-Item -ItemType Directory -Force (Join-Path $dir 'tmp') | Out-Null
  $zip = Join-Path (Join-Path $dir 'tmp') $asset
  try { Invoke-WebRequest -UseBasicParsing $url -OutFile $zip } catch { Warn "Download failed: $url"; return }
  try {
    Expand-Archive -Path $zip -DestinationPath (Join-Path $dir 'tmp') -Force
    $exe = Get-ChildItem -Path (Join-Path $dir 'tmp') -Recurse -Filter 'nats-server.exe' | Select-Object -First 1
    if ($exe) { Copy-Item $exe.FullName -Destination (Join-Path $dir 'nats-server.exe') -Force; Info "Installed nats-server to $dir" } else { Warn 'nats-server.exe not found in archive' }
  } catch { Warn 'Extraction failed' }
}

function Nats-Menu {
  while ($true) {
    Banner 'NATS Manager' 'Windows local or WSL-based broker'
    Write-Host @'
  WINDOWS (local)
    1) Install local NATS (no admin)
    2) Start NATS at nats://127.0.0.1:4222
    3) Stop NATS
  WSL (Linux)
    5) Install NATS in WSL (no sudo)
    6) Start NATS in WSL (127.0.0.1:4222)
    7) Stop NATS in WSL
    8) Show WSL connection + GUI/WSLg info
    9) Set default WSL distro
    10) Open Windows Terminal in WSL
  Utils
    4) Check connectivity (Windows)
    0) Back
'@
    $pick = Read-Host 'Select'
    switch ($pick) {
      '1' { Install-NatsLocal }
      '2' {
        $dir = Join-Path $root '.arw\nats'
        $exe = Join-Path $dir 'nats-server.exe'
        if (-not (Test-Path $exe)) { Warn 'nats-server.exe not installed'; break }
        $runDir = Join-Path $root '.arw\run'; New-Item -ItemType Directory -Force $runDir | Out-Null
        $logs = Join-Path $root '.arw\logs'; New-Item -ItemType Directory -Force $logs | Out-Null
        $p = Start-Process -FilePath $exe -ArgumentList @('-a','127.0.0.1','-p','4222') -PassThru -RedirectStandardOutput (Join-Path $logs 'nats-server.out.log') -RedirectStandardError (Join-Path $logs 'nats-server.err.log')
        ($p.Id) | Out-File -FilePath (Join-Path $runDir 'nats-server.pid') -Encoding ascii -Force
        Info ("nats-server started pid " + $p.Id)
      }
      '3' {
        $pidFile = Join-Path $root '.arw\run\nats-server.pid'
        if (Test-Path $pidFile) { try { $pid = Get-Content $pidFile | Select-Object -First 1; if ($pid) { Stop-Process -Id $pid -Force } } catch {} } else { Stop-Process -Name 'nats-server' -Force -ErrorAction SilentlyContinue }
      }
      '4' { try { $ok = (Test-NetConnection -ComputerName 127.0.0.1 -Port 4222 -WarningAction SilentlyContinue).TcpTestSucceeded; if ($ok) { Info 'NATS reachable' } else { Warn 'NATS not reachable' } } catch { }; Read-Host 'Continue' | Out-Null }
      '5' { Wsl-Install-Nats }
      '6' { Wsl-Start-Nats }
      '7' { Wsl-Stop-Nats }
      '8' { Wsl-Show-Info }
      '9' { Wsl-Set-Default }
      '10' { Wsl-Open-Terminal }
      '0' { break }
      default { }
    }
  }
}

function Wsl-Select-Distro {
  $wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
  if (-not $wsl) { Warn 'WSL not found. Install WSL (requires admin): wsl --install -d Ubuntu'; return $null }
  $list = & wsl.exe -l -q 2>$null | Where-Object { $_ -and $_.Trim() -ne '' }
  if (-not $list) { Warn 'No WSL distributions installed. Run elevated: wsl --install -d Ubuntu'; return $null }
  if ($list.Count -eq 1) { return $list[0].Trim() }
  Write-Host 'Available WSL distros:'; $i=1; foreach ($d in $list) { Write-Host ("  $i) " + $d); $i++ }
  $pick = Read-Host 'Select distro number'
  $idx = [int]$pick - 1
  if ($idx -ge 0 -and $idx -lt $list.Count) { return $list[$idx].Trim() } else { return $list[0].Trim() }
}

function Wsl-Run($distro, $cmd) {
  & wsl.exe -d $distro -- bash -lc $cmd
}

function Wsl-Install-Nats {
  $d = Wsl-Select-Distro; if (-not $d) { return }
  $ver = '2.10.19'
  $arch = (& wsl.exe -d $d -- uname -m).Trim()
  if ($arch -match 'aarch64|arm64') { $a = 'arm64' } else { $a = 'amd64' }
  $asset = "nats-server-v$ver-linux-$a.tar.gz"
  $url = "https://github.com/nats-io/nats-server/releases/download/v$ver/$asset"
  $cmd = @"
set -e
mkdir -p ~/.arw/nats/tmp ~/.arw/logs ~/.arw/run
cd ~/.arw/nats/tmp
if command -v curl >/dev/null 2>&1; then curl -L "$url" -o "$asset"; elif command -v wget >/dev/null 2>&1; then wget -O "$asset" "$url"; else echo 'need curl or wget' && exit 1; fi
tar -xzf "$asset"
f=
f=$(find . -type f -name nats-server | head -n1)
if [ -z "$f" ]; then echo 'nats-server not found in archive'; exit 1; fi
cp "$f" ~/.arw/nats/nats-server
chmod +x ~/.arw/nats/nats-server
"@
  Wsl-Run $d $cmd
  Info "Installed nats-server inside WSL:$d at ~/.arw/nats/nats-server"
}

function Wsl-Start-Nats {
  $d = Wsl-Select-Distro; if (-not $d) { return }
  $cmd = @"
mkdir -p ~/.arw/logs ~/.arw/run ~/.arw/nats
if [ ! -x ~/.arw/nats/nats-server ]; then echo 'nats-server missing; run Install NATS first'; exit 1; fi
nohup ~/.arw/nats/nats-server -a 0.0.0.0 -p 4222 > ~/.arw/logs/nats-server.out.log 2> ~/.arw/logs/nats-server.err.log < /dev/null &
echo $! > ~/.arw/run/nats-server.pid
"@
  Wsl-Run $d $cmd
  Info "Started nats-server in WSL:$d (listens on 0.0.0.0:4222)"
  Write-Host 'Connect from Windows: nats://127.0.0.1:4222'
  try {
    Add-Type -AssemblyName PresentationFramework -ErrorAction SilentlyContinue
    [void][System.Windows.MessageBox]::Show("WSL NATS started in '$d'\nConnect from Windows: nats://127.0.0.1:4222","NATS (WSL)")
  } catch {}
}

function Wsl-Stop-Nats {
  $d = Wsl-Select-Distro; if (-not $d) { return }
  $cmd = @"
if [ -f ~/.arw/run/nats-server.pid ]; then pid=$(cat ~/.arw/run/nats-server.pid); kill "$pid" 2>/dev/null || true; sleep 0.3; kill -9 "$pid" 2>/dev/null || true; else pkill -f nats-server 2>/dev/null || true; fi
"@
  Wsl-Run $d $cmd
  Info "Stopped nats-server in WSL:$d"
}

function Wsl-Show-Info {
  $d = Wsl-Select-Distro; if (-not $d) { return }
  $ver = (& wsl.exe --version 2>$null)
  $wslg = ($ver | Select-String -Pattern 'WSLg').Line
  $ip = (& wsl.exe -d $d -- bash -lc 'hostname -I 2>/dev/null | awk "{print $1}"').Trim()
  Write-Host ("WSL Distro: " + $d)
  if ($wslg) { Write-Host ("WSLg: " + $wslg) } else { Write-Host 'WSLg: not reported (requires Windows 11 + latest WSL)'}
  $ipDisplay = if ($null -ne $ip -and $ip -ne '') { $ip } else { 'unknown' }
  Write-Host ("WSL primary IP: " + $ipDisplay)
  Write-Host 'Windows connect: nats://127.0.0.1:4222 (localhost forwarding)'
  Write-Host 'WSL connect: nats://127.0.0.1:4222 (inside WSL)'
  Write-Host 'GUI note: On Windows 11, WSLg enables Linux GUI apps automatically.'
  Write-Host "To test GUI (if packages installed): wsl -d $d -- xclock (install via: sudo apt-get install -y x11-apps)"
  Read-Host 'Continue' | Out-Null
}

function Wsl-Open-Terminal {
  $d = Wsl-Select-Distro; if (-not $d) { return }
  # Prefer Windows Terminal (wt.exe), fallback to wsl.exe in conhost
  $wt = Get-Command wt.exe -ErrorAction SilentlyContinue
  if ($wt) {
    # Open wt in the selected distro, starting in home
    Start-Process -FilePath wt.exe -ArgumentList @('-w','0','-p',$d,'new-tab') | Out-Null
    Info ("Opened Windows Terminal in WSL:" + $d)
  } else {
    Start-Process -FilePath wsl.exe -ArgumentList @('-d',$d) | Out-Null
    Info ("Opened wsl.exe shell in WSL:" + $d)
  }
}

function Wsl-Set-Default {
  $d = Wsl-Select-Distro; if (-not $d) { return }
  try { & wsl.exe -s $d; Info ("Default WSL distro set to " + $d) } catch { Warn 'Failed to set default WSL distro' }
}

function Security-Tips {
  Banner 'Security Tips' 'Protect admin endpoints'
  Write-Host '  • Sensitive endpoints: /debug, /probe, /memory/*, /models/*, /governor/*, /introspect/*, /chat/*, /feedback/*'
  Write-Host '  • In development, ARW_DEBUG=1 is convenient; disable it otherwise.'
  Write-Host '  • Set ARW_ADMIN_TOKEN and send header: X-ARW-Admin: <token>'
  Write-Host '  • Adjust admin rate limiting via ARW_ADMIN_RL (default 60/60).'
  Write-Host '  • Consider a reverse proxy with TLS for multi-user environments.'
  Read-Host 'Continue' | Out-Null
}

Main-Menu
