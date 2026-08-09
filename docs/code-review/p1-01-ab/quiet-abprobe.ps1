# Real-corpus A/B, three arms, quiet box.
#   a   = baseline 689d9c5
#   cur = working tree (M1-M3 + m1-m4)
#   b   = baseline + one never-called pub fn -> layout floor for these metrics
#
# Arm order alternates direction per iteration so drift cannot favour one arm.
param(
  # Worktrees holding the two baseline arms; see abprobe.rs for how to make them.
  [string]$BaseA = "$PSScriptRoot\..\..\..\..\ab-base-a",
  [string]$BaseB = "$PSScriptRoot\..\..\..\..\ab-base-b",
  [string]$Current = 'E:\FastAdHunter',
  [string]$Corpus = 'E:\FastAdHunter\.probe-corpus\deployed',
  [int]$Iterations = 11
)
$ErrorActionPreference = 'Stop'
$corpus = $Corpus
$iterations = $Iterations

$exe = @{
  a   = "$BaseA\target\release\examples\abprobe.exe"
  cur = "$Current\target\release\examples\abprobe.exe"
  b   = "$BaseB\target\release\examples\abprobe.exe"
}
foreach ($k in $exe.Keys) { if (-not (Test-Path $exe[$k])) { throw "missing $($exe[$k])" } }

function Invoke-Probe([string]$Arm, [string]$Mode) {
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $exe[$Arm]
  $psi.Arguments = "`"$corpus`" $Mode"
  $psi.RedirectStandardOutput = $true
  $psi.UseShellExecute = $false
  $p = [System.Diagnostics.Process]::Start($psi)
  try { $p.ProcessorAffinity = [IntPtr]15; $p.PriorityClass = 'High' } catch { }
  $peak = 0
  while (-not $p.HasExited) {
    Start-Sleep -Milliseconds 40
    try { $p.Refresh(); if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 } } catch { }
  }
  $out = $p.StandardOutput.ReadToEnd()
  $out -split "`r?`n" | Where-Object { $_ -match '^\[ab\]' }
  "[ab] peak_working_set_bytes=$peak"
}

foreach ($mode in @('phases', 'boot')) {
  for ($i = 1; $i -le $iterations; $i++) {
    $order = if ($i % 2 -eq 1) { @('a', 'cur', 'b') } else { @('b', 'cur', 'a') }
    foreach ($arm in $order) {
      Write-Output "=== iter=$i arm=$arm mode=$mode ==="
      Invoke-Probe $arm $mode
    }
  }
}
Write-Output 'ABDONE'
