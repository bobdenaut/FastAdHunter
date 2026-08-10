# Soak 0.2.14 — closed

`0.2.14` on the RB5009, `mode=dns+http`, deployed **2026-08-09T10:32:09Z**.
**Closed 2026-08-10T08:15Z at 21.7 h**, 108,810 queries. Verdict: **no drift,
no leak.**

| Capture | Stamp | Position |
| --- | --- | --- |
| T0 | `20260809T103538Z` | uptime 102 s |
| T+12.5 h | `20260809T231020Z` | interim, no `routeros-*` |
| **T+21.7 h** | `20260810T081513Z` | **final, complete** |

Each stamp covers `telemetry`, `stats`, `config`, `lists`, `history-summary`,
`history-perf`, `history-top-blocked`, `debug-memory`, `health`; the final one
adds `routeros-resource` and `routeros-container`.

Endpoint paths that cost a retry: `health` is at `/health`, while `debug/memory`
and `history/top` sit **under** `/api/v1`, not beside it.

## Decisions

- **The window is split at the recompile, and only the clean half decides.**
  `p2-14`'s three `PUT /rules/user` at 21:39:55Z are a level shift; a slope
  measured across them describes the shift, not the process.
- **`memory-current` is not the number to compare against the 128 MiB budget.**
  It is a cgroup figure carrying reclaimable page cache; `process_rss` is FAH's.
- Closed at 21.7 h rather than 24: the drift question the last hours existed to
  answer is already answered by 102 recompile-free samples.

## Bugs found

None. Two behaviours that look like faults and are not:

1. **RSS steps ~41 MiB at the recompile and stays.** Allocator ratchet, not
   growth — already tracked in [project-state.md](../../project-state.md)
   §Open items. `M4` cuts 12.00 MB off the transient that causes it.
2. **`9.9.9.9` reports `consecutive_failures: 2`.** It is queried only when
   `1.1.1.1` fails, so its streak is two rare events, not two seconds. The
   field is a last-attempt record, not a health score.

## Measurements

### Memory — the verdict

| RSS slope | value | reading |
| --- | ---: | --- |
| whole window, 21.5 h, 216 samples | +0.863 MiB/h | contains the recompile |
| **recompile-free, 10.1 h, 102 samples** | **+0.017 MiB/h** | flat |
| — first half | +1.481 | |
| — second half | **−0.459** | **signs disagree → no drift** |

RSS over the clean window: min 57.56, max 68.81, mean 64.80 MiB. The
whole-window figure *fell* from +1.06 MiB/h at T+12.5 h to +0.863 here — a fixed
numerator divided by a longer window, which is what a level shift looks like and
what real growth does not.

| Allocator, both captures 9 h apart | bytes |
| --- | ---: |
| `allocator_committed_bytes` | 369,164,288 |
| `allocator_committed_peak_bytes` | 369,164,288 |

Identical to the byte, and peak equals current: the process asked the kernel for
nothing after the recompile.

| `peak_rss` | MiB |
| --- | ---: |
| boot, held 10:39 → 21:27Z | 117.73 |
| after the recompiles | **173.62** |

**Monotone across all 216 samples**, one step. The 24 rows `0.2.13` wrote still
read back with `peak_rss: 0`, so the `serde(default)` migration survives a
version boundary. This closes `p2-13`'s criterion empirically.

### The two memory numbers, reconciled

| Source | MiB | What it counts |
| --- | ---: | --- |
| `container.memory-current` | 114.02 | cgroup: RSS + page cache + tmpfs |
| `debug/memory.process_rss` | 66.59 | FAH's resident set |
| RouterOS total used | 291.1 | whole device, 732.9 MiB free of 1024 |

The ~47 MiB gap is page cache from reading `/data`, reclaimable under pressure.

### Stale-while-refresh partitions exactly

| | |
| --- | ---: |
| `forward` | 3,783 |
| `cache_misses` | 3,783 |

Equal, as in `0.2.13`. SWR: 15,314 enqueued, 457 deduplicated, 15,312 completed,
**2 failed, 0 dropped** — and the 2 failures are the same 2 queries both
upstreams refused.

### Upstreams

| | attempts | failures | consecutive |
| --- | ---: | ---: | ---: |
| 1.1.1.1 | 22,465 | 14 | 0 |
| 9.9.9.9 | 14 | 2 | 2 |

Every one of `1.1.1.1`'s 14 failures fell through to `9.9.9.9`; 2 of those also
failed, so **2 queries of 108,810 got no upstream answer**.

### Traffic and latency

| | count | mean |
| --- | ---: | ---: |
| block | 75,530 | 42 µs |
| cache hit | 29,497 | 52 µs |
| forward | 3,783 | 35.2 ms |
| http pass | 1,755 | 59.6 ms |

69.4 % blocked. Cache: 3,549 entries of 50,000, 2.97 MiB of 64 MiB, **0
evictions, 0 expired**; 216 cleanup runs removed nothing. Neither cap binds, so
`p1.5-05`'s byte-cap eviction path is still unit-test-only. `events_dropped: 0`.

## Remaining TODOs

- **`compile_duration_seconds` now reads 2.788 s, not the 2.844 s recorded for
  boot** — the field tracks the most recent compile and `p2-14`'s recompiles
  overwrote it. The `0.2.15` A/B must therefore be **boot-to-boot from a fresh
  container**, against 2.844 s, not against whatever the live field says.
- **A falsifiable prediction for the next refresh** (≈ 2026-08-11T10:33Z): RSS
  should *not* step again, because a second compile of the same size reuses the
  retained commit. Another ~40 MiB step would be the first real signal of a
  problem.
- That refresh also drops `big.oisd.nl` 436,345 → 253,558 entries, which ends
  the window in which any boot-compile comparison against `0.2.14` is valid.
