# Soak 0.2.15 — 72 h, RB5009 (in progress)

Device: RB5009UG+S+, RouterOS 7.21.5 (long-term), 4× ARMv8, 1 GB shared.
Container `fastadhunter`, image `fastadhunter-rosready-0.2.15.tar`, distroless/musl.
Allocator: mimalloc — `MIMALLOC_PURGE_DELAY=0 MIMALLOC_PURGE_DECOMMITS=1
MIMALLOC_ARENA_EAGER_COMMIT=0`.
Workload: live household DNS, no synthetic load.

| Point | Wall clock | Uptime | Stamp |
| --- | --- | --- | --- |
| Container start (boot #1) | 2026-08-11 05:25:50Z | — | — |
| T0 | 2026-08-11 05:31:57Z | 327 s | `20260811T053157Z` |
| **Router reboot** | 2026-08-11 ≈ 06:36Z | — | — |
| Container start (boot #2) | 2026-08-11 ≈ 06:42:30Z | — | — |
| T1 | 2026-08-12 05:15:04Z | 81 143 s (22.5 h) | `2026-08-12T05-15-04.409925Z` |
| T2 | 2026-08-13 12:07:26Z | 192 312 s (53.4 h) | `20260813T120726Z` |
| T3 | ≈ 2026-08-14 06:45Z | ≈ 259 200 s | pending |

**The soak clock is boot #2, not T0.** `process_peak_rss` falls 137.05 → 119.21
MiB across T0 → T1, which no single process can do; uptime at T1 and T2 both
resolve the start to 2026-08-11 06:42:30Z ±30 s. The T1 snapshot's own
`routeros-resource` gives the cause — router `uptime: 22h39m1s`, i.e. RouterOS
booted 06:36Z and took the container with it. T0 therefore measures a process
that no longer exists; it is kept as a boot measurement, not as this run's
baseline. The 72 h endpoint moves with it.

T2 was sampled at 12:07Z against T0/T1's ≈ 05:15Z — roughly 7 h further into the
diurnal cycle. Any two-point memory delta between them carries that offset.

No RouterOS snapshot accompanies T2: the router's SSH port is closed from the
dev box, so `routeros-resource`/`routeros-container` for that stamp are absent.

Snapshots live beside this file. Predecessor run:
[../soak-0.2.15/report.md](../soak-0.2.15/report.md).

## Decisions

- Ruleset steps are attributed, not avoided: 14 of 17 lists carry
  `last_refresh: null` and refresh at a point the API does not expose.
- Snapshot cadence is event-driven — T+6 h, then any time `ruleset.rules` moves.
- `residual_bytes` is the primary bounded-memory signal; `process_rss` alone is
  unusable at this sample count.
- Three diurnal cycles are the minimum to separate a load-correlated sawtooth
  from monotonic growth.
- Two withdrawn list URLs are replaced before this run; `tif-mini` joins the set.

Two bugs found, both in the list refresh clocks — a manual refresh does not move
the scheduler's due time, and `last_refresh` does not survive a restart. Written
up with their fixes in
[../boot-refresh-clock-and-orphan-sweep.md](../boot-refresh-clock-and-orphan-sweep.md)
§8; the evidence is §T1 → T2 below.

## Measurements — T0

### Boot

Serving DNS 2.145 s after process start. The three list fetches run in the
background and do not gate the listeners.

| Δ from start | Event |
| --- | --- |
| +2.123 s | refresh schedule restored from cached copies, `lists=14` |
| +2.124 s | ruleset compiled from cache, `rules=589963` |
| +2.145 s | DNS listeners bound, `udp`/`tcp [::]:53` |
| +2.148 s | privileges dropped, uid/gid 65532 |
| +2.152 s | API listening, SWR pool (3 workers) + cache cleanup started |
| +6.616 s | scheduled refresh complete, `refreshed=3 failed=0 rules=662141` |

Two compile figures, not interchangeable:

| Figure | Value | Rules | Contents |
| --- | --- | --- | --- |
| Boot path (log delta) | ~2.12 s | 589 963 | compile + config load + 14 cached list reads |
| `ruleset.compile_duration_seconds` | 2.504 s | 662 141 | instrumented compile only |

Per-rule 3.60 µs vs 3.78 µs — disk read is a small share of the boot path.
Nothing is logged between +2.152 s and +6.616 s, so the 4.46 s of network fetch
is not broken down per list.

### Baseline

| Field | T0 |
| --- | --- |
| rules / duplicates_removed | 662 141 / 457 214 |
| compile_duration | 2.504 s |
| ruleset_bytes | 21.02 MiB |
| process_rss | 51.02 MiB |
| process_peak_rss | 137.05 MiB |
| residual_bytes | 28.98 MiB |
| allocator_committed (= peak) | 254.81 MiB |
| cache entries / bytes | 146 / 184.8 KiB |
| cache evictions | 0 |
| cgroup `memory-current` | 91 975 680 |

### Lists

All 17 report `last_status: ok`, all at `refresh_hours: 48`.

| List | Rules | last_refresh |
| --- | --- | --- |
| dyndns | 1 523 | 2026-08-11T05:25:57Z |
| doh-vpn-proxy-bypass | 16 871 | 2026-08-11T05:25:57Z |
| tif-mini | 168 280 | 2026-08-11T05:25:57Z |
| other 14 | 933 467 total | `null` — restored from cached copies |

T0 is not refresh-clean. The 14 restored lists refresh at an unobservable point
inside the window, which is what confounded the predecessor run (its step landed
4.6 h in). Waiting longer before T0 does not help; the schedule is not exposed.

## Measurements — T1, T2

T0 is a different process (see the points table); it is in the table for
continuity, not as a term in any delta.

| Field | T0 (boot #1) | T1 | T2 |
| --- | --- | --- | --- |
| uptime | 327 s | 81 143 s | 192 312 s |
| rules / duplicates_removed | 662 141 / 457 214 | 662 141 / 457 214 | 661 832 / 456 117 |
| compile_duration | 2.504 s | 2.509 s | 2.419 s |
| ruleset_bytes | 21.02 MiB | 21.02 MiB | 21.00 MiB |
| process_rss | 51.02 MiB | 59.60 MiB | 71.22 MiB |
| process_peak_rss | 137.05 MiB | 119.21 MiB | 164.65 MiB |
| accounted_bytes | 22.04 MiB | 26.10 MiB | 26.70 MiB |
| residual_bytes | 28.98 MiB | 33.50 MiB | 44.52 MiB |
| allocator_committed (= peak) | 254.81 MiB | 259.19 MiB | 385.38 MiB |
| cache entries / bytes | 146 / 184.8 KiB | 3 133 / 2.73 MiB | 3 761 / 3.16 MiB |
| cache evictions | 0 | 0 | 0 |
| stale served / swr completed | 0 / 0 | 13 656 / 13 020 | 36 708 / 35 594 |
| swr failed | 0 | 0 | 0 |
| DNS queries (cumulative) | 378 | 60 828 | 142 833 |
| upstream failures (1.1.1.1) | 0 / 213 | 8 / 28 408 | 15 / 60 532 |
| block mean latency | 49.1 µs | 44.8 µs | 44.9 µs |
| cache-hit mean latency | 48.6 µs | 50.6 µs | 50.9 µs |
| forward mean latency | 36.7 ms | 22.1 ms | 30.1 ms |
| minor page faults | 84 877 | 193 605 | 512 027 |

### Residual is a sawtooth, not a ramp

A point sample of `residual_bytes` is unusable on its own — the envelope from
`history/perf` is the reading that means something:

| Window (24 h, 240 points) | residual min / max / last | rss min / max |
| --- | --- | --- |
| T0 → T1 | 20.1 / 82.6 / 39.8 MiB | 37.0 / 106.7 MiB |
| T1 → T2 | 35.6 / 72.3 / 48.2 MiB | 59.9 / 96.8 MiB |

The floor rises (20.1 → 35.6 MiB) while the ceiling falls (82.6 → 72.3 MiB) —
the band narrows rather than translating upward, which monotonic growth cannot
do. Two windows still cannot separate a load-correlated floor from a slow leak;
the third diurnal cycle is what settles it.

`accounted_bytes` moves +0.60 MiB across the 31 h between T1 and T2, so the
+11.02 MiB of residual and the +126.19 MiB of `allocator_committed` are arena
retention, not tracked structures. `allocator_committed` is the figure to watch
at T3: +126 MiB in one day is the largest single-window move this run has shown.

### Load and cache

Blocked share falls 53.8 % → 45.4 % over the two 24 h stats windows on nearly
identical volume (63 085 → 62 594 queries), and cache-hit rises 40.6 % → 50.8 %.
Both track which clients were awake, not a matcher change. Cache is at 7.5 % of
`max_entries` and 4.9 % of `max_bytes` with **zero evictions** across 53 h —
the cache bound has never been the binding constraint in this run.

SWR: 35 594 enqueued and completed, 0 failed, 0 dropped, 1 113 deduplicated.

### T1 → T2: the refresh clocks

| List field | T0 | T1 | T2 |
| --- | --- | --- | --- |
| `last_refresh` non-null | 3 of 17 | **0 of 17** | 17 of 17, all `2026-08-13T05:26:21Z` |

Two defects, visible only because the window happens to span a restart, a manual
refresh and a scheduled one:

1. **The restart erased `last_refresh`.** Three lists carried a real timestamp
   at T0; after the 06:42Z restart all 17 read `null`, while `last_status` stayed
   `ok`. `ListStatus::last_refreshed` lives in memory only. The durable anchor
   is the cached copy's mtime, which at that point only the scheduler read.
2. **The manual refresh did not move the due time.** `POST /lists/refresh` ran
   at `2026-08-12T10:12:23Z` and succeeded (`refreshed=17 failed=0
   rules=661055`). The scheduled refresh still fired at `2026-08-13T05:26:21Z` —
   48 h + 24 s after the *original* 2026-08-11T05:25:57Z fetch, as if the manual
   one had never happened. Cost: 17 redundant downloads, one full compile
   (2.42 s of ARM CPU) and the peak-RSS transient to 164.65 MiB that T2 records.

Both are fixed in `fah-rules`; mechanism, fix and tests are in
[../boot-refresh-clock-and-orphan-sweep.md](../boot-refresh-clock-and-orphan-sweep.md)
§8. Neither affects the memory question this soak exists to answer — the
redundant compile is a transient the run would have paid a day later anyway.

## Files changed

None in the measurement itself. The two defects above are fixed in
`crates/fah-rules/src/lifecycle/mod.rs`, which is a separate change from this
run's binary — the figures here are all `0.2.15` as deployed.

## Remaining TODOs

- T3 at ≈ 2026-08-14 06:45Z (72 h from boot #2), with the RouterOS pair — needs
  SSH from the router console or a paste, the dev box cannot reach port 22.
- Answer at T3: does `allocator_committed` keep climbing after the +126.19 MiB
  T1 → T2 step, or settle; does the `residual_bytes` band keep narrowing across
  the third diurnal cycle.
- Expose list refresh schedule (next-refresh time) so a soak window can be
  placed against it. The T2 finding sharpens this: `last_refresh` alone cannot
  answer "when does this list refresh next", which is why the redundant
  2026-08-13 refresh was invisible until the log was read.
