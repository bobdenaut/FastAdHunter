# Upstream idle pool — the concurrency-scaled RSS residual

Dev-box investigation of `burst → peak → residual` on the HTTP path, continuing
[resoak-0.3.1-memory-diagnosis](resoak-0.3.1-memory-diagnosis.md) (F29, F36) and
[ADR-0006](../../decisions/0006-http-allocation-domains.md). 2026-09-12.

## Summary

- Retained RSS scales with **in-flight concurrency**, not with bytes, connections
  or domain count: +1.81 / +10.07 / +25.37 MB at concurrency 1 / 8 / 32 for
  byte-identical 400 MiB waves at a fixed `http_runtimes = 2`.
- A 128 KiB cap on both pass-through H1 legs removes 35–47 % of peak and a fixed
  ~4 MB of residual, and leaves the concurrency slope intact (+8.75 vs +9.65 MB
  going 8 → 32). The H1 buffer sets the *size* of what is retained, not how much.
- Retained bytes sit in two size classes, 448 KiB and 512 KiB — 18.1 of 20.5 MB
  visitor-visible committed. Pages with zero live blocks hold 2–3 % of it.
- `mi_theap_collect(force)` run **on both owning domain threads** frees ~1,300
  small-class blocks per round and **zero** blocks in those two classes: the
  blocks are genuinely live, not freed-but-uncollected.
- They are the idle upstream pool. Live large blocks count 16 / 16 / 15 against
  `MAX_IDLE_UPSTREAMS_PER_HOST` (8) × `http_runtimes` (2); with the pool disabled
  the count is 0 / 0 / 0 and a collect returns 93 % of committed.
- Root cause B1: `pool_idle_timeout` is inert without a client-side pool timer,
  so nothing was ever reaped. Fixed in `8941770`.

## Decisions

- **Allocator untouched.** The retention was live application state; no mimalloc
  arm was run and none is warranted. mimalloc stays (owner, 2026-09-05).
- **H1 buffer cap not taken.** It does not address the slope. 408 KiB stays, per
  the 2026-09-06 owner decision in [resoak-0.3.1-H1_MAX_BUF-diagnosis](resoak-0.3.1-H1_MAX_BUF-diagnosis.md).
- **Pool size and idle timeout unchanged**; only the missing timer was added.
- **Stale comments deleted, not reworded** — `.claude/hooks/no-rust-comments.sh`
  rejects any edit whose replacement text contains a comment line.
- **CONFIGURATION.md `idle_timeout_ms` left as written**: it already described
  the post-fix behaviour. The code disagreed with the doc, not the reverse.

## Bugs found

| Id | Where | What |
| --- | --- | --- |
| B1 | `fah-http/src/proxy.rs:218` | `.pool_idle_timeout()` set without `.pool_timer()`. hyper-util 0.1.20 `pool.rs:425 spawn_idle_interval` returns early when the pool timer is `None`, so no reaper task is ever spawned and idle connections are evicted only lazily at checkout. Trap: `.timer()` on the same builder is h2-only (`client.rs:1509`) and does **not** substitute; the server builder's `.timer()` at `proxy.rs:293` is a different object. |
| B2 | `fah-http/src/proxy.rs:219–221` (removed) | Comment asserted idle connections were "reaped on a timer" — the exact behaviour that was absent. |
| B3 | `fastadhunter/src/main.rs:695–698` (removed) | Doc comment implied pooled memory was a function of configuration. The count is bounded (8/host/domain); the bytes are not, because the H1 buffer grows to hyper's 408 KiB default. |

## Measurements

Device: dev box, x86_64, Windows 11, 32 vCPU / 31.6 GB. mimalloc v3,
`MIMALLOC_PURGE_DELAY=0`, `PURGE_DECOMMITS=1`, `ARENA_EAGER_COMMIT=0`.
Tree `66df219`; variant arms from a throwaway worktree at the same commit.
Workload: `http_runtimes = 2`, loopback origin, 1 MiB bodies, 40 connections ×
10 keep-alive requests = 400 MiB per wave, no DNS load, IP-literal host.
`WorkingSetSize` / `PrivateUsage`, not `/proc` RSS. Nothing here measures the
RB5009 or musl. Raw series and harness: `E:/fah-conc-sweep` (not in the repo).

### Concurrency sweep — 3 waves per cell, `http_runtimes = 2`

| concurrency | wave-1 Δpeak | Δpeak per in-flight conn | cell-total held (WS) | held (private commit) |
| --- | --- | --- | --- | --- |
| 1 | +0.39 | 0.39 | +1.81 | +5.32 |
| 8 | +7.29 | 0.91 | +10.07 | +8.57 |
| 32 | +25.97 | 0.81 | +25.37 | +10.67 |

Per-wave held, MB: conc 1 `+0.61 / +0.60 / +0.60`; conc 8 `+7.55 / +1.90 / +0.62`;
conc 32 `+11.58 / +14.67 / −0.88`. No cell ratchets without bound.

### H1 cap A/B — 128 KiB on both legs vs hyper's 408 KiB default

