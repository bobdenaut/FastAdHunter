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

---

## Interim analysis — 2026-09-15, day 3 of 7

Read-only pass over the 73 pulls present (`20260911T221321Z-t0` …
`20260914T210002Z`). The soak is still running; every number below is superseded
by the tend analysis.

**Units.** This section uses MiB (÷2^20). The t0 table above uses decimal MB
(÷10^6), so its `peak_rss` 144.61 MB is 137.91 MiB here.

### Dataset integrity

| Check | Result |
| ----- | ------ |
| Pulls | 73, span 70.8 h, cadence 3600 s ±0 outside the two t0-adjacent pulls |
| `meta.errors` | empty in 73/73 — the dashboard catch-all never fired |
| Restarts | none; `uptime_seconds` monotonic to 70.94 h, `version` 0.3.4 in all 73 |
| Perf series, deduped, `ts >= 2026-09-11T22:02:48Z` | 710 rows, 710 expected, **no holes**; one 395 s interval at boot, all others 360 s |
| Container log `WARN`/`ERROR` | 0 lines (the 27 `grep -i error` hits are `parse_errors=0`) |
| RouterOS warning/error log | nothing newer than 2026-09-12T01:07 (a WAN re-dial); identical in all 73 pulls |

The 1.2 h `history/perf` window per hourly pull covers the 6 min series with
overlap to spare. Collection design is sound; no change needed.

### Flags — status at day 3

| Flag | Verdict |
| ---- | ------- |
| Memory drift | **TRIPPED** — see Finding 1. The `residual_bytes` floor rises while every tracked component stays flat |
| Container divergence | clear — `memory-current` minus `process_rss` is 54.9 MiB p50, stdev 2.93 over the last 48 pulls |
| Cache pressure | clear — `evictions` 0, `expired` max 9, 2101 of 50000 entries, 1.98 of 64 MiB |
| Shed | clear — `events_dropped` 0, `udp_inflight.shed` 0, `swr.dropped` 0 |
| Answer quality | clear — `servfail_synthesized` 0, `servfail_relayed` 1, `refused_relayed` 0 |
| Upstream health | clear — 2 failures in 42114 attempts (0.005 %), 0 penalties, 0 penalized seconds |
| Listener bounds | clear — `dns_tcp_connections.peak` 33 of 1024, `closed_oversize` 0 |
| List refresh | clear — refreshes do not sit beside the RSS excursions (Finding 2) |

### Finding 1 — the `residual_bytes` floor rises ~3 MB/day while `accounted_bytes` is flat

`residual = process_rss − accounted`; `crates/fah-api/src/wire.rs:980` names
growth here with the components flat as the leak signal. The mean is useless —
RSS swings ±20 MiB on traffic. The **floor** of each 6 h window is the part that
never comes back.

| 6 h window (UTC) | residual floor | p50 | peak | `accounted` |
| ---------------- | -------------- | --- | ---- | ----------- |
| 09-11 22 → 09-12 03 | 19.00 | 33.36 | 37.86 | boot/compile |
| 09-12 04 → 09 | 23.22 | 28.75 | 34.11 | 26.4–26.9 |
| 09-12 10 → 15 | 25.67 | 31.10 | 32.98 | 27.1–27.6 |
| 09-12 16 → 21 | 25.40 | 30.54 | 32.94 | 27.9–28.1 |
| 09-12 22 → 09-13 03 | 25.18 | 28.45 | 31.18 | 27.9 |
| 09-13 04 → 09 | 25.90 | 27.69 | 31.15 | 27.9–28.8 |
| 09-13 10 → 15 | 26.84 | 32.50 | 42.58 | 28.8 |
| 09-13 16 → 21 | 27.78 | 29.54 | 33.43 | 28.8 |
| 09-13 22 → 09-14 03 | 29.37 | 32.15 | 37.07 | 28.9–29.0 |
| 09-14 04 → 09 | 28.63 | 35.07 | 37.53 | 28.7–29.0 |
| 09-14 10 → 15 | 30.59 | 33.36 | 40.45 | 28.3–28.4 |
| 09-14 16 → 20 | 31.47 | 35.58 | 55.17 | 28.5 |

Floor 23.22 → 31.47 MiB over 60 h. Linear fit, warm-up window excluded:
**+2.96 MB/day, R² 0.926**.

Over the same 710 samples `accounted_bytes` is bounded: min 25.59, p50 27.44,
p95 27.97, **max 28.00 MiB** — asymptotic, and its whole rise is cache warming
(`cache` 0.13 → 1.98 MiB). `ruleset_bytes` 24.14 → 24.12, `stats_clients_bytes`
1.01 flat, `stats_aggregates_bytes` 0.43 → 0.46. Nothing tracked is growing.

