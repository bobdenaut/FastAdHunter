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

- [x] Reducer written — `reduce.py`, one file, no arguments runs every section
      (`pulls perf memory floor diurnal peak container upstream http service lists`).
      Every figure in this document is recomputed from `pulls/` alone.
- [ ] At tend: `schtasks /delete /tn "FAH-soak-0.3.4" /f`, backfill the full
      perf series in one request, then analyse.
- [ ] Decide whether `sample_interval_seconds` 360 is kept for future soaks.
      The §Decision below recommends keeping it; day 5 found nothing that 60 s
      would have caught. Still the owner's checkbox.
- [x] `collect-soak.py` fixed 2026-09-16 — `own_container_memory()` picks the
      `fastadhunter-*` entry out of `/container/print detail` instead of keeping
      the last `memory-current=` in the file, and a no-match now lands in
      `meta.errors` instead of writing a plausible wrong number. Replayed over
      all 116 stored pulls: 116/116 match, one value changes (Problem 1).
- [ ] Re-derive the container-offset series with `20260916T100001Z` repaired —
      it moves n from 115 to 116 and shifts the offset mean and trend slightly.

---

## Interim analysis — 2026-09-15, day 3 of 7

Read-only pass over the 73 pulls present at the time of writing. The soak is
still running; four days remain.

**Data cutoff — everything below stops here.**

| Boundary | Value |
| -------- | ----- |
| First pull | `20260911T221321Z-t0` (uptime 595 s) |
| **Last pull analysed** | **`20260914T210002Z`** = 2026-09-14T21:00:02Z, uptime 70.94 h |
| Last perf row analysed | 2026-09-14T20:57:26Z (row 710 of the deduped series) |
| Covered | h0 → **h71**. The G2 window is h48 → h168, so h71 → h168 is still missing |
| Soak ends | ~2026-09-18T22:00Z (h168) |

Any pull from `20260914T220002Z` onward is **not** in these numbers. The next
analysis re-reads from `20260911T221321Z-t0` rather than continuing from here:
the cumulative counters are process-lifetime totals and the floor fit needs the
whole h48–h168 window in one pass.

**Units.** MiB (÷2^20) throughout this section. The t0 table above uses decimal
MB (÷10^6), so its `peak_rss` 144.61 MB is 137.91 MiB here.

**Method.** Floors are **hourly minima, least squares**, per the G2 method in
[resoak-0.3.1-memory-diagnosis.md](../resoak-0.3.1-memory-diagnosis.md)
§Hand-off. 6 h minima appear only where F33 is quoted for comparison, because
F33 used them.

### The headline: 0.3.4 predates the fix for the HTTP idle-pool retention observed in this run

`8941770` — *fix(http): reap idle upstream connections by giving the pool a
timer*, committed 2026-09-12 22:46 +0300 — is **not an ancestor of 0.3.4**.
`4e7a6de` bumped the version at 2026-09-12 01:05 +0300, three minutes after this
soak booted; the fix landed 22 h later. `git merge-base --is-ancestor 8941770
4e7a6de` returns false.

So the running binary still has the defect named in
[upstream-pool-idle-retention.md](../upstream-pool-idle-retention.md):
`pool_idle_timeout` is inert without a client-side pool timer, so idle upstream
connections are evicted only lazily at checkout and survive while no traffic
flows. Eight per host × `http_runtimes` 2, each pinning a grown ~408 KiB H1
buffer. The dev-box measurement for the same defect: memory held 300 s after a
400 MiB burst is **+18.23 MB without the fix, +4.26 MB with it**.

The corresponding dev-box retention is **live application state**, not allocator
residue — a forced `mi_theap_collect` on the owning threads returns zero blocks
in the 448/512 KiB classes. Live state outside the accounted set would land
wholly in `residual_bytes`, which is consistent with the shape of everything
below. **How much of this run's floor it accounts for is not measured**; the
RB5009 has never carried the A/B.

**This section therefore reports against a known defect, not a new one.** It is
worth finishing the week anyway: this run is the "before" half of that A/B.

### Finding 1 — the excursions are attributed. F35's open gap is closed

[resoak-0.3.1-memory-diagnosis.md](../resoak-0.3.1-memory-diagnosis.md) F35 left
two evening spikes unattributed: every exported counter was flat at their onset,
and "the only path with no per-sample counter is the HTTP proxy — the gap the
`concurrent_connections` high-water mark (`4eddc39`) closes in the next soak."

0.3.4 is that next soak, and it carries the counter. Every RSS excursion above
5 MiB that is not boot-related lands in the same hour as the largest HTTP
transfers of the run:

| Hour (UTC) | HTTP bytes delta | `http.pass` delta | RSS | RSS delta |
| ---------- | ---------------- | ----------------- | --- | --------- |
| 09-11 23:00 | 0.00 MB | 12 | 60.25 | +9.04 (boot: 10 list bodies, 26 MB) |
| 09-13 11:00 | 69.16 MB | 172 | 67.23 | +11.44 |
| 09-14 15:00 | 6.24 MB | 101 | 65.20 | +5.82 |
| 09-14 19:00 | **184.42 MB** | 328 | 82.81 | **+22.21** |
| 09-14 20:00 | 0.01 MB | 21 | 67.89 | −14.93 |
| 09-14 21:00 | 0.10 MB | 29 | 61.44 | −6.45 |

