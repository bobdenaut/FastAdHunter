# Session s3 (2026-09-03): the six declared dev-box bench targets re-run from the
# current checkout with the declared methodology, no arm changed.
#   01 cache, 02 matcher, 03 pipeline, 04 proxy (D1/D2 ^http_, D6/D7 splice),
#   05 intercept, 06 certs.
# Pinning per PERFORMANCE.md §Measuring reliably (post-F8):
#   CPU-bound microbenches (cache, matcher, pipeline, certs): one core,
#   ProcessorAffinity = 4, High priority, default worker count (as run-ab.ps1).
#   fah-http throughput/socket benches (proxy, intercept): four physical cores,
#   ProcessorAffinity = 0x55 + TOKIO_WORKER_THREADS=4 (as run-pinned4.ps1).
# A/B/A/B against the pre-phase-3 worktree (64be513) for the four benches that
# exist on both sides; tip-only for splice16, intercept, certs. The 64 KiB
# splice variant is NOT built: it needs a src const edit, forbidden this run.
# Harness note: proxy/intercept carry the 2026-09-02 ruleset + event wiring
# (phase3-audit §Fixes applied 1-2); figures are a new series, not comparable
# to r*/p4b* without saying so.
$ErrorActionPreference = 'Stop'
$Prefix = 'S3'
$Pre = 'E:\FastAdHunter-pre3'
$Tip = 'E:\FastAdHunter'
$Out = "$Tip\docs\code-review\phase3\p3-06-bench"

function Newest($pattern) {
  (Get-ChildItem "$Tip\target\release\deps\$pattern" | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
}

$PreExe = @{
  cache    = "$Pre\target\release\deps\cache-154e5f17eeed2bfe.exe"
  matcher  = "$Pre\target\release\deps\matcher-8741650bb0d6d70c.exe"
  pipeline = "$Pre\target\release\deps\pipeline-c4be41b4c82eea58.exe"
  proxy    = "$Pre\target\release\deps\proxy-b705efb52bf05727.exe"
}
# Exact executables from `cargo bench --no-run -p <crate> --bench <name>` on
# 2026-09-03 (build log in the s3 session record). pipeline needs the X2
# override CARGO_PROFILE_BENCH_DEBUG_ASSERTIONS=true, exactly as the p3-06
# session built it, or fah-api's compile_error! refuses the bench profile.
$TipExe = @{
  cache     = "$Tip\target\release\deps\cache-9593a339ee586848.exe"
  matcher   = "$Tip\target\release\deps\matcher-2870d630620b2540.exe"
  pipeline  = Newest 'pipeline-*.exe'
  proxy     = "$Tip\target\release\deps\proxy-5803552a0cd59340.exe"
  intercept = "$Tip\target\release\deps\intercept-6ca2c9d04bfd81a9.exe"
  certs     = "$Tip\target\release\deps\certs-ad0605c435fa4fa8.exe"
}

function Invoke-Bench($Root, $Exe, $Filter, $Log, [int]$Mask) {
  $started = Get-Date
  $args = @('--bench', '--noplot')
  if ($Filter -ne '') { $args += $Filter }
  $p = Start-Process -FilePath $Exe -ArgumentList $args `
    -WorkingDirectory $Root -NoNewWindow -PassThru `
    -RedirectStandardOutput "$Out\$Log.out.txt" -RedirectStandardError "$Out\$Log.err.txt"
  $p.ProcessorAffinity = $Mask
  $p.PriorityClass = 'High'
  $p.WaitForExit()
  $elapsed = [int]((Get-Date) - $started).TotalSeconds
  Add-Content -Path "$Out\session.log" -Value ("{0}  {1,-28} exit={2} {3}s (mask={4} workers={5})" -f (Get-Date -Format 'HH:mm:ss'), $Log, $p.ExitCode, $elapsed, $Mask, $env:TOKIO_WORKER_THREADS)
}

$rev = (git -C $Tip rev-parse --short HEAD)
Add-Content -Path "$Out\session.log" -Value ("session {0} start {1} (dev-box 01-06 re-run; tip {2} + working tree; A = 64be513; exes: {3})" -f $Prefix, (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'), $rev, (($TipExe.GetEnumerator() | Sort-Object Name | ForEach-Object { Split-Path $_.Value -Leaf }) -join ' '))

# 01-03: one core, A/B/A/B
foreach ($round in 1, 2) {
  Invoke-Bench $Pre $PreExe.cache    'dns_cache'                                                "$Prefix-r$round-A-cache"    4
  Invoke-Bench $Tip $TipExe.cache    'dns_cache'                                                "$Prefix-r$round-B-cache"    4
  Invoke-Bench $Pre $PreExe.matcher  'matcher_lookup'                                           "$Prefix-r$round-A-matcher"  4
  Invoke-Bench $Tip $TipExe.matcher  'matcher_lookup'                                           "$Prefix-r$round-B-matcher"  4
  Invoke-Bench $Pre $PreExe.pipeline 'full_pipeline/(blocked_query|forwarded_query_overhead)$' "$Prefix-r$round-A-pipeline" 4
  Invoke-Bench $Tip $TipExe.pipeline 'full_pipeline/(blocked_query|forwarded_query_overhead)$' "$Prefix-r$round-B-pipeline" 4
}

# 04-05: four physical cores, four workers
$env:TOKIO_WORKER_THREADS = '4'
foreach ($round in 1, 2) {
  Invoke-Bench $Pre $PreExe.proxy '^http_' "$Prefix-r$round-A-proxy" 85
  Invoke-Bench $Tip $TipExe.proxy '^http_' "$Prefix-r$round-B-proxy" 85
}
foreach ($round in 1, 2) {
  Invoke-Bench $Tip $TipExe.proxy     'https_sni_splice' "$Prefix-r$round-splice16"  85
  Invoke-Bench $Tip $TipExe.intercept ''                 "$Prefix-r$round-intercept" 85
}
Remove-Item Env:TOKIO_WORKER_THREADS

# 06: one core, tip only
foreach ($round in 1, 2) {
  Invoke-Bench $Tip $TipExe.certs '' "$Prefix-r$round-certs" 4
}

Add-Content -Path "$Out\session.log" -Value ("session {0} end {1}" -f $Prefix, (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'))
