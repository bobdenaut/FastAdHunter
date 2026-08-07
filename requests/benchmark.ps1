<#
.SYNOPSIS
  Reproducible Phase 1 benchmark against a live FastAdHunter instance.

.DESCRIPTION
  Clears every list, re-imports them one at a time, and records the numbers
  that PERFORMANCE.md budgets: registration latency, fetch+compile time, rule
  count, ruleset heap, and query latency. Prints one table per section so runs
  are directly comparable across builds.

  RSS is NOT collected here — it lives on the router, not in the API. Read it
  alongside each step with, on RouterOS:

      /container/print detail where name~"fastadhunter"

  Per-import COMPILE time is not collected either, but the whole-ruleset
  figure is: ruleset.compile_duration_seconds on /api/v1/telemetry is the last
  compile's real wall time.
  'Fetch+compile' below is wall-clock for both together, which is the figure
  earlier runs recorded. Startup compile time comes from the boot log.

.PARAMETER Host
  Base URL of the API, e.g. https://172.17.0.2:8443

.PARAMETER ApiKey
  Bearer token. Printed once on first boot.

.EXAMPLE
  ./benchmark.ps1 -Host https://172.17.0.2:8443 -ApiKey 74ceb...

.NOTES
  DESTRUCTIVE: deletes every configured list, including their /data cache
  copies. That is the point — the benchmark measures a cold import.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ApiHost,
    [Parameter(Mandatory)][string]$ApiKey
)

$ErrorActionPreference = 'Stop'

# The API certificate is self-signed by rcgen on first boot (SECURITY.md) and
# is not meant to chain to a public root.
$script:Common = @{
    Headers            = @{ Authorization = "Bearer $ApiKey" }
    SkipCertificateCheck = $true
}
$base = "$ApiHost/api/v1"

# The lists under test, in import order. Ordered so the ruleset grows from
# smallest to largest, which makes the per-list heap deltas readable.
$Lists = @(
    @{ id = 'oisd-basic';  url = 'https://small.oisd.nl' }
    @{ id = 'hosts';       url = 'https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts' }
    @{ id = '1hosts-xtra'; url = 'https://raw.githubusercontent.com/badmojr/1Hosts/master/Xtra/hosts.txt' }
)

function Get-Telemetry {
    Invoke-RestMethod "$base/telemetry" @script:Common
}

# Live rule counts, straight from the engine. `GET /lists` reports what each
# list contributes to the ruleset that is *currently serving*, with no polling
# delay — unlike /telemetry's ruleset block, which the binary refreshes on a
# 10 s timer (TELEMETRY_POLL). Reading it right after an import returns the
# PREVIOUS ruleset; that is what made the first run of this script nonsense.
function Get-ListRules {
    $items = (Invoke-RestMethod "$base/lists" @script:Common).items
    [pscustomobject]@{
        Items = $items
        Total = ($items | Measure-Object -Property rules_total -Sum).Sum
    }
}

# /telemetry's ruleset block is refreshed on the binary's 10 s poll, so wait
# until its rule count agrees with the engine's live count — only then does the
# heap figure belong to the ruleset we just built.
function Wait-ForGauge {
    param([int]$ExpectedRules, [int]$TimeoutSeconds = 60)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $t = Get-Telemetry
        if ([int]$t.ruleset.rules -eq $ExpectedRules) { return $t }
        Start-Sleep -Milliseconds 500
    }
    throw "ruleset gauge did not reach $ExpectedRules rules within $TimeoutSeconds s"
}

function Wait-ForRefresh {
    param([string]$Id, [int]$TimeoutSeconds = 300)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $list = (Invoke-RestMethod "$base/lists" @script:Common).items |
                Where-Object { $_.id -eq $Id }
        if ($list.last_status -ne 'never') { return $list }
        Start-Sleep -Milliseconds 200
    }
    throw "list $Id did not finish refreshing within $TimeoutSeconds s"
}

