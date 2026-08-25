$ErrorActionPreference = 'Stop'

$root    = 'C:\Users\liviu\AppData\Local\Temp\claude\e--FastAdHunter\ca46d503-25e5-4704-9082-fdbce9f7f4e7\scratchpad'
$out     = Join-Path $root 'm8-session'
$post    = 'E:\FastAdHunter\target\release\deps\upstream-ea19378b095aff70.exe'
$control = Join-Path $root 'pre786e8c8\target\release\deps\upstream-e66f4ae0cec9d942.exe'

New-Item -ItemType Directory -Force -Path $out | Out-Null

function Run-Pinned($exe, $stdout, $stderr) {
    $args = @('--bench', '--sample-size', '200', 'upstream/forward_udp_answered')
    $p = Start-Process -FilePath $exe -ArgumentList $args -PassThru -NoNewWindow `
                       -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        $p.ProcessorAffinity = 4
        $p.PriorityClass = 'High'
    } catch {
        "affinity/priority set failed: $_" | Out-File -Append -FilePath $stderr
    }
    $p.WaitForExit()
    return $p.ExitCode
}

# Pair 0 is the pre-declared discarded warm-up; pairs 1..8 are measured.
foreach ($i in 0..8) {
    $tag = '{0:d2}' -f $i
    $rc1 = Run-Pinned $post    (Join-Path $out "pair$tag-post.out")    (Join-Path $out "pair$tag-post.err")
    $rc2 = Run-Pinned $control (Join-Path $out "pair$tag-control.out") (Join-Path $out "pair$tag-control.err")
    "pair $tag done post=$rc1 control=$rc2" | Tee-Object -Append -FilePath (Join-Path $out 'progress.txt')
}

'SESSION COMPLETE' | Tee-Object -Append -FilePath (Join-Path $out 'progress.txt')
