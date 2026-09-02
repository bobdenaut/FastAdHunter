# p3-06 dev-box A/B session. Pre-declared in the review file before this ran.
# A = pre-phase-3 checkout 64be513 (worktree ..\FastAdHunter-pre3), B = phase3-06 tip.
# Interleaved A/B/A/B; CPU-bound microbenches pinned to one core at High priority,
# fah-http benches unpinned (PERFORMANCE.md §Measuring reliably).
$ErrorActionPreference = 'Stop'
$Pre = 'E:\FastAdHunter-pre3'
$Tip = 'E:\FastAdHunter'
$Out = "$Tip\docs\code-review\phase3\p3-06-bench"

$PreExe = @{
  cache    = "$Pre\target\release\deps\cache-154e5f17eeed2bfe.exe"
  matcher  = "$Pre\target\release\deps\matcher-8741650bb0d6d70c.exe"
  pipeline = "$Pre\target\release\deps\pipeline-c4be41b4c82eea58.exe"
  proxy    = "$Pre\target\release\deps\proxy-b705efb52bf05727.exe"
}
$TipExe = @{
  cache     = "$Tip\target\release\deps\cache-bf10defca99ac4da.exe"
  matcher   = "$Tip\target\release\deps\matcher-9d08b25c059f6b2d.exe"
  pipeline  = "$Tip\target\release\deps\pipeline-19d6e1534d291626.exe"
  proxy     = "$Tip\target\release\deps\proxy-11b0f50089530489.exe"
  # SPLICE_BUF A/B: both variants from the same single-package build command
  # (`cargo bench --no-run -p fah-http --bench proxy`), 16 KiB in place,
  # 64 KiB copied aside before the const was restored.
  proxy16   = "$Tip\target\release\deps\proxy-5803552a0cd59340.exe"
  proxy64   = "$Tip\target\release\deps\proxy-splice64.exe"
  intercept = "$Tip\target\release\deps\intercept-8ee203ae7c0a3ce0.exe"
  certs     = "$Tip\target\release\deps\certs-8535341c917c6586.exe"
}

function Invoke-Bench($Root, $Exe, $Filter, $Log, [bool]$Pin) {
  $started = Get-Date
  $p = Start-Process -FilePath $Exe -ArgumentList @('--bench', '--noplot', $Filter) `
    -WorkingDirectory $Root -NoNewWindow -PassThru `
    -RedirectStandardOutput "$Out\$Log.out.txt" -RedirectStandardError "$Out\$Log.err.txt"
  if ($Pin) {
    $p.ProcessorAffinity = 4
    $p.PriorityClass = 'High'
  }
  $p.WaitForExit()
  $elapsed = [int]((Get-Date) - $started).TotalSeconds
  Add-Content -Path "$Out\session.log" -Value ("{0}  {1,-28} exit={2} {3}s" -f (Get-Date -Format 'HH:mm:ss'), $Log, $p.ExitCode, $elapsed)
}

Set-Content -Path "$Out\session.log" -Value ("session start {0}" -f (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'))

foreach ($round in 1, 2) {
  Invoke-Bench $Pre $PreExe.cache    'dns_cache'                                            "r$round-A-cache"    $true
  Invoke-Bench $Tip $TipExe.cache    'dns_cache'                                            "r$round-B-cache"    $true
  Invoke-Bench $Pre $PreExe.matcher  'matcher_lookup'                                       "r$round-A-matcher"  $true
  Invoke-Bench $Tip $TipExe.matcher  'matcher_lookup'                                       "r$round-B-matcher"  $true
  Invoke-Bench $Pre $PreExe.pipeline 'full_pipeline/(blocked_query|forwarded_query_overhead)$' "r$round-A-pipeline" $true
  Invoke-Bench $Tip $TipExe.pipeline 'full_pipeline/(blocked_query|forwarded_query_overhead)$' "r$round-B-pipeline" $true
  Invoke-Bench $Pre $PreExe.proxy    '^http_'                                               "r$round-A-proxy"    $false
  Invoke-Bench $Tip $TipExe.proxy    '^http_'                                               "r$round-B-proxy"    $false
}

foreach ($round in 1, 2) {
  Invoke-Bench $Tip $TipExe.proxy16 'https_sni_splice' "r$round-splice16" $false
  Invoke-Bench $Tip $TipExe.proxy64 'https_sni_splice' "r$round-splice64" $false
}

foreach ($round in 1, 2) {
  Invoke-Bench $Tip $TipExe.intercept '' "r$round-intercept" $false
  Invoke-Bench $Tip $TipExe.certs     '' "r$round-certs"     $true
}

Add-Content -Path "$Out\session.log" -Value ("session end {0}" -f (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'))
