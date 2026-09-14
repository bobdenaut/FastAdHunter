# p3-10 Track B1 — x86 characterization

Task: [plan/wip/phase3/p3-10-post-merge-performance.md](../../../plan/wip/phase3/p3-10-post-merge-performance.md)

| | |
| --- | --- |
| Device | dev box — 13th Gen Intel Core i9-13980HX, 32 GiB, Windows 11 IoT Enterprise LTSC 2024 |
| Build | `c8e11b9`, `rustc 1.96.0`, criterion bench profile |
| Date | 2026-09-13 |
| Method | every bench run **twice**, back to back, with no code change between the two. A figure is reported as usable only if the two medians agree |
| Machine state | browser closed, no other interactive load |

## Summary

B1 measured six of its eight rows. The dev box resolves per-request cost in a
warm session and does not resolve per-connection setup cost.

Usable: plain HTTP pass-through, small opaque bodies, the H2 download arms, and
the leaf-cache prewarm hop. Not usable: the intercepted handshake, the SNI
splice groups, and `splicebench`'s throughput ranking — all three moved 44–101%
between two runs of identical code. The common shape is that each measures a
whole connection, where setup is dominated by OS scheduling.

Two rows were not run at all: DoT/DoH rate and latency, and admin latency under
DoH load. Both need a DoT/DoH load generator that does not exist. One row, RSS
on the splice path, has no verified runner.

**B1 does not close the W1 performance gate.** The rows that carry W1 — splice
cost and splice throughput — are exactly the ones this box cannot resolve. The
gate stays with **B2 on the RB5009**, which is where the budgets live anyway.

**Superseded for splice, 2026-09-14.** A probe container on the router settled
those rows without waiting for the deploy —
[p3-10-track-b2-rb5009.md](p3-10-track-b2-rb5009.md). It repeated inside ±5 % on
every candidate where this box moved 15–19 %, and it answers the upstream-buffer
question the other way round. Nothing in this file about splice should be used
to choose the router's configuration, and x86 `splicebench` is not run again.
The rest of this file stands.

## Decisions

- **The W1 gate is not moved by B1.** Splice figures here are shape, not
  verdict. B2 on the device remains the gate, per the owner's decision of
  2026-09-13.
- **Do not chase the noisy rows with more samples.** Raising `splicebench` from
  5 to 15 repetitions cut the spread from 6x to ~20% but left its own decision
  metric swinging from +25.9% to +12.5% across a +10% threshold. More
  repetitions buy little and cost much.
- **No percentage claim is made from a noisy bench.** Where two runs disagree by
  more than the effect, the row records the disagreement and stops.
- **No threshold is invented.** `splicebench` returns `pick: none` on every run;
  that is recorded as the result, not overridden by choosing a candidate.
- **Criterion's stored `change:` lines are not evidence.** They compare against
  baselines on this box dated 2026-09-01/02. A/B belongs against a real
  pre-change checkout (root `CLAUDE.md`).

## Bugs found

None in product code. Two factual corrections to the task file:

| Claim in p3-10 | Correction |
| --- | --- |
| `certs.rs` "never run… has had no round at all" | `target/criterion/` holds baselines for all four of its benches, dated 2026-09-01 and 2026-09-02. It had a round on the branch before the merge. What is true is narrower: it sits outside the merge plan's six-crate loop, so it had no **post-merge** round until now |
| B1's splice rows treated as runnable | The harnesses exist and execute. They do not **resolve**. Recorded here rather than left as a row that looks answered |

## Measurements

### Usable — two runs agree

| Bench | Run 1 | Run 2 | Delta |
| --- | --- | --- | --- |
| `http_pass_through/direct_to_origin` | 32.224 µs | 33.540 µs | +4.1% |
| `http_pass_through/through_proxy` | 65.400 µs | 66.186 µs | +1.2% |
| `http_opaque_body/direct_to_origin/8 KiB` | 35.016 µs | 35.094 µs | +0.2% |
| `http_opaque_body/through_proxy/8 KiB` | 69.015 µs | 68.462 µs | −0.8% |
| `http_opaque_body/direct_to_origin/8 MiB` | 5.1770 ms | 5.2611 ms | +1.6% |
| `http_opaque_body/through_proxy/8 MiB` | 6.2959 ms | 6.3109 ms | +0.2% |
| `https_h2_download/direct_to_origin` | 6.3450 ms | 6.8167 ms | +7.4% |
| `https_h2_download/spliced` | 9.2224 ms | 8.5668 ms | −7.1% |
| `https_h2_download/intercepted` | 16.380 ms | 15.473 ms | −5.5% |
| `prewarm_hop/inline_cached_leaf` | 94.459 ns | 93.622 ns | −0.9% |
| `prewarm_hop/inline_prewarm_warm` | 120.41 ns | 119.79 ns | −0.5% |
| `prewarm_hop/spawn_blocking_prewarm` | 4.1652 µs | 5.0899 µs | +22% |

`spawn_blocking_prewarm` is listed here despite its spread: the gap to the
inline arms is ~40x, far larger than the disagreement, so the ordering holds
even though the value does not.

Structural readings that survive the noise:

| Reading | Evidence |
| --- | --- |
| The plain proxy costs ~2x direct on a small request | 65.4 / 32.2 and 66.2 / 33.5 |
| Proxy overhead is nearly fixed, not proportional to body size | +33 µs at 8 KiB, +1.05 ms at 8 MiB, against a 1000x body |
| H2 download ordering: direct < spliced < intercepted | both runs; gaps of ~+35% and ~2.4x against direct |
| The prewarm hop belongs inline, not on `spawn_blocking` | ~40x |