The largest is a single-interval step at 6 min resolution: 18:27:26 RSS 60.00 →
18:33:26 RSS 82.69 MiB, with `accounted` unmoved at 27.58 and `cache` unmoved at
1.96. All of it is `rss_anon`. It decays over ~2.5 h back to ~61 MiB — the shape
lazy-at-checkout eviction produces as later requests trickle in.

**F35 is answered: the unattributed evening spikes are the HTTP proxy.**

One figure does not fit the dev-box curve and is left as an observation.
[upstream-pool-idle-retention.md](../upstream-pool-idle-retention.md) measured
retention scaling with in-flight concurrency and *not* with bytes: +1.81 /
+10.07 / +25.37 MB at concurrency 1 / 8 / 32 for byte-identical 400 MiB waves.
Here 184 MB at a 6 min `concurrent_connections.http` high-water of **6** left
+22.7 MiB, well above that curve, while 09-13 10:33 at a high-water of **36**
left +14.0 MiB. Either the 6 min high-water understates the instantaneous peak,
or ARM64 at `http_runtimes = 2` behaves differently from the dev box. Not
resolvable from this data; needs its own test.

### Finding 2 — the floor: a slope exists, but it cannot be read yet

`residual = process_rss − accounted` ([memory.rs:188](../../../crates/fah-model/src/memory.rs)).
`accounted` has exactly four members ([memory.rs:140](../../../crates/fah-model/src/memory.rs)):
`ruleset + cache + stats.aggregates + stats.clients`. Everything else in RSS —
including the idle upstream pool above — is residual by construction.

Over the 710 perf samples `accounted_bytes` is bounded: min 25.59, p50 27.44,
p95 27.97, **max 28.00 MiB**, and its whole rise is cache warming (`cache` 0.13
→ 1.98 MiB). `ruleset_bytes` 24.14 → 24.12, `stats_clients_bytes` 1.01 flat,
`stats_aggregates_bytes` 0.43 → 0.46. Nothing tracked is growing.

Least squares on hourly minima:

| Window | Series | Slope | R² | n |
| ------ | ------ | ----- | -- | - |
| h4–h71 | RSS floor | +3.14 MB/day | 0.494 | 67 |
| h4–h71 | residual floor | +2.67 MB/day | 0.410 | 67 |
| h48–h71 | RSS floor | +3.68 MB/day | **0.175** | 23 |
| h48–h71 | residual floor | +4.33 MB/day | **0.230** | 23 |

**The flag cannot be called at day 3.** Its own wording is "slopes up *across
the week*", and the G2 window is h48→h168 — 23 of 120 hours are in hand, at an
R² that supports nothing. An earlier draft of this section reported R² 0.926;
that came from fitting 6 h window minima, which compresses twelve points into
one and inflates the fit. On the project's own method the same data gives 0.49
and 0.18. The slope is real enough to watch and far too noisy to conclude from.

The comparison that does carry information is against 0.3.1 on the same device,
same method — 6 h RSS minima, F33:

```
0.3.1  43.4 52.1 52.3 54.5 58.2 58.4 63.3 61.4 | 64.1 64.4 63.8 63.5 63.2 63.0 61.2 60.8 62.5
0.3.4  44.6 49.9 52.5 52.5 52.3 53.2 54.7 55.6 | 57.3 56.4 58.1 59.0
                                          h48 ─┘
```

