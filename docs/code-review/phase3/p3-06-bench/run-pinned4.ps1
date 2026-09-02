# p3-06 F8 re-run: the fah-http throughput/socket arms pinned to four cores.
# Second session (p4b-*): ProcessorAffinity = 0x55 (logical CPUs 0,2,4,6 = four
# distinct P-cores on the i9-13980HX, no HT siblings) + TOKIO_WORKER_THREADS=4
# so the runtime is sized to the mask. The first session (p4-*, mask 15 = two
# P-cores' hyperthreads, 32 workers) measured oversubscription and is kept as
# the record of the trap (run-pin-probe.ps1). Same A/B/A/B shape as run-ab.ps1.
# A = pre-phase-3 checkout 64be513 (worktree ..\FastAdHunter-pre3), B = phase3-06 tip
# with the post-review fixes (listener counters; F6 harness change reverted).
$ErrorActionPreference = 'Stop'
$env:TOKIO_WORKER_THREADS = '4'
$Mask = 85
$Prefix = 'p4b'
$Pre = 'E:\FastAdHunter-pre3'
$Tip = 'E:\FastAdHunter'
$Out = "$Tip\docs\code-review\phase3\p3-06-bench"

$PreProxy   = "$Pre\target\release\deps\proxy-b705efb52bf05727.exe"
# Both tip proxy variants from `cargo bench --no-run -p fah-http --bench proxy`:
# 16 KiB in place, 64 KiB copied aside before the const was restored.
$TipProxy16 = "$Tip\target\release\deps\proxy-5803552a0cd59340.exe"
$TipProxy64 = "$Tip\target\release\deps\proxy-splice64-p4.exe"
$TipIntercept = (Get-ChildItem "$Tip\target\release\deps\intercept-*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName

function Invoke-Bench($Root, $Exe, $Filter, $Log) {
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

Add-Content -Path "$Out\session.log" -Value ("pinned-4 session ({0}) start {1} (F8 re-run; mask={2} workers={3}; intercept exe {4})" -f $Prefix, (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'), $Mask, $env:TOKIO_WORKER_THREADS, (Split-Path $TipIntercept -Leaf))

foreach ($round in 1, 2) {
  Invoke-Bench $Pre $PreProxy   '^http_' "$Prefix-r$round-A-proxy"
  Invoke-Bench $Tip $TipProxy16 '^http_' "$Prefix-r$round-B-proxy"
}

foreach ($round in 1, 2) {
  Invoke-Bench $Tip $TipProxy16 'https_sni_splice' "$Prefix-r$round-splice16"
  Invoke-Bench $Tip $TipProxy64 'https_sni_splice' "$Prefix-r$round-splice64"
}

foreach ($round in 1, 2) {
  Invoke-Bench $Tip $TipIntercept '' "$Prefix-r$round-intercept"
}

Add-Content -Path "$Out\session.log" -Value ("pinned-4 session ({0}) end {1}" -f $Prefix, (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'))
