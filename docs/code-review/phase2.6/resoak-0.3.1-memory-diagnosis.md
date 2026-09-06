# 0.3.1 RSS floor climb — dev-box diagnosis

Read-only against production and the router. Every experiment below ran on the
dev box in Linux containers (Docker Desktop). Owner soak figures are quoted from
[resoak-0.3.1/](resoak-0.3.1/) and are never restated as results of this run.

Revision 2, 2026-09-04T17:0xZ, at 18 h of run time. Revision 1 (6 h) called the
residual a settle and named thread abandonment as the candidate mechanism; both
were wrong at 18 h and are corrected below, with the 6 h reading kept where it
was superseded. Revision 4, 2026-09-05T00:xxZ: F14 read (env lever fails),
base2 at 24 h (F15), root-cause statement below. Revision 5, 2026-09-05
09:4xZ: the cross-thread-free model tested and falsified at 32 workers (F17,
F18), then named precisely by the whole-process page dump (F29) and confirmed
at production geometry (F24, F30); read D (F31); the production floor over the
whole soak (F32, F33); the owner's decision on the fix. Revision 6, 2026-09-05
19:5xZ: the RouterOS memory graph reconciled (F34); the excursions traced back
across three soaks and the remaining unattributed ones narrowed (F35); the
isolated-runtime A/B (F36, [allocation-domains-proposal.md](allocation-domains-proposal.md));
the 4-worker drift arms stopped as superseded; the `concurrent_connections`
counter committed (`4eddc39`). 2026-09-06 07:xxZ: F34 closed by the owner
(image tars), §Hand-off added for the next agent.

## Summary

**Root cause, as far as measured.** In the measured windows FAH shows no
live-object leak: live bytes stay near flat while RSS climbs, and after every
burst the application's allocated bytes return to baseline (counting arm). The
measured problem is allocator retention in mimalloc under a workload mixing
short-lived and long-lived allocations. For the large steps the mechanism is
demonstrated end to end (counting allocator, musl arm). For the slow drift the
general mechanism is known. At 50 QPS ~40 % of it survives the allocator swap
(musl +0.22 MiB/h); at the production rate of 2 QPS musl is flat for 15 h
(F31) while mimalloc climbs +0.28, so at that rate the drift is mimalloc's.
Every structural candidate was ablated without effect (recompiles, cache
lifetime, cache size, page policy, two collect strategies, F16/F31); F29 names
the mechanism as remote frees pending on the pages of workers that have parked
for good, and the 4-worker drift arms decide whether it exists at production
geometry at all (F32: the production floor is flat over 21 h).

**Revision 4, 01:xxZ — the cross-thread-free model is falsified.** A per-worker
`mi_collect(true)` on every tokio park (F17) and mimalloc's own periodic collect
(F18) both leave +15.6 / +19.0 MiB after the F14 burst, against +21.5 unmodified.
The residue is inside mimalloc's arena and mimalloc counts those pages as still
in use (F17: 137 → 296 pages) while the application has freed the bytes. The
discriminators then showed (F20–F26): nothing per request survives (1 500
sequential requests leave 0); the residue appears with no DNS traffic at all
and never decays in an idle process (+25.6); a second burst adds only +4.7, so
it is bounded; and it scales with the number of tokio workers — **4 workers
retain +2.2 for the burst that 32 workers retain +21..+26 on**. The retained
memory is per-thread-heap page state, and the whole-process page dump (F29)
names it: **remote frees pending on pages owned by tokio workers that woke for
the burst, allocated the connection state, then parked for good**. mimalloc
only reclaims a block on the owner's thread; a worker that never runs again
never collects, and `page->used` stays high on pages the application has freed.
The park hook (F17) ran on those workers' way into idle, before the frees
arrived. At the RB5009's 4 workers every worker keeps cycling, so the residue
is ~2 MiB per burst and production's own 18:04Z excursion (+22 MiB) decayed
fully with no floor step (F26). The decisive run for the fix is the park hook
at 4 workers (`park4-b`, `park4-2` vs `w4-2`).

- **Reproduced.** Under `db2f9b2` on x86 the unaccounted residual climbs for the whole 18 h with no plateau: +15.1 MiB at 50 QPS (11.1 → 26.2 MiB hourly minima), +5.1 MiB at 2 QPS. The 2 QPS rate, +0.34 MiB/h, is the owner's production rate (+19 MiB / 48 h = +0.40 MiB/h) on the same code at the same traffic.
- It is allocator retention, not a live-object leak: a counting allocator shows live bytes +0.65 MiB over h8–h18 while the RSS gap above live bytes grew +4.5 MiB; musl's allocator under the same workload climbs at 40 % of mimalloc's rate.
- mimalloc v2 is worse than v3 (+23 MiB / 18 h). Thread keep-alive and `page_reclaim_on_free` change nothing or make it worse; mimalloc's abandoned-page count is flat over h6–h18 while the residual climbs, so the thread-churn hypothesis of revision 1 is closed negative.
- Recompile rate is not the driver: 12 forced recompiles per hour with a 5 000-entry cache climbs at the same 0.31 MiB/h as one per hour. HTTP proxy traffic at 4× production volume adds nothing distinct.
- **Revision 3 (2026-09-04 evening): the steps have a source.** Production took a +17 MiB anonymous excursion at 18:04Z with no fetch, no traffic rise and no counter moving; the owner's own 48 h series holds four more of the same class (+8 to +37 MiB, evenings), one sitting exactly under the h34–36 floor step. Replicated on the dev box: 500 keep-alive connections through the transparent proxy cost +26 MiB while open; the application frees every byte when they close (counting allocator: live bytes back to the pre-burst value), and mimalloc keeps **+22 MiB** of it, flat at 40 min. musl keeps +1.3 MiB. A refused-path burst (no upstream leg) leaves nothing; a second pass-path burst adds +13.5 MiB, so it ratchets with partial reuse.
- Two components, then: a slow drift under DNS-only churn (+0.3–0.5 MiB/h on x86, any QPS, 40 % of it allocator-independent) and step residues from proxy connection bursts (household evening traffic). The steps are the G2 killer; the drift is the G1 band.
- Verdict: REPRODUCED.

## Method

| | |
| --- | --- |
| Device | dev box, Docker Desktop 29.7.2, WSL2 kernel 6.18.33, 32 vCPU, 16 GB. x86_64, not arm64 — nothing here measures the RB5009 |
| Build | worktree at `db2f9b2`, production `Dockerfile` unchanged (rust:1.96.0-alpine, musl static, distroless). Image `fah:db2f9b2` = `sha256:33e7241b4a5a…` ([build-db2f9b2.log:677](resoak-0.3.1-diag/build-db2f9b2.log)) |
| Variant images | same worktree + one edit each, A/B'd against `fah:db2f9b2` only: `-count` (counting `GlobalAlloc` wrapper around mimalloc, 60 s `ALLOCREPORT` + `mi_stats_print` to stderr), `-sysalloc` (`std::alloc::System` = musl mallocng), `-v2` (`libmimalloc-sys` feature `v2`), `-keepalive` (`Builder::thread_keep_alive(86400 s)`) |
| Config | production config at T0 ([pull0-t0-config.json](resoak-0.3.1/pull0-t0-config.json)) with: the 16 lists served from a local HTTP server, `refresh_hours = 1` on every list, `phishdestroy` mutated by one line every 20 min (one recompile per hourly wave); 4 upstreams = 4 ports of one CoreDNS `template` (any name → one A record TTL 300, `*.nx.example` → NXDOMAIN); `api.tls = false`; `fast2` sets history/stats/cleanup intervals to 30 s; the compressed arms set `max_entries = 5000` and take a forced `POST /api/v1/lists/refresh` every 5 min after a list mutation ([tools/forcer.py](resoak-0.3.1-diag/tools/forcer.py), [series/forcer.log](resoak-0.3.1-diag/series/forcer.log)). Container env identical to production: `MIMALLOC_PURGE_DELAY=0`, `MIMALLOC_PURGE_DECOMMITS=1`, `MIMALLOC_ARENA_EAGER_COMMIT=0`, `TZ=Europe/Bucharest` |
| Corpus | 16 production lists fetched 2026-09-03 22:2xZ, 28 MB, 753 223 rules after dedup (production 756 420), `ruleset_bytes` 24.07 MiB (production 24.1) |
| DNS workload | [tools/workload.py](resoak-0.3.1-diag/tools/workload.py): 55 % blocked ad names, 15 % hot set (100 names), 20 % warm set (4 000 names), 5 % unique never-seen names, 5 % NXDOMAIN, 20 % of queries AAAA; one client IP; 2 QPS (production ≈ 1–4) or 50 QPS. Stale share 47 % of hits at 50 QPS, 77 % at 2 QPS (production 55 %) |
| HTTP workload | [tools/httpload.py](resoak-0.3.1-diag/tools/httpload.py): 1 req/s through the transparent proxy to a name resolving to the list server, sizes 3 KB–6 MB weighted to ~190 KB, 50 % keep-alive |
| Sampling | host poller, `GET /api/v1/debug/memory` + `/telemetry` + `/cache` every 60 s ([tools/poll.py](resoak-0.3.1-diag/tools/poll.py)); `/proc` sidecar per container, `status` every 60 s, `smaps` every 10 min ([tools/sidecar.sh](resoak-0.3.1-diag/tools/sidecar.sh)) |
| Duration | At revision 5 (2026-09-05 09:4xZ): 35 h for the six first arms (T0 2026-09-03T22:27Z / 22:33Z), 31 h HTTP arms, 30 h keep-alive and reclaim arms, 28 h compressed arms, 16 h `stale50` / `retain50` / `cctl` / `cstale` / `cretain` / `sysalloc2`, 9 h `park2` / `gc2` / `norec2` / `manyrec2` / `c100-2`, 1.5 h `park4-2` / `w4-2`; burst arms 0.3–1.3 h each. Containers still running; raw series in `E:/fah-diag/out/`, derived CSVs in [resoak-0.3.1-diag/series/](resoak-0.3.1-diag/series/), burst runs and dumps in [resoak-0.3.1-diag/burst/](resoak-0.3.1-diag/burst/) |
| Deviations from production | x86_64; one client IP (production registry holds 4 096); synthetic single-record answers; no API TLS; no household burst pattern; QPS and refresh cadence compressed; cache churn far above production on the 50 QPS arms (50 000 entries reached at 4.5 h vs ~3 000 flat) |

