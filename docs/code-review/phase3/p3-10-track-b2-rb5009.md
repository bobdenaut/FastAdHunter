# p3-10 Track B — RB5009 figures

Task: [plan/wip/phase3/p3-10-post-merge-performance.md](../../../plan/wip/phase3/p3-10-post-merge-performance.md)

This file holds every measurement taken on the router. Two kinds live here and
they are not interchangeable:

- **Probe measurements** — a second container, self-contained, no deploy
  needed. The splice section below is one. They answer "what does this code cost
  on this CPU".
- **The B2 sweep** — N=0/2/3/4 under the shipped workload with `:443` steered.
  **Still BLOCKED**: it needs the merged build deployed and p3-11's dst-nat 443
  rule. Nothing below is the sweep.

| | |
| --- | --- |
| Device | MikroTik RB5009, RouterOS 7.21.5, 4× ARMv8, 1 GiB shared with RouterOS |
| Probe | `fah-probe`, `veth3`, image built from `Dockerfile.splicebench` at `4caba28` |
| Date | 2026-09-14 |
| Live container | `fastadhunter-0.3.4` never stopped; it held `veth1` throughout |

## Summary

The splice rows that [p3-10-track-b1-x86.md](p3-10-track-b1-x86.md) could not
resolve are resolved here. On the RB5009 the benchmark repeated across two runs
inside ±5 % on every candidate; on the x86 dev box the same figures moved
15–19 % between runs, and one candidate spanned 6× within a single run.

The two platforms disagree on the one tuning question the benchmark asks, and
they disagree consistently. **The router result is the one used.**

The whole thing took 13 seconds per run. The live resolver was not stopped.

## Decisions

- **On RB5009, both runs returned `pick: none`; no tested candidate satisfied
  the budget selection rule. Both runs kept upstream sensitivity below +10 %, so
  the measurements do not justify widening the upstream buffer beyond 16. The
  x86 runs exceeded the same threshold and would have selected 64. The platform
  results therefore diverge, and the RB5009 measurements are the relevant
  evidence for the router configuration.**
- **That conclusion is about the upstream buffer alone.** It is not a selection
  of the complete `16/16` configuration, and nothing here recommends one.
- **The RB5009 runs were repeatable**: every candidate's median moved less than
  5 % between the two runs. The x86 runs moved 15–19 % on two candidates, so the
  RB5009 result supersedes the x86 one for decisions about this hardware.
- **x86 `splicebench` is not run again.** The measurement exists on the hardware
  that matters; repeating it on a box that cannot resolve it feeds no decision.
- **Open, and not resolved here:** `pick: none` does not establish that every
  tested candidate is intrinsically unsuitable. It may instead mean the 32 MiB
  budget or the `0.9×` acceptance rule is too strict. Settling that needs
  separate evidence and is the owner's decision; no new budget is invented in
  this file.

## Measurements — SNI splice throughput

`splicebench --reps 15`, 64 MiB per connection, 32 MiB worst-case budget,
candidates interleaved per rep, mimalloc with production's three `MIMALLOC_*`
settings. Medians in MiB/s.

| Candidate (up/down KiB) | Run 1 | Run 2 | Δ | Run 1 min–max | Run 2 min–max | Worst-case MiB | In budget |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `loopback_origin` | 934.9 | 956.9 | +2.4 % | 809.6–1090.1 | 831.2–1039.4 | — | — |
| 16 / 16 | 342.8 | 354.2 | +3.3 % | 309.2–363.7 | 270.5–360.9 | 32 | **yes** |
| 16 / 32 | 448.4 | 458.0 | +2.1 % | 296.1–492.5 | 435.1–480.3 | 48 | no |
| 16 / 64 | 484.4 | 461.0 | −4.8 % | 358.4–518.0 | 319.0–513.7 | 80 | no |
| 16 / 128 | 505.9 | 506.3 | +0.1 % | 369.7–526.1 | 423.3–535.3 | 144 | no |
| 64 / 64 | 486.8 | 485.3 | −0.3 % | 387.5–510.6 | 451.5–513.1 | 128 | no |

| Result | Run 1 | Run 2 |
| --- | --- | --- |
| `pick` | `none — no in-budget candidate reaches 0.9 x best 505.9` | `none — … 506.3` |
| `up sensitivity: 64/64 over 16/64` | +0.5 % | +5.3 % |
| What that metric supports (threshold +10 %) | below threshold: no justification to widen the upstream buffer beyond 16 | below threshold: same |

Counters were clean on every candidate in both runs: 16 connections, 16
requests, zero blocked, zero refused, zero resolve failures, zero dropped
events.

### Against the x86 runs

x86 figures from [p3-10-track-b1-x86.md](p3-10-track-b1-x86.md), same flags,
two runs, on a 13th Gen Intel Core i9-13980HX.

