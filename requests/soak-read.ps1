<#
.SYNOPSIS
  Capture one soak reading from the production FastAdHunter API.

.DESCRIPTION
  The 0.3.3 soak's headline metric, process_rss, is already sampled every
  360 s into /data/history/perf on the appliance, so it needs no help from
  this script. What this captures is what history does NOT hold:

    - cpu_user_ms / cpu_system_ms from /api/v1/debug/memory. Cumulative
      counters, never sampled into history; without periodic endpoints you
      cannot compute CPU over any window.
    - a dated snapshot of telemetry and stats, so one lost file does not
      cost the whole soak.

  One HTTPS request per endpoint. It will not perturb the soak - the risk to
  this run is load generators and restarts, not a read.

.PARAMETER Marker
  Filename marker. Defaults to t<N>, N being whole days since T0, matching
  the soak-0.2.18 convention (t0, t1, ... tend12h, tend). Pass 'tend12h' or
  'tend' explicitly for the closing captures.

.PARAMETER OutDir
  Where the files land. Defaults beside the T0 capture.

.EXAMPLE
  ./soak-read.ps1
  ./soak-read.ps1 -Marker tend
#>

[CmdletBinding()]
param(
    [string] $Marker,
    [string] $OutDir,
    [string] $Base    = 'https://fah-api.localbox.ro:8443',
    [string] $KeyFile,
    [string] $Version = '0.3.3',
    [switch] $Force
)

$ErrorActionPreference = 'Stop'

# Resolved here, not in the param block: $PSScriptRoot is not populated during
# parameter binding under Windows PowerShell 5.1.
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if (-not $OutDir)  { $OutDir  = Join-Path $repo 'docs\code-review\phase3\soak-0.3.3' }
if (-not $KeyFile) { $KeyFile = Join-Path $repo '.vscode\settings.json' }

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# T0 of the 0.3.3 soak: container start in the router log. Day 7 = 2026-09-16.
$T0 = [datetimeoffset]::Parse('2026-09-09T11:15:15Z')

if (-not $Marker) {
    $days   = [int][math]::Floor(([datetimeoffset]::UtcNow - $T0).TotalDays)
    $Marker = "t$days"
}

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
$stamp = [datetimeoffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')

# A capture is a measurement, not a draft. Refuse to overwrite one - the day-0
# marker collides with the T0 baseline, and silently clobbering it would
# replace the soak's zero point with a reading taken hours later.
$clash = Get-ChildItem -Path $OutDir -Filter "soak-$Version-$Marker-*" -ErrorAction SilentlyContinue
if ($clash -and -not $Force) {
    throw ("marker '$Marker' already has {0} files in $OutDir. " -f $clash.Count) +
          "Pass -Marker with a distinct name, or -Force to overwrite."
}

# /health sits at the root, not under /api/v1.
$endpoints = [ordered]@{
    'health'       = "$Base/health"
    'debug-memory' = "$Base/api/v1/debug/memory"
    'telemetry'    = "$Base/api/v1/telemetry"
    'stats'        = "$Base/api/v1/stats"
    'history-perf' = "$Base/api/v1/history/perf"
}

$failed = @()
foreach ($name in $endpoints.Keys) {
    $out = Join-Path $OutDir "soak-$Version-$Marker-$name.json"
    try {
        Invoke-WebRequest -Uri $endpoints[$name] -Headers $headers `
            -TimeoutSec 30 -UseBasicParsing -OutFile $out
    } catch {
        $failed += "$name : $($_.Exception.Message)"
    }
}

Set-Content -Path (Join-Path $OutDir "soak-$Version-$Marker-timestamp.txt") `
            -Value $stamp -NoNewline

# Summary line, so a read that silently returned nothing is visible.
$mem = Get-Content (Join-Path $OutDir "soak-$Version-$Marker-debug-memory.json") -Raw |
       ConvertFrom-Json
$elapsed = [math]::Round(([datetimeoffset]::UtcNow - $T0).TotalDays, 2)

"marker   : $Marker  ($elapsed days since T0, day 7 = 2026-09-16)"
"captured : $stamp"
"rss      : {0:N0} bytes ({1:N1} MiB)" -f $mem.process_rss, ($mem.process_rss / 1MB)
"cpu      : user $($mem.cpu_user_ms) ms, system $($mem.cpu_system_ms) ms"
"files    : $OutDir"

if ($failed) {
    Write-Warning "endpoints that failed:`n  $($failed -join "`n  ")"
    exit 1
}