### Too noisy to support a percentage claim

| Bench | Run 1 | Run 2 | Delta |
| --- | --- | --- | --- |
| `https_handshake/intercepted` | 4.0592 ms | 8.1745 ms | +101% |
| `https_sni_splice/through_splice` | 5.7416 ms | 8.2472 ms | +44% |
| `https_sni_splice/direct_to_origin` | 956.93 µs | 843.20 µs | −12% |
| `https_sni_splice_steady_state/direct_to_origin` | 21.449 ms | 18.152 ms | −15% |
| `https_sni_splice_steady_state/through_splice` | 87.168 ms | 83.796 ms | −3.9% |
| `http_opaque_body/through_proxy/1 MiB` | 1.0434 ms | 1.0970 ms | +5.1% |

`https_handshake/spliced` was not captured in either run and is not reported.
`https_handshake/direct_to_origin` read 1.0758 ms then 1.1522 ms (+7.1%); it is
listed as noisy by association, since its group's other arm is unusable.

In run 2 criterion reported `https_sni_splice/through_splice` as "Performance
has regressed, +93%" against its stored baseline. The code was identical. That
line is the clearest statement of the instrument's limit in this file.

**This is not a new discovery.**
[`main-phase3-integration-audit.md`](main-phase3-integration-audit.md) F9
measured the instrument floor on 2026-09-08, on this box, by running one
checkout against itself: with the browser closed,
`http_opaque_body/direct_to_origin/1 MiB` spread 60.2%, and 7 of 54 benches sat
above the root `CLAUDE.md` 10% gate. F9 concluded that those benches need a
quieter machine or a harness without threads before any verdict rests on them.

What B1 adds is independent confirmation on different benches — F9 did not
cover the splice groups or the intercepted handshake — and the same conclusion
reached from a different direction. Where the two overlap they agree:
`http_opaque_body`'s 1 MiB arm is unusable in both, and its 8 KiB arms are
usable in both.

### `splicebench` — throughput ranking

Two runs at `--reps 15`, medians in MiB/s. `--size-mib 64`, `--budget-mib 32`.

| Candidate (up/down KiB) | Run 1 | Run 2 | Delta | Worst-case MiB | In budget |
| --- | --- | --- | --- | --- | --- |
| `loopback_origin` | 6943.2 | 7229.5 | +4.1% | — | — |
| 16 / 16 | 1420.0 | 1315.3 | −7.4% | 32 | **yes** |
| 16 / 32 | 1580.5 | 1877.3 | +18.8% | 48 | no |
| 16 / 64 | 2072.3 | 2391.7 | +15.4% | 80 | no |
| 16 / 128 | 2841.2 | 2712.3 | −4.5% | 144 | no |
| 64 / 64 | 2609.1 | 2691.6 | +3.2% | 128 | no |

| Result | Runs |
| --- | --- |
| `pick: none — no in-budget candidate reaches 0.9 x best` | every run |
| The only in-budget candidate, 16/16, is also the slowest | every run |
| `up sensitivity: 64/64 over 16/64` | +25.9% then +12.5%, against a +10% decision threshold |

At `--reps 5` the same candidate spanned 214.9 to 1361.3 MiB/s — a 6x range
within one run. That is why the reported runs use 15.

The stable conclusion is structural, not numeric: **the current buffer budget
admits exactly one candidate, and that candidate is the slowest of the five.**
Whether 32 MiB is the right worst-case budget is not decided here.

### `fah-certs/benches/certs.rs` — one run

| Bench | Median |
| --- | --- |
| `certs_mint` | 55.707 µs |
| `certs_cache_hit` | 49.593 ns |
| `certs_prewarm_warm` | 74.990 ns |
| `certs_replay_zipf` | 17.474 µs |

`certs_replay_zipf` workload: 100000 handshakes, Zipf(s=1) over 4096 hosts, LRU
512. Result: `prewarm_hits=67323 minted_total=32677 evictions=32165
hit_rate=0.6732`.

Reading: the measured leaf-cache path makes minting far more expensive than a
cache hit — 55.707 µs against 49.593 ns, ~1100x. For repeated SNI values,
subsequent lookups can hit the cache. **The benchmark does not establish that
production DoT traffic always hits**: the cache is shared and bounded, and this
workload evicted 32165 entries. What the shipped build's DoT hit rate is, is a
soak reading, not a bench reading.

## Files changed

None. Measurement only; no product code, no default, no doc outside this file.

## Remaining TODOs

| Row | State |
| --- | --- |
| DoT/DoH request rate and latency (W1) | **not run** — the load generator does not exist. Its own deliverable and its own go |
| Admin latency under DoH load, at and above 64 concurrent (W1) | **not run** — same generator |
| RSS burst → idle → collect, splice path (W1) | **not run** — no verified runner; the plain-HTTP procedure in the soak tooling may be reusable, unverified |
| Held memory with interception on (W2) | not run |
| Splice cost and throughput | measured here, **unresolved here**, and **resolved on the device 2026-09-14** — [p3-10-track-b2-rb5009.md](p3-10-track-b2-rb5009.md). The router repeated inside ±5 % where this box moved 15–19 %, and the two platforms answer the buffer question in opposite directions. The router's answer is the one used; x86 `splicebench` is not run again |
| `certs.rs` second run | single run only; the four figures have no run-to-run check |
