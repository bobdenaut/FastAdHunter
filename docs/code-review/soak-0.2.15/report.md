# Soak 0.2.15 — 20.4 h, RB5009

Device: RB5009UG+S+, RouterOS 7.21.5 (long-term), 4× ARMv8, 1 GB shared.
Container `fastadhunter`, image `fastadhunter-rosready-0.2.15.tar`, distroless/musl.
Allocator: mimalloc — `MIMALLOC_PURGE_DELAY=0 MIMALLOC_PURGE_DECOMMITS=1
MIMALLOC_ARENA_EAGER_COMMIT=0`.
Workload: live household DNS, no synthetic load, 1.06 qps mean / 5.98 qps peak.

| Point | Wall clock | Uptime | Stamp |
| --- | --- | --- | --- |
| T0 | 2026-08-10 08:41:50Z | 356 s | `20260810T084150Z` (wider set `20260810T083852Z`) |
| T1 | 2026-08-11 05:03:33Z | 73 659 s | `20260811T050333Z` |

Snapshots live beside this file. The 72 h follow-up is in
[../soak-0.2.15-72h/report.md](../soak-0.2.15-72h/report.md).

## Decisions

- This run is not a memory baseline for 0.2.15 — a list refresh moves
  `ruleset_bytes` −6.41 MiB mid-window.
- Block-path latency (42.5 µs) and the correctness counters are reportable; the
  memory question is not.
- `residual_bytes` is the primary bounded-memory signal; `process_rss` alone
  carries no information at this sample count.
- Retention is a weak explanation for a rising `allocator_committed` under
  mimalloc's eager-purge configuration.
- T0 taken 356 s into the process is too early to serve as a latency baseline.

No bugs found.

## Measurements

Same process across the window: `process_peak_rss` 176 295 936 and
`major_page_faults` 13 at both points. Router boots 2026-08-10 08:35Z; the
container start is the run start.

### Ruleset — moves mid-window

A refresh lands 2026-08-10 13:18Z, 4.6 h in. `big.oisd.nl` shrinks
436 345 → 251 486 rules upstream. `config-*.json` is byte-identical after `jq -S`.

`dyndns` and `doh-vpn-proxy-bypass` report `last_status: failed` for the whole
run — their authors withdrew the source URLs. The appliance keeps the cached
rules active and filtering is unaffected: refresh failure degrades to the last
good copy, which is the designed behaviour.

| Field | T0 | T1 | Δ |
| --- | --- | --- | --- |
| rules | 798 760 | 608 140 | −190 620 |
| duplicates_removed | 349 264 | 343 099 | −6 165 |
| compile_duration | 2.561 s | 2.110 s | −0.451 s |
| ruleset_bytes | 25.81 MiB | 19.40 MiB | −6.41 MiB |

### Memory

| Field | T0 | T1 | Δ |
| --- | --- | --- | --- |
| process_rss | 54.00 MiB | 50.86 MiB | −3.14 MiB |
| ruleset_bytes | 25.81 MiB | 19.40 MiB | −6.41 MiB |
| cache_estimated_bytes | 172.7 KiB | 3.30 MiB | +3.13 MiB |
| stats_aggregates_bytes | 634.0 KiB | 548.6 KiB | −85 KiB |
| stats_clients_bytes | 258.8 KiB | 258.6 KiB | flat |
| residual_bytes | 27.14 MiB | 27.37 MiB | +0.23 MiB |
| allocator_committed (= peak) | 283.87 MiB | 292.12 MiB | +8.25 MiB |
| cgroup `memory-current` | 92 131 328 | 90 107 904 | −1.93 MB |

RSS series, `history-perf`, 203 samples after container start:

| first | last | min | max | mean |
| --- | --- | --- | --- | --- |
| 54.55 | 50.00 | 45.42 | 85.87 | 54.82 MiB |

A 40 MiB peak-to-trough band on a 50 MiB process: single-point RSS comparisons
are not usable at this sample count.

Unexplained drift, hourly means:

| Window | Mean RSS | Traffic |
| --- | --- | --- |
| 14:00–19:00Z | 50.92 MiB | ~7 000 q/h |
| 00:00–04:00Z | 61.68 MiB | ~1 200 q/h |

Series halves: H1 53.12 MiB, H2 56.50 MiB — H2 sits entirely after the
−6.41 MiB ruleset step, so its non-ruleset part is ~9.8 MiB above H1. RSS rises
while qps falls; load does not explain it. Against a leak reading: the cgroup
counter falls 1.93 MB over the same window.

Scoped claim: **over 20.4 h, on this device, at ~1 qps, accounted memory is
bounded and `residual_bytes` is flat; a ~10 MiB upward RSS drift is unexplained,
and 20 h at 1 qps cannot separate retention from growth.**

### Throughput and latency

Window deltas (T1 − T0). T0's own means come from 727 / 176 / 211 samples over
six minutes and are not a usable baseline.

| Path | Count | Mean |
| --- | --- | --- |
| DNS block | 56 878 | 42.5 µs |
| DNS cache hit | 18 154 | 52.3 µs |
| DNS forward | 2 337 | 25.49 ms |

77 372 queries in 73 303 s, block rate 73.5 %. Forward latency is upstream RTT.

### Correctness counters

| Counter | Value |
| --- | --- |
| events_dropped | 0 |
| SWR enqueued / completed / deduplicated / dropped / failed | 9 875 / 9 875 / 559 / 0 / 0 |
| cache cleanup | 204 runs, 0 removed, 0 bytes freed, last 333 µs |
| cache entries | 2 292 / 50 000 (4.58 %) |
| cache bytes | 2.06 MiB / 64 MiB (3.22 %) |
| cache evictions | 0 |
| upstream 1.1.1.1 | 17 579 attempts, 23 failures (0.13 %), 0 consecutive |
| upstream 9.9.9.9 | 23 attempts, 4 failures, 0 consecutive |
| RouterOS | cpu-load 0, container cpu 0.2 %, free 836 MB / 1 024 MB |

Zero evictions at 4.58 % load: the 50 000-entry cap is far above this
household's need and is not a tuning target until load percentage moves.

## Files changed

None — measurement only.

## Remaining TODOs

- Memory question carried to the 72 h run:
  [../soak-0.2.15-72h/report.md](../soak-0.2.15-72h/report.md).
- Expose list refresh schedule (next-refresh time) so a soak window can be
  placed against it. `last_refresh` alone cannot tell when the next step lands.
