# Whole A/B matrix in one script: no bash -> PowerShell argument passing, which
# is what broke the previous attempt.
#
# Arms: a = baseline 689d9c5, cur = working tree, b = baseline + one unused pub
# fn (identical behaviour, different code layout — the layout probe).

param(
  [string]$BaseA = "$PSScriptRoot\..\..\..\..\ab-base-a",
  [string]$BaseB = "$PSScriptRoot\..\..\..\..\ab-base-b",
  [string]$Current = 'E:\FastAdHunter',
  [string]$OutDir = $PSScriptRoot
)
$ErrorActionPreference = 'Stop'
$SP = $OutDir
$roots = @{ a = $BaseA; cur = $Current; b = $BaseB }
$exes = @{
  pipeline = 'target\release\deps\pipeline-a3c196dca33b04c2.exe'
  matcher  = 'target\release\deps\matcher-5e50475f0e0e2df5.exe'
}
# pipeline runs a 4-worker throughput bench; matcher is a single-threaded
# microbench (PERFORMANCE.md §Measuring reliably).
$affinity = @{ pipeline = 15; matcher = 4 }

function Invoke-Bench([string]$Arm, [string]$Bench, [int]$Pass) {
  $exe = Join-Path $roots[$Arm] $exes[$Bench]
  if (-not (Test-Path $exe)) { throw "missing $exe" }
  $out = "$SP\q-$Bench-$Arm-$Pass.txt"

  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $exe
  $psi.Arguments = '--bench'
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.UseShellExecute = $false
  $psi.WorkingDirectory = $roots[$Arm]

  $p = [System.Diagnostics.Process]::Start($psi)
  try { $p.ProcessorAffinity = [IntPtr]$affinity[$Bench]; $p.PriorityClass = 'High' } catch { }

  $stdout = $p.StandardOutput.ReadToEndAsync()
  $stderr = $p.StandardError.ReadToEndAsync()
  $peak = 0
  while (-not $p.HasExited) {
    Start-Sleep -Milliseconds 200
    try { $p.Refresh(); if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 } } catch { }
  }
  $p.WaitForExit()

  Set-Content -Path $out -Value ($stdout.Result + "`n" + $stderr.Result) -Encoding UTF8
  Add-Content -Path $out -Value "peak_working_set_bytes=$peak" -Encoding UTF8
  Write-Output "done $Bench $Arm pass$Pass -> $out"
}

# Arm order is mirrored between passes so a drifting box cannot favour one arm.
foreach ($arm in @('a', 'cur', 'b')) { Invoke-Bench $arm 'pipeline' 1 }
foreach ($arm in @('b', 'cur', 'a')) { Invoke-Bench $arm 'matcher' 1 }
foreach ($arm in @('a', 'cur', 'b')) { Invoke-Bench $arm 'matcher' 2 }
Write-Output 'QUIETDONE'
