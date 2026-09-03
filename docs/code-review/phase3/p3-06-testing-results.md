# p3-06 — testing results

Figures for the arms declared in
[plan/wip/phase3/p3-06-testing-plan.md](../../../plan/wip/phase3/p3-06-testing-plan.md)
— the plan is the binding definition; the legend below is a copy for reading
this file on its own. One result row per run; raw output in the cited
`p3-06-probe/results-<ts>/` directory. A figure not in the results table is
not a result.

## Legend — what each ID means

| ID | Question | Method (script, arms, count) | Gate statistic |
| --- | --- | --- | --- |
| SNI | does a blocked domain close at SNI, before any certificate | `p0-sni.mjs`, LAN client → probe `:8444`, one blocked and one allowed name, plus a no-SNI hello | every attempt closed before any certificate — boolean |
| P1-LAN | splice throughput on the deployment path | `p1-lan.mjs`, bobdenaut → probe → LAN origin (`p1-origin.mjs` on the second endpoint), 64 MiB, one connection, 5 runs; `--connections 8` aggregate arm | median of 5 ≥ 100 MiB/s **and** inside P1-control's min–max |
| P1-control | what the same LAN path carries without the probe | `p1-lan.mjs --direct`, bobdenaut → LAN origin, 5 runs | none — its min–max is the noise band |
| P1-loopback | buffer sensitivity of the splice loop (`SPLICE_BUF` sweep) | `Dockerfile.splicebench` on the device, criterion `proxy`, up 16 KiB × down {16, 32, 64, 128} KiB + 64/64 control, 5 reps each, interleaved; `/tool/profile` share during | none — per-candidate median picks (plan §Choosing `SPLICE_BUF`) |
| P2 | handshake cost: direct vs spliced vs intercepted | `p2-handshake.mjs` **on the wired bridged VM**, one public origin, 200 rounds, three arms interleaved; issuer per row proves the leg | handshake p50: intercepted ≤ 2 × spliced; row value spliced p50 − direct p50 |
| P3 throughput | intercepted h2 relay rate | `p3-h2stall.mjs`, listed client, public h2 origin, one unstalled 8 MiB stream, 3 runs | median of 3 ≥ 50 MiB/s |
| P3 RSS | process RSS growth under a 64-stream stall | `p3-h2stall.mjs`, warm-up, 64 streams to `:status 200` + first DATA then stalled, `process_rss` per second; 3 stall runs interleaved with 3 matched no-stall control runs | max stall delta over 3 runs vs ≈ 5.5 MiB, read only when attribution `RESOLVED` (min stall > max control) |
| P4 | DoT / DoH added latency vs UDP, in-engine, reused connection | `Dockerfile.p4` on the device, `encrypted_latency` harness, 3 × 2 000 per transport, interleaved | per transport p50 − UDP p50 — sets the PERFORMANCE.md row |
| P4-LAN | the same as the LAN sees it | `p4-lan.mjs`, bobdenaut → probe, blocked domain, 3 × 2 000, one connection per transport per batch | none — diagnostic |
| P5 | cold leaf mint per first-sight host | `p5-mint.mjs`, 256 hosts over DoT `:853`, CA on the probe, one first-sight and one repeat handshake per host | median(first-sight) − median(repeat) < 1 ms — incremental p50 estimate |
| P6 | CA generate / API-pair import time | `p6-certs-time.mjs`, 5 generates + 5 imports, `time_starttransfer − time_appconnect` | median of 5: generate < 100 ms, import < 50 ms |
| P7-store | key material never leaves the probe over the API | `p7-store.mjs`, `ca/export` both formats + `/config` searched for the CA key payload, traversal list against `:8443` | each check pass / fail |
| P8-probe | CPU the probe burns under a stage's load | `/tool/profile cpu=all`, read-only, idle baseline first, test binary named apart | none — diagnostic |
| P9-probe | probe boot-to-serving with the phase-3 listeners | container log timestamps | none — diagnostic |

## Results

| ID (legend) | Date (UTC) | Tip hash | Device / workload / corpus | Idle check | Gate statistic vs budget | Validity / attribution | Raw directory |
| --- | --- | --- | --- | --- | --- | --- | --- |
