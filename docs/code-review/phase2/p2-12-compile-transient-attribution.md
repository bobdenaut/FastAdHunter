# p2-12 — Compile Transient: Attribution

**Task:** [`plan/wip/phase2/p2-12-compile-transient-structural.md`](../../../plan/wip/phase2/p2-12-compile-transient-structural.md)
· **Instrument:** [`p2-12-attribution/`](p2-12-attribution/) (probe source, its
`Cargo.toml`, raw output for three list orders) · **Corpus:** the 17 deployed
lists, 26.26 MiB raw, pinned copy · **Zero repo changes.**

## Summary

Every term of the refresh transient is attributed. 106.61 MB of the device's
125.69 MB is measured exactly on the dev box and reproduces to the byte across
runs; the remaining 19.08 MB is allocator/OS and is bounded, not explained.

The dominant term is the **parsed form**, not the raw text: `big.oisd.nl`'s
`ParsedRuleList` is 56.51 MB — 53 % of the transient — for data the builder
copies into an arena at a quarter the size and then drops.

Two hypotheses were tested and killed, so neither needs revisiting: realloc
transients (+2.12 MB, not ~20 MB) and a ruleset-accounting defect (`heap_bytes()`
is accurate to 0.15 %).

## Decisions

- **Attribution is the deliverable and it is complete.** No code changed. The
  levers are sized below; whether any is worth taking is a separate call.
- **Size-class rounding bounds the device gap but does not explain it.** It is
  large-block bin rounding whose tail pages are never written, so they should
  not be resident.
- **List order is a real ±19.92 MB lever that nothing controls**, and taking it
  is *not* free — order decides duplicate-winner and arena layout.
- **The peak's location is structural**: the instant the largest list's parsed
  form and its arena copy are both fully live.
- Device-side confirmation stays blocked while the 0.2.13 soak runs — a
  triggered refresh perturbs it and a deploy zeroes it.

## Measurements

Dev box, x86_64 Windows, mimalloc v3.3.2 via `mimalloc 0.1.52` +
`libmimalloc-sys 0.1.49` — the same allocator and version the container runs.
Counters record bytes **requested**; `usable` is `mi_usable_size` on the same
live set.

### The transient, above the old-ruleset-only baseline

| Term | Requested MB | Usable MB | Class |
| --- | ---: | ---: | --- |
| 17 raw texts, all resident at once | 26.26 | 27.95 | incidental |
| dedup index, 2,511,658 slots × 4 B | 9.58 | 10.00 | structural |
| `big.oisd.nl` `ParsedRuleList` | 56.51 | 58.04 | **incidental** |
| — Vec spine, 524,288 cap × 80 B | 40.00 | | |
| — of which used, 436,341 rules | 33.29 | | |
| — of which doubling slack | 6.71 | | |
| — `Arc<str>` domains | 16.51 | 18.04 | |
| matcher arena + records at the peak | 14.26 | | structural |
| **dev-box transient** | **106.61** | | |
| device transient (180.07 − 54.38) | 125.69 | | |
| **unattributed — allocator / OS** | **19.08** | | allocator |

`size_of::<ParsedRule>()` = 80 B. `Arc<str>` domains cost 39.7 B/rule requested,
43.3 B/rule usable. Whole parsed list: **135.8 B/rule**, against **33.9 B/rule**
in the arena the builder copies it into — a 4× amplification on a buffer
materialized whole and consumed once, in order.

### Bounds on the 19.08 MB gap

| Candidate | Measured | Verdict |
| --- | ---: | --- |
| Copying reallocs, both buffers live at the crossover | +2.12 MB | real, small |
| Size-class rounding across the whole live set | +10.20 MB | upper bound only |
| Fetched list bodies still live during compile | 0 | eliminated |
| `heap_bytes()` understating the ruleset | 0.04 MB (0.15 %) | eliminated |

Rounding is an **upper bound on its RSS contribution, not a measurement of it**:
the compiled ruleset is 1,118 live allocations for 798,606 rules, so the term is
large-block bin rounding, and Rust never writes past the requested size — the
tail pages are untouched and should not be resident on Linux. The residue is
mimalloc segment metadata, page granularity, and musl/ARM64 behaviour the dev
box cannot reproduce.

Copying reallocs are measured by pointer identity (`realloc` returning a
different address), not modelled.

### List order

Same corpus, same resulting ruleset (798,606 rules, 25.80 MB) in all three.

| Order | Peak requested MB | vs deployed |
| --- | ---: | ---: |
| deployed (`big.oisd.nl` first) | 132.45 | — |
| largest-first | 132.45 | 0 |
| smallest-first | **152.37** | **+19.92** |

The deployment sits at the best case because `big.oisd.nl` is entry 0 in
`entries`, which is config order — nothing enforces it. A large list added last
costs +15 %.

**Sorting the compile loop is not semantically free.** Insertion order decides
which duplicate wins and the order the arena is appended in, so a sorted build
changes arena bytes and per-list attribution. It would fail this task's own
`Fingerprint` + `url` guard, which is the guard working as intended.

### CPU term, uncosted until now

78 copying reallocs above 1 MiB move **556.19 MB**, worst single 20.00 MB. A
streaming parse removes the `Vec<ParsedRule>` doublings from that total.

### Lever sizes, corrected

| # | Change | Task file | Measured basis |
| --- | --- | ---: | --- |
| 1 | Stream parse into the builder | −42 MB | ≈ **−45 MB** — removes the whole 56.51 MB term, leaving the arena |
| 2 | One list text at a time | −16 MB | **−16.24 MB** — 26.26 minus `big.oisd.nl`'s own 10.02 |
| 3 | Pre-size arena/records, tighten the dedup bound | −7 to −10 MB | **≈ −3.7 MB** — 2.87 arena/records slack + 0.82 dedup overshoot |

Item 3's estimate is roughly **2× optimistic**. The 2.87 MB is what `build()`
frees beyond the dedup index (90.35 → 77.95 MB, of which 9.58 is the index); the
0.82 MB is the ceiling overshooting the 1,148,707 rules actually parsed by 9.3 %.

Items 1 and 2 remain projections from measured terms — nothing is implemented,
so neither has been observed.

## Traps

- **`process_rss` returns `None` on Windows.** The dev box measures logical live
  heap; only the device measures RSS. Do not compare the two directly without
  the baseline subtraction used here.
- **Do not sample this with the history sampler** — a compile is 2.7 s and at
  360 s a sample lands inside one 0.76 % of the time.
- The 19.08 MB gap is a *subtraction* of two figures from different machines and
  inherits both errors. It is the softest number in this document.

## Files changed

None. `docs/code-review/phase2/p2-12-attribution/` holds the instrument and its raw
output so the numbers can be re-derived.

## Remaining TODOs

- On-device: peak across a real refresh with ~130 ms polling of
  `/api/v1/debug/memory`. Blocked until the 0.2.13 soak ends.
- Whether the allocator term reproduces on musl/ARM64 or exceeds the dev box's.
- Carried from `p2-11`: `MIMALLOC_PURGE_DELAY = 0` unmeasured above ~0.5 qps; no
  regression guard exists for this peak, and any guard must be on-device.