0.3.4 sits **6–8 MiB below** 0.3.1 at the same hour and climbs more slowly. But
0.3.1 had flattened by h48 (F33: "from h48 to h102 the floor is flat to slightly
down"), and 0.3.4 is still rising through h72. Slower warm-up, or a floor that
does not stop — the four remaining days decide, and that is the only question
this soak still has open.

### Finding 3 — the RSS high-water mark is set by list refresh, and it explains the `memory-high=200M` kill

| `peak_rss` step | When | Cause |
| --------------- | ---- | ----- |
| 90.23 MiB | boot, +3 s | pre-ruleset |
| 137.91 MiB | +7 min | ruleset compile |
| 142.02 / 146.12 MiB | +43 min / +1 h | first refresh, 10 list bodies |
| **147.46 MiB** | 09-13 11:03 | one list body; `list_fetch.bodies` 17 → 18 in that interval |

Steady-state RSS is ~60 MiB — 2.4× below the high-water mark. The container
tracks `process_rss` with a stable offset of **54.9 MiB p50** (stdev 2.93 over
the last 48 pulls; F34 identified it as page cache plus kernel, not process
memory). At the peak that is 147.5 + 54.9 ≈ **202 MiB**.

`memory-high=200M` was the setting that OOM-killed the live resolver. The
arithmetic now reconciles: it was under the peak by ~2 MiB, on a figure the
steady state never hints at. Any future limit is sized off 147 MiB of process
plus the container offset plus headroom.

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

Accounting reconciles to the query, which is the strongest correctness evidence
here: upstream attempts 42114 = `cache_misses` 4324 + `swr.completed` 37788 + 2
failover retries. And `cache_stale` 39288 = `swr.enqueued` 37788 +
`swr.deduplicated` 1498, within 2 — every stale hit produced exactly one refresh
or one dedup, 0 failed, 0 dropped.

RTT for 1.1.1.1 is stable: p50 5.0 ms in all 73 pulls, per-pull interval mean
8.06–12.21 ms, cumulative 9.23 ms. The reported p99 moves 250 → 100 ms only
because it is a bucket edge. The 800 ms timeout has ~80× headroom over p50.

**Caveat, outside the flag list.** `state: "healthy"` on upstreams 2–4 is the
*initial* value, never tested: 0 attempts and 0 probes. `adaptive` probes only
endpoints that have been penalised, so an idle standby is never checked. Both
IPv6 upstreams could be unreachable in silence — and the WAN prefix did change
on 2026-09-12T01:04, `pd=2a02:2f04:5400:cc00::/56` becoming
`2a02:2f04:5305:5f00::/56` — with no signal until 1.1.1.1 and 9.9.9.9 fail at
once. Needs its own test, not a change on this evidence.

### Flags — status at day 3

| Flag | Verdict |
| ---- | ------- |
| Memory drift | **cannot be read yet** — 23 of the 120 h in the G2 window, R² 0.175. Finding 2 |
| Container divergence | clear — offset 54.9 MiB p50, stdev 2.93 over the last 48 pulls |
| Cache pressure | clear — `evictions` 0, `expired` max 9, 2101 of 50000 entries, 1.98 of 64 MiB |
| Shed | clear — `events_dropped` 0, `udp_inflight.shed` 0, `swr.dropped` 0 |
| Answer quality | clear — `servfail_synthesized` 0, `servfail_relayed` 1, `refused_relayed` 0 |
| Upstream health | clear — 2 failures in 42114 attempts (0.005 %), 0 penalties, 0 penalized seconds |
| Listener bounds | clear — `dns_tcp_connections.peak` 33 of 1024, `closed_oversize` 0 |
| List refresh | clear — refreshes do not sit beside the RSS excursions (Finding 1) |

### Dataset integrity

| Check | Result |
| ----- | ------ |
| Pulls | 73, span 70.8 h, cadence 3600 s ±0 outside the two t0-adjacent pulls |
| `meta.errors` | empty in 73/73 — the dashboard catch-all never fired |
| Restarts | none; `uptime_seconds` monotonic to 70.94 h, `version` 0.3.4 in all 73 |
| Perf series, deduped, `ts >= 2026-09-11T22:02:48Z` | 710 rows, 710 expected, **no holes**; one 395 s interval at boot, all others 360 s |
| Container log `WARN`/`ERROR` | 0 lines |
| RouterOS warning/error log | nothing newer than 2026-09-12T01:07 (a WAN re-dial); identical in all 73 pulls |

The 1.2 h `history/perf` window per hourly pull covers the 6 min series with
overlap to spare. Collection design is sound; no change needed.

### Service figures, 70.9 h

| Figure | Value |
| ------ | ----- |
| Queries | 405519 (mean 1.59 qps; hour-of-day median 0.6 → 3.1) |
| Blocked | 331148 = 81.66 % (the hourly rollup agrees: 65.9–86.4 % on recent hours) |
| Cache hit rate, of non-blocked | 94.19 % (70047 hits / 4324 misses) |
| Stale share of hits | 56.09 %, served stale then refreshed by SWR, 0 failures |
| Mean latency | forward 20.43 ms, block 0.034 ms, cache hit 0.048 ms |
| CPU | 194 s user + 244 s system over 255396 s = **0.171 % of one core** |
| HTTP | 276.9 MB `response_bytes`, 2595 pass, 0 block, 283 refused |
| Lists | 34 bodies, 74.69 MB fetched, `not_modified` **2** |
| Router free-memory | 787.0 → 741.5 MiB (−15 MiB/day, tracking `memory-current` +22 MiB) |
| Router NAND | `write-sect-since-reboot` +3064, `free-hdd-space` flat, so `/data` is not on internal storage |

Observations needing their own test, never findings from this run: the 56 %
stale-serve share, with `min_ttl_seconds` 600 so hot names expire faster than
they are re-queried; `not_modified` 2 of 36 fetch attempts, expected at
`refresh_hours` 48 since most ad lists do change in two days, but it means every
refresh buffers a full body; `http.refused` 283 and climbing ~4/h, expected from
`allow_ip_literal_hosts=false` but unverified.

### What is outside `accounted_bytes`

The residual is not a mystery bucket — `accounted` names four things and
everything else falls outside it. Ranked by how well each explains a floor that
survives burst → release:

| Candidate | Code | Status |
| --------- | ---- | ------ |
| HTTP idle upstream pool | [proxy.rs:270](../../../crates/fah-http/src/proxy.rs), bound at [main.rs:955](../../../crates/fastadhunter/src/main.rs), passed at [main.rs:1141](../../../crates/fastadhunter/src/main.rs) | **Defect present in this build.** 8 per *host* × 2 runtimes, so it scales with distinct origins, not with config as its comment claims. Fixed by `8941770`, not in 0.3.4 |
| H1 buffers pinned by those connections | `H1_MAX_BUF` 128 KiB at [intercept.rs:41](../../../crates/fah-http/src/intercept.rs) is set on the intercept path only; pass-through in `proxy.rs` sets no `max_buf_size`, so hyper's default grows to ~408 KiB | Same root cause, sets the size of what is retained |
| mimalloc page retention | `crates/fastadhunter/src/allocator.rs` | Secondary. F29 names the mechanism; at the RB5009's 4 workers it is ~2 MiB per burst |
| SWR queue | [swr.rs:93](../../../crates/fah-dns/src/swr.rs), `workers × QUEUE_DEPTH_PER_WORKER` | Bounded by config. `swr.dropped` 0 all run |
| Event channel | [main.rs:38](../../../crates/fastadhunter/src/main.rs), 4096 | Bounded. `events_dropped` 0 all run |
| History writers | `crates/fah-stats/src/history/perf.rs` | JSONL on disk; the writer holds a path, a retention bound and a cursor |
| Binary pages | — | `rss_file` 8.91 → 8.97 MiB over the whole run |

Not audited: per-connection DNS TCP state, the DNS upstream pool's health and
RTT state, the config store, dashboard assets. None shows growth in any exported
counter, but none was read.

### Decision — keep `sample_interval_seconds` at 360

**Keep it.** For this run the question is moot:
[config_store.rs:56](../../../crates/fah-api/src/config_store.rs) classifies it
as a boot key, so changing it restarts the process and destroys the series. For
future soaks, keep it anyway.

| Argument | Evidence from this run |
| -------- | ---------------------- |
| The metric that most needs resolution does not need it | `peak_rss` is a monotone high-water mark carried in **every** sample. The 147.46 MiB peak was captured at 360 s and would be captured at any interval |
| The finding that matters is a multi-day trend | The floor moves ~3 MB/day and needs 120 h to be readable at all. 240 samples/day is already far more than that; 1440 adds nothing |
| Attribution came from counters, not resolution | The 18:33 excursion was attributed to the HTTP proxy from `http.response_bytes` and `concurrent_connections.http`, both present at 360 s |
| Read cost at tend is real | `history/perf` is JSONL at ~2.4 KB/row. A 7-day backfill in one request is 1680 rows ≈ 4 MB at 360 s, but **10080 rows ≈ 24 MB at 60 s**, serialised into one response on a 1 GB ARM64 router |
| Storage at 30-day retention | ~17 MB at 360 s against ~104 MB at 60 s. F34 measured the same write rate independently: ~0.6 MB/day |

What 360 s genuinely cost: the +22.7 MiB step arrived fully formed inside one
interval, so its ramp shape is unknown. That did not block attribution and is
not worth 6× the rows. If a future transient needs the ramp, run a short,
separate 60 s capture aimed at it — do not raise the resolution of a week-long
soak to catch a six-minute event.

This answers the third open TODO above; the checkbox is left for the owner.

### For the tend read

- **Take the final floor reading before the full backfill, not after.** F35's
  `hist4` arm measured +2.0 MiB of RSS for six 4300-row `/history/perf` reads on
  0.3.1. The tend backfill is ~1680 rows and will perturb the last point it is
  meant to measure.
- Compute G2 as hourly minima, least squares, **h48 → h168**. Not 6 h windows —
  they inflate R².
- Compare the result against 0.3.1's F33 sequence, not against t0.
- `8941770` is the "after" arm. This run is the "before". The A/B is worth
  having on the RB5009, where the fix has never been measured.

### Verdict at day 3

No crash, no restart, no shed, no eviction, no SERVFAIL synthesis, no upstream
penalty, no listener saturation and no log error in 70.9 h. `adaptive` matches
its specification exactly. The excursions are attributed to the HTTP proxy,
closing F35; the mechanism that best explains them is a defect already diagnosed
and already fixed, just not in this build, though the size of its contribution
here is unmeasured. Whether the floor plateaus is the one open question, and
it cannot be answered before h168. Nothing here justifies stopping the soak or
touching the router.

---

## Interim analysis — 2026-09-16, day 5 of 7

Second read-only pass, re-read from `20260911T221321Z-t0` as the day-3 section
said it would. Every figure below comes from `reduce.py`; nothing is carried
forward from the day-3 numbers. The soak is still running; two days remain.

**Data cutoff — everything below stops here.**

| Boundary | Value |
| -------- | ----- |
| Pulls | 116, `20260911T221321Z-t0` → `20260916T160001Z` |
| **Last pull analysed** | **`20260916T160001Z`** = 2026-09-16T16:00:01Z, uptime 113.94 h |
| Last perf row analysed | 2026-09-16T15:57:26Z (row 1140 of the deduped series) |
| Covered | h0 → **h113**. The G2 window is h48 → h168, so 66 of 120 h are in hand |
| Soak ends | ~2026-09-18T22:00Z (h168) |

**Units.** MiB (÷2^20) throughout, as in the day-3 section.

### What day 5 supersedes

| Day-3 statement | Day-5 value |
| --------------- | ----------- |
| `peak_rss` high-water **147.46 MiB** | **155.52 MiB** — two further steps, both on a list refresh |
| Container offset "stable, 54.9 MiB p50, stdev 2.93 over the last 48 pulls" | Not stable across the week: **+3.21 MiB/day, R² 0.630**, mean 52.26 over the first 24 pulls against 65.72 over the last 24 |
| "0.3.4 sits 6–8 MiB below 0.3.1 at the same hour" | True to h72 only. The two floors **cross at h72**; 0.3.4 is above 0.3.1 in four of the five 6 h windows since, by up to **+6.0 MiB** (one window, h78–84, is 1.7 MiB below) |
| Upstream health "clear — 0 penalties" | **1 penalty, 24 penalized seconds, 1 probe** at h93.3 |
| `memory-high` sized off 147 MiB + offset ≈ 202 MiB | Neither 202 nor 225 MiB is the right basis — size from p2-11's measured saturation, ≈ **250 MiB** with the offset. Finding 6 |
| Memory drift "cannot be read yet" at 23 of 120 h | **It can now.** The residual floor doubled, 19.0 → 38.6 MiB, and remained elevated across every 12 h bucket. Finding 4 |

The day-3 section is left as written. Its method was right; four of its numbers
were simply read too early.

### Finding 4 — the residual floor doubled while everything accounted for stayed flat

`residual = process_rss − accounted`
([memory.rs:188](../../../crates/fah-model/src/memory.rs)). Across the run so
far — h0 to h113.9, every hour of it — **the residual floor doubled and remained
elevated across every 12 h bucket**, and nothing the process can name accounts
for it. That is the claim, not "the slope is positive": a positive slope with a
mediocre R² reads like a weak signal, and this is not one.

| Floor, MiB | h0–12 | h96–108 | Change |
| ---------- | ----- | ------- | ------ |
| RSS | 44.6 | 66.2 | **+21.6** |
| Residual | 19.0 | 38.6 | **+19.6 — it doubled** |
| `accounted_bytes` | 25.59 (min of 1140 samples) | 28.00 (max) | +2.4, bounded |

Same hours compared each day, so the household's evening traffic cannot flatter
the trend:

| Day | Floor 01–06 UTC | Δ | Floor 12–17 UTC | Δ |
| --- | --------------- | - | --------------- | - |
| 09-12 | 51.6 | — | 52.5 | — |
| 09-13 | 52.9 | +1.3 | 54.7 | +2.2 |
| 09-14 | 58.3 | +5.3 | 59.0 | +4.3 |
| 09-15 | 66.5 | **+8.2** | 65.2 | **+6.3** |
| 09-16 | 66.2 | −0.3 | 64.9 | −0.4 |

Both windows agree, so this is not a traffic artefact, and the rise **accelerated**
through 09-15 rather than settling. The last day is flat in both windows; one day
against four decides nothing, and h168 says whether it was anything.

12 h floors, RSS / residual. Two buckets dip on the one before them — h24 and the
half-length h108 — and **neither comes back down toward the h0 level**; that is
what "remained elevated" means here, not strict monotonicity:

```text
h0    44.6 / 19.0     h48   56.4 / 28.6     h96   66.2 / 38.6
h12   52.5 / 25.4     h60   58.1 / 30.6     h108  64.9 / 37.1  (half window)
h24   52.3 / 25.2     h72   61.3 / 33.9
h36   54.7 / 26.8     h84   63.7 / 36.3
```

Method figure, for continuity with day 3 and F33 — hourly minima, least squares,
per the G2 method in
[resoak-0.3.1-memory-diagnosis.md](../resoak-0.3.1-memory-diagnosis.md)
§Hand-off: h4–end is +4.25 MiB/day RSS and +4.09 residual at R² 0.703/0.678;
the G2 window h48–end is +4.48 and +4.54 at R² 0.430/0.431. **A linear fit
averages an acceleration and reads gentler than the floors above.** The floors
are the claim; the slope is a summary of them, not a weaker version of them.

The 6 h RSS minima against 0.3.1 on the same device, same method (F33):

```text
0.3.1  43.4 52.1 52.3 54.5 58.2 58.4 63.3 61.4 | 64.1 64.4 63.8 63.5 63.2 63.0 61.2 60.8 62.5
0.3.4  44.6 49.9 52.5 52.5 52.3 53.2 54.7 55.6 | 57.3 56.4 58.1 59.0 64.3 61.3 65.2 63.7 68.5 66.2 64.9
                                          h48 ─┘                    h72 ─┘
```

0.3.1 flattened at h48 and stayed in a 60.8–64.4 band for the rest of its run.
0.3.4 kept climbing through it, crossed at h72, and peaked at 68.5 in h96–102.
**The day-3 read of a slower, lower floor does not survive two more days.**
What survives is the day-3 caveat: `8941770` is not in this build, so this run
is the "before" arm and the gap is the size of what the fix is worth here.

`accounted_bytes` is still bounded — min 25.59, p50 27.47, p95 27.96, max 28.00
MiB over 1140 samples. `ruleset_bytes` 24.14 → 24.36, `stats_clients_bytes`
1.01 flat, `stats_aggregates_bytes` 0.39–0.60, `cache_estimated_bytes` 0.00 →
1.88. `rss_file` 5.07 → 8.97. Everything else is residual by construction.

`stats_clients_bytes` is byte-identical (1058917) in all 116 pulls while the
client count goes 651 → 691. That is not a frozen estimator:
[heap.rs:58](../../../crates/fah-stats/src/heap.rs) rounds the bucket count to
the next power of two, and both 651 and 691 land in the same 1024 slots.

**The mechanism is already named and it is not new.** `8941770` — the idle
upstream-pool reaper — is not an ancestor of 0.3.4 (day-3 §The headline). Idle
connections are evicted only lazily at checkout, and live state outside the
accounted set lands wholly in `residual_bytes`. The HTTP bursts release in full
(§Excursions); the floor underneath them does not. **How much of the +19.6 MiB
that mechanism accounts for is still unmeasured on this device** — that is what
the A/B against `8941770` is for, and this run is its "before" arm.

### Finding 5 — RETRACTED: the committed counter cannot decrease, so its shape says nothing

**Withdrawn 2026-09-16, same day it was written.** It claimed the allocator was
holding memory it never returned. The evidence for that claim was
`allocator_committed_bytes` being monotone across 1140 samples — which is a
property of the counter, not of the memory.

[memory.rs:98-112](../../../crates/fah-model/src/memory.rs) says so outright,
measured on this device at 0.2.7: mimalloc v3 **does not decrement
`current_commit` when a purge returns pages to the OS**, so it reads as a
lifetime high-water mark, and "318 MB against 70 MB RSS was the measured state".
The field's own documentation ends "Do not subtract anything from this field and
present the result as retention." This section did exactly that.

`current_commit == peak_commit` in **116 of 116 pulls** is the documented
signature of a counter that never decrements, so the observation is consistent
with any amount of memory having been released, including all of it. Should the
two ever diverge, the counter tracks releases and the question reopens.

The steps below are kept because they are real and they date the refresh
transients. They are **not** evidence of retention. What they shadow is
`peak_rss`, and that mechanism was measured two phases ago:
[p2-11-compile-transient.md](../phase2/p2-11-compile-transient.md) — "not one
compile's cost, it is a ratchet across successive compiles, driven by mimalloc's
deferred purge, **saturating at ~230 MiB**", cut to 181.4 MiB by
`MIMALLOC_PURGE_DELAY=0`. A ratchet with a measured ceiling is bounded.

| Committed step | When | `list_fetch.bodies` in that interval |
| -------------- | ---- | ------------------------------------ |
| 141.1 → 226.8 | boot +7 min | 0 → 4 (ruleset compile) |
| 226.8 → 287.7 | boot +43 min | 4 → 13 |
| 287.7 → 291.7 | boot +1 h | 13 → 14 |
| 291.7 → **311.2** | 09-15 11:45, h85.7 | 37 → 38 |
| 311.2 → **323.3** | 09-15 22:09, h96.1 | 38 → 42 |
| 323.3 → **324.2** | 09-15 22:51, h96.8 | 50 → 51 |

Every step lands on a list-refresh sample; none lands on an HTTP burst, which is
the one durable thing this table shows — the refresh transient, not the proxy,
is what moves the high-water marks. 0.3.4 reads 155.52 MiB of `peak_rss` on
763017 rules, **below** p2-11's post-fix 181.4 MiB on 798250, so the ratchet is
sitting lower in this build than the last time it was measured.

Committed is address space mimalloc accounts for, not resident memory: it never
enters the cgroup's `memory-current` and it is not the floor. Finding 4 is
measured on RSS from `/proc/self/status` and is untouched by this retraction.

### Finding 6 — the high-water mark moved, and the `memory-high` arithmetic moves with it

| `peak_rss` step | When | Cause |
| --------------- | ---- | ----- |
| 90.23 MiB | boot, +3 s | pre-ruleset |
| 137.91 MiB | +7 min | ruleset compile |
| 142.02 / 146.12 MiB | +43 min / +1 h | first refresh |
| 147.46 MiB | 09-13 11:03, h37.0 | one body, `bodies` 17 → 18 |
| 154.80 MiB | 09-15 11:45, h85.7 | `bodies` 37 → 38 |
| **155.52 MiB** | 09-15 22:45, h96.7 | `bodies` 42 → 50 |

Day 3's Finding 3 holds on mechanism and fails on magnitude: list refresh still
sets the high-water, and the high-water is 8 MiB higher than the number that
section published. Steady-state RSS is ~65 MiB, now **2.4× below** the mark.

Container offset (own container only, the one mis-parsed pull excluded): n=115,
mean 58.24, sd 5.62, p50 57.10, range 42.26–69.45, **trend +3.21 MiB/day at R²
0.630**. Within a day it is tight — sd 1.84 over the last 24 pulls. Across the
week it is not. Highest container `memory-current` actually observed: **158.4
MiB**, at 09-15 19:00 with `process_rss` 91.1.

**Sizing — and 225 MiB is the wrong basis.** 155.5 MiB of process + 69.5 MiB of
offset ≈ 225 MiB is this run's *ratchet position*, not where the ratchet stops.
[p2-11-compile-transient.md](../phase2/p2-11-compile-transient.md) measured the
saturation point on this device: ~230 MiB pre-fix, **181.4 MiB** after
`MIMALLOC_PURGE_DELAY=0`, on 798250 rules against this run's 763017.

| Basis | Figure |
| ----- | ------ |
| This run's observed peak + max offset | 155.5 + 69.5 ≈ 225 MiB |
| p2-11 saturation, post-fix, larger corpus | 181.4 MiB |
| Saturation + the same offset | 181.4 + 69.5 ≈ **250 MiB** |

A limit sized from a soak's observed peak fits today's corpus and not next
quarter's; the rule list grew 7381 rules in five days. **Size from a measured
saturation point on the current corpus, never from a run's high-water.** The
container is `memory-high=unlimited` today, which is why nothing died this week.
This does not change the verdict on `memory-high=200M` — it was under the peak
then and is further under every basis above now.

### Finding 7 — the first upstream penalty of the run, inside the heaviest HTTP hour

| When | Event |
| ---- | ----- |
| h61.6 | 1.1.1.1 `failures` 0 → 1, isolated |
| h66.8 | 1 → 2, isolated |
| h92.2 | 2 → 3, isolated |
| **h93.3, 2026-09-15T19:21:26Z** | **3 → 7 in one 6 min interval**: `failure_runs` gains a run of 4, `penalty_failures=2` arms, 1 penalty, 24 `penalized_seconds_total`, 1 probe, 1 probe success |
| h111.9 | 7 → 8, isolated |

`failure_runs=[4,0,0,1]` reads as four runs of one and one run of four — 4·1 +
1·4 = 8 failures, which is the whole count. `adaptive` did exactly what
ARCHITECTURE.md §Upstreams specifies: penalise after 2 consecutive, skip for 24 s,
probe on the way past, restore on success. No SERVFAIL reached a client
(`servfail_synthesized` 0, `servfail_relayed` 1 for the whole run).

The run of 4 sits at the tail of the largest HTTP hour of the soak — 1537.3 MB
of `response_bytes` in the hour ending 09-15 18:00, RSS ramping 65.9 → 90.6.
**Observation, not a finding**: heavy proxy traffic and the only DNS upstream
failure run of the week fall in the same 90 minutes. One co-occurrence proves
nothing; it needs its own test on a quiet device.

### Excursions — unchanged mechanism, released in full

The 09-15 burst at 6 min resolution, the best-resolved excursion of the run:

```text
17:03  rss 65.9  anon 56.9  acc 27.2  resid 38.7  conc_http  6
17:21  rss 74.5  anon 65.5  acc 27.3  resid 47.2  conc_http 36
17:33  rss 86.1  anon 77.1  acc 27.4  resid 58.7  conc_http  5
18:51  rss 90.6  anon 81.7  acc 27.4  resid 63.2  conc_http  0
19:57  rss 64.8  anon 55.8  acc 27.5  resid 37.3  conc_http  1
```

`accounted` never moves. All of it is `rss_anon`, all of it is residual, and
**all of it comes back** in ~2.5 h. Same shape as the day-3 excursion, so the
bursts themselves are not what lifts the floor.

Eight hourly RSS steps of ≥8 MiB in the run; the four positive ones are at
h36, h68, h76 and h91, all in the hours carrying the largest HTTP transfers.
Pearson correlation of per-pull Δ`http.response_bytes` against Δ`residual_bytes`
is **+0.451** over 115 intervals — real, and far from a clean law.

The day-3 observation that retention does not scale with bytes on ARM64 gets
stronger, not weaker: 1537 MB at a high-water of 36 left +24.7 MiB; 184 MB at a
high-water of 6 left +22.7 MiB. Bytes moved by 8×, retention by 9 %.

### Problems found

| # | Problem | Evidence | Impact |
| - | ------- | -------- | ------ |
| 1 | `collect-soak.py` wrote the **last** `memory-current=` it saw into `meta.json`, with no container filter — **fixed 2026-09-16** | `20260916T100001Z` recorded 33.9 MiB while the FAH container was at 135.7 | Was silent: `meta.errors` stayed empty. `own_container_memory()` now selects the `fastadhunter-*` entry and records an error when there is none |
| 2 | A second container, **`fah-diagprobe`** (33.9 MiB, `cpu-usage=21.5`), was running on the RB5009 at 09-16 10:00 | `pulls/20260916T100001Z/routeros-container.txt`, two entries | Contaminates that hour: router `free-memory` dips to 666.8 MiB, its lowest of the run, against 710.3 and 710.2 either side |
| 3 | ~~Committed allocator address space never returns~~ — **retracted, Finding 5** | `current_commit == peak_commit` in 116/116 pulls is the documented signature of a counter that never decrements | None. The claim had no evidence behind it; `memory.rs` and p2-11 had already answered it |
| 4 | Day-3 and day-5 figures alike were published from windows too short to carry them (offset stability, floor advantage over 0.3.1, the allocator claim) | §What day 5 supersedes, Finding 5 | Method risk, not a code defect. Three of the four came from reading a within-window observation as a property, and one from not reading the field's own documentation first |

Problem 1 was the only one that wanted a code change, and it was in the
collector, not in FAH. **No problem found in this pass is a FAH defect.** The
one finding that points at FAH is the floor, and its named mechanism —
`8941770`, absent from this build — was already diagnosed before the soak began.

### Flags — status at day 5

| Flag | Verdict |
| ---- | ------- |
| Memory drift | **trips, without qualifier** — the residual floor **doubled** (19.0 → 38.6 MiB) and remained elevated across every 12 h bucket, while `accounted_bytes` stayed bounded at 28.00 MiB over 1140 samples. Finding 4 |
| Container divergence | **trips** — offset +3.21 MiB/day at R² 0.630, 52.26 → 65.72 MiB. Finding 6 |
| Upstream health | **trips** — 8 failures, 1 penalty, 24 penalized seconds. Behaviour matched the spec exactly; the flag is about the counter moving, and it moved. Finding 7 |
| List refresh | **trips for the high-water, clear for the floor** — both new `peak_rss` steps land on refresh samples; the ≥8 MiB hourly excursions do not. The mechanism is the p2-11 compile ratchet, which has a measured ceiling. Finding 6 |
| Cache pressure | clear — `evictions` 0, `expired` 0, 1923 of 50000 entries, 1.88 of 64 MiB, 1139 cleanup runs freeing 3.15 MB |
| Shed | clear — `events_dropped` 0, `udp_inflight.shed` 0, `swr.dropped` 0, `swr.failed` 0 |
| Answer quality | clear — `servfail_synthesized` 0, `servfail_relayed` 1, `refused_relayed` 0 |
| Listener bounds | clear — `dns_tcp_connections.peak` 33 of 1024, `closed_oversize` 0 |

Four flags trip at day 5 against none at day 3. Memory drift is the one that
matters: the other three are a page-cache offset, a bounded compile ratchet and
a 24-second failover that worked.

### Dataset integrity

| Check | Result |
| ----- | ------ |
| Pulls | 116, span 113.78 h; cadence 3598–3602 s outside the two t0-adjacent pulls |
| `meta.errors` | empty in 116/116 — the dashboard catch-all never fired |
| `collector.log` | 115 lines, `errors=0` on every one (t0 was pulled by hand and is not logged) |
| Restarts | none; `uptime_seconds` strictly increasing to 113.94 h, `version` 0.3.4 in all 116 |
| Perf series, deduped, `ts >= 2026-09-11T22:02:48Z` | **1140 rows, 1140 expected, no holes**; one 395 s interval at boot, 1138 at 360 s |
| Container log `WARN`/`ERROR` | 0 lines across all 116 pulls |
| RouterOS problem log | newest line identical in 116/116 — `2026-09-12 01:07:38 IPv6 global UP` |
| Config | 6 daily `config.json`, **1 distinct hash** — nothing was changed under the run |
| Container count | 1 in 115 pulls, 2 in one (Problem 2) |

Router uptime is 114.0 h, matching the process: the RB5009 rebooted at the soak's
t0, so `write-sect-since-reboot` 134 → 5257 (+5123, ~1080/day) covers exactly
this run. `free-hdd-space` is flat at 973.8 MiB, so `/data` is still not on
internal storage.

### Service figures, 113.9 h

| Figure | Value |
| ------ | ----- |
| Queries | 546653 (mean 1.333 qps; hour-of-day median 0.46 → 3.20; busiest sample 20.33 qps) |
| Blocked | 434118 = 79.41 % |
| Cache hit rate, of non-blocked | 94.71 % (106584 hits / 5951 misses) |
| Stale share of hits | 56.94 %, refreshed by SWR: 58607 enqueued, 58607 completed, 2077 deduplicated, 0 failed, 0 dropped |
| Mean latency | forward 21.57 ms, block 0.034 ms, cache hit 0.048 ms |
| CPU | 294 s user + 379 s system over 410194 s = **0.164 % of one core** |
| HTTP | 2183.6 MB `response_bytes`, 6214 pass, 0 block, 317 refused (2.78/h), mean 124.6 ms |
| Lists | 53 bodies, 118.1 MB fetched, `not_modified` **3**; ruleset 755636 → 763017 rules (+7381, +0.22 MiB) |
| Clients | 651 → 691 (+8/day), `stats_clients_bytes` unchanged |
| Upstream RTT | 1.1.1.1 p50 5.0 ms in all 116 pulls, cumulative mean 9.00 ms; p99 moves 250 ↔ 100 ms on a bucket edge only |
| Router free-memory | 787.0 → 706.6 MiB (−16.9 MiB/day), container `memory-current` 94.4 → 136.9 MiB |

Accounting still reconciles to the query. Upstream attempts 64566 = `cache_misses`
5951 + `swr.completed` 58607 + **8** failover retries, and 8 is exactly the
failure count. `cache_stale` 60686 = `swr.enqueued` 58607 + `swr.deduplicated`
2077, within 2.

Observations carried forward unchanged, each needing its own test: the 57 %
stale-serve share at `min_ttl_seconds` 600; `not_modified` 3 of 56 fetch
attempts at `refresh_hours` 48, so nearly every refresh buffers a full body;
`http.refused` 317 at 2.78/h, expected from `allow_ip_literal_hosts=false` but
still unverified. And the day-3 caveat stands: both IPv6 upstreams report
`state: "healthy"` on 0 attempts and 0 probes, which is the initial value, never
tested.

### For the tend read — revised

- **Take the final floor reading before the full backfill**, unchanged from day 3.
- **Lead with the 12 h floors and the same-hour-of-day table, not with a slope.**
  A linear fit averages an acceleration and reads gentler than the floors do.
  `reduce.py floor` prints both.
- Compute G2 as hourly minima, least squares, **h48 → h168**, and keep it as the
  continuity figure with F33 — not as the claim.
- **Do not fit short tail windows.** h72/h84/h96-to-end have n of 42/30/18 at R²
  0.009/0.035/0.453; day 5 gave them equal billing with the main trend and read
  a plateau into a noisy series. They belong in the script's output, not in a
  finding.
- **Do not derive retention from `allocator_committed_bytes`.** It is monotone by
  construction ([memory.rs:98-112](../../../crates/fah-model/src/memory.rs)) and
  day 5 got this wrong. Record it, do not interpret it.
- The container-offset series can now use all 116 pulls — the collector is fixed
  and `20260916T100001Z` re-parses to 135.7 MiB (Problem 1).
- **`8941770` is the next measurement on this device**, not a tend chore. This
  run is the "before" arm of an A/B that has never been run on the RB5009, and
  the floor is what it is meant to move.

### Verdict at day 5

No crash, no restart, no shed, no eviction, no SERVFAIL synthesis, no listener
saturation, no log error and no config drift in 113.9 h at 0.164 % of one core.
`adaptive` handled its first real failover to specification. The excursions are
HTTP and they release in full.

What changed since day 3 is the floor, and it is the whole story of this pass.
**The residual floor doubled — 19.0 → 38.6 MiB — and remained elevated across
every 12 h bucket**, while everything the process can account for stayed inside
28.00 MiB. It did not flatten where 0.3.1's did, it crossed above 0.3.1 at h72,
and the rise accelerated (+1.3, +5.3, +8.2 MiB/day on same-hour comparisons)
rather than settling. One flat final day does not undo four rising ones.

Day 5 also had to withdraw one of its own findings: the allocator-retention claim
was built on a counter that cannot decrease, and the repo had documented that
before this soak began. That correction removes a false cause; it removes
nothing from the floor, which is measured on RSS and stands.

Nothing here justifies stopping the soak or touching the router. It does justify
treating `8941770` as the next thing to measure on this device.
