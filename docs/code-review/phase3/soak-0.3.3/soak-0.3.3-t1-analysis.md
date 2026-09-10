# soak 0.3.3 — T1 against T0

T0 context, artifact list and reading rules: [`README.md`](README.md).

## Summary

T1 = `2026-09-10T15:55:29Z`, uptime 103,172 s (28 h 39 m) — day 1.2 of 7.

The plateau holds. `process_rss` is 0.93 MB **below** T0 and `residual_bytes` —
the leak signal — is 4.65 MB below it. Every byte of accounted growth is cache
fill. Cache sits at 2,960/50,000 entries and 2.5/64 MB with zero evictions, so
the plateau is not yet cap-limited and has room to rise before day 7.

One upstream incident, confined to a single 6-minute bucket. Two reporting
defects surfaced that the artifacts cannot show on their own: forward-latency
percentiles saturate at 100 ms, and a penalized upstream keeps that label after
its penalty expires.

## Decisions

- Plateau verdict stays on track; no memory action.
- Stats compare **T1-forward only**. T0 `stats.json` carries the previous run's
  24 h window across the restart — 166,350 queries at 78.4 % blocked is the
  contaminated verification traffic this soak exists to exclude, not a baseline.
  T1's 52,741 at 39.8 % is the household.
- `allocator_committed_bytes` is read as a lifetime high-water mark per
  [`wire.rs:1114-1120`](../../../../crates/fah-api/src/wire.rs#L1114-L1120). Its
  +139 MB is not a footprint change.
- Both reporting defects are recorded, not fixed: 0.3.3 is the deployed build
  and a rebuild ends the soak.
- `not_modified = 0` is a TODO, not a bug — no evidence yet which side fails.

## Bugs found

| # | Defect | Evidence | Severity |
| --- | --- | --- | --- |
| 1 | DNS forward latency p50/p99 cannot exceed 0.1 s — `BUCKETS_SECONDS` ends there ([`histogram.rs:14-16`](../../../../crates/fah-metrics/src/histogram.rs#L14-L16)) | Cumulative mean is 105 ms (443.22 s / 4,217) while `forward_p99` is at most 0.1 s in all 240 windows. The outage's multi-second forwards are invisible in percentiles | Observability — timeouts unreadable from percentiles alone |
| 2 | `UpstreamStatus.state` is read from the packed word with no penalty-deadline check ([`mod.rs:209-213`](../../../../crates/fah-dns/src/upstream/mod.rs#L209-L213)); probes are query-claimed, so a healthy primary starves the recheck | 9.9.9.9 and 2620:fe::fe report `penalized` at T1, 4 h after the incident, with `probes=0` and `penalized_seconds_total` of 660 s / 1,260 s | Cosmetic — label is stale, not live degradation |

Upstream RTT escapes defect 1: `UPSTREAM_RTT_BUCKETS_SECONDS` reaches 1.0 s.

## Measurements

### Memory — `debug/memory`, T0 vs T1

| Reading | T0 (uptime 120 s) | T1 (uptime 103,172 s) | Δ |
| --- | --- | --- | --- |
| `process_rss` | 59,031,552 | 58,060,800 | −970,752 |
| `residual_bytes` | 31,970,340 | 27,321,153 | −4,649,187 |
| `accounted_bytes` | 27,061,212 | 30,739,647 | +3,678,435 |
| `cache_estimated_bytes` | 233,344 | 3,922,352 | +3,689,008 |
| `cache_entries` | 200 | 2,960 | +2,760 |
| `ruleset_bytes` | 25,338,883 | 25,273,722 | −65,161 |
| `stats_aggregates_bytes` | 430,068 | 484,656 | +54,588 |
| `stats_clients_bytes` | 1,058,917 | 1,058,917 | 0 |
| `process_rss_file` | 9,285,632 | 9,285,632 | 0 |
| `process_peak_rss` | 133,156,864 | 148,373,504 | +15,216,640 |
| `allocator_committed_bytes` | 229,441,536 | 368,705,536 | +139,264,000 |
| `major_page_faults` | 13 | 13 | 0 |
| `minor_page_faults` | 72,731 | 701,121 | +628,390 |
| `cpu_user_ms` | 6,624 | 64,090 | +57,466 |
| `cpu_system_ms` | 1,100 | 67,182 | +66,082 |

CPU over the interval: 123.6 s of 103,052 s wall = 0.12 %.

### Memory — 240-sample series, last 24 h

| Series | min | max | last | trend |
| --- | --- | --- | --- | --- |
| `rss_bytes` | 53,432,320 | 76,455,936 | 58,560,512 | none |
| `rss_anon_bytes` | 44,146,688 | 67,170,304 | 49,274,880 | none |
| `rss_file_bytes` | 9,285,632 | 9,285,632 | 9,285,632 | flat |
| `memory.residual_bytes` | 25,499,661 | 48,464,174 | 29,222,721 | none |
| `memory.stats_clients_bytes` | 1,058,917 | 1,058,917 | 1,058,917 | flat |
| `cache.bytes` | 1,063,696 | 2,712,464 | 2,521,136 | rising |
| `cache.evictions` | 0 | 0 | 0 | flat |

The series starts at `2026-09-09T15:55:29Z`, so the soak's first 4 h 40 m appear
in no history window.

### The two RSS excursions

| Window (UTC) | RSS | Cause | Released |
| --- | --- | --- | --- |
| 16:57–17:21 on 09-09 | 55,750,656 to 76,455,936 (+20.7 MB) | HTTP proxy, `concurrent_connections.http` = 6 (run max 16; `https` = 0 throughout). Growth is 100 % `residual` / `rss_anon`, unaccounted by any counter | by 17:27 |
| 22:39–22:57 on 09-09 | unchanged, ~58 MB | list refresh — 10 bodies, 23.9 MB fetched. `peak_rss` 133,156,864 to 146,067,456; `allocator_committed_bytes` 236,519,424 to 365,428,736 | n/a |

`allocator_committed_bytes` and `peak_rss` step **only** at list refreshes
(22:39, 22:45, 22:57, then 10:57 next day). Neither tracks steady-state traffic.

### Upstream incident — `2026-09-10T11:57:56Z`, one 6-minute bucket

3 of 4 upstreams fail together. 68 `servfail_synthesized` + 82
`servfail_relayed` — the whole run's SERVFAILs bar 6. 1.1.1.1 is penalized and
healthy again by 12:03. It is the only upstream-state change in 240 samples
besides the recovery.

| Upstream | attempts | failures | rate | penalties | penalized s | probes | state @ T1 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1.1.1.1 | 23,333 | 52 | 0.2 % | 11 | 2,460 | 1 | healthy |
| 9.9.9.9 | 137 | 46 | 33.6 % | 5 | 660 | 0 | penalized (stale) |
| 2606:4700:4700::1111 | 225 | 86 | 38.2 % | 6 | 960 | 0 | healthy |
| 2620:fe::fe | 130 | 68 | 52.3 % | 7 | 1,260 | 0 | penalized (stale) |

Secondary failure rates are inflated by selection: they are tried only while the
primary is penalized, which is when the network is already failing.

### Traffic and latency, T0 to T1 (counters reset at T0)

| Counter | T1 value | Note |
| --- | --- | --- |
| `dns.pass` / `dns.block` / `dns.allow` | 41,865 / 34,829 / 3 | 45.4 % blocked since restart |
| `dns.cache_hits` / `cache_misses` | 37,651 / 4,217 | 89.9 % hit |
| `swr.enqueued` / `completed` / `deduplicated` | 19,424 / 19,424 / 726 | drains fully |
| `cache_cleanup.runs` / `entries_removed` | 286 / 344 | 1,434 µs last run |
| `latency.dns.forward` | 4,217 obs, 443.22 s | 105 ms mean — see defect 1 |
| 1.1.1.1 `rtt` | 23,281 obs, 340.68 s | 14.6 ms mean, p50 5 ms |
| `latency.dns.block` | 34,829 obs, 1.27 s | 37 µs mean |
| `latency.dns.cache_hit` | 37,651 obs, 1.82 s | 48 µs mean |
| `http.pass` / `refused` | 1,563 / 186 | `response_bytes` 269,785,291 |
| `lists.bodies` / `bytes_fetched` / `not_modified` | 17 / 31,870,510 / **0** | see TODO |
| `ruleset.rules` | 754,405 (T0: 756,492) | `duplicates_removed` 436,837 (T0: 451,062) |

Excluding the outage bucket, forward mean is ~5 ms.

### Cache size disagreement

| Source | T0 | T1 | implied B/entry |
| --- | --- | --- | --- |
| `telemetry.cache.bytes` | 185,552 | 2,525,776 | — |
| `debug/memory.cache_estimated_bytes` | 233,344 | 3,922,352 | — |
| gap | 47,792 | 1,396,576 | 239 then 472 |

Per-entry gap is not constant, so it is not a fixed overhead. Quote one figure,
not both.

## Files changed

None. Analysis only.

## Remaining TODOs

| # | Item |
| --- | --- |
| 1 | `list_fetch.not_modified = 0` across all 17 body fetches (31.87 MB in 28.7 h) while conditional GET is wired ([`lifecycle/mod.rs:730-738`](../../../../crates/fah-rules/src/lifecycle/mod.rs#L730-L738)). Determine whether the sources send no validators or the on-disk validator cache is not being hit after a restart |
| 2 | Attribute the 20.7 MB HTTP-path excursion. It is the largest unaccounted movement in the window and lands entirely in `residual` |
| 3 | Widen `BUCKETS_SECONDS` past 0.1 s, or publish the forward mean beside the percentiles, so timeouts are readable (defect 1) |
| 4 | Check the penalty deadline when reporting `UpstreamStatus.state`, or expose the deadline (defect 2) |
| 5 | Re-read at day 7 (`2026-09-16T11:15:15Z`). Cache is at 5.9 % of capacity, so the plateau can still rise |

## Scope

RB5009, `fastadhunter:0.3.3`, this household's traffic, 28 h 39 m of 168. Says
nothing about P3's per-session RSS ceiling — `p3-06-testing-plan.md` owns that.
