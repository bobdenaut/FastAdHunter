# PERFORMANCE

Performance is the primary feature. This document is the contract: golden
rules that every change must respect, and numeric budgets that `cargo bench`
verifies.

## Golden rules

1. **Zero-copy where possible** — reference buffers, don't copy them.
2. **Streaming before buffering** — process incrementally; never load whole
   documents into memory.
3. **No runtime regex compilation** — and no regex on the hot path at all.
   Rules compile to hash/trie matchers at load time.
4. **No GC, no hidden allocations** — the hot path is allocation-free;
   allocations happen at load/reload time.
5. **No global locks** — atomic swap for ruleset/config, sharding for the
   cache, bounded channels between components.
6. **Cache-friendly layouts** — compact contiguous structures; pointer-chasing
   is the enemy on a 1.4 GHz ARM core.
7. **Bounded everything** — cache, ring buffers, channels, retention. Memory
   must not grow with traffic or uptime.
8. **Deterministic execution** — predictable latency beats occasional
   brilliance; avoid work with unbounded tails on the query path.
9. **Every feature justifies its runtime cost** — a PR that touches the hot
   path states its cost in its description.

## Budgets (acceptance targets)

Reference hardware: MikroTik RB5009 — Marvell Armada quad-core ARMv8 @ 1.4 GHz,
1 GB RAM shared with RouterOS. Verified with criterion benches in `benches/`
(`cargo bench`) and soak tests on the device.

| Metric | Budget |
|--------|--------|
| RAM steady-state, 1M blocked domains loaded | ≤ 128 MB |
| Compiled ruleset for 1M domains | ≤ 40 MB |
| RAM hard ceiling (container limit) | 256 MB |
| Verdict + cache hit, in-engine p99 | < 1 ms |
| Blocked query, in-engine p99 | < 1 ms |
| Forwarded query overhead added by engine, p99 | < 1 ms |
| Sustained throughput on RB5009 | ≥ 10 000 QPS |
| Startup to serving (cached lists, 1M-domain parse) | 1–3 s (< 3 s hard, ~1 s goal) |
| Container image size | ≤ 30 MB |
| **HTTP** pass-through added latency, p99 *(Phase 2 — to measure)* | < 5 ms |
| **HTTP** pass-through throughput, opaque body *(Phase 2 — to measure)* | to establish in p2-02 |
| **HTTP** concurrent connections | bounded by `[http] max_connections` (default 1024) |

Notes:

- In-engine latency excludes upstream RTT — we measure what we add.
- **Measured on the RB5009, 2026-07-19, at 1 213 640 rules:** startup to
  serving **2 440 ms**, compiled ruleset 28.3 MiB. Startup was 3 113 ms before
  `c61c00b` removed two allocations per rule from the parser — the only budget
  that has ever been outside its range. Parse dominates what remains (~75%),
  so chunked parallel parsing is the next lever if list sizes grow.
  `bench_startup_phases` splits read/parse/build; run it before optimising, as
  the split is not what intuition suggests.
- Startup is timed from container log timestamps, not from the process:
  `compile_duration_seconds` is still hardcoded to zero, so the figure carries
  the RouterOS log's one-second resolution. Fix that metric before trying to
  demonstrate anything finer than ~10%.
- **Sustained throughput, measured 2026-07-24** (dev box, four cores per
  §Measuring reliably, realistic mix — a third blocked, a third cache-hit, a
  third forwarded): **~567 000 elem/s** (median of 3 pinned runs, range
  560.7–574.8 K; 336 µs per 192-query wave). The budget
  is 10 000 QPS on the RB5009; this is the assembled pipeline with the cache's
  byte cap active, so the cap costs nothing at steady state. A dev box is not
  an RB5009 — the figure that counts against the budget is the device's — but
  57× headroom on the same code is what makes the device number safe.
- **DNS cache is bounded twice** (p1.5-05): `dns.cache.max_entries` and
  `dns.cache.max_bytes` (default 64 MiB), both enforced by the same O(1)
  amortized FIFO eviction, which runs until *both* hold. Entry count alone did
  not bound memory — the ~91h soak plateaued at ~230 MiB under an adversarial
  large-answer mix, 80% over the 128 MB budget. The tracked figure is the sum
  of the per-entry estimates, maintained incrementally on insert/evict/clean so
  the resolve path never walks a shard.
- 10k QPS is ~100× a busy household's peak; the headroom is the proof of
  efficiency, and it's what keeps p99 flat at real loads.