**What this does not yet establish.** Over 3 days, elapsed time, cumulative
queries and cumulative HTTP bytes are collinear, so the regression cannot
separate them: R² is 0.926 against hours, 0.891 against queries, 0.820 against
HTTP bytes. Allocator fragmentation under burst-and-release raises the floor too
and eventually plateaus. Three days cannot tell a slow leak from a fragmentation
curve that is still climbing.

**The decisive test is already scheduled: the remaining four days.** Linear to
tend means leak, and the extrapolation is +21 MB over 7 days, ~90 MB over the
30-day retention window — material on a 1 GB device shared with RouterOS.
Flattening means fragmentation, and the plateau is the real steady-state figure.
Recompute this same floor table at tend before concluding either way.

### Finding 2 — RSS excursions are HTTP-proxy transients, not DNS

Every RSS excursion above 5 MiB that is not boot-related lands in the same hour
as the largest HTTP transfers of the run.

| Hour (UTC) | HTTP bytes delta | `http.pass` delta | RSS | RSS delta |
| ---------- | ---------------- | ----------------- | --- | --------- |
| 09-11 23:00 | 0.00 MB | 12 | 60.25 | +9.04 (boot: 10 list bodies, 26 MB) |
| 09-13 11:00 | 69.16 MB | 172 | 67.23 | +11.44 |
| 09-14 15:00 | 6.24 MB | 101 | 65.20 | +5.82 |
| 09-14 19:00 | **184.42 MB** | 328 | 82.81 | **+22.21** |
| 09-14 20:00 | 0.01 MB | 21 | 67.89 | −14.93 |
| 09-14 21:00 | 0.10 MB | 29 | 61.44 | −6.45 |

At 6 min resolution the largest one is a single-interval step: 18:27:26 RSS
60.00 → 18:33:26 RSS 82.69 MiB, `accounted` unmoved at 27.58, `cache` unmoved at
1.96, `concurrent_connections.http` 5 → 6. All of it is `rss_anon`. It then
decays over ~2.5 h back to ~61 MiB.

Two distinct costs are visible:

| Event | Driver | Cost |
| ----- | ------ | ---- |
| 09-13 10:33 | `concurrent_connections.http` 36 — the maximum of the run | +14.0 MiB, so ~0.4 MiB per concurrent connection |
| 09-14 18:33 | 184 MB streamed through the proxy, only 6 connections | +22.7 MiB, so ~12 % of bytes transferred, held transiently |

Neither is a leak — both return. But the transient scales with proxy traffic and
the release takes hours, so it stacks on top of whatever the floor turns out to
be. Total for the run: 276.9 MB of `http.response_bytes` across 2595 passes.

### Finding 3 — the RSS high-water mark is set by list refresh, not by traffic

| `peak_rss` step | When | Cause |
| --------------- | ---- | ----- |
| 90.23 MiB | boot, +3 s | pre-ruleset |
| 137.91 MiB | +7 min | ruleset compile |
| 142.02 / 146.12 MiB | +43 min / +1 h | first refresh, 10 list bodies |
| **147.46 MiB** | 09-13 11:03 | one list body; `list_fetch.bodies` 17 → 18 in that interval |

Steady-state RSS is ~60 MiB — **2.4× below the high-water mark**. Any
`memory-high` for this container must be sized off 147 MiB plus headroom, never
off the steady state. This is the same trap that OOM-killed the resolver at
`memory-high=200M`: 200 M is only 1.36× the observed peak.

### Upstream `adaptive` — works exactly as specified

[ARCHITECTURE.md](../../../ARCHITECTURE.md) §Upstreams defines `adaptive` as an
ordered walk with penalise-and-skip, **not** RTT-weighted balancing. Measured
against that definition:

| Upstream | Order | Attempts | Failures | `consecutive` | Penalties | Probes | State |
| -------- | ----- | -------- | -------- | ------------- | --------- | ------ | ----- |
| 1.1.1.1 | 1 | 42112 | 2 | 0 | 0 | 0 | healthy |
| 9.9.9.9 | 2 | 2 | 0 | 0 | 0 | 0 | healthy |
| 2606:4700:4700::1111 | 3 | 0 | 0 | 0 | 0 | 0 | healthy |
| 2620:fe::fe | 4 | 0 | 0 | 0 | 0 | 0 | healthy |

| Behaviour | Expected | Observed |
| --------- | -------- | -------- |
| All traffic to the first configured endpoint | yes | 42112 of 42114 = 99.995 % |
| Failover to the next in order, one attempt each | yes | exactly 2 attempts on 9.9.9.9, both succeeded, no SERVFAIL reached a client |
| No penalty below `penalty_failures=2` **consecutive** | yes | `failure_runs=[2,0,0,0]` is two isolated failures, not a run of 2, so the threshold never armed: 0 penalties |
| Probes only on the way past a penalised endpoint | yes | nothing penalised, so `probes=0` everywhere |