## Runs

Residual = `residual_bytes` = RSS − accounted components. "min" = hourly minima of the
residual, first and last full hour ([slopes-samewindow.txt:17–27](resoak-0.3.1-diag/slopes-samewindow.txt)).
Slopes are least squares over h6–end, i.e. after every 50 QPS cache has reached its cap
([slopes-samewindow.txt:2–14](resoak-0.3.1-diag/slopes-samewindow.txt)). RSS growth before
h4.5 on the 50 QPS arms is the cache filling and is not a finding.

| Run | Image | Allocator | QPS | Extra | h | Residual min first → last hour, MiB | Slope h6–end, MiB/h | Read |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| base2 | db2f9b2 | mimalloc v3 | 2 | — | 18.4 | 13.8 → 18.9 | +0.34 | climbs at the production rate |
| fast2 | db2f9b2 | mimalloc v3 | 2 | ticks 30 s (2 200 history rows = 9 production days) | 18.4 | 13.3 → 19.6 | +0.40 | same as base2; ticks not the driver |
| base50 | db2f9b2 | mimalloc v3 | 50 | — | 18.4 | 11.1 → 26.2 | +0.55 | climbs, steps at h7 +3.1, h16 +2.0, h17 +1.8 |
| count50 | -count | mimalloc v3 + counter | 50 | — | 18.3 | 12.3 → 27.8 | +0.53 | climbs; live bytes flat from h8 |
| v250 | -v2 | mimalloc v2 | 50 | — | 18.3 | 11.9 → 35.2 | +0.99 | worst |
| sysalloc50 | -sysalloc | musl mallocng | 50 | — | 18.3 | 8.9 → 14.5 | +0.22 | 40 % of v3's rate |
| http50 | db2f9b2 | mimalloc v3 | 50 | + 1 req/s proxy, 1.9 GB / 2.8 h then continuing | 15.1 | 15.2 → 24.4 | +0.36 | no distinct signal |
| httpcount50 | -count | mimalloc v3 + counter | 50 | + proxy | 15.1 | 15.1 → 25.3 | +0.16 | no distinct signal |
| keepalive50 | -keepalive | mimalloc v3, blocking threads never retire | 50 | — | 13.9 | 12.6 → 24.8 | +0.24 (base50 over the same h6–13.8 window: +0.24) | **fails**: identical slope, residual 4 MiB higher |
| reclaim50 | db2f9b2 | mimalloc v3, `MIMALLOC_PAGE_RECLAIM_ON_FREE=1` | 50 | — | 13.9 | 11.8 → 23.8 | +0.97 | **fails**: worse than base |
| cbase | db2f9b2 | mimalloc v3 | 50 | cache 5 000, 12 recompiles/h | 11.2 | 11.7 → 17.0 | +0.31 | climbs at base2's rate despite 12× compiles |
| cka | -keepalive | mimalloc v3 | 50 | cache 5 000, 12 recompiles/h | 11.2 | 13.8 → 21.9 | +1.20 | fails, worst of the three |
| creclaim | db2f9b2 | mimalloc v3, reclaim-on-free | 50 | cache 5 000, 12 recompiles/h | 11.2 | 11.8 → 18.9 | +0.93 | fails |
| cctl | db2f9b2 | mimalloc v3 | 50 | cache 5 000, 1 recompile/h (control for the two below) | 7.1 | 12.3 → 17.6 | +0.51 (h1–end) | F16 |
| cstale | db2f9b2 | mimalloc v3 | 50 | cache 5 000, `serve_stale = false` | 7.1 | 12.6 → 16.3 | +0.51 (h1–end) | **no effect** on the drift |
| cretain | db2f9b2 | mimalloc v3, `MIMALLOC_PAGE_FULL_RETAIN=0` | 50 | cache 5 000 | 7.1 | 12.2 → 16.3 | +0.46 (h1–end) | **no effect** on the drift |
| bhttp-base / -retain / -musl | db2f9b2 / +`PAGE_FULL_RETAIN=0` / -sysalloc | v3 / v3 / musl | 50 | burst arms, egress allowed | 0.3 | — | — | F14: burst residue +21.5 / +15.1 / +0.5 |
| norec2 | db2f9b2 | mimalloc v3 | 2 | `refresh_hours = 720` — no recompile in the window | started 00:05Z 09-05 | | | pending — F15 ablation |
| manyrec2 | db2f9b2 | mimalloc v3 | 2 | forced refresh every 5 min | started 00:05Z 09-05 | | | pending — F15 ablation |
| park-b / park2 | -park (owner-approved worktree) | mimalloc v3 + `mi_collect(true)` on worker park, ≤ 1/10 s/worker | 50 / 2 | burst arm / drift arm | started 00:17Z 09-05 | | | F17: burst residue **+15.6, fails**; park2 drift pending |
| gc-b / gc2 | db2f9b2 + `MIMALLOC_GENERIC_COLLECT=1000` | mimalloc v3 | 50 / 2 | burst arm / drift arm | started 00:21Z 09-05 | | | F18: burst residue **+19.0, fails**; gc2 drift pending |
| bq-seq / bq-idle | db2f9b2 | mimalloc v3 | 50 | 1 500 requests over 50 sequential connections / 500 idle connections, no request | started 01:02Z 09-05 | | | F20 discriminator, burst at 01:17Z |
| c100-2 | db2f9b2 | mimalloc v3 | 2 | `max_entries = 100` | started 01:02Z 09-05 | | | page-pinning bound test: residual growth should cap near 100 × 64 KiB if cache entries are the pinning population |
| bq-nodns | db2f9b2 | mimalloc v3 | 50 then 0 | DNS load stopped before the burst; second burst 02:09Z | 1.3 | — | — | F23: **+25.6** with no traffic at all; F25: second burst **+4.7** |
| bq-w4 | db2f9b2 | mimalloc v3 | 50 | `TOKIO_WORKER_THREADS=4` | 0.7 | — | — | F24: **+2.2** — one tenth of the 32-worker residue |
| visit-b / visit2-b | -park + heap visitors (owner-approved) | mimalloc v3 | 50 | per-worker / whole-process page dumps | 0.7 each | — | — | F28 / F29: the retained pages hold uncollected remote frees owned by workers that no longer park |
| park4-b / park4-2 / w4-2 | -park / -park / db2f9b2 | mimalloc v3 | 50 / 2 / 2 | `TOKIO_WORKER_THREADS=4` (production geometry): burst arm, drift arm, unmodified drift control | 0.7 / 1.9 + 0.8 | — | — | F30: burst **+1.1 / +2.1**, same as the control. Drift arms **not run to 18 h**: run 1 stopped at 1.9 h (rig stop 10:15Z), run 2 relaunched 16:32Z and stopped at 0.8 h by owner decision — the 4-worker drift question is answered by the router itself (F32, F33: flat h48–h102) and no matrix outcome could change a decision. Series `burst/`-adjacent in `E:/fah-diag/out/*-run1.jsonl`, `*-run2.jsonl` |
| httprt4-b / ctl4-b | db2f9b2 + HTTP proxy on its own tokio runtime (throwaway worktree `E:/FastAdHunter-var-httprt`, image `fah:db2f9b2-httprt`) / db2f9b2 | mimalloc v3 | 50 | `TOKIO_WORKER_THREADS=4`, `FAH_HTTP_WORKER_THREADS=2` then `=1`; F14 burst at 15 min, same-time control | 0.7 each | — | — | F36 |
| hist4 | db2f9b2 | mimalloc v3 | 0 | `TOKIO_WORKER_THREADS=4` on the `fast2` volumes (4 300 perf rows on disk); six full-row `GET /history/perf?max_points=5000` reads | 0.6 | — | — | F35: history reads cleared on 0.3.1 |
| stale50 | db2f9b2 | mimalloc v3 | 50 | `serve_stale = false`, cache 50 000 | 16.0 | 12.0 → 22.2 | +0.33 (h6–16; base50 same window +0.29) | **no effect** on the drift (F31) |
| retain50 | db2f9b2 | mimalloc v3, `MIMALLOC_PAGE_FULL_RETAIN=0` | 50 | — | 16.0 | 11.4 → 27.6 | +0.69 (h6–16) | **worse** (F31) |
| sysalloc2 | -sysalloc | musl mallocng | 2 | — | 15.4 | 10.3 → 10.2 | **−0.03** (h1–15.4; base2 same window +0.28) | **flat at production rate for 15 h** (F22, F31) |

