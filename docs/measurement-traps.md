# Measurement traps

Readings that look obvious and are wrong. Each one produced a false conclusion
at least once. Budgets live in [PERFORMANCE.md](../PERFORMANCE.md); this is only
about how to *read* a number.

## Carry a control arm

**One measurement the change cannot plausibly affect, run in the same session.**
It is the cheapest instrument here and has caught three separate wrong
conclusions:

- `http_pass_through/direct_to_origin` — which does not go through the proxy at
  all — moved **+8.8 %** (p=0.13), which alone said the box was drifting and the
  proxy arm's +9.5 % was not real.
- The deployed corpus at 8 KiB moved **−4.6 %** across two on-device sessions
  while RouterOS reported the clock going 350 → 1400 MHz. A real 4× clock change
  had to show ~4× there. That refuted "the RB5009 does not boost, so every ARM
  figure is an upper bound."
- The same arm exposed a cost model fitted across two corpora differing 26× in
  rule count, which had charged 97 % of a lookup to a term that was really 32 %.

The pattern in all three: **a caveat on a weak comparison does not make it safe.
A control arm does.**

## Interleave the arms — one pair per arm is not an A/B

Pinning removes jitter *within* a run. It does nothing about drift *between*
runs, and drift on this dev box is ±5 %: two consecutive runs of the **same**
baseline binary moved `matcher_lookup/miss` by −4.3 % and
`full_pipeline/sustained_throughput` by −3.9 %.

Run **A/B/A/B**, then compare means and ranges, never a single pair. p2.5-05's
`forwarded_query_overhead` read **+8 %** on its first base-then-post pair —
the whole change looked like a regression. Two interleaved pairs put post
*faster* than base both times; across four runs per arm the means were 0.4 %
apart with fully overlapping ranges. The +8 % was ordering, not code.

The control arm says how wide the noise band is; interleaving is what keeps the
measured arms from each sitting in a different part of it.

## Calibration

| Trap | Reality |
| --- | --- |
| Scaling an x86 figure by the reported CPU frequency | **`cpu-frequency` / `scaling_cur_freq` must never scale a result.** Two runs of the same probe reported 350 and 1400 MHz and a control arm moved −4.6 % where a real 4× clock change had to show ~4×. Use the measured **~9× x86 → RB5009 factor**. |
| Criterion's `change:` line | It compares against the **previous run**, whatever that was — meaningless across variants. |
| Unpinned benchmarks | **6× swings between consecutive runs of an unmodified binary** on this dev box (matcher `miss`: 65 → 400 → 65 ns), which manufactured a phantom "+159 % regression". Pinned, the same bench holds a CI under 1 %. Recipe in [PERFORMANCE.md](../PERFORMANCE.md) §Measuring reliably. Pin *throughput* benches to four cores, not one, to match the RB5009 — see the next row for what "four cores" must mean. |
| A four-core affinity mask on its own | **Two ways to pin four cores and measure something else** (p3-06 F8, 2026-09-02). (1) The mask restricts the process but tokio still spawns one worker per *machine* CPU (32 on the dev box), so four cores run 32 spinning workers: set `TOKIO_WORKER_THREADS=4` with the mask. (2) `ProcessorAffinity = 15` is logical CPUs 0–3, which on a hyper-threaded part is **two** physical cores; use one logical CPU per physical core (`0x55` on the i9-13980HX). Measured: `http_pass_through/direct_to_origin` 70 µs ± 20 % with mask 15 and 32 workers, 31.7 µs ± 0.6 % with `0x55` and 4 workers — the unpinned mean, five times tighter. The handshake arm under the wrong pin read 13.8 ms against 1.1 ms unpinned. |
| A synthetic URL corpus | Can contain 0 unindexed rules and hide a real-corpus regression entirely. |
| Local benches for an allocator question | Criterion on Windows diffs against the **Windows heap**, not musl `mallocng`, and is single-threaded, so it structurally cannot show a cross-thread allocator win. Only an on-device A/B settles it. |

## Memory