Accounting reconciles exactly, which is the strongest correctness evidence here:
upstream attempts 42114 = `cache_misses` 4324 + `swr.completed` 37788 + 2
failover retries. And `cache_stale` 39288 = `swr.enqueued` 37788 +
`swr.deduplicated` 1498, within 2 — every stale hit produced exactly one refresh
or one dedup, 0 failed, 0 dropped.

RTT for 1.1.1.1 is stable across the run: p50 5.0 ms in all 73 pulls, per-pull
interval mean 8.06–12.21 ms, cumulative 9.23 ms. The reported p99 moves 250 →
100 ms only because it is a bucket edge. The 800 ms timeout has ~80× headroom
over p50.

**Caveat worth acting on, outside the flag list.** `state: "healthy"` on
upstreams 2–4 is the *initial* value, never tested: 0 attempts and 0 probes. The
`adaptive` design only probes endpoints that have been penalised, so an idle
standby is never checked. Both IPv6 upstreams could be unreachable in silence —
and the WAN prefix did change on 2026-09-12T01:04, `pd=2a02:2f04:5400:cc00::/56`
becoming `2a02:2f04:5305:5f00::/56` — with no signal until 1.1.1.1 and 9.9.9.9
fail at once. This needs its own test, not a change on this evidence.

### Service figures, 70.9 h

| Figure | Value |
| ------ | ----- |
| Queries | 405519 (mean 1.59 qps; hour-of-day median 0.6 → 3.1) |
| Blocked | 331148 = 81.66 % (the hourly rollup agrees: 65.9–86.4 % on recent hours) |
| Cache hit rate, of non-blocked | 94.19 % (70047 hits / 4324 misses) |
| Stale share of hits | 56.09 %, served stale then refreshed by SWR, 0 failures |
| Mean latency | forward 20.43 ms, block 0.034 ms, cache hit 0.048 ms |
| CPU | 194 s user + 244 s system over 255396 s = **0.171 % of one core** |
| Lists | 34 bodies, 74.69 MB fetched, `not_modified` **2** |
| Router free-memory | 787.0 → 741.5 MiB (−15 MiB/day, tracking `memory-current` +22 MiB) |
| Router NAND | `write-sect-since-reboot` +3064, `free-hdd-space` flat, so `/data` is not on internal storage |

Observations needing their own test, never findings from this run: the 56 %
stale-serve share, with `min_ttl_seconds` 600 so hot names expire faster than
they are re-queried; `not_modified` 2 of 36 fetch attempts, meaning conditional
GETs are almost never honoured, at a cost of 74.69 MB of egress in 3 days;
`http.refused` 283 and climbing ~4/h, expected from `allow_ip_literal_hosts=false`
but unverified.

### Decision — keep `sample_interval_seconds` at 360

**Keep it.** For this run the question is moot:
`crates/fah-api/src/config_store.rs:56` classifies it as a boot key, so changing
it restarts the process and destroys the series. For future soaks, keep it
anyway.

| Argument | Evidence from this run |
| -------- | ---------------------- |
| The metric that most needs resolution does not need it | `peak_rss` is a monotone high-water mark carried in **every** sample. The 147.46 MiB peak was captured at 360 s and would be captured at any interval |
| The actual finding is a multi-day trend | The residual floor moves ~3 MB/day. 240 samples/day is already far more than that trend needs; 1440 adds nothing |
| Attribution came from counters, not resolution | The 18:33 excursion was attributed to the HTTP proxy from `http.response_bytes` and `concurrent_connections.http`, both present at 360 s |
| Read cost at tend is real | `history/perf` is JSONL on disk at ~2.4 KB/row. A 7-day backfill in one request is 1680 rows ≈ 4 MB at 360 s, but **10080 rows ≈ 24 MB at 60 s**, serialised into one response on a 1 GB ARM64 router |
| Storage at 30-day retention | ~17 MB at 360 s against ~104 MB at 60 s |

What 360 s genuinely cost: the +22.7 MiB step arrived fully formed inside one
interval, so its ramp shape is unknown. That did not block attribution here and
is not worth 6× the rows. If a future transient needs the ramp, run a short,
separate 60 s capture aimed at it — do not raise the resolution of a week-long
soak to catch a six-minute event.

This answers the third open TODO above; the checkbox is left for the owner.

### Verdict at day 3

No crash, no restart, no shed, no eviction, no SERVFAIL synthesis, no upstream
penalty, no listener saturation and no log error in 70.9 h. One open question —
whether the `residual_bytes` floor plateaus — and the remaining four days are the
test for it. Nothing here justifies stopping the soak or touching the router.