## Findings

| Id | Evidence | What | Attribution |
| --- | --- | --- | --- |
| F1 | [slopes-samewindow.txt:19](resoak-0.3.1-diag/slopes-samewindow.txt) (base50 hourly minima), [base50.csv](resoak-0.3.1-diag/series/base50.csv) | Residual hourly minima 11.1 → 26.2 MiB over 18 h with no plateau, +0.55 MiB/h after the cache cap; `accounted_bytes` moved 52.9 → 59.7 MiB in the same span (cache bytes at constant entry count), so the residual is net of it. Same direction as the owner's series, ~1.4× the production rate at 25× the query rate | **reproduced** — allocator |
| F2 | [count50-allocreport.txt:480](resoak-0.3.1-diag/series/count50-allocreport.txt) → [:1080](resoak-0.3.1-diag/series/count50-allocreport.txt) | Counting allocator h8 → h18: `live_bytes` 56.40 → 57.05 MiB (+0.65), `live_allocs` 214 162 → 221 623, while `rss − live` 26.7 → 31.2 MiB (+4.5). Live objects do not grow; allocator-held pages do | **allocator**, not live object |
| F3 | [slopes-samewindow.txt:5](resoak-0.3.1-diag/slopes-samewindow.txt), [:22](resoak-0.3.1-diag/slopes-samewindow.txt) | musl mallocng under the identical workload: +0.22 MiB/h, hourly minima 8.9 → 14.5. Some page-level waste is allocator-independent; ~60 % of the mimalloc climb is mimalloc's own retention. mimalloc v2 ([:3](resoak-0.3.1-diag/slopes-samewindow.txt), [:20](resoak-0.3.1-diag/slopes-samewindow.txt)) is worse than v3: +0.99 MiB/h | **allocator** — mimalloc share bounded above by the musl arm |
| F4 | [slopes-samewindow.txt:10–11](resoak-0.3.1-diag/slopes-samewindow.txt), [:25–26](resoak-0.3.1-diag/slopes-samewindow.txt) | At 2 QPS the residual climbs +0.34 MiB/h (base2) and +0.40 (fast2) over h6–18; hourly minima 13.8 → 18.9 and 13.3 → 19.6. Revision 1 read these arms as flat at 6 h — they are not, the first 6 h sit inside the band. `fast2` ran 9 production days of history, stats and cleanup ticks with the same slope, so those writers are not the driver | **reproduced at production rate** (owner: +0.40 MiB/h) |
| F5 | [slopes-samewindow.txt:12](resoak-0.3.1-diag/slopes-samewindow.txt), [:27](resoak-0.3.1-diag/slopes-samewindow.txt), [series/forcer.log](resoak-0.3.1-diag/series/forcer.log) (134 forced recompiles, 0 failed) | 12 recompiles per hour with a 5 000-entry cache: +0.31 MiB/h, the 2 QPS rate. Each recompile still leaves a +0.5–1 MiB transient that decays inside 10 min (seen in every arm including musl). Recompile rate does not set the climb | **not the driver** — compile transient is separate and bounded |
| F6 | [slopes-samewindow.txt:6–7](resoak-0.3.1-diag/slopes-samewindow.txt) vs [:2](resoak-0.3.1-diag/slopes-samewindow.txt) (same h6–13.8 window), [:13–14](resoak-0.3.1-diag/slopes-samewindow.txt); [count50-mistats.txt:40](resoak-0.3.1-diag/series/count50-mistats.txt) → [:74](resoak-0.3.1-diag/series/count50-mistats.txt) (abandoned pages current 183 → 186 while threads total 257 → 719, [:66](resoak-0.3.1-diag/series/count50-mistats.txt) → [:100](resoak-0.3.1-diag/series/count50-mistats.txt)) | Revision 1's mechanism, tested three ways: keep-alive build (threads pinned at 37 all run) has base50's exact slope, +0.24 vs +0.24, with 4 MiB more residual; `page_reclaim_on_free = 1` is worse (+0.97); on the compressed rig both are worse than the unmodified build (+1.20, +0.93 vs +0.31). mimalloc's abandoned-page count stayed flat from h6 to h18 while 462 more threads retired and that arm's residual climbed 8 MiB (19.8 → 27.8) | **closed negative** — thread abandonment is not the mechanism; keeping blocking threads alive costs memory |
| F7 | [slopes-samewindow.txt:8–9](resoak-0.3.1-diag/slopes-samewindow.txt); production `counters.http` 2 401 requests / 455 MB in 48 h ([pull2-telemetry.json](resoak-0.3.1/pull2-telemetry.json)) | Proxy at 1 req/s for 15.1 h — 54 420 requests, 10.8 GB (24× the production soak's 48 h volume): +0.36 / +0.16 MiB/h, inside the DNS-only band | **not reproduced** as a distinct cause |
| F8 | [prod-floor-analysis.txt:3–46](resoak-0.3.1-diag/prod-floor-analysis.txt) vs [:49–55](resoak-0.3.1-diag/prod-floor-analysis.txt) (derived from the owner's [pull2-perf.json](resoak-0.3.1/pull2-perf.json)) | Owner's floor steps at h1 (+8.2, warm-up), h17 (+3.9), h22–23 (+3.8), h34–36 (+5.7), h44 (+2.6). List bodies changed at h3.3, h15.2, h27.3, h39.2–39.3. No step within 1.5 h of a fetch; cache entries never dropped by >150 in a sample; `peak_rss` rose only at h3.3, h15.2, h39.3. The h35 step coincides with the soak's only `servfail_relayed` burst (118 in hour 35) | quoted — production steps not aligned with recompiles or cache cleanup; the x86 series steps the same way (base50 h7, h16, h17) with one recompile every hour, so alignment cannot be tested here |
| F9 | [smaps.py](resoak-0.3.1-diag/tools/smaps.py) over `side-*/smaps.log`; [proc-base50.log](resoak-0.3.1-diag/series/proc-base50.log) | All growth is anonymous memory inside mimalloc's single 1 GiB arena reservation (kernel splits it into sub-VMAs by commit state); `RssFile` flat at 7.6–8.1 MiB in every arm; thread count 33–38 | matches the owner's `rss_file` flat at 8.8 MiB |

What this run cannot say: anything about arm64 page behaviour, the household's
client mix (4 096 registry entries, IPv6 rotation), or real upstream latency and
failure paths. It also does not name the structure whose churn spreads mimalloc's
pages — the cache (entries live up to 24 h stale next to per-query transients)
and the stats counters (`Arc<str>` keys evicted and re-inserted in hourly
buckets) are the two long-lived churning populations, and neither was ablated.

## Reading pass — what lives on blocking threads (read-only, 2026-09-04 05:3xZ)

Written at revision 1, kept because it enumerates every off-hot-path allocation
site; its conclusion (F6 hypothesis) was falsified by the arms at 13.9 h. Scope:
every `spawn_blocking` and `tokio::fs` site at `db2f9b2`, asking one question per
site — does it allocate something that outlives the thread? Plus the mimalloc v3
free path for a block whose owning thread is gone. No file changed.

| Id | Evidence | What | Reading |
| --- | --- | --- | --- |
| R1 | [main.rs:809–830](../../../crates/fastadhunter/src/main.rs) (`collect_memory`), `TELEMETRY_POLL` 10 s at [main.rs:43](../../../crates/fastadhunter/src/main.rs) | The 10 s snapshot runs on the blocking pool. It reads `/proc/self/status`, calls `stats.heap()`, `pipeline.cache_stats()`, `allocator::stats()` and returns plain `MemoryBreakdown` + `CacheStats` (no heap fields). Everything it allocates dies on the same thread | pins nothing; it is the thread-churn source (~37 threads/h) — churn shown harmless by F6 |
| R2 | [pipeline.rs:202](../../../crates/fah-dns/src/pipeline.rs) → [cache.rs:762–800](../../../crates/fah-dns/src/cache.rs), `sweep_queue` at [cache.rs:385](../../../crates/fah-dns/src/cache.rs) | Cache sweep on a blocking thread: `HashMap::retain` and `VecDeque::retain`, both in place | pins nothing |
| R3 | history [perf.rs:60–63](../../../crates/fah-stats/src/history/perf.rs), stats [snapshot.rs](../../../crates/fah-stats/src/snapshot.rs), reader [routes.rs:288](../../../crates/fah-api/src/routes.rs), list validation [lifecycle/mod.rs:764](../../../crates/fah-rules/src/lifecycle/mod.rs) | `tokio::fs` moves buffers built on worker threads to the blocking side for the syscall; the history read builds `Vec<PerfSample>` on a blocking thread and drops it whole on a worker; the validation parse is transient | pins nothing |
| R4 | [lifecycle/mod.rs:1268–1308](../../../crates/fah-rules/src/lifecycle/mod.rs), [matcher.rs:785–802](../../../crates/fah-rules/src/matcher.rs) | The whole `Matcher` is built on a blocking thread and lives until the next recompile: `arena`, `records`, `slots`, `policy_mask` are single large blocks; `lists`, the `dnstype` / `rewrite` / `clients` maps, `UrlIndex` and the per-list `RefreshStats` map are small blocks in that thread's pages | the ruleset's small-block share sits in abandoned pages (accounted in `ruleset_bytes`); nothing survives the swap (R6) |
| R5 | mimalloc v3 `free.c:248–255, 322–333, 375–395`, `init.c:307` (cargo registry, `libmimalloc-sys-0.1.49/c_src/mimalloc/v3/src/`) | Freeing into an abandoned page: all free → page returned; else reclaim only into the originating heap under the default `page_reclaim_on_free = 0`; else re-abandon mapped if below 7/8 used; else leave unowned | the retention shape revision 1 assumed; F6 shows it is not what accrues |
| R6 | [lifecycle/mod.rs:1348–1358](../../../crates/fah-rules/src/lifecycle/mod.rs) (`swap_in`), [lifecycle/mod.rs:102–109](../../../crates/fah-rules/src/lifecycle/mod.rs), [url_matcher.rs:1123](../../../crates/fah-rules/src/url_matcher.rs) | `Arc::new(matcher)` boxes the struct on the calling worker, the old `Arc` is released by `ArcSwap::store` on that worker, `RefreshStats` is plain counters, `UrlIndex` is boxed slices plus two small maps | no survivor by reading |

## Burst experiments — revision 3 (2026-09-04 18:00–21:30Z)

Owner-authorised read-only pulls against production (`GET` only, key from
`.vscode/production.key`, never printed) are in [resoak-0.3.1-diag/prod/](resoak-0.3.1-diag/prod/)
and are **not** part of the owner's declared pull log. Burst runs are recorded in
[burst/burst-notes.txt](resoak-0.3.1-diag/burst/burst-notes.txt).

| Id | Evidence | What | Attribution |
| --- | --- | --- | --- |
| F10 | [prod/perf-20260904T1904Z.json](resoak-0.3.1-diag/prod/perf-20260904T1904Z.json) (samples 17:58Z → 18:46Z), [prod/telemetry-20260904T1904Z.json](resoak-0.3.1-diag/prod/telemetry-20260904T1904Z.json) vs [pull2-telemetry.json](resoak-0.3.1/pull2-telemetry.json) | Production at T0+82.6 h: RSS 63.0 → 69.5 → 79.1 → 85.2 MiB between 17:58Z and 18:46Z, `rss_anon` +22 MiB, `rss_file` flat 8.8, `peak_rss` unchanged 151, `list_fetch.bodies` unchanged 35, queries per 6 min falling 1 073 → 457, cache +50 entries. Decayed to 69.6 by 20:11Z. Proxy counters since pull 2: +1 047 pass, +146 MB, **+83 refused**; in the hour after 19:04Z only +30 pass, so the burst itself is not visible in any counter the sample carries | an anonymous transient of the same class as the owner's 09-01 17:16Z (+8), 17:34Z (+13), 22:10Z (+9) and 09-02 17:46Z (+37 MiB) excursions ([prod-floor-analysis.txt](resoak-0.3.1-diag/prod-floor-analysis.txt) scan, no fetch at any of them); the 09-02 one precedes the h34–36 floor step |
| F11 | [burst/burst-notes.txt:1–4](resoak-0.3.1-diag/burst/burst-notes.txt), [burst/httpcount50-burst-allocreport.txt:1](resoak-0.3.1-diag/burst/httpcount50-burst-allocreport.txt) → [:12](resoak-0.3.1-diag/burst/httpcount50-burst-allocreport.txt) → [:20](resoak-0.3.1-diag/burst/httpcount50-burst-allocreport.txt) | 3 rounds × 500 keep-alive connections through the proxy (pass path, 3.4 KB body each, 50 s hold): `http50` 84.6 → 101.0 MiB open, **105.3 after close, 104.8 at +5 min, 105.7 at +13 min, 106.0 at +40 min**. Counting arm: live 57.2 → 64.7 MiB open → **57.3 after** (every byte freed), RSS 88.0 → 116 → 110.4, allocator-held gap 30.8 → 53.0 MiB | **allocator residue, +22 MiB per burst, no decay in 40 min** — the floor-step mechanism, reproduced end to end |
| F12 | [burst/burst-notes.txt:5–9](resoak-0.3.1-diag/burst/burst-notes.txt); **corrected in revision 4** from the minute rows in [readC-20260905T0011Z.txt](resoak-0.3.1-diag/readC-20260905T0011Z.txt): the 20:46:44Z row first used as `base50`'s pre value was already inside the burst (`pass` = 473) | Same burst on the refused path (no egress allow-list, 403 per request): `base50` residual 29.8 (20:45Z) → 65.0 open → 41.5 at +5 min → **42.5 at +15 min, +12.7 MiB**; `sysalloc50` 15.3 → 24.1 → 16.8, +1.5. Second pass-path burst on `http50`: 105.8 → 126.6 → 119.3 RSS, **+13.5** on top of the first +21.5 | the residue does **not** need the upstream leg — accept, parse and a 403 on 500 concurrent connections leave 60 % of the pass-path residue; it ratchets with partial reuse; musl keeps ~10 % of what mimalloc keeps. Revision 3's "residue 0 on the refused path" was a mid-burst pre row |
| F13 | [slopes-samewindow.txt:19](resoak-0.3.1-diag/slopes-samewindow.txt) continued to h22 (base50 minima 26.2 → 28.8, sysalloc50 14.4 → 14.8) | The DNS-only drift continues underneath and is unrelated to bursts | two components; the drift is F1–F5, the steps are F10–F12 |
| F14 | [burst/burst-notes.txt](resoak-0.3.1-diag/burst/burst-notes.txt) (F14 block), `E:/fah-diag/out/bhttp-*.jsonl`; arms `bhttp-base`, `bhttp-retain` (`MIMALLOC_PAGE_FULL_RETAIN=0`), `bhttp-musl`, egress allowed, burst 21:16:26Z (the ~21:11Z launch never ran; `pass` was 0 on all three) | Same burst on three fresh arms, residual MiB pre → peak → +5 → +15 min: base 11.8 → 37.4 → 32.3 → 33.3, **+21.5**; retain 11.8 → 35.5 → 29.2 → 26.9, **+15.1**; musl 10.9 → 24.1 → 11.3 → 11.4, **+0.5**. Fresh 50 QPS arms, cache still filling (+2.8 MiB accounted over the window), so residual is the figure, not RSS | `PAGE_FULL_RETAIN=0` **fails** — 30 % below base, 30× musl; lever (b) closed. musl passes on the pass path too |
| F15 | [readB-20260904T2234Z.txt](resoak-0.3.1-diag/readB-20260904T2234Z.txt), `docker logs diag-base2` | base2 / fast2 at 24.1 h: slope h18–end **+0.67** / +0.41 MiB/h (base2 was +0.34 over h6–18), minima 22.4 / 23.3. One step > 1 MiB, base2 h21 → h22 (+1.8): minute series 19.7 (19:45Z) → 21.2 (19:50Z) → 20.5 (20:00Z), floor stays; the wave's `phishdestroy` recompile is 19:47:43Z. Next waves: 20:48Z left ~+0.5, 21:49Z decayed fully. Cache at 2 QPS still filling at 24 h (18 k entries, accounted +0.5 MiB/h) | at 2 QPS the recompile transient (F5: decays inside 10 min at 50 QPS) is retained 0 to +0.8 MiB per wave — the drift's order of magnitude. cbase (12/h, +0.31) did not scale, so retention per recompile depends on what happens between waves; **lead, not finding** |
| F16 | [readC-20260905T0011Z.txt](resoak-0.3.1-diag/readC-20260905T0011Z.txt) | `cctl` / `cstale` (`serve_stale = false`) / `cretain` (`MIMALLOC_PAGE_FULL_RETAIN=0`) at 7.1 h, 50 QPS, 5 000-entry cache, one recompile per hour: slope h1–end **+0.51 / +0.51 / +0.46 MiB/h**, final third +0.35 / +0.28 / +0.35, minima 12.3 → 17.6, 12.6 → 16.3, 12.2 → 16.3 | decision table: **neither** — cache lifetime and page policy do not move the drift; next is isolating a structure's churn (`norec2` / `manyrec2`, started 00:05Z 09-05: base2's config with `refresh_hours = 720`, and with a forced refresh every 5 min) |
| F17 | [burst/burst-notes.txt](resoak-0.3.1-diag/burst/burst-notes.txt) (F17 block), `docker logs diag-park-b` (`ALLOCREPORT … parks= collects=` + `mi_stats_print`), worktree `E:/FastAdHunter-var-park` (owner-approved: `-count` diff + `on_thread_park(allocator::collect_on_park)`, `mi_collect(true)` at most once per 10 s per worker) | F14 burst on `park-b`: residual 12.1 → 34.1 peak → 25.9 at +5 → **27.7 at +15 min, +15.6 MiB**. 32 workers, ~18 collects/min steady, ~30/min during the burst. `mi_stats`: pages current 137 → 296, threads total 45 → 58, committed 265.5 → 274.1 MiB; live bytes 30.5 → 34.1 = cache fill (accounted +3.8) | **fails** — per-worker collection on idle does not return the pages; the residue is not remote frees waiting for their owner |
| F18 | burst-notes (F18 block), arm `gc-b` = `fah:db2f9b2` + `MIMALLOC_GENERIC_COLLECT=1000` (v3 `page.c:1004`, a non-forced heap collect every 1 000 slow-path mallocs on the allocating thread) | Same burst: 11.8 → 37.0 → 29.2 at +5 → **30.8 at +15 min, +19.0 MiB** (1 487 of 1 500 passed) | **fails** — allocation-path collection does not return them either |
| F19 | `side-base50/smaps.log` 20:40:27Z → 21:00:27Z across the refused burst ([readC](resoak-0.3.1-diag/readC-20260905T0011Z.txt) method) | +12.5 MB, all of it in the `[anon]` VMAs of mimalloc's `0x413…` arena reservation; `/fastadhunter` mappings +60 KB; VMA count 15 → 14 | the residue is arena memory mimalloc holds as in-use pages, not thread stacks or kernel buffers |
| F20 | burst-notes (F20 block), arms `bq-seq` / `bq-idle` (fresh `db2f9b2`, 50 QPS, egress allowed, burst at 15 min uptime like F14) | Same 1 500 requests over 50 **sequential** keep-alive connections (60 s): residual 12.7 → 12.6 → **12.4 at +15 min, residue 0**. 3 × 500 **idle** connections, no byte sent, 50 s hold: 12.4 → 19.6 peak → **14.7 at +15 min, +2.4** (34 % of a +7 peak; full bursts keep 70–100 % of a +22 peak) | residue scales with **concurrent transient volume × burst duration**, not with requests or connections: nothing per request survives, and what pins the pages is placed there while the transients are live |
| F21 | `cside-*/cpu.log` (utime + stime from `/proc/1/stat`, 60 s), 1.33 h windows | CPU ticks per hour: `park2` 216 vs `base2` 225 at 2 QPS; `park-b` 3 431 vs `bhttp-base` 3 467 at 50 QPS; `gc2` 203, `gc-b` 3 194 | the park collect costs nothing measurable on 32 workers; noted for the cost question, not a lever |
| F22 | [slopewin](resoak-0.3.1-diag/readB-20260904T2234Z.txt) method on `sysalloc2` at 8.0 h | musl at 2 QPS: slope **−0.01 MiB/h**, hourly minima 10.3 10.4 10.4 10.4 10.5 …; `base2` over its own first 8 h: 13.8 → 15.1 | at production rate the musl allocator does not climb in 8 h; the 2 QPS drift is mimalloc-specific at this window (16 h read pending) |
| F23 | burst-notes (F21 block), arm `bq-nodns` (fresh `db2f9b2`, warmed 15 min at 50 QPS, then `load-bq-nodns` stopped at 01:38:10Z; DNS query counter frozen at 44 740 and cache at 9 110 entries throughout) | F14 burst with **no DNS traffic**: residual 12.0 → 40.3 peak → 37.5 at +5 → **37.6 at +15 min, 37.7 at +17 — +25.6 MiB**, flat to 0.1 MiB in an idle process. The origin (`listsrv.py`, `SimpleHTTP/0.6`, HTTP/1.0) closes after every response, so no idle upstream connection survives a burst | the pinning population is **not** DNS-side inserts and **not** pooled upstream connections; it is produced by the HTTP burst itself and never decays without allocation activity — the largest residue is the one with the least traffic |
| F24 | burst-notes (F24 block), arm `bq-w4` = `db2f9b2` + `TOKIO_WORKER_THREADS=4` (the RB5009's count; 9 threads vs 34), 50 QPS DNS running | Same burst: residual 10.9 → 26.5 peak → 14.2 at +5 → 11.8 at +9 → **13.1 at +15 min, +2.2 MiB** (accounted +3.8 over the window) | **the residue scales with the number of tokio workers, not with transient volume**: 4 workers retain ~1/10 of what 32 retain for the identical burst. Production's geometry is the 4-worker one |
| F25 | burst-notes (F25 block), second identical burst on the idle `bq-nodns` | 37.7 → 45.2 peak → **42.4 flat, +4.7** on top of the first +25.6 | repeated bursts of the same size-class mix reuse the retained pages; the residue is **bounded**, not open-ended |
| F26 | [prod/perf-20260905T0250Z.json](resoak-0.3.1-diag/prod/perf-20260905T0250Z.json) (owner-mandated read-only `GET /api/v1/history/perf?from=2026-09-04T12:00:00Z`, 148 samples, not part of the declared pull log) | Production, 4 workers, ~1 QPS: the 18:04Z excursion 63.0 → 85.2 (18:46Z) → 69.8 (20:10Z); hourly RSS minima from 12:00Z: 63 65 65 66 66 63 **69 69** 65 64 62 61 62 63 61. A new excursion 62.5 → 69.0 at 02:40Z 09-05 | **the excursion decayed fully within 4 h and left no floor step**; the floor over these 15 h is flat (63 → 61). Matches F24: at 4 workers a burst retains ≤ ~2 MiB. The +19 MiB / 48 h of the owner's series is therefore not a constant-rate drift plus permanent steps; it is warm-up (+8.2 at h1) plus steps that this window does not reproduce |
| F27 | `telemetry.latency.dns` deltas over the same 6 h wall-clock window per pair (poller jsonl), ~1 M queries per 50 QPS arm | Mean per-query latency, mimalloc → musl: `base50` → `sysalloc50` cache hit 14.0 → 15.4 µs, block 12.6 → 13.9, forward 422 → 444; `base2` → `sysalloc2` 15.6 → 17.4, 13.8 → 15.2, 387 → 411; `bhttp-base` → `bhttp-musl` 14.5 → 15.1, 12.8 → 13.4, 448 → 427 | the allocator swap costs **+4..+12 % on the cached and blocked paths** on x86 (≈ +1.4 µs, ≈ +13 µs on the RB5009 by the 9× factor), at or just over the 10 % bench gate; forward is upstream-dominated and within noise. Dev-box figure, server-side counters, not a criterion bench |
| F28 | burst-notes (F28 block), [burst/visit-b-worker.log](resoak-0.3.1-diag/burst/visit-b-worker.log); arm `visit-b` = the park image plus a per-worker `mi_theap_visit_blocks` dump on every 6th collect and an abandoned-page dump each minute (owner-approved worktree edit) | Same burst: residue **+14 MiB** at +15 min (as park-b). On the workers that dump (only workers that keep parking do), the post-burst theaps hold small-class pages that are nearly empty: `bs=320` **14 pages, 6 live blocks** (0.75 MiB), `bs=80` 15 pages, 132 live blocks (0.88 MiB). Abandoned pages stay at 2–11 (≤ 0.65 MiB) except a 101-page / 6.5 MiB transient during the burst that is gone two minutes later | the residue is **per-worker pages in the small size classes the HTTP path uses, held with ~0 live blocks after a forced `mi_collect(true)` on that very thread**. Whether they hold zero blocks (mimalloc keeps them) or one or two (sparse-live) is what the whole-process dump (`fah:db2f9b2-visit2`, `mi_heap_visit_blocks(mi_heap_main())` from the reporter) reads next |
| F29 | burst-notes (F29 block), [burst/visit2-b-process.log](resoak-0.3.1-diag/burst/visit2-b-process.log); arm `visit2-b` = park image + `mi_heap_visit_blocks(mi_heap_main())` every 60 s from the reporter thread (whole process, all thread heaps) | Same burst, residue ~+14.5. Whole-process pages 223 / 38.3 MiB committed before → **866 / 57.8 MiB** after, stable from +3 min. The extra pages are **not empty**: per class, `8192` 70 pages / 447 blocks in use (3.6 MiB), `12288` 26 / 147 (2.3), `3584` 40 / 485 (1.7), `1536` 25 / 452, `512` 34 / 510, `384` 34 / 565, `256` 32 / 1 079, `128` 33 / 527 — **~450–500 blocks per HTTP-path class, the last round's connections**. The three workers that still park after the burst own none of those pages | `page->used` only decrements when the **owning** thread heap collects its remote-free list. These are remote frees pending on pages owned by workers that woke for the burst, allocated the connection state, then parked for good; the park hook ran on their way into idle, before the frees arrived, and never runs again. It explains every earlier reading: 4 workers keep cycling (+2.2), production decays over hours (F26), park-b returned the 30 % whose owners happened to re-park, the idle arm never decays, a second burst reuses the pages (+4.7) |
| F30 | burst-notes (F30 block), arm `park4-b` = park image + `TOKIO_WORKER_THREADS=4`, 50 QPS DNS; control `bq-w4` (F24) | Same burst at production geometry with the hook: residual 11.0 → 24.6 peak; after each round it is back to 12.2 / 12.0 within a minute; done 08:42Z; **12.1 at +9 min (+1.1), 13.1 at +15 min (+2.1, recompile wave inside the window)**. Control without the hook: 11.8 at +9 (+0.9), 13.1 at +15 (+2.2) | at 4 workers the residue is ~+1 MiB with or without the hook; the hook only shortens the return between rounds. **The steps pass the ≤ +1.3 gate at production geometry on the unmodified build**; no burst fix is needed for the RB5009 |
| F31 | [readD-20260905T0903Z.txt](resoak-0.3.1-diag/readD-20260905T0903Z.txt) (read D) | 16 h arms over h6–16: `stale50` (`serve_stale = false`) **+0.33**, `retain50` (`PAGE_FULL_RETAIN=0`) **+0.69**, `base50` +0.29, `sysalloc50` +0.25 MiB/h. At 2 QPS over h1–15.4: `sysalloc2` **−0.03** (minima 10.3 … 10.2), `base2` +0.28. Drift arms at 9 h, h1–end: `park2` +0.29, `gc2` +0.48, `norec2` +0.44, `manyrec2` +0.53, `c100-2` +0.33 | decision table: **neither** cache lifetime nor page policy moves the drift, and neither does a 100-entry cache, no recompiles, 12 recompiles/h, or either collect strategy — on 32 workers every mimalloc arm climbs +0.3–0.5 MiB/h and musl is flat for 15 h at production rate. The drift is the F29 mechanism at low rate: idle workers that tokio wakes occasionally own pages whose remote frees are never collected |
| F32 | [prod/perf-20260905T0903Z.json](resoak-0.3.1-diag/prod/perf-20260905T0903Z.json) (read-only pull, 02:00Z → 09:03Z) | Production RSS hourly minima 61 63 63 62 62 63 (67 for the partial last hour); a 07:04Z excursion to **90.2 MiB at 07:16Z** back to 63 by 07:46Z. Together with F26 the floor over 21 h is 61–63 MiB with two +22..+27 MiB excursions that decayed within 45 min | at 4 workers and ~1 QPS the production floor is **flat over 21 h**; the earlier +0.34 MiB/h reading came from a window with warm-up and steps in it. The G2 risk on the RB5009 is the excursion itself, not a floor climb |
| F33 | [prod/perf-full-20260905T0931Z.json](resoak-0.3.1-diag/prod/perf-full-20260905T0931Z.json) (read-only pull of the whole soak, T0 → T0+102 h, 981 samples) | 6-hour RSS minima from T0: **43.4, 52.1, 52.3, 54.5, 58.2, 58.4, 63.3, 61.4, 64.1, 64.4, 63.8, 63.5, 63.2, 63.0, 61.2, 60.8, 62.5**; residual minima 17.7 → 36.9 (h54–60) → 34.1–35.9 since; accounted 25.8 → 28.5 → 26.7. Excursions ≥ 80 MiB: 09-02 17:46–18:40Z (peak 99), 09-04 18:22–18:46Z (85), 09-05 07:04–07:16Z (90) | the whole climb is inside the first 48 h (warm-up +8.7, then three steps of +2..+5 under the evening excursions); **from h48 to h102 the floor is flat to slightly down (64.4 → 60.8 → 62.5)**. This is the bounded reuse of F25 seen in production: each new size-class mix pins its pages once, later bursts reuse them. On this trajectory the day-7 G2 read passes; what remains is the excursion peak (bounded by `[http] max_connections`) and its attribution |
| F34 | Owner's read-only router output 2026-09-05 17:20Z (`/system/resource/print`, `/container/print detail`, `/disk/print detail`), [prod/prod-memory-20260905T1716Z.json](resoak-0.3.1-diag/prod/prod-memory-20260905T1716Z.json), [prod/prod-perf-20260905T1717Z.json](resoak-0.3.1-diag/prod/prod-perf-20260905T1717Z.json), [measurement-traps.md §Memory](../../measurement-traps.md) | RouterOS "Memory Usage" graph: used 391.2 MiB (1024 − 632.8 free), climbing in stairs ~250 → 391 since Tue. Container `memory-current` **129.9 MiB**, `memory-high=unlimited`; FAH `process_rss` 64.4, `rss_file` 8.8 — so the container is 64 process + ~65 page cache/kernel, and **~261 MiB is RouterOS itself**. Router uptime 4d9h53m ⇒ **rebooted 07:26Z 09-01, 100 s before T0**: the graph is the first 4.5 days after a reboot. FAH writes ~0.6 MB/day of history (`sample_interval_seconds=360`, `retention_days=30`) plus 28 MB of list bodies replaced in place: cache ceiling ~50 MB, reclaimable. Production RSS over the graph's daily window (17Z 09-04 → 17Z 09-05): hourly minima 61–65, two excursions (85 at 18Z, 90 at 07Z) that returned; the router steps coincide with them and never return | the stairs cannot be FAH's writes (0.6 MB/day vs 20–30 MiB steps); the container's share is bounded at ~64 + 50 and cannot be OOM-killed for cache. Day-after check 2026-09-06 06:38Z ([prod/prod-memory-20260906T0645Z.json](resoak-0.3.1-diag/prod/prod-memory-20260906T0645Z.json)): router used 391.2 → **325.5 (−65.7)**, container `memory-current` 129.9 → 127.7, FAH RSS 65.1. **Cause found by the owner:** nine p3-06 verification image tars (`fah-probe*`, `fah-bench*`, `fah-certs*`, `fah-splicebench*`, `fah-p4*`, ~138 MB) uploaded to `kingston` on 09-04/09-05 at the times of the steps; the page cache from those writes counted as "used" and was released when the files were deleted. The +141 MiB of stairs reconcile to the tars' size. **RouterOS-side, closed.** Consequence for any router benchmark: delete the uploaded image tar before reading memory |
| F35 | [phase2.6-audit.md](phase2.6-audit.md) §"Excursion cause — FOUND" and §"Re-soak termination — 0.3.0"; [resoak-0.3.0/pull2-perf.json](resoak-0.3.0/pull2-perf.json); pull times from `uptime_seconds` in `resoak-0.3.1/pull*-telemetry.json`; [burst/hist4-run.log](resoak-0.3.1-diag/burst/hist4-run.log) | The excursions are not new to 0.3.1: **0.2.20** had +12..+23 MiB steps traced to full list re-downloads buffered in RAM (fixed by conditional GET in 0.3.0); **0.3.0** had +7..+18 MiB per history read (dashboard, gate pulls parsing `upstreams`; fixed by `d420f38`, in 0.3.1) and RSS max 94.8 with 12 rows > 80 MiB; **0.3.1** keeps three ≥ 80 MiB spikes. Of those, 09-05 07:04Z is a recompile (`bodies` 37 → 39). 09-02 17:46Z and 09-04 18:04Z are **not** a pull (declared pulls 22:19Z 09-01, 05:16Z, 08:14Z 09-02, 07:40Z 09-03; ad-hoc 19:04Z 09-04, 02:50Z / 09:03Z / 09:28Z / 17:1xZ 09-05 — none inside a spike), **not** the dashboard (owner does not open it), **not** upstreams (8 rows with any failure/penalty in 4 days, none at either onset; pool UDP, no TLS), **not** DNS (queries flat or falling, cache and accounted flat). `hist4`: six full-row 4 300-row `/history/perf` reads on 0.3.1, 4 workers: RSS 38.6 → 40.6 at +15 min (+2.0); history reads cleared as a spike source on 0.3.1 | two evening spikes remain **unattributed**; every exported counter is flat at their onset, the only path with no per-sample counter is the HTTP proxy — the gap the `concurrent_connections` high-water mark (`4eddc39`) closes in the next soak |
| F36 | [allocation-domains-proposal.md](allocation-domains-proposal.md) §Validation plan; [burst/httprt4-b-2w.jsonl](resoak-0.3.1-diag/burst/httprt4-b-2w.jsonl), [ctl4-b-2w](resoak-0.3.1-diag/burst/ctl4-b-2w.jsonl), [httprt4-b-1w](resoak-0.3.1-diag/burst/httprt4-b-1w.jsonl), [ctl4-b-1w](resoak-0.3.1-diag/burst/ctl4-b-1w.jsonl); burst-notes (two `rig … httprt A/B` lines) | HTTP proxy on its own tokio runtime, DNS on 4 workers, same-time control, F14 burst. **2 HTTP workers**: residual pre 11.8 / 11.0, peak 28.6 / 27.9, done+9 **+11.6 / +0.2**, done+17 **+12.3 flat / +1.7** — six times the control, no decay. **1 HTTP worker**: peak 25.3 / 29.3, done+0 **+1.8 / +15.2**, done+1 +1.4 / +9.6, done+17 +3.7 / +1.9 (inside the ±6 MB purge band) | F29 confirmed from the other side: a multi-worker isolated runtime *creates* owners that park for good (tasks migrate between its workers, then both idle), and retains; a single-worker runtime has only local frees and returns the burst within one sample, while the shared 4-worker pool heals in ~9 min through DNS-driven owner cycling. **Isolated runtimes must be single-threaded.** No floor difference shown at +17 min; the topology is not a floor fix on this evidence — its value, if any, is instant return and per-domain attribution (proposal, unvalidated) |

## Proposed fix

No code change yet. Two problems, two measurements.

**Steps (F10–F12), the G2 killer.** Each household proxy burst leaves ~20 MiB
of freed-but-retained mimalloc pages. Levers, cheapest first: (a) an allocator
return after the burst — `mi_collect(true)` from the 10 s telemetry tick when
RSS fell by more than N MiB since the last tick, or unconditionally every N
minutes (one call in `allocator.rs`, no hot-path cost); (b) ~~`MIMALLOC_PAGE_FULL_RETAIN=0`~~
closed by F14 (+15.1 MiB residue); (c) a lower
`[http] max_connections` so a burst cannot reach 500 concurrent — bounds the
transient, not the residue per connection; (d) the musl allocator, which keeps
6 % of the residue but was rejected for hot-path cost. Measurement per lever:
the F11 burst on a fresh arm, RSS at +15 min minus RSS before the burst, pass if
≤ the musl arm's residue (+1.3 MiB) and the `fah-dns` bench within 10 % of
`db2f9b2`. Then one 7-day soak on the RB5009 against the standing gates, G3 now
able to attribute a step to a burst only if the proxy exports a concurrent
connection high-water mark per sample — add that counter to the perf sample
first, whatever else is chosen.

**Drift (F1–F5, F15), the G1 band.** +0.34–0.67 MiB/h at production rate on
x86. The arms `cstale` / `cretain` / `stale50` / `retain50` / `sysalloc2` decide
whether cache lifetime or page policy moves it; read them before choosing
(F16: `cctl` / `cstale` / `cretain` at 7.1 h, +0.51 / +0.51 / +0.46 — neither
separates; the 16 h read of `stale50` / `retain50` is pending). F15 names the
running ablation: `norec2` (no recompile in the window) and `manyrec2` (one
every 5 min) against base2 over the same window.

**Mechanism, revision 4.** The cross-thread-free model (remote frees waiting
for the owning worker) was tested by F17 (`mi_collect(true)` on every worker
park) and F18 (mimalloc's allocation-path collect) and **falsified**: both leave
70–90 % of the residue. The page-pinning-by-survivors model was then narrowed
by F20–F25: no per-request survivor exists (sequential requests leave 0), the
stats top-N allocates a key only for a new domain, the origin closes every
upstream connection, and the residue needs no concurrent DNS inserts. What is
left is **per-worker page state**: the residue scales with the number of tokio
worker threads (32 → +25, 4 → +2.2), is bounded (second burst +4.7), and is
mimalloc-specific (musl +0.5..+1.5). Which blocks hold those pages is not
named; the proposed heap visitor ([park-visit.patch.rs](resoak-0.3.1-diag/park-visit.patch.rs),
not applied) would print per worker and size class how many pages are held and
how many blocks in them are live.

**What this means for the fix (F29).** The mechanism is now specific: a
block freed on a thread other than the owner of its page is reclaimed only when
the owner next collects, and a tokio worker that has parked for good never
does. mimalloc's own periodic collect runs on the allocation path (`generic_collect`,
F18), so it has the same blind spot. Two levers follow, cheapest first:

1. **Collect on the owner when it goes idle *and* keep collecting while idle.**
   `on_thread_park` covers the first half (F17); the second half needs the
   parked workers to run once in a while. At the RB5009's 4 workers every
   worker cycles through park on ordinary DNS traffic, so the hook alone may
   be enough there — `park4-b` / `park4-2` vs `w4-2` decide it. On a 32-worker
   box it is not, and that is the dev-box result.
2. **Let idle workers hand their pages back.** If a worker abandons its thread
   heap when it parks (the path an exiting thread takes, F6/R5), a later remote
   free on an abandoned page is collected by the freeing thread itself
   (`free.c`, `mi_free_try_collect_mt`) and an all-free page is returned at once.
   mimalloc exposes this only through thread exit; doing it from the park hook
   would need a v3 entry point that is not public today, so this is an
   upstream question, not a FastAdHunter patch.

**Owner decision, 2026-09-05 12:4x local.** (1) `concurrent_connections`
high-water mark per listener in the perf sample — yes, the one code item.
(2) `[http] max_connections` — **not touched** without evidence that 1024
causes an operational problem on the router; F32/F33 show the excursions are
transient and the floor holds, so no preventive cap. (3) The park hook waits
for the 4-worker drift arms. (4) G2 is confirmed on the day-7 read of the real
router. (5) The allocator stays as it is until 4-worker data asks otherwise.

**Same day, evening.** (1) is implemented and committed as `4eddc39` on
`phase3-06` (not deployed; deploy after the day-7 read, so the soak is not
restarted). (3) is closed without the 18 h read: the router's own floor (F32,
F33) answers the 4-worker drift question and no arm outcome could change a
decision; the rig was stopped. (5) is re-confirmed as a **no**: musl's
+4..12 % per cached/blocked query (F27) is rejected on the performance
objective, whatever it buys in RSS. The remaining lever under mimalloc is the
runtime topology of [allocation-domains-proposal.md](allocation-domains-proposal.md)
(F36: single-threaded isolated runtimes return a burst at once; floor effect
not shown), pursued only for attribution, at the owner's option.

Production geometry (F24, F26) makes lever 1 the candidate. The drift arms
(`park2`, `gc2`, `norec2`, `manyrec2`, `c100-2`, `sysalloc2`, `park4-2`,
`w4-2`) at 16–18 h decide whether the same mechanism is the drift; `sysalloc2`
flat at 13 h (F22) is the allocator-swap reference at +4..+12 % per query
(F27). Measurement: F14 burst residue ≤ +1.3 MiB **at 4 workers**, 18 h slope
≤ 0.22 MiB/h at 2 QPS with 4 workers, `fah-dns` bench within 10 %, then the
7-day soak on the RB5009. Whatever ships, add the concurrent-connection
high-water mark to the perf sample so a production step can be attributed.

## Hand-off — state at 2026-09-06 07:xxZ (10:xx local)

For the next agent. Rules first, then state, then what is next. Nothing here
is a permission.

**Rules that were broken in this diagnosis and cost time.** (1) Every `.md`
edit, including files the agent itself created minutes earlier, is proposed as
old → new and waits for a yes; a go covers one edit, never the file. (2) No
commit, tag, push or `scp` without a go for that exact changeset. (3) The
router is read-only; a needed change is proposed as exact commands and the
owner runs them. (4) Lead with the answer; the owner reads plain sentences,
not dense paragraphs. (5) Chat times: UTC with Z, then Bucharest (UTC+3).

**Verdict, in one paragraph.** FAH has no leak. The 43 → 64 MiB climb is
mimalloc retention after bursts (F29), bounded at 4 workers, finished by h48,
floor flat h48–h102 (F32, F33). The ≥ 80 MiB spikes predate 0.3.1 (F35); one
is a recompile, two evening ones stay unattributed because the proxy exports
no per-sample counter — `4eddc39` adds it. musl would remove the retention at
+4..12 % per query and is **rejected** by the owner (performance objective).
The RouterOS "used" stairs were page cache from p3-06 image tars on the SSD,
not the container (F34). An isolated-runtime topology was tested: 2 workers
FAIL, 1 worker returns bursts at once, no floor difference (F36); it lives on
as [allocation-domains-proposal.md](allocation-domains-proposal.md), unvalidated
on the router.

**Where things are.**

| Item | State |
| --- | --- |
| Production | 0.3.1 = `db2f9b2` (on `main`), mode `dns+http`, one runtime, 4 workers, mimalloc. RSS 65.1, residual 35.6, accounted 29.6 at 06:45Z 09-06. Container `memory-current` 127.7, `memory-high=unlimited` |
| Soak | T0 2026-09-01T07:27:49Z; **day 7 closes 2026-09-08T07:27:49Z = 10:27:49 local**. Owner pulls after that; G2 = slope of the residual/RSS floor from h48 to h168, gate per [resoak-0.3.1-predeclaration.md](resoak-0.3.1-predeclaration.md). Method: hourly minima, least squares, `E:/fah-diag/out/slopewin.py` shape. On the F33 trajectory it passes |
| Repo | branch `phase3-06`, HEAD `4eddc39` (counter). HEAD − `db2f9b2` = all of Phase 3, **not soaked**; 0.3.2 must be `db2f9b2` + `4eddc39` only (cherry-pick conflicts in `fah-http` `lib.rs`, `server.rs`; `tls_server.rs` absent there) |
| Uncommitted | this file, `allocation-domains-proposal.md`, `docs/project-state.md` §Next (2), `resoak-0.3.1-predeclaration.md`, `resoak-0.3.0/origin-log.tsv`, the whole `resoak-0.3.1-diag/` and `resoak-0.3.1/pull*` raws. One docs commit after the owner's go |
| Dev box rig | every container under `E:/fah-diag` stopped, not removed (`docker ps -a`); one poller pattern in `tools/poll.py`; series in `E:/fah-diag/out/` (`*-run1/run2.jsonl`, `httprt/*-2w.jsonl`, `httprt/*-1w.jsonl`, `hist4/`); production pulls in `resoak-0.3.1-diag/prod/`; key in `E:/FastAdHunter/.vscode/production.key`, never printed |
| Worktrees | `E:/FastAdHunter-db2f9b2`, `E:/FastAdHunter-var-{count,sysalloc,v2,keepalive,park,httprt}`, all detached at `db2f9b2`, throwaway; images `fah:db2f9b2`, `-park`, `-visit`, `-visit2`, `-count`, `-sysalloc`, `-v2`, `-keepalive`, `-httprt` |
| Owner decisions | 2026-09-05: counter yes (done); `max_connections` untouched; park hook closed; allocator stays mimalloc (re-confirmed evening); topology only as a measured A/B on the router; dashboard not implicated; no rollback to pre-dashboard code |

**Next, in order.** Each step is spelled out in
[allocation-domains-proposal.md §Execution prompt](allocation-domains-proposal.md):

1. 2026-09-08 after 10:28 local: owner's day-7 pull; compute G2 from h48; close `p2.6-11`.
2. Part A: `release/0.3.2` = `db2f9b2` + `4eddc39` (go), gates, release commit + tag (go), arm64 image, `scp` (permission), owner swaps the container, tar deleted from `kingston`. Before the swap, on the running 0.3.1 and only after the day-7 read: router-benchmark rows 1–3 (A half); row 5 = the swap itself. Then the 0.3.2 soak with its pre-declaration (go).
3. Part B: `alloc-domains/0.3.2` build, topology only, dev-box gates, deploy, rows 1–6, verdict per §Scope, ADR if it passes.
4. Runbook 6 (24 h full-mode soak) after the 0.3.2 soak.

**Not to do.** Bursts or downloads against production before the day-7 read. Any container restart before it. Allocator changes. `max_connections` changes. Deploying HEAD.

**Opener for the next session** (root `CLAUDE.md` and `plan/CLAUDE.md` load by themselves):

```text
Continue the FastAdHunter 0.3.1 memory work. Read, in order:
1. docs/code-review/phase2.6/resoak-0.3.1-memory-diagnosis.md §Hand-off (end of file), then §Summary.
2. docs/code-review/phase2.6/allocation-domains-proposal.md §Execution prompt.
Rules there are absolute. Do nothing that changes the repo, the router, or a .md without my explicit go per action. Report state first, then wait.
```

All rig containers (`diag-*`, `load-*`, `side-*`, `cside-*`, `httpload-*`,
`burst-*`, `diag-coredns`, `diag-lists`, `diag-lists80`, network `fahdiag`) are
**stopped, not removed**, since 2026-09-05 19:40Z; raw series stay in
`E:/fah-diag/out/`. Worktrees `E:/FastAdHunter-db2f9b2`,
`E:/FastAdHunter-var-{count,sysalloc,v2,keepalive,park,httprt}` (the last two
owner-approved diagnostics: park-collect hook + heap visitors; HTTP on its own
runtime — images `fah:db2f9b2-park`, `-visit`, `-visit2`, `-httprt`) are
detached at `db2f9b2` and can be removed with `git worktree remove`. Repo
change: `4eddc39` only.

REPRODUCED — mechanism named (F29) and confirmed from the topology side (F36);
at production geometry the floor is flat (F32, F33); two evening spikes remain
unattributed until the next soak carries `concurrent_connections` (F35); the
RouterOS graph is mostly not the container (F34); the day-7 G2 read decides.