| Trap | Reality |
| --- | --- |
| `allocator_committed_bytes` as a live figure | **Lifetime high-water mark.** mimalloc v3 never decrements it on purge, so `peak == current` always and it routinely exceeds RSS several times over (318 MB against 70 MB resident). Subtracting `accounted` from it and calling the result retention produced 260 MiB of "retention" in a 70 MiB process — impossible, and the field was deleted. |
| RouterOS `memory-current` climbing | **cgroup v2 charges page cache**, while `free-memory` counts that same cache as available. Reconciled to 0.06 %: 70.29 MiB RSS + 525.05 MiB on-disk data = 595.34 vs 595.0 measured. A container restart dropped it 595.0 → 60.1 MiB. |
| `free-memory` as a footprint proxy | Useless. RSS fell 66 MiB across a swap while the router reported 1.5 MiB *more* free. Judge memory only by `/debug/memory` `process_rss` + `/history/perf`. |
| Growth toward a cap | **Not a leak.** A bounded structure filling to its cap looks identical to unbounded growth if you compare a near-empty state against a saturated one. Compare two *saturated* states — a test once read 9.8× growth for exactly this reason. |
| A residual read twice | Two readings cannot establish a trend. State a **slope over the final third** of the window; mimalloc's purge band alone is ±6 MB, so under ~2 MB of half-to-half drift is noise. |
| `debug/memory.cache_estimated_bytes` vs `/api/v1/cache.bytes` | They differ **by design** at the same entry count (1,450,032 vs 1,024,000) — the former adds table slabs and queue rings. Consequence: `max_bytes` bounds entry heap only; true footprint ran **1.42×** it. |

## Traffic and rates

| Trap | Reality |
| --- | --- |
| `/stats` block rate during or after a load test | Synthetic queries swamp the household sample. A real **51.8 %** read as **0.242 %** — a 200× error — because 29.2 M synthetic queries at 0.1 % blocked drowned 59,177 real ones. Household mean is ~0.68 QPS. |
| Upstream `failures` as client timeouts | `UpstreamPool::forward` bumps a server's `failures` per *attempt* but returns `Ok` as soon as any server answers. A primary drop rescued by the fallback is a **client success**. Under `strategy = "adaptive"` the counter also *under*-reports the outage: a penalized endpoint is skipped, so its `attempts` and `failures` grow at the probe rate, not the query rate — the longer the outage runs, the smaller it looks. Size an outage from `penalties` and the row below; `state` says what is true now. |
| `penalized_seconds_total` as time spent unavailable | It is **scheduled**, not elapsed: the nominal penalty for the round, banked when the endpoint enters `penalized`, jitter excluded, and never advanced while it sits there. It is a lower bound when no query arrives to probe after the deadline, and an upper bound when an in-flight answer restores `healthy` early. A figure flat across two reads means no new penalty, not a recovered endpoint. |
| `failure_runs` bucket length as outage duration | A run is consecutive failed *attempts*, and `adaptive` throttles a penalized endpoint to one probe per penalty round. At the defaults (`timeout_ms = 800`, `penalty_failures = 2`) the nominal ladder is 24 / 48 / 96 / 192 s then 300 s a round, so a 30-minute outage costs 2 opening failures plus 8 probes — a run of **≈10**, not 1800/300. Every such run lands in the `>= 4` bucket, which is where the histogram stops resolving. Read run length as attempts, never as time. |
| `tls_handshakes` as a per-query counter | It must track **time**, not queries. Idle-close + reconnect is correct behaviour, so an absolute cap fails a healthy run. Test: zero increments during a continuous burst, single digits per server over 24 h. Bug signature is the ratio approaching 1. |
| A traffic collapse mid-soak | Check `/api/v1/clients` `last_seen` first. One phone leaving took the window from ~2300 q/h to ~135 q/h and turned a load soak into an idle soak — which proves far less. |

## Disk and retention

- **`/data` is bounded by age alone.** `[history] retention_days` is the only
  retention control left; there is no byte cap on any series, so size is
  `cadence × retention`, and halving `sample_interval_seconds` doubles the disk.
- History costs **~740 B per perf sample** — per *sample*, not per query. ~31 MB
  at the default 30-day retention, plus ~6.9 MB for the p2-07 memory breakdown.
- **A deploy that changes what `stats` accounts for steps the residual.**
  Removing the query log's ring moved its bytes out of the `stats` component and
  into the residual at once. Re-baseline across such a deploy; a step is not a
  slope.

## Cost attribution

- Subtract harness cost before attributing time to a pipeline stage.
- A two-point fit across corpora differing 26× in rule count **cannot** attribute
  a gap to one variable. That mistake produced "97 % of the lookup is the
  unindexed scan"; the real figure was 32 %.
- Per-rule parse cost uses the **parsed** count as denominator, not the compiled
  one — using compiled gives a wrong "barely scales" conclusion.
- **Size a refactor off the call graph, not the type graph.** The p2-01 audit
  listed four consumers to change for widening `QueryEvent`; in practice
  `Stats::record` and `Metrics::record` are each called from exactly one place,
  so it was one dispatch site plus two new entry points.
- Perf priority order for this pipeline, in order: insert-path allocation churn
  (entry drop, key clones), cache lookup, DNS packet parse, response serialize,
  the tokio/socket path, lock contention, policy engine. **The
  matcher is not on this list** — it is 3 % of a cache-hit query and ~20 % of a
  blocked one; tuning it buys effectively nothing. Build a decomposition bench
  before optimising any of them.
