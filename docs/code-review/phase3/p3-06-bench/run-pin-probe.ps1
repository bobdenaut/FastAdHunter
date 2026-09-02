# F8 pinning probe (2026-09-02): the pinned-4 session (mask 15) made every
# socket arm 2-10x slower than unpinned. Two suspects, tested separately:
#   (a) tokio spawns one worker per machine CPU (32 here) inside a 4-CPU mask
#       -> oversubscription; fixed by TOKIO_WORKER_THREADS=4.
#   (b) mask 15 = logical CPUs 0-3 = two physical P-cores with HT on the
#       i9-13980HX (8P+16E, 32 logical); mask 0x55 = CPUs 0,2,4,6 = four
#       distinct P-cores.
$ErrorActionPreference = 'Stop'
$Tip = 'E:\FastAdHunter'
$Out = "$Tip\docs\code-review\phase3\p3-06-bench"
$Intercept = "$Tip\target\release\deps\intercept-6ca2c9d04bfd81a9.exe"
$Proxy = "$Tip\target\release\deps\proxy-5803552a0cd59340.exe"

function Invoke-Probe($Exe, $Filter, $Log, $Mask, $Workers) {
  if ($Workers -ne '') { $env:TOKIO_WORKER_THREADS = $Workers } else { Remove-Item Env:TOKIO_WORKER_THREADS -ErrorAction SilentlyContinue }
  $started = Get-Date
  $p = Start-Process -FilePath $Exe -ArgumentList @('--bench', '--noplot', $Filter) `
    -WorkingDirectory $Tip -NoNewWindow -PassThru `
    -RedirectStandardOutput "$Out\$Log.out.txt" -RedirectStandardError "$Out\$Log.err.txt"
  $p.ProcessorAffinity = $Mask
  $p.PriorityClass = 'High'
  $p.WaitForExit()
  $elapsed = [int]((Get-Date) - $started).TotalSeconds
  Add-Content -Path "$Out\session.log" -Value ("{0}  {1,-32} exit={2} {3}s (mask={4} workers={5})" -f (Get-Date -Format 'HH:mm:ss'), $Log, $p.ExitCode, $elapsed, $Mask, $Workers)
}

Add-Content -Path "$Out\session.log" -Value ("pin probe start {0}" -f (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'))
Invoke-Probe $Intercept 'https_handshake'   'probe-mask15-w4-handshake'   15 '4'
Invoke-Probe $Proxy     '^http_pass_through' 'probe-mask15-w4-passthrough' 15 '4'
Invoke-Probe $Intercept 'https_handshake'   'probe-mask55-w4-handshake'   85 '4'
Invoke-Probe $Proxy     '^http_pass_through' 'probe-mask55-w4-passthrough' 85 '4'
Invoke-Probe $Intercept 'https_handshake'   'probe-mask55-w32-handshake'  85 ''
Add-Content -Path "$Out\session.log" -Value ("pin probe end {0}" -f (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'))
