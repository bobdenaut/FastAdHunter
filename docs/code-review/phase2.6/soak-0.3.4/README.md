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
