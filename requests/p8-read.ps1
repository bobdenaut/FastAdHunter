<#
.SYNOPSIS
  Capture one P8 CPU spot read from the RB5009, plus the load it ran under.

.DESCRIPTION
  P8 asks what full-mode interception costs the device's CPU envelope under
  household browsing. Its statistic is the `fastadhunter` process share from
  RouterOS `/tool/profile`, taken at matched hours before and after the
  full-mode deploy - the API cannot supply it, because the shipped telemetry
  carries no CPU seconds.

  Each run captures three things:

    - /tool/profile cpu=all duration=10s, read-only, over SSH.
    - /api/v1/telemetry and /api/v1/stats from the same moment, which supply
      the query rate the CPU figure has to be read against. A share means
      nothing without the load that produced it.

  Matched hours are the whole comparison, so the filename carries the local
  hour and the script refuses a second read in the same hour bucket. Run it
  at the four declared hours - 09, 13, 19, 21 local - and again at the same
  four after the deploy.

  Read-only throughout. `/tool/profile` samples, it does not configure, and
  three HTTPS GETs will not perturb the soak.

.PARAMETER Hour
  Hour bucket, two digits local. Defaults to the current local hour.

.PARAMETER Phase
  Which half of the comparison this read belongs to: 'before' the full-mode
  deploy or 'after' it. Defaults to 'before'.

.PARAMETER OutDir
  Where the files land. Defaults beside the T0 capture, under p8/.

.EXAMPLE
  ./p8-read.ps1
  ./p8-read.ps1 -Hour 19
  ./p8-read.ps1 -Phase after
#>

[CmdletBinding()]
param(
    [ValidatePattern('^\d{2}$')]
    [string] $Hour,
    [ValidateSet('before', 'after')]
    [string] $Phase   = 'before',
    [string] $OutDir,
    [string] $Base    = 'https://fah-api.localbox.ro:8443',
    [string] $KeyFile,
    [string] $SshHost = 'bobdenaut',
    [int]    $SshPort = 2202,
    [int]    $Duration = 10,
    [switch] $Force
)

$ErrorActionPreference = 'Stop'

# Resolved here, not in the param block: $PSScriptRoot is not populated during
# parameter binding under Windows PowerShell 5.1.
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if (-not $OutDir)  { $OutDir  = Join-Path $repo 'docs\code-review\phase3\soak-0.3.3\p8' }
if (-not $KeyFile) { $KeyFile = Join-Path $repo '.vscode\settings.json' }

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$now = [datetimeoffset]::Now
if (-not $Hour) { $Hour = $now.ToString('HH') }

$stamp  = [datetimeoffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
$marker = "$Phase-h$Hour"

# The API key lives in settings.json under the REST Client extension's
# $shared environment. settings.json is JSONC, so line comments are stripped
# before parsing - ConvertFrom-Json rejects them.
$raw = (Get-Content $KeyFile -Raw) -replace '(?m)^\s*//.*$', ''
$key = ($raw | ConvertFrom-Json).'rest-client.environmentVariables'.'$shared'.apiKey
if (-not $key) {
    throw "no rest-client.environmentVariables.`$shared.apiKey in $KeyFile"
}
$headers = @{ Authorization = "Bearer $key" }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# One read per hour bucket per phase. A second read in the same hour would
# quietly replace the one the comparison is built on.
$clash = Get-ChildItem -Path $OutDir -Filter "p8-$marker-*" -ErrorAction SilentlyContinue
if ($clash -and -not $Force) {
    throw ("bucket '$marker' already has {0} files in $OutDir. " -f $clash.Count) +
          "Pass -Hour with a distinct bucket, or -Force to overwrite."
}

# Read-only. MSYS2_ARG_CONV_EXCL is not needed from PowerShell, but the
# leading slash is why the same command must carry it from Git Bash.
#
# ssh writes its post-quantum notice to stderr on every connection, and under
# ErrorActionPreference = 'Stop' PowerShell turns any native stderr line into
# a terminating error. -q silences the notice; the filter drops whatever else
# arrives on stderr so a warning cannot masquerade as profile output. The exit
# code stays the authority on whether the read worked.
$profileOut = Join-Path $OutDir "p8-$marker-profile.txt"
$eap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$profileText = & ssh -q -p $SshPort $SshHost "/tool/profile cpu=all duration=${Duration}s" 2>&1 |
    Where-Object { $_ -isnot [System.Management.Automation.ErrorRecord] }
$sshExit = $LASTEXITCODE
$ErrorActionPreference = $eap
if ($sshExit -ne 0) {
    throw "ssh to $SshHost`:$SshPort failed with exit code $sshExit"
}
Set-Content -Path $profileOut -Value $profileText -Encoding utf8

$failed = @()
$endpoints = [ordered]@{
    'telemetry' = "$Base/api/v1/telemetry"
    'stats'     = "$Base/api/v1/stats"
}
foreach ($name in $endpoints.Keys) {
    $out = Join-Path $OutDir "p8-$marker-$name.json"
    try {
        Invoke-WebRequest -Uri $endpoints[$name] -Headers $headers `
            -TimeoutSec 30 -UseBasicParsing -OutFile $out
    } catch {
        $failed += "$name : $($_.Exception.Message)"
    }
}

Set-Content -Path (Join-Path $OutDir "p8-$marker-timestamp.txt") `
            -Value "$stamp  local hour $Hour  phase $Phase" -NoNewline

# Summary line, so a read that resolved nothing is visible as such rather
# than passing for a zero. /tool/profile resolves 0.5% per core, so a share
# reported as 0% is a bound, not a measurement.
$shares = Select-String -Path $profileOut -Pattern '^\s*fastadhunter\s' |
          ForEach-Object { ($_.Line -split '\s+' | Where-Object { $_ -match '%$' }) } |
          ForEach-Object { [double]($_ -replace '%', '') }
$peak = if ($shares) { ($shares | Measure-Object -Maximum).Maximum } else { $null }

$tel = Get-Content (Join-Path $OutDir "p8-$marker-telemetry.json") -Raw | ConvertFrom-Json
$dns = $tel.counters.dns
$queries = $dns.pass + $dns.allow + $dns.block
$rate = if ($tel.process.uptime_seconds) { $queries / $tel.process.uptime_seconds } else { 0 }

"bucket   : $marker"
"captured : $stamp (local hour $Hour)"
"samples  : {0} fastadhunter rows" -f $shares.Count
"peak     : {0}" -f $(if ($null -eq $peak) { 'no fastadhunter row in the profile' }
                      elseif ($peak -eq 0)  { '0% - below the 0.5% instrument floor' }
                      else                  { "$peak% of one core" })
"load     : {0:N0} queries over {1:N0} s uptime, {2:N2} q/s mean" -f $queries, $tel.process.uptime_seconds, $rate
"files    : $OutDir"

if ($failed) {
    Write-Warning "endpoints that failed:`n  $($failed -join "`n  ")"
    exit 1
}
