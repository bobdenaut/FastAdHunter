# p2-11 — The compile transient, measured

**Task:** [plan/wip/phase2/p2-11-compile-peak-rss.md](../../plan/wip/phase2/p2-11-compile-peak-rss.md)
**Device:** RB5009, 0.2.10, `dns+http`, 16 lists / 798 250 compiled rules
**Date:** 2026-08-06 · **No code was changed.** The whole result is allocator
configuration.

---

## 1. Summary

`process_peak_rss` read 230.7 MiB and nobody knew why. It is **not one
compile's cost** — it is a ratchet across successive compiles, driven by
mimalloc's deferred purge, saturating at ~230 MiB.

Setting `MIMALLOC_PURGE_DELAY=0` cut the peak to **181.4 MiB** and steady RSS to
**46.6 MiB**, and removed a 7.4 s post-compile plateau entirely. A ~59 MiB
ratchet above the boot figure survives and is a different mechanism, not chased.

## 2. Decisions

- **`MIMALLOC_PURGE_DELAY` 100 → 0** on the device. Reverses the 0.2.7 change,
  which was made on a theoretical concern rather than a measurement (§6).
- **`MIMALLOC_ARENA_RESERVE` not tested.** Separate experiment, justified only
  if the target becomes ≤128 MiB or if Phase 3's footprint makes 75 MB of
  remaining headroom tight.
- **No compile-path code change.** Streaming the list parse — the lever
  PERFORMANCE.md carried since 0.2.10 — would not have helped; the dominant
  term was never raw text.
- **DHAT profiling abandoned as the method.** `process_rss` is read from
  `/proc/self/status` and returns `None` on Windows (API.md §Memory), so the
  dev box cannot measure the quantity in question at all.
- Peak stays measured with `process_peak_rss` + a ~130 ms poll of
  `/api/v1/debug/memory`. The history sampler cannot see a 2.7 s event.

## 3. What the numbers say

### Peak is a ratchet, not a compile cost

Fresh process, `PURGE_DELAY=50`:

| Event | `process_peak_rss` | allocator commit |
| --- | ---: | ---: |
| boot compile | 131.5 MiB | 195.2 MiB |
| refresh #1 | 209.9 MiB | 307.9 MiB |
| refresh #2 | 228.7 MiB | 376.7 MiB |
| at rest | — | 395.0 MiB |

The 36 h process it replaced ended at **230.7 MiB / 394.1 MiB**. Two independent
processes converge on the same pair — a saturation point, not a spike.

**Boot is cheap.** 131.5 MiB (and 123.74 MiB at T0, 122.0 MiB at `delay=0`).
Every reading above ~180 MiB is the second or later compile.

### `PURGE_DELAY` arms

| | `delay=50` | `delay=0` |
| --- | ---: | ---: |
| boot peak | 131.5 MiB | **122.0 MiB** |
| peak after 2 refreshes | 228.7 MiB | **173.7 MiB** |
| peak after 4 refreshes | — | 181.4 MiB |
| post-swap plateau | **7.4 s** at 153–179 MiB | **none** |
| steady RSS at rest | 62.8 MiB | **46.6 MiB** |
| commit at rest | 395.0 MiB | 379.7 MiB |
| minor faults, 2 compiles | — | +141 690 |
| minor faults at idle | — | 6.3 /s |

Against the T0 record for the same binary: steady **58.13 → 46.6 MiB (−20 %)**,
peak **230.7 → 181.4 MiB (−21 %)**.

### The plateau

At `delay=50`, RSS held **153.5 MiB for 9.9 s** and **152.6 MiB for 7.4 s** after
the compile finished — `ruleset_bytes` had already flipped 25.77 → 25.79 MiB, so
the new matcher was live and the memory was dead. ~65 MiB, held after the work
was over.

At `delay=0` it is gone: RSS drops on the instant each compile ends, both times.

### Back-to-back compiles invert with arena state

| Arena | R1 peak | R2 peak |
| --- | ---: | ---: |
| fresh (post-boot) | 209.9 MiB | 228.7 MiB — climbs |
| saturated (36 h) | 200.1 MiB | 175.2 MiB — reuses |

Fill, then reuse. This is what rules out a leak and rules in the allocator.

## 4. Retracted

Three claims made during this investigation died on later data. Kept because
each was acted on.

1. ~~"230.7 MiB comes from the boot compile."~~ Boot is 122–131 MiB. The peak is
   reached on the second and later compiles.
2. ~~"It is a regression against 0.2.8's ~158 MiB."~~ 158 MiB is most plausibly
   an earlier point on the same ratchet, read after fewer compiles. **There is
   no evidence of a code regression between 0.2.8 and 0.2.10.**
3. ~~"The 7.4 s plateau is tokio's 10 s blocking-thread keep-alive."~~ It varied
   7.4–9.9 s, and `PURGE_DELAY=0` removes it outright. It was the purge.

A fourth was predicted and not tested: `mi_collect(true)` was expected to flatten
every compile to ~131 MiB. `delay=0` is the aggressive end of the same mechanism
and lands at 173–181 MiB, so a collect would likely buy the same ~55 MiB and no
more.

## 5. What is still unexplained

**~59 MiB of ratchet above boot survives `delay=0`** (122 → 181.4), but it
decelerates sharply — refreshes 3 and 4 added 7.7 MiB combined. Saturating, not
unbounded.

Likely suspect, already on the record from 0.2.7 §5.3: **`arena_reserve` is
1 GiB**, the size of the router's whole RAM. With `arena_eager_commit=0` that is
address space, not physical, but arena growth inside it is what would ratchet
commit 196 → 380 MiB. `MIMALLOC_ARENA_RESERVE` is the untested knob.

## 6. The 0.2.7 decision this reverses

0.2.7 moved `PURGE_DELAY` 0 → 100, reasoning that `0` means "a `MADV_DONTNEED`
syscall on every eligible free with no coalescing". That review states its own
limitation: *"The stress test is not evidence that `0` is safe: at 98.93 % cache
hits it barely exercised the allocation-heavy forward path."*

**This measurement does not close that gap either.** It ran at ~0.5 qps. The
+141 690 faults are confined to compiles and idle costs 6.3 faults/s, but the
forward path at 85 qps remains unmeasured under `delay=0`.

**Falsification is free and is now running:** watch
`rate(fastadhunter_process_minor_page_faults_total)` against steady RSS over the
coming days. Flat → 0.2.7's concern was theoretical. Climbing → revert to 100.

## 7. Files changed

None. Device configuration only:

| Key | Was | Now |
| --- | --- | --- |
| `MIMALLOC_PURGE_DELAY` | 100 | **0** |

`MIMALLOC_VERBOSE` is also gone from `fah-env` — the 0.2.7 to-do is closed.

## 8. Remaining TODOs

- Watch the minor-fault rate under real load; decide `delay=0` vs `100` on that
  evidence, not on argument.
- `MIMALLOC_ARENA_RESERVE` arm, if ≤128 MiB becomes the target or Phase 3's
  footprint makes 75 MB of headroom tight.
- Re-measure the peak once Phase 3 lands — TLS state and per-connection buffers
  land on top of this transient.
