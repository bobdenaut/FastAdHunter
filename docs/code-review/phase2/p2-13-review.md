# p2-13 — `peak_rss` in the Perf History

**Task:** [`plan/wip/phase2/p2-13-peak-rss-in-history.md`](../../../plan/wip/phase2/p2-13-peak-rss-in-history.md)
· **Shipped in:** `0.2.14`, deployed 2026-08-09T10:33:17Z · **Verified
on-device the same day**

## Summary

The ruleset compile's RSS peak was invisible to every persisted series, which is
how `p2-11` found it had moved unnoticed. `PerfSample` now carries `peak_rss` and
`/history/perf` serves it, selectable through `?fields=`.

It is cheap because `getrusage(ru_maxrss)` is a **high-water mark**: a sampler
minutes apart records the step without having to land inside the 2.85 s compile,
which `p2-12` measured as a 0.76 % chance. No new syscall, no storage format
change.

**Carries no compile optimisation.** `p2-12`'s levers stay untaken; this makes
the number observable before anything is spent reducing it.

## Decisions

- **One convention, not two.** `u64` in the model, `Option` on the wire, `0`
  meaning "not recorded" — identical to `minor_page_faults`.
- **No new plumbing.** `build_perf_sample` already receives the
  `MemoryBreakdown` that carries the field.
- `PerfFields` was **not** exported from `fah-api` to let a test iterate
  `NAMES`. `wire` is private and widening the public surface for a test
  assertion is the wrong trade; the arity is compile-checked by `[_; 11]`.
- Deployment waited for the `0.2.13` soak to end, since a deploy restarts the
  container and zeroes what the soak accumulates.

## Bugs found

None. One behaviour that reads like a bug and is not — see §The boot row.

## Measurements

### On-device verification, `0.2.14`

| Criterion | Result |
| --- | --- |
| Rows written by `0.2.13` read back | **241 rows, every other field intact** |
| `?fields=peak_rss` | keys exactly `ts,peak_rss` |
| Unknown field name | `400`, lists all 11 accepted names |
| First post-deploy sample | `peak_rss` **117.73 MiB** |
| `/debug/memory` at the same time | `process_peak_rss` **117.73 MiB** |
| Gates | 878 tests across 41 binaries |

**The backward-compatibility criterion is the load-bearing one.** It is the only
one with a silent failure mode: without `#[serde(default)]` the whole 30-day
retained series would have been orphaned at the deploy, and nobody would have
noticed until they looked back.

**The live/persisted agreement rules out a wiring fault, not a wrong
`ru_maxrss`.** The two match by construction — `build_perf_sample` receives the
breakdown `/debug/memory` serves rather than re-deriving it — so the check
excludes a wrong field, a stale copy or a unit error. Nothing here validates
`ru_maxrss` itself.

### The boot row is empty, by design

| Field | `10:33:17Z` (boot) | `10:39:55Z` (first real sample) |
| --- | ---: | ---: |
| `peak_rss` | 0 | 117.73 MiB |
| `ruleset_bytes` | 0 | 25.81 MiB |
| `minor_page_faults` | 0 | 38,684 |
| `rss_bytes` | 43.66 MiB | 53.65 MiB |

All three zeros come from the same breakdown, which the 10 s telemetry poll has
not published yet; `rss_bytes` is read separately from `/proc`. A
`ruleset_bytes` of 0 is impossible as a real value, which is what identifies the
row.

**`peak_rss` non-zero while `ruleset_bytes` is 0 would be the bug.** That is the
comparison to make if this ever needs re-checking.

### Monotonicity

Structurally guaranteed by `getrusage(ru_maxrss)` and observed for the first
post-deploy sample; each refresh adds an empirical point. A **drop in the series
is a restart**, never a reclaim — documented at the field, in `API.md` and in the
TUI's expectations, because a reader who does not know it will file a bug.

`peak_rss` is neither an average nor instantaneous RSS. After the next scheduled
refresh the correct state is `peak_rss ≈ 180 MiB` alongside `rss ≈ 52 MiB`, and
that divergence is the feature.

### Retention

~20 B/row: **+0.15 MB/30 days** at the deployed 360 s interval, +0.9 MB at 60 s,
against `p2-07`'s ~6.9 MB/30 days.

## Files changed

| File | Change |
| --- | --- |
| `crates/fah-model/src/perf.rs` | `peak_rss: u64`, `#[serde(default)]`, + legacy-row test |
| `crates/fastadhunter/src/main.rs` | one line in `build_perf_sample`, + 2 tests |
| `crates/fah-api/src/wire.rs` | response field, `PerfFields` × 5, mapping |
| `crates/fah-api/tests/api.rs` | selector + rejection tests |
| `crates/fastadhunter/tests/history_e2e.rs` | end-to-end: sampler → JSONL → API |
| `API.md` | field list, example, semantics paragraph |

`SAMPLE_PEAK_RSS` in the e2e is deliberately far above `SAMPLE_RSS_BYTES`, so
swapping the two fields anywhere in the round trip fails rather than coincides.

## Remaining TODOs

- The refresh peak has not yet appeared in the series. `refresh_hours = 48`, so
  the first one lands up to two days after deploy and should raise the value
  117.73 → ~180 MiB.
- No threshold or alert. This makes the peak visible; deciding what value is too
  high needs the series to exist first.
- The TUI does not display it. Worth doing, and not this task.