- Budgets are compared against `main` on every perf-relevant change; a >10%
  regression on a hot-path bench needs an explicit justification
  (see [CONTRIBUTING.md](CONTRIBUTING.md)).

## Rule deduplication — measured trade (p1.5-05)

The compiled matcher holds distinct rules only (RULE_ENGINE.md
§Deduplication). All figures below: dev box, pinned to one core per
§Measuring reliably, 2026-07-24, synthetic domains with a real list's length
distribution.

**What it saves.** Two 1M-rule lists compiled together, by how much of the
second repeats the first — `cargo bench -p fah-rules --bench matcher` prints
this table:

| overlap | duplicates removed | compiled | saved | build |
|---------|-------------------|----------|-------|-------|
| 0%      | 0                 | 57.7 MiB | —     | 493 ms |
| 25%     | 250 000           | 50.4 MiB | 7.6 MiB | 469 ms |
| 50%     | 500 000           | 43.0 MiB | 15.2 MiB | 454 ms |
| 90%     | 900 000           | 31.3 MiB | 27.3 MiB | 430 ms |
| 100%    | 1 000 000         | 28.3 MiB | 30.3 MiB | 437 ms |

≈30 bytes per duplicate (arena + 8-byte record + ~1.43 slots). Note the 0% row:
two non-overlapping 1M lists compile to 57.7 MiB, **past the 40 MB budget** —
and note build time *falling* as overlap rises, because a duplicate's bytes are
never appended in the first place.

**What it costs.** Only compile time, and only in the worst case for it — a
corpus with nothing to collapse. On 1M *unique* rules the build phase goes
**41.6 ms → 93.3 ms**; parse (175 ms) and disk read (9 ms) are untouched, so
whole-compile cost rises ~19%. Paid once per compile (boot, and each list
refresh — default every 24 h), never per query. The remaining ~52 ms is
essentially one random memory access per rule against the transient dedup
index, which is the floor for membership-testing 1M rules; a 0.5 load factor
and a rejected 8-byte tagged-slot variant are documented in `matcher.rs`.

**What it also buys — the part the memory number hides.** Collapsing duplicates
shrinks the open-addressing slot table, so probe chains shorten. Two 500k-rule
lists sharing 250k domains (`--bench overlap_lookup`, A/B against a pre-dedup
checkout):

| lookup | before | after | |
|--------|--------|-------|---|
| hit, domain carried by **both** lists | 154.4 ns | ~83 ns | **−46%** |
| hit, domain in one list | 57.2 ns | 44.5 ns | −22% |
| miss | 63.1 ns | 57.1 ns | −10% |
| compiled size | 28.2 MiB | 21.2 MiB | −7.0 MiB |

"After" is the median of 3 pinned runs (shared-hit ranged 79.8–85.5 ns). A
domain both lists carry used to occupy two slots that hash to the same place,
and every query for it walked both. Dedup is therefore a **hot-path
improvement**, not only a memory one — which is what settles the trade.

## Measuring reliably

The hot-path benches resolve sub-microsecond work, which is below the noise
floor of a loaded desktop. An unpinned `cargo bench` on a busy dev machine has
been observed swinging **6×** between consecutive runs of an unmodified binary
— enough to manufacture a "+159% regression" that does not exist. Before
believing any regression, re-measure with the process pinned to one core:

```powershell
# Windows: run the bench executable directly, one core, high priority
$p = Start-Process -FilePath 'target\release\deps\<bench>-<hash>.exe' `
     -ArgumentList '--bench','--sample-size','200' -NoNewWindow -PassThru
$p.ProcessorAffinity = 4; $p.PriorityClass = 'High'; $p.WaitForExit()
```

```sh
# Linux
taskset -c 2 nice -n -5 cargo bench -p <crate> --bench <bench>
```

Pinned, the same benches hold a confidence interval under 1%. Trust a criterion
delta only when its interval is narrow relative to the change it reports — a
result quoted as `[366.0 ns 366.8 ns 367.5 ns]` is a measurement; one quoted as
`[737 ns 882 ns 1.04 µs]` is noise wearing a number's clothes.

Throughput is the exception to core-pinning: restrict it to four cores
(`ProcessorAffinity = 15` / `taskset -c 0-3`) so the figure is shaped like the
RB5009's quad-core budget rather than a dev box's full core count.

## Positioning

Beat AdGuard Home and Blocky on **both** axes:

- **Efficiency** — their steady-state RAM (roughly 100–200 MB and 50–100 MB
  respectively) is our ceiling territory; our target is below both.
- **Functionality** — streaming HTML rewriting powered by lol_html (Phase 4)
  filters inside pages, which neither does.
