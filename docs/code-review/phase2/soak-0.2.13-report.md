# 0.2.13 — 18.5 h Soak

**Captures:** [`soak-0.2.13/`](soak-0.2.13/) — 11 files at `20260808T145811Z`
(T0) and `20260809T092434Z` (T+18.5 h) · **Device:** RB5009, `dns+http`,
`memory-high=unlimited` · **Load:** 1.08 qps mean, 5.25 peak

## Summary

One container lifetime, 66,602 s, no restart. The stale-serve fix `0.2.13`
shipped for is **proven on-device** — T0 could not prove it, because
`cache_stale` was 0 and the `FromSwr` arm had never executed. It has now run
9,568 times and the latency stages partition exactly.

Memory does not drift. RSS moves 47.6 → 52.0 MiB, almost all of it the cache
filling (57 → 1,939 entries), and the whole-window and final-third slopes
disagree in sign.

Every upstream failure in the window is attributable to the WAN link dropping,
not to an upstream.

## Decisions

- **0.2.13 is verified.** The phase-2 soak criterion is met; the next deploy is
  unblocked.
- `MIMALLOC_PURGE_DELAY = 0` does not thrash at this load — `p2-11` left this
  open. Scope: **~1 qps**, not a claim about high load.
- `9.9.9.9` needs no action. It is healthy and its `consecutive_failures = 2` is
  an artefact of being reached only when `1.1.1.1` fails.
- No refresh ran (`refresh_hours = 48`), so the ~180 MB compile transient is
  **not** exercised here. `p2-12` owns that number.

## Bugs found

None. Two behaviours worth recording rather than filing:

1. **`consecutive_failures` goes stale by construction.** Under
   `strategy = "fallback"` a secondary upstream is attempted only when the
   primary fails, so a secondary flagged unhealthy stays flagged until the next
   primary failure. Already on the roadmap as health-aware selection.
2. **A fallback pool cannot survive a link outage** — both upstreams fail in the
   same interval, because the failure is the WAN, not the resolver.

## Measurements

### The stale-serve fix (`0.2.13`'s reason to exist)

Latency stages partition every resolved query, exactly:

| Stage histogram | Counter | Equal |
| --- | ---: | --- |
| `block` | 67,288 / 67,288 | ✅ |
| `cache_hit` | 16,185 / 16,185 | ✅ |
| `forward` | 2,052 / 2,052 | ✅ |

`hits + misses == pass + allow` → `16,185 + 2,052 == 18,219 + 18 == 18,237`.

**The 9,568 stale serves are inside `cache_hit`, not `forward`.** `forward`
counts exactly the cache misses, so a stale serve is timed as the cache read it
is rather than as network work.

SWR accounting closes on both sides:

```text
cache_stale 9,568 − deduplicated 156 = enqueued 9,412
completed   9,410 + failed        2  = enqueued 9,412
dropped 0
```

Every stale serve either enqueues a refresh or joins one in flight; the 3
workers never shed.

### Memory

| Field | T0 | T+18.5 h | Δ |
| --- | ---: | ---: | ---: |
| `process_rss` | 47.63 MiB | 52.00 MiB | +4.37 |
| `process_peak_rss` | 117.82 MiB | 117.82 MiB | 0.00 |
| `ruleset_bytes` | 25.81 MiB | 25.81 MiB | 0.00 |
| `cache_estimated_bytes` | 0.07 MiB | 2.94 MiB | +2.86 |
| `residual_bytes` | 21.02 MiB | 22.53 MiB | +1.51 |
| `allocator_committed_bytes` | 196.00 MiB | 207.44 MiB | +11.44 |
| `minor_page_faults` | 37,188 | 84,908 | +47,720 |

RSS slope over 185 samples since boot: **+0.12 MiB/h whole-window, −1.26 MiB/h
final third.** Signs disagree, so this is noise rather than growth — the same
test `p2-07` used. Range 44.8–60.5 MiB.

`process_peak_rss` unchanged confirms no compile ran. `allocator_committed` is
the documented monotone counter, not retention.

One **−8.96 MiB step at 17:12Z** with no restart (uptime continuous) and the
cache still growing across it (514 → 582 entries): a mimalloc purge, consistent
with `PURGE_DELAY = 0`.

Minor page faults, per 6 h block: **3,512 / 1,154 / 2,944 per hour.** Flat and
traffic-shaped — the quiet block is overnight — so no purge thrash at this load.

### Upstreams

| Upstream | Attempts | Failures | Consecutive |
| --- | ---: | ---: | ---: |
| `1.1.1.1` | 13,556 | 4 (0.03 %) | 0 |
| `9.9.9.9` | 4 | 2 | 2 |

Failures occur in exactly two intervals, and in the second **both upstreams fail
together**:

| Interval (UTC) | `1.1.1.1` | `9.9.9.9` |
| --- | ---: | ---: |
| 07:36:32Z | +2 | — |
| 08:48:32Z | +2 | +2 |

The 08:48:32Z interval covers 11:42:32–11:48:32 local, which contains the PPPoE
redial burst recorded at 11:42:25 and 11:43:25 (see
[`routeros-traps.md`](../../routeros-traps.md) §IPv6). Simultaneous failure of two
independent resolvers 2.4 ms away is a dead link, not two sick resolvers; both
answer `/ping` with 0 % loss.

**The ISP's PPPoE redials cause real DNS resolution failures**, not merely
address churn.

### Ruleset and traffic

| | |
| --- | --- |
| Rules | 798,760 (349,264 duplicates removed) |
| Compile | 2.846 s |
| Lists | 16 |
| DNS | 67,288 blocked / 18,219 pass / 18 allow |
| HTTP | 1,046 pass, 0 blocked, 15.18 MB relayed |
| Device | 785 MiB free of 1,024, CPU 0 % |

## Files changed

None — measurement only. Captures in `soak-0.2.13/`.

## Remaining TODOs

- Deploy `p2-13` (`peak_rss` in the history) — unblocked by this soak.
- `p2-14` stays blocked on the ISP's IPv6 routing, not on this soak.
- The `PURGE_DELAY = 0` result is scoped to ~1 qps and is still unmeasured
  above ~0.5 qps sustained load in the sense `p2-11` meant.