# Write-Host "`n=== Clearing every configured list ===" -ForegroundColor Cyan
# foreach ($list in (Invoke-RestMethod "$base/lists" @script:Common).items) {
#     Invoke-RestMethod -Method Delete "$base/lists/$($list.id)" @script:Common | Out-Null
#     Write-Host "  deleted $($list.id)"
# }

$baseline = Wait-ForGauge -ExpectedRules 0
Write-Host ("`nBaseline: {0} rules, {1:N0} B ruleset heap" -f `
    $baseline.ruleset.rules, $baseline.memory.ruleset_bytes)

Write-Host "`n=== Importing ===" -ForegroundColor Cyan
$rows = @()
$prevHeap = $baseline.memory.ruleset_bytes
$prevRules = $baseline.ruleset.rules

foreach ($spec in $Lists) {
    # 1. Registration: validate, persist to fastadhunter.toml, register in the
    #    engine. No network — this is the admin-plane write path.
    $body = @{ url = $spec.url; id = $spec.id } | ConvertTo-Json
    $register = Measure-Command {
        Invoke-RestMethod -Method Post "$base/lists" -Body $body `
            -ContentType 'application/json' @script:Common | Out-Null
    }

    # 2. Fetch + compile: download, parse, build the matcher, atomic swap.
    $fetch = Measure-Command {
        Invoke-RestMethod -Method Post "$base/lists/$($spec.id)/refresh" @script:Common | Out-Null
        $status = Wait-ForRefresh -Id $spec.id
    }
    if ($status.last_status -ne 'ok') {
        Write-Warning "$($spec.id) refresh reported '$($status.last_status)'"
    }

    # Live truth first, then block until the polled heap gauge catches up to it.
    $live  = Get-ListRules
    $rules = [int]$live.Total
    $t     = Wait-ForGauge -ExpectedRules $rules
    $heap  = $t.memory.ruleset_bytes

    $rows += [pscustomobject]@{
        List             = $spec.id
        'Register (ms)'  = [math]::Round($register.TotalMilliseconds, 1)
        'Fetch+compile (ms)' = [math]::Round($fetch.TotalMilliseconds, 1)
        'List rules'     = $status.rules_total
        'Ruleset rules'  = $rules
        'Heap (MB)'      = [math]::Round($heap / 1MB, 2)
        'Heap (B)'       = $heap
        'B/rule (incr.)' = if ($rules -gt $prevRules) {
                               [math]::Round(($heap - $prevHeap) / ($rules - $prevRules), 2)
                           } else { 'n/a' }
    }
    $prevHeap = $heap; $prevRules = $rules
}

$rows | Format-Table -AutoSize

Write-Host "=== Query latency ===" -ForegroundColor Cyan
Write-Host "Engine-side duration is counted per stage; run some traffic first."
Write-Host "  Blocked:   Resolve-DnsName doubleclick.net    -Server <container-ip>"
Write-Host "  Cache hit: Resolve-DnsName example.com        -Server <container-ip>  (twice)"
Write-Host ""
Write-Host "latency.dns.{block,cache_hit,forward} carry count + sum_seconds. Two"
Write-Host "reads, delta both, divide -> the mean over that interval:"
Write-Host "  Invoke-RestMethod `"$base/telemetry`" -Headers @{Authorization='Bearer <key>'} -SkipCertificateCheck"
Write-Host ""
Write-Host "For percentiles use the windowed series instead:"
Write-Host "  Invoke-RestMethod `"$base/history/perf?fields=latency`" -Headers @{Authorization='Bearer <key>'} -SkipCertificateCheck"
Write-Host ""
Write-Host "=== Startup ===" -ForegroundColor Cyan
Write-Host "Restart the container, then read the boot log. Startup at this rule"
Write-Host "count is the number to compare across builds — compile dominates it."
Write-Host "  /container/stop  [find name~`"fastadhunter`"]"
Write-Host "  /container/start [find name~`"fastadhunter`"]"
Write-Host "  /log/print where message~`"listening`""
