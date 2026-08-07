# Soak — 0.2.12

Started **2026-08-07 21:23:11Z** on the RB5009. Target: ≥ 48 h of real
household traffic. Budgets in [PERFORMANCE.md](../../../PERFORMANCE.md)
§Budgets; how to read a figure in
[measurement-traps.md](../../measurement-traps.md).

First release with `/metrics` removed, so `/api/v1/telemetry` is the only
snapshot surface.

## Capture

One command, from the repo root. Filenames are timestamped, so a later run
adds files rather than overwriting them.

```sh
D=docs/code-review/soak-0.2.12
K=<api key>            # tui-monitor/config.toml, or POST /config/apikey/rotate
B=https://172.17.0.2:8443
TS=$(date -u +%Y%m%dT%H%M%SZ)

for e in telemetry stats lists config debug/memory history/summary history/perf; do
  curl -sk -H "Authorization: Bearer $K" "$B/api/v1/$e" \
    -o "$D/$(echo $e | tr '/' '-')-$TS.json"
done
curl -sk -H "Authorization: Bearer $K" \
  "$B/api/v1/history/top?kind=blocked&n=25" -o "$D/history-top-blocked-$TS.json"
curl -sk "$B/health" -o "$D/health-$TS.json"

ssh bobdenaut "/system/resource/print"  > "$D/routeros-resource-$TS.txt"
ssh bobdenaut "/container/print detail" > "$D/routeros-container-$TS.txt"
```

`/history/perf` carries the continuous series between snapshots, so the gap
between captures is not a blind spot.

## T0 — 20260807T212311Z

| Figure | T0 | Budget |
| ------ | -- | ------ |
| version / uptime | 0.2.12 / 1002 s | — |
| ruleset rules / duplicates | 798 287 / 348 567 | — |
| ruleset compile | 2.774 s | — |
| ruleset heap | 27.0 MB | ≤ 40 MB |
| RSS | 59.7 MB | ≤ 128 MB |
| **peak RSS** | **125.1 MB** | ≤ 128 MB |
| residual | 31.6 MB (53 % of RSS) | — |
| minor / major faults | 37 746 / 0 | — |
| container (RouterOS view) | 56.4 MiB | — |
| DNS block / pass / allow | 900 / 393 / 0 | — |
| cache hit / miss / stale | 168 / 225 / 42 | — |
| cache entries | 213 / 50 000 (242 KB) | — |
| events_dropped | 0 | 0 |
| SWR enqueued / completed / failed / dropped | 42 / 42 / 0 / 0 | — |
| latency mean — block | 47 µs | p99 < 1 ms |
| latency mean — cache_hit | 46 µs | p99 < 1 ms |
| latency mean — forward | 10.75 ms | — |
| upstream 1.1.1.1 attempts / failures | 277 / 0 | — |
| upstream 9.9.9.9 attempts | 0 (fallback, unused) | — |

Means, not percentiles: `/telemetry` serves `count` + `sum_seconds`.
Percentiles come from `/history/perf`, windowed.

## What to check at T+48 h

| Question | Where | Fails if |
| -------- | ----- | -------- |
| Memory bounded? | `memory.process_rss`, `process_peak_rss` | peak > 128 MB, or RSS trending up across the whole window |
| Purge thrash? | `minor_page_faults` vs RSS | faults rising while RSS is flat — `MIMALLOC_PURGE_DELAY=0` returning pages it immediately refaults |
| Fragmentation? | `memory.residual_bytes` vs faults | residual rising while faults are flat |
| Under-counting? | `counters.events_dropped` | non-zero — Statistics *and* Metrics both lost events |
| Stability? | `process.uptime_seconds` | lower than the elapsed window — the container restarted |
| Latency held? | `/history/perf` p50/p99 | p99 > 1 ms for block or cache_hit |
| Upstreams healthy? | `upstreams[].consecutive_failures` | non-zero at rest |
| SWR keeping up? | `counters.swr.dropped`, `.failed` | either climbing |
| Lists refreshing? | `/lists` `last_status` | anything but `ok` after a scheduled refresh |

## Traps

- **Every counter is process-lifetime.** Delta two captures, and check
  `process.uptime_seconds` first — a restart returns them all to zero and the
  delta is meaningless, not merely small.
- **`cache_cleanup.last_duration_micros` is a gauge**, not a total. Do not
  delta it.
- **Peak RSS is a lifetime high-water mark** driven by the ruleset compile
  transient, not steady state. It does not fall back.
- **`cache_hits + cache_misses == pass + allow`**, never `+ block` — a blocked
  query never reaches the cache. Divide a hit ratio by resolved queries.