| Candidate | x86 median (mean of 2) | RB5009 median (mean of 2) | x86 ÷ RB5009 |
| --- | --- | --- | --- |
| `loopback_origin` | 7086 | 946 | 7.5× |
| 16 / 16 | 1368 | 349 | 3.9× |
| 16 / 32 | 1729 | 453 | 3.8× |
| 16 / 64 | 2232 | 473 | 4.7× |
| 16 / 128 | 2777 | 506 | 5.5× |
| 64 / 64 | 2650 | 486 | 5.5× |

| | x86 | RB5009 |
| --- | --- | --- |
| Run-to-run spread on the medians | 15–19 % on two candidates | ≤ 5 % on all five |
| Widest single-run range, 16/16 | 6× at `--reps 5` | 1.18× and 1.33× at `--reps 15` |
| `up sensitivity` | +25.9 %, then +12.5 % | +0.5 %, then +5.3 % |
| What that metric supports | above threshold: would have selected 64 | below threshold: no justification to widen beyond 16 |

**The ratio column is not a CPU factor.** The ~9× x86 → RB5009 figure in
PERFORMANCE.md is a CPU-time conversion; these are throughput numbers on a
loopback path, and they range from 3.8× to 7.5× depending on buffer size. Do
not read one as the other, and do not use either column to derive the other
platform's number.

## What this does not say

| Reading it would be wrong to take | Why |
| --- | --- |
| "16/16 is optimal" | Both runs returned `pick: none`. 16/16 is the only candidate inside the 32 MiB budget and it is the **slowest** of the five. The tool refused to pick it |
| "The router is faster than we thought" | It is 3.8–7.5× slower than the dev box. What is better on the router is the *measurement*, not the throughput |
| "Splice can carry 1 Gbit" | 349 MiB/s is loopback inside one container, with no NIC, no forwarding path and no other traffic. The LAN transfer path on this router caps near 67–70 MiB/s for unrelated reasons ([alloc-domains-n-sweep.md](../phase2.6/alloc-domains-n-sweep.md)) |
| "This settles Track B" | It settles the splice rows. The N sweep is the gate and is still blocked on the deploy decision |

## Procedure, so the next run is the same run

1. `docker buildx build --platform linux/arm64 -f Dockerfile.splicebench -t fah-splicebench:1 -o type=docker,dest=fah-splicebench-arm64.tar .`
2. **Check the layout** — buildx emitted OCI here, which makes RouterOS hang at
   `extracting` with no error. Convert:
   `skopeo copy --insecure-policy oci-archive:…/fah-splicebench-arm64.tar docker-archive:…/fah-probe.tar:fah-splicebench:1`
   Verify `manifest.json` lists `Layers` as `<hash>.tar`, not `blobs/sha256/…`,
   and that the config blob says `"architecture": "arm64"`.
3. `scp fah-probe.tar bobdenaut:kingston/`
4. Three env entries, `list=` and not `name=`
   ([routeros-traps.md](../../routeros-traps.md)): `MIMALLOC_ARENA_EAGER_COMMIT=0`,
   `MIMALLOC_PURGE_DECOMMITS=1`, `MIMALLOC_PURGE_DELAY=0` — production's
   allocator settings, so the figures describe the allocator that ships.
5. `/container/add file=kingston/fah-probe.tar interface=veth3 root-dir=/kingston/fah-probe/root envlists=fahprobe-env cpu-list=cpu0,cpu1,cpu2,cpu3 workdir=/home/nonroot logging=yes start-on-boot=no comment="fah-probe"`
6. Wait for `S` in `/container/print`, then `/container/start`, then
   `/log/print where topics~"container"`, then `/container/remove`.

**No mounts.** `splicebench` writes nothing and reads no corpus; its origin,
proxy and client all sit on the container's own loopback. A mount list whose
target directory does not exist makes a container write silently into the
container store and lose it on removal.

**`cpu-list` matches production** — `cpu0,cpu1,cpu2,cpu3`. Pinning the probe
narrower would stop it standing in for the thing it measures.

**The binary is `splicebench`, not `fastadhunter`.** `/tool/profile cpu=all`
keys on the process name and cannot separate two containers sharing one.

## Files changed

None. Measurement only.

## Remaining TODOs

| Row | State |
| --- | --- |
| B2 N sweep, N=0/2/3/4 under W1 | **BLOCKED** — deploy decision, then p3-11's dst-nat 443 |
| rustls AES-GCM ms/MiB on the device | not run. A probe could answer it without the deploy, the same way this one did |
| DNS p50/p99 under combined plain-HTTP and splice load | needs the deploy — it is about the live listener, not a self-contained bench |
| Held memory after 900 MiB WAN + 15 min, split by path | needs the deploy |
| Peak concurrent DoT connections | needs the deploy **and** `p3-10b` |
| Whether the 32 MiB budget or the `0.9×` acceptance rule is too strict | owner decision, undecided. `pick: none` on both platforms is consistent with either reading, and with neither being wrong |