| concurrency | Δpeak A → B | cell-total held A → B |
| --- | --- | --- |
| 8 | +7.53 → +4.00 (−47 %) | +9.39 → +4.80 (−49 %) |
| 32 | +20.12 → +13.09 (−35 %) | +18.14 → +14.45 (−20 %) |

Concurrency increment 8 → 32: residual **+8.75 (A) vs +9.65 (B)**; peak +12.59 vs
+9.09. The cap shifts the level, not the slope.

### Heap visitor by size class — conc 32, +15 min

`mi_subproc_visit_heaps` + `mi_heap_visit_blocks`; v3 keeps one `mi_heap_t` per
subproc and the walk covers its arena page bitmaps, so all threads' pages are in
scope. Per page: `block_size`, `used`, `committed`, `reserved`, `full_block_size`.

| block size | pages | live | capacity | committed | fill |
| --- | --- | --- | --- | --- | --- |
| 458,752 | 3 | 5 | 23 | 10.06 MB | 22 % |
| 524,288 | 2 | 11 | 16 | 8.00 MB | 69 % |
| all ≥64 KiB | — | 17 | 40 | 18.14 MB (88 % of total) | 42 % |
| all <64 KiB | — | 3,131 | 15,931 | 2.39 MB | 20 % |
| zero-live pages | — | 0 | — | 0.78 MB (3 %) | — |

### Forced collect on both owning domain threads — `collects=2` every round

| wave | 448 KiB used | 512 KiB used | process live blocks | visitor committed |
| --- | --- | --- | --- | --- |
| 1 | 8 → 8 | 8 → 8 | 3,327 → 1,957 | 27.45 → 24.93 MB |
| 2 | 8 → 8 | 8 → 8 | 3,488 → 2,227 | 22.28 → 22.01 MB |
| 3 | 9 → 9 | 6 → 6 | 3,525 → 2,261 | 25.81 → 25.54 MB |

### Pool ablation — `max_idle_per_host` 8 vs 0, same workload

| | pool = 8 | pool = 0 |
| --- | --- | --- |
| live blocks, 448 K + 512 K (w1/w2/w3) | 16 / 16 / 15 | 0 / 0 / 0 |
| committed in those classes | 22.88 / 19.88 / 23.31 MB | 8.06 / 13.56 / 14.94 MB |
| after collect | unchanged | classes absent from the dump |
| visitor committed pre → post collect (w1) | 27.45 → 24.93 MB | 20.68 → 1.47 MB |
| WS after collect vs pre-wave floor | +20.6 | −1.5 |

### Fix A/B — production binaries, interleaved, 300 s idle window

| arm | floor | peak | +60 s | +300 s | held | idle CPU / 300 s |
| --- | --- | --- | --- | --- | --- | --- |
| pre-fix r1 | 27.86 | 49.75 | 46.45 | 46.61 | +18.75 | 0.031 s |
| post-fix r1 | 27.90 | 52.59 | 49.06 | 33.21 | +5.31 | 0.016 s |
| pre-fix r2 | 16.85 | 39.79 | 34.36 | 34.55 | +17.70 | 0.016 s |
| post-fix r2 | 16.84 | 39.86 | 32.26 | 20.04 | +3.20 | 0.000 s |

Held mean **+18.23 → +4.26 MB (−77 %)**; private commit at +300 s 13.5 MB lower.
Pre-fix is flat from +60 s to +300 s; post-fix falls only after the 60 s deadline.
Throughput 559.4 / 558.1 → 545.4 / 549.0 MiB/s — same direction both rounds,
inside this session's 494–588 MiB/s spread, not established as a real cost.

## Files changed

| Commit | File | Change |
| --- | --- | --- |
| `8941770` | `crates/fah-http/src/proxy.rs` | `.pool_timer(TokioTimer::new())`; B2 comment removed; two tests |
| `8941770` | `crates/fastadhunter/src/main.rs` | B3 comment removed |

Tests: `an_idle_upstream_connection_is_reaped_after_the_idle_timeout` (verified to
fail without the timer — "never closed" at 10 s) and
`an_active_upstream_connection_is_reused_across_requests` (passes either way, so
reuse does not depend on the reaper).

## Remaining TODOs

- [ ] Deploy. The 0.3.4 soak runs pre-fix code, so its `residual_bytes` flag
      still measures the old behaviour.
- [ ] Confirm on the RB5009 (arm64, musl, 4 workers). Every figure here is x86.
- [ ] Attribute the ~4 MB still held post-fix; the concurrency sweep's
      concurrency-independent term was ~+0.6 MB per wave.
- [ ] `mi_heap_visit_abandoned_blocks` (owned vs abandoned split) was built for
      but never run; the small-class C3b component is unattributed.
- [ ] Decide whether `http.idle_timeout_ms` at 60 s is right now that it binds.
- [ ] Remove worktree `E:/FastAdHunter-var-h1cap` (H1 cap knobs, heap visitor,
      domain collect hook, pool override) when no longer needed.
