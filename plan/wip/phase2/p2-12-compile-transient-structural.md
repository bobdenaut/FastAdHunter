# P2-12 — Compile Transient, Structural Terms

**Phase:** 2 · **Depends on:** [`p2-11`](p2-11-compile-peak-rss.md) (which
answered the allocator term and left these three untested) · **Model:** Opus

**Do not restart p2-11's investigation.** It established that the peak is a
ratchet across compiles driven by mimalloc's deferred purge, shipped
`MIMALLOC_PURGE_DELAY = 0`, and cut peak 230.7 → 181.4 MiB. That finding stands
and the setting is live. This task takes the terms p2-11 explicitly did not
test.

## Goal

Decide whether the ~180 MiB refresh transient is worth reducing, and if so
reduce it — **without changing the resulting ruleset or refresh semantics**.

The first deliverable is the decision, not a patch. p2-11 deferred this work
with a condition: "worth running only if ≤128 MiB becomes the target or Phase 3
makes 75 MB of headroom tight." Answer that condition before spending anything.

## What is already known

Measured 2026-08-08 on 0.2.12, RB5009 (`process_peak_rss` across a real
scheduled refresh at 13:16:35Z) and reproduced structurally on the dev box:

| | |
| --- | --- |
| Device peak, refresh | **180.07 MB** (p2-11 predicted 181.4 MiB — same number) |
| Device peak, boot | 125.14 MB |
| Steady RSS after refresh | 54.38 MB — returns, no persistent cost |
| Structural live heap at peak | **132.45 MB** |
| Exact peak point | `MatcherBuilder::add_parsed_list_masked` for `big.oisd.nl` |

Live-heap decomposition at the peak, measured with a counting `GlobalAlloc`
against the real 26.26 MiB corpus:

| Term | Bytes | Structural? |
| --- | ---: | --- |
| old ruleset, held live by `ArcSwap` until `swap_in` | 25.84 MB | **yes** — the atomic swap |
| all 17 raw list texts, resident simultaneously | 26.26 MB | no |
| dedup index (2 511 658 slots × 4 B) | 9.58 MB | partly |
| `big.oisd.nl` `ParsedRuleList` (436 341 rules) | **56.51 MB** | **no** |
| matcher arena + records, mid-doubling | 14.25 MB | no |
| = structural live heap | 132.45 MB | |
| + rest of process at rest | ~29 MB | |
| + allocator overhead / unpurged pages | ~19 MB | |
| ≈ device RSS peak | ~180 MB | |

**The dominant term is the parsed form, not the raw text.** `ParsedRule` is
80 B and its `DomainRule.domain` is an `Arc<str>` — one heap allocation per
rule, **135.8 B/rule measured**, for data the builder immediately copies into
an arena at 33.9 B/rule. It is materialized whole and consumed once, in order.

This does **not** contradict p2-11's retraction of PERFORMANCE.md's
streaming-parse lever. That lever described streaming the *raw text*, which is
26.26 MB and correctly not dominant. The parsed form is a different allocation
that no measurement had reached, because p2-11 abandoned the dev-box repro
(`process_rss` returns `None` on Windows). Measuring live heap instead of RSS
is what made it visible.

## Method

Measure before changing anything, and reuse p2-11's instruments rather than
inventing new ones.

1. **On-device:** ~130 ms poll of `/api/v1/debug/memory` around a triggered
   `refresh_all`. p2-11 verified this tracks `getrusage` to within 2 %.
   **Do not use the history sampler** — a compile is 2.7 s and at 360 s
   sampling a sample lands inside one 0.76 % of the time.
2. **Dev box:** counting `GlobalAlloc` over `fah-rules`' public API against the
   pinned corpus. Gives live heap, which is allocator- and OS-independent —
   the one thing the dev box *can* measure here.
3. Both arms per change. RSS is the budget; live heap is the attribution.

## Work, in dependency order

| # | Change | Expected peak saving | Risk |
| --- | --- | ---: | --- |
| 1 | Stream parse → builder; never materialize `Vec<ParsedRule>` | −42 MB | public parser signature changes; `RefreshStats::from(&parsed)` and `looks_misparsed()` must accumulate incrementally |
| 2 | Read one list text at a time inside the blocking compile | −16 MB | `upper_bound` currently needs every text before the loop — needs a second pass or a bound derived from file size |
| 3 | Pre-size arena/records; tighten the dedup bound toward the real rule count | −7 to −10 MB | low |

1 + 2 together: structural 132.45 → ~74 MB, device peak ≈ 122 MB. **That
clears 128 MB by 6 MB — a pass, not headroom.** 1 alone does not clear it
(~138 MB). Adding 3 gives ≈ 112 MB.

## Criteria

- [ ] **The condition is answered first**: is ≤128 MiB the target, given
      697 MiB free on the device and `memory-high=unlimited`? A "no" closes
      this task with the measurements recorded and no code changed — that is a
      legitimate outcome, as p2-11 demonstrated.
- [ ] Compile CPU measured before and after, on ARM64. Streaming removes
      436 k `Vec` pushes and a realloc chain, so it should be neutral or
      better — **unmeasured, and must not be assumed.** Current on-device
      compile is 2.75 s.
- [ ] Resulting ruleset **byte-identical**: same rule count, same
      `duplicates_removed`, same verdicts over the corpus. The determinism test
      is the guard.
- [ ] Refresh semantics unchanged: the old ruleset serves until `swap_in`, a
      failed fetch still keeps the last-good copy.
- [ ] Steady-state RSS unchanged or better (p2-11 improved it 20 %; do not
      regress that).
- [ ] Peak re-measured on-device across a real refresh, not a boot.
- [ ] Whether the ~19 MB allocator term reproduces on musl/ARM64 or exceeds
      the dev box's figure.

## Explicitly out of scope

- The `ArcSwap` old-ruleset term (25.84 MB). That is the atomic-swap guarantee
  (hard rule 3). Serving from a dropped ruleset is not a trade available here.
- `arena_reserve` = 1 GiB, p2-11's suspect for the ~59 MiB of surviving
  ratchet. Separate variable, separate experiment; do not change it in the same
  arm as anything above or neither result will be attributable.

## Left open by p2-11, still open

- `MIMALLOC_PURGE_DELAY = 0` is unmeasured above ~0.5 qps. Watch minor page
  faults at flat RSS and revert to 100 if they climb.
- No regression guard exists for this peak — it is invisible to every
  automated gate, which is how it moved unnoticed. Any guard must be
  on-device.
