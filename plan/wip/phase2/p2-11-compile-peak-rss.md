# P2-11 — Compile Peak RSS (measure the transient, then reduce it)

**Phase:** 2 · **Depends on:** p2-07 (the memory instrument that found it) ·
**Model:** Opus

**ANSWERED 2026-08-06 — no code changed.** Result and full measurements:
[`docs/code-review/phase2/p2-11-compile-transient.md`](../../../docs/code-review/phase2/p2-11-compile-transient.md).

> **Continued in [`p2-12`](p2-12-compile-transient-structural.md).** This task
> answered term 4 (allocator retention) and states that terms 1–3 were never
> tested. They have since been measured: the structural live heap at the peak
> is 132.45 MB, of which **56.51 MB is one list's `ParsedRuleList`** — not the
> raw text, so the retraction below stands unchanged. `p2-12` decides whether
> to spend the remaining terms; do not re-open the allocator question here.

| | |
| --- | --- |
| Cause | mimalloc deferred purge; the peak **ratchets across compiles**, it is not one compile's cost |
| Fix | `MIMALLOC_PURGE_DELAY` 100 → **0** on the device |
| Peak | 230.7 → **181.4 MiB** (−21 %) |
| Steady RSS | 58.13 → **46.6 MiB** (−20 %) |
| Post-compile plateau | 7.4 s at ~153 MiB → **gone** |

## Goal

Explain why a ruleset compile peaks at **230.7 MiB**, then reduce it.

Measurement decides the fix — four allocations plausibly dominated, taking four
different remedies.

**It was the fourth: allocator retention.** Terms 1–3 were never tested, and on
this evidence they did not need to be.

## Why this existed

| When | Peak RSS |
| --- | ---: |
| 0.2.8 | ~158 MiB (`lifecycle/mod.rs` `fetch_and_commit`) |
| 0.2.10 boot, 2026-08-02 | 123.74 MiB |
| 0.2.10, 36 h process | **230.7 MiB** |

**The "0.2.8 regression" framing this task shipped with is retracted.** Boot
measures 122–131 MiB across three independent boots; the peak is only reached on
the *second and later* compiles in a process. 158 MiB is most plausibly the same
ratchet read after fewer compiles, so there is no evidence of a code regression.

## Method — what actually worked

1. **Boot vs refresh** via `process_peak_rss` before/after a triggered
   `refresh_all`. Split the hypothesis space in one step and cost nothing.
2. **~130 ms poll of `/api/v1/debug/memory`** around each compile. This is the
   instrument; it tracked `getrusage` to within 2 %.
3. **Env-var arms with a container restart between them** — `PURGE_DELAY`
   50 vs 0. No build, no code, one variable at a time.

**Abandoned:** DHAT profiling and the pinned-corpus dev-box repro. `process_rss`
is read from `/proc/self/status` and returns `None` on Windows (API.md §Memory),
so the dev box cannot measure RSS at all — and mimalloc decommits via
`VirtualFree` there against `madvise` on musl, so a null result would not
transfer.

**Held up:** *do not use the history sampler.* A compile is 2.7 s; at 360 s
sampling a sample lands inside one 0.76 % of the time, at 60 s only 4.5 %.

## What was established

- **The peak is a ratchet.** 131.5 → 209.9 → 228.7 MiB over boot + two
  refreshes, saturating at ~230 MiB. Two independent processes converge on the
  same saturation pair (~230 MiB peak / ~394 MiB commit).
- **~65 MiB was dead memory held 7.4–9.9 s after the swap**, with the new
  matcher already live. `delay=0` removes the plateau entirely.
- **Back-to-back compiles invert with arena state** — R2 peaks *above* R1 on a
  fresh arena, *below* it on a saturated one. Fill, then reuse: not a leak.
- **Fault cost is confined to compiles.** +141 690 for two compiles; 6.3/s at
  idle.

## Criteria — revised, and why

The criteria this task shipped with were written before any measurement and two
of them are now the wrong targets. Stated plainly rather than quietly dropped:

| Original | Outcome |
| --- | --- |
| Name the dominant term with a number | **Met** — allocator retention, −55 MiB when purge is forced |
| Quantify term 4 as `RSS_peak − DHAT_peak` | **Not met, superseded.** DHAT is unrunnable here; the `delay` A/B measures the same thing directly |
| Boot and refresh each get their own reading | **Met** |
| Peak ≤ 158 MiB after cold boot and `refresh_all` | **Not met — 181.4 MiB.** The 158 target rested on the retracted regression framing |
| Dev-box bench failing above a threshold | **Not met.** The dev box cannot read RSS; a guard would have to be on-device |
| Steady RSS and p2-07 residual unchanged | **Exceeded** — steady RSS *improved* 20 % |
| PERFORMANCE.md peak-vs-steady fixed | **Done** in the same change |

## Left open

- **`delay=0` is unmeasured under load.** 0.2.7 moved it 0 → 100 on a syscall-churn
  concern and admitted its own stress test did not exercise the forward path;
  this ran at ~0.5 qps and does not close that gap either. Watch
  `rate(fastadhunter_process_minor_page_faults_total)` and revert to 100 if it
  climbs at flat RSS.
- **~59 MiB of ratchet survives** `delay=0` (122 → 181.4), decelerating hard.
  Suspect is `arena_reserve` = 1 GiB (0.2.7 §5.3). Separate experiment, worth
  running only if ≤128 MiB becomes the target or Phase 3 makes 75 MB of
  headroom tight.
- **No regression guard exists.** This peak is invisible to every automated
  gate, which is how it moved unnoticed. Any guard must be on-device.
