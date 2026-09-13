# Soak 0.3.4 — seven days on RB5009

## Summary

0.3.4 runs on the RB5009 from 2026-09-11T22:02:48Z to ~2026-09-18T22:00Z, with
one hourly snapshot pull per hour into `pulls/`. No load is offered: the
workload is the household's own DNS and HTTP traffic. The question is whether
memory, upstream health and cache behaviour stay bounded over seven days;
`residual_bytes` and the container's `memory-current` are the figures that
decide it. Every pull writes raw JSON and raw RouterOS text only — derived
numbers are recomputed at tend, so `pulls/` alone reproduces the analysis.

## Decisions

- **Pull cadence is hourly, not 8-hourly.** RouterOS `memory-current` and
  `/system/resource` have no device-side history; they exist only in pulls.
- **`history/perf` ships in every pull** despite the device retaining 30 days,
  so a `/data` loss does not take the series with it. Disk is not a constraint.
- **The flag list below is fixed before the data exists.** Anything outside it
  at tend is an observation needing its own test, never a finding.
- **The API key lives outside the repo** at `%USERPROFILE%\.fah-soak\token.txt`;
  `run-pull.cmd` reads it at run time so no secret is ever committed.
- **RouterOS access is read-only `print` only**, over passwordless SSH to host
  `bobdenaut` (CLAUDE.md §The router is off limits).

## Traps

| Trap | Consequence |
| ---- | ----------- |
| `history.sample_interval_seconds` is **360**, not the 60 s default in [CONFIGURATION.md](../../../CONFIGURATION.md) | Perf resolution is 6 min; ~1680 rows over the week, not 10k. Events shorter than 6 min are invisible |
| `history/perf` holds rows from **before** the 0.3.4 boot — history survives restarts | Analysis filters `ts >= 2026-09-11T22:02:48Z` or it mixes 0.3.3 samples into the 0.3.4 series |
| There is **no `/metrics` endpoint** (`crates/fah-api/tests/api.rs:785` asserts 404) | Any unknown path returns the dashboard `index.html` with a **200**. HTTP status alone never proves an endpoint exists |
| The scheduled task is **Interactive only** | No pulls while logged off. Screen lock is fine |

## t0 baseline

`pulls/20260911T221321Z-t0`, uptime 595 s.

| Figure | Value |
| ------ | ----- |
| version / boot | 0.3.4 / 2026-09-11T22:02:48Z |
| ruleset | 755636 rules, 1 policy, 16 lists, compile 2.78 s from cache |
| process_rss / peak_rss | 54.67 MB / 144.61 MB |
| allocator_committed / peak | 237.76 MB / 237.76 MB |
| ruleset_bytes / residual_bytes | 25.32 MB / 27.68 MB |
| container `memory-current` | 94.4 MiB (`memory-high=unlimited`) |
| router free-memory | 782.8 MiB of 1024.0 MiB |
| upstreams | 4 × udp, all `healthy`, 0 failures; only 1.1.1.1 carries traffic |
| strategy / timeout / penalty_failures | `adaptive` / 800 ms / 2 |
| cache | 50000 entries, 64 MiB, `serve_stale`, 3 SWR workers |
| `dns_tcp_connections.peak` / `tcp_max_connections` | 7 / 1024 |
| `dns_udp_inflight` / `udp_max_inflight` | all 0 / 0 (guard off) |
| `events_dropped` | 0 |
| mode / http_runtimes | `dns+http` / 2 |

## Flags — fixed 2026-09-12, before the week's data exists

| Flag | Trips when |
| ---- | ---------- |
| Memory drift | `residual_bytes` or `process_rss` slopes up across the week independent of cache size |
| Container divergence | container `memory-current` pulls away from `process_rss` |
| Cache pressure | `cache.evictions > 0` or `expired` grows without cleanup runs |
| Shed | `events_dropped > 0` |
| Answer quality | `answers.servfail_synthesized` rate rises |
| Upstream health | any upstream accumulates `failures`, `penalties` or `penalized_seconds_total` |
| Listener bounds | `dns_tcp_connections.peak` approaches 1024, or `closed_oversize > 0` |
| List refresh | `counters.lists.bodies` delta beside an RSS excursion |

## Collection

| What | Where |
| ---- | ----- |
| Collector | `collect-soak.py` — one directory per pull, raw JSON/text |
| Runner | `run-pull.cmd` — appends one line per pull to `collector.log` |
| Schedule | Task `FAH-soak-0.3.4`, hourly, user `liviu`, Interactive only |
| Every pull | telemetry, debug/memory, health, cache, history/summary, lists, history/perf (1.2 h window), RouterOS resource + container detail + warning/error log |
| Daily (00 UTC) | clients, stats, history/top, config, policies, full container log |

A non-JSON body (the dashboard catch-all) is recorded in `meta.errors` and
exits non-zero, so the task's Last Result surfaces it.

## Host power settings

Set on the collector laptop so the week is not cut short. Values applied to the
active scheme, AC and DC: sleep `never`, hibernate `never`, unattended sleep
timeout `0`, lid close `do nothing`, PCIe link state power management `Off`,
disk timeout AC `never`. Ethernet driver power saving is already disabled.

Undo at tend: `powercfg /change standby-timeout-dc 600`, lid close back to `1`.

## Remaining TODOs

- [ ] Write the reducer around day 2; validate every column by hand against
      `20260911T221321Z-t0` before pointing it at the full set.
- [ ] At tend: `schtasks /delete /tn "FAH-soak-0.3.4" /f`, backfill the full
      perf series in one request, then analyse.
- [ ] Decide whether `sample_interval_seconds` 360 is kept for future soaks.
