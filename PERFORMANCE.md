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
   allocations happen at load/reload time. The ones that do happen are served by
   **mimalloc**, not by musl's `mallocng` (see `crates/fastadhunter/src/allocator.rs`): the artefact is static-musl,
   so the allocator is a deliberate choice rather than a consequence of the
   target libc. RSS figures recorded in this document and in the soak baselines
   predate that swap and are not comparable to post-swap readings.
5. **No global locks** — atomic swap for ruleset/config, sharding for the
   cache, bounded channels between components.
6. **Cache-friendly layouts** — compact contiguous structures; pointer-chasing
   is the enemy on the RB5009, whose measured single-threaded benchmark
   throughput is roughly 9× slower than the development machine (see §Budgets).
7. **Bounded everything** — cache, ring buffers, channels, retention. Memory
   must not grow with traffic or uptime.
8. **Deterministic execution** — predictable latency beats occasional
   brilliance; avoid work with unbounded tails on the query path.
9. **Every feature justifies its runtime cost** — a PR that touches the hot
   path states its cost in its description.

## Budgets (acceptance targets)

Reference hardware: MikroTik RB5009 — Marvell Armada quad-core ARMv8, nominally
1.4 GHz, 1 GB RAM shared with RouterOS. Verified with criterion benches in
`benches/` (`cargo bench`) and soak tests on the device.

> **The nominal clock is not what single-threaded work was observed to get.**
> Measured 2026-08-01 (`docs/code-review/p2-08-url-lookup-arm.md`): during the
> measurements, a single busy core remained at **350–700 MHz** — 38 of 40
> samples at 350, two at 700, one at 466 — and **no boost to the nominal
> 1.4 GHz was observed**. The router's own counter and the container's
> `/sys/.../cpufreq/scaling_cur_freq` agree.
>
> For FastAdHunter measurements, treat the nominal 1.4 GHz as an architectural
> specification rather than the observed operating frequency. Budget
> calculations should use the measured x86 → RB5009 factor (~9×) unless new
> measurements supersede it.
>
> Whether all-cores load behaves differently is **untested**; the measured
> 20k+ QPS ceiling hints it might, and that is an open question rather than a
> claim in either direction.
>
> **Where the ~9× comes from:** 8.25–10.0× across twelve arms spanning three
> orders of magnitude and two corpora, median ~9.05, measured with the same
> binary over the same corpus on both sides. Flat across URL lengths, which says
> the gap is CPU throughput rather than memory bandwidth — so it converts, and a
> pinned bench on this dev box usually answers the on-device question without
> building a probe container.

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
| **HTTP** request verdict (URL tier), in-engine p99 | < 1 ms — **breached at ≥ 4 KiB URLs with the EasyList target corpus**, see below |

Notes:

- In-engine latency excludes upstream RTT — we measure what we add.
- **Measured on the RB5009, 2026-07-19, at 1 213 640 rules:** startup to
  serving **2 440 ms**, compiled ruleset 28.3 MiB. Startup was 3 113 ms before
  `c61c00b` removed two allocations per rule from the parser — the only budget
  that has ever been outside its range. Parse dominates what remains (~75%),
  so chunked parallel parsing is the next lever if list sizes grow.
  `bench_startup_phases` splits read/parse/build; run it before optimising, as
  the split is not what intuition suggests.
- **Compile is now measured by the process, not by log timestamps.**
  `fastadhunter_ruleset_compile_duration_seconds` was hardcoded to zero from the
  day it shipped until 0.2.8; it now reports the real figure, so anything finer
  than the RouterOS log's one-second resolution is finally demonstrable.
  Measured on the RB5009 at 0.2.8: **2.317 s** to read, parse and build, from
  1 047 409 parsed rules down to 702 178 compiled (345 231 duplicates, 33%).
  That is ~2.2 µs per **parsed** rule — the parsed count is the denominator, not
  the compiled one — and it consumes 2.32 s of the 3 s hard budget, leaving
  roughly 300 k parsed rules of headroom before startup breaches it.
- **A restart no longer compiles twice.** Until 0.2.8 the scheduler's first tick
  found every list "never attempted" — its clock is a monotonic `Instant` that
  resets with the process — so a restart compiled from cache and then refetched
  and recompiled everything ~7 s later, for a byte-identical ruleset. The clock
  is now seeded from the cached copies' mtimes. Serving was never blocked either
  way; what this returns is ~2.3 s of ARM CPU, ~24 MB of downloads and a second
  ~158 MiB peak-RSS transient per restart.
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
- **A stale cache hit no longer costs an upstream round trip** (ADR-0005). It
  used to fall through to the forwarder and only serve from cache if that
  forward failed, so every client asking in the window between expiry and the
  next refresh paid 20–50 ms — and they all paid it in parallel. A stale hit is
  now answered from cache and the refresh runs on a fixed pool of
  `[dns.cache] swr_workers` detached tasks. This is golden rule 8 applied to the
  one cache state that still had an unbounded tail on the query path.
  - Expect the `cache_hit` ratio to **rise** and the forwarded-query rate to
    fall. That is this change moving queries between buckets, not the cache
    becoming more efficient — do not read it as one.
  - The pool never back-pressures: enqueue is `try_send`, and a full queue drops
    the refresh rather than delaying a client. Watch
    `fastadhunter_swr_refreshes_dropped_total` — sustained growth means the pool
    is undersized, not that anything is failing.
  - Cost on the cache side: one `Option<Instant>` per `Entry` (~16 B), which the
    byte accounting charges per *bucket*, so ~262 KB at the default 10 000
    entries and ~2.6 MB at 100 k. Measure it against `max_bytes` rather than
    assuming it is free at large `max_entries`.
- **Deployed throughput moved with the allocator.** The `/tool profile` ceiling
  on the RB5009 was ~15–16 k QPS before mimalloc replaced musl's `mallocng`
  (`docs/code-review/0.2.7-router-memory-and-throughput.md`); the bench put that
  swap at +27% throughput and −17% CPU per query, and the deployed box now
  sustains **20 k+ QPS** — the two agree to within the precision either method
  offers. Still FAH-handling-bound rather than ingest-bound: all four cores
  share evenly, so `SO_REUSEPORT` stays a recipe rather than shipped code.
- **The cleanup sweep is free at real occupancy.** Measured on the RB5009 at
  0.2.9: **79 µs** for a full 16-shard walk at 409 resident entries. At the
  default 360 s cadence that is ~19 ms of CPU per day. The figure is what the
  deferred `map.shrink_to_fit()` question gets judged against — re-read it at a
  cache holding tens of thousands of entries before concluding anything, since
  the walk is O(entries) and this sample is not.
- **Expired entries are swept on a schedule** (`[dns.cache]
  cleanup_interval_seconds`, default 360 s), on the blocking pool rather than a
  DNS worker — `clean` is synchronous and O(entries), which at a raised
  `max_entries` is exactly the unbounded tail golden rule 8 keeps off the query
  path. It holds one shard lock at a time, so a concurrent resolve waits at most
  one shard's walk. It is **not** a bound: `max_entries`/`max_bytes` are, and
  they hold with the sweep disabled.
  - At default settings it will usually reclaim nothing, because
    `serve_stale = true` means an entry is only sweepable 24 h past its TTL and
    capacity eviction reaches such entries first under load. The case it serves
    is a cache idling *below* both caps. Read
    `fastadhunter_cache_cleanup_bytes_freed_total` near zero as normal, not as a
    failure.
  - A clean now also returns the eviction-queue nodes the removed entries left
    behind (each held a cloned domain), so `cache_estimated_bytes` falls where
    it previously stayed flat. The hash-table slab is still **not** returned:
    `shrink_to_fit` is a reallocation plus a full rehash.
    **The baseline that decision needs now exists** — the 0.2.9 soak caught the
    sweeper reclaiming for the first time
    (`docs/code-review/0.2.9-soak-24h.md`), 29 non-empty sweeps fitting

    ```text
    duration_us ≈ 330 + 64.4 × entries_removed     (R² = 0.928, n = 29)
    ```

    at 900–1,100 entries: the walk is the 330 µs intercept and each removal
    costs ~64 µs, the second O(len) pass `Shard::sweep_queue` makes on every
    shard that lost an entry. **It still does not settle the shrink**, and the
    reason is worth stating rather than re-deriving: `table_bytes` follows
    `map.capacity()`, and this cache peaked at 1,100 entries — 2.2 % of
    `max_entries`. The tables never grew, so nothing could shrink. Settling it
    needs a fill-then-drain, not a soak.
  - Freed memory goes back to **mimalloc**, not necessarily to the kernel, so
    RSS lags `cache_estimated_bytes` (CONTEXT.md §Accounted/Residual).
- **The URL-tier verdict budget is met for ordinary URLs and breached for long
  ones at target-corpus scale.** Measured on the RB5009 2026-08-01
  (`docs/code-review/p2-08-url-lookup-arm.md`), minimum of ~5,000 batches:

  | URL length | EasyList + EasyPrivacy (77 unindexed) | Deployed lists (3 unindexed) |
  |---|---|---|
  | 64 B | 35.0 µs | 2.7 µs |
  | 1 KiB | 452.7 µs | 55.5 µs |
  | 4 KiB | **2,091.9 µs** ❌ | 195.4 µs |
  | 8 KiB | **5,335.7 µs** ❌ | 376.8 µs |

  Two things this table must not be read as saying. It is **not** "the URL tier
  is too slow" — at the corpus this router runs, 8 KiB costs 377 µs and the
  budget holds with 2.7× to spare. It is **not** "the deployment is at risk"
  either. The discriminator is the **unindexed-rule count**, not the URL-rule
  count: 714 rules with 3 unindexed are cheap, 18,781 with 77 unindexed are not,
  and a corpus with 15,000 well-tokenised URL rules would also be cheap.

  Long URLs are ordinary traffic, not an attack — OAuth redirects, ad-tech
  beacons and analytics payloads routinely carry multi-KB query strings. The
  adversarial case is separately capped by the p2-03 work allowance, which was
  never reached in this run.

  ~97 % of an 8 KiB lookup is the unindexed scan (≈176 µs fixed + ~67 µs per
  unindexed rule), so a substring index would return it to ~176 µs. That work
  is decided and recorded in `plan/wip/phase2/CLAUDE.md`; ~176 µs is also the
  floor a *perfect* index would leave, since tokenization scales with URL
  length too.
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

Three further traps, each of which produced a wrong number during p2-03/p2-04
before being caught:

- **Criterion's `change:` line compares against the *previous run*, whatever
  that was.** Run the same bench under a different corpus — or after any earlier
  variant — and the percentage is meaningless. It once reported `−77 %` for a
  change that was noise-level, because the stored baseline came from a
  real-corpus run and the new one was synthetic. Quote **absolutes** when
  comparing variants, and `rm -rf target/criterion` when establishing a baseline.
- **The default corpus can hide the regression the real one shows.**
  `benches/url_matcher.rs` falls back to a synthetic EasyList-shaped corpus that
  compiles **zero** unindexed rules; the real lists compile 77, and that is where
  URL-tier cost concentrates. A change that measured +4 % synthetic was +28 % on
  real lists. Set `FAH_URL_CORPUS` before believing a URL-tier number.
- **Subtract the harness's own cost before attributing a stage.** A per-stage
  profile put header stripping at 1.34 µs; timing the setup alone
  (`HeaderMap::clone`) showed 1.28 µs of that was the harness. The real figure
  was ~170 ns — an 8× misattribution that would have aimed optimisation at the
  wrong function.

A **control arm** — one the change cannot possibly affect — is the cheapest
noise detector available. When `http_pass_through/direct_to_origin`, which never
touches the proxy, moved +8.8 % (p = 0.13), that alone said the box was drifting
and the proxy arm's +9.5 % was not real.

Throughput is the exception to core-pinning: restrict it to four cores
(`ProcessorAffinity = 15` / `taskset -c 0-3`) so the figure is shaped like the
RB5009's quad-core budget rather than a dev box's full core count.

### Measuring on the RB5009

The device cannot be benched the way the dev box can: the image is
distroless and RouterOS exposes no `docker exec`, so an on-device measurement
ships as **its own throwaway container** with the measurement as the entrypoint,
reporting through `/log print where topics~"container"`. `Dockerfile.probe` and
`crates/fah-rules/examples/urlbench.rs` are the working example — note it bakes
its corpus in rather than mounting `/data`, which the production container holds
read-write.

Two rules that a wrong number has already been traced to:

- **Sample `/system/resource/print` *during* the run, not before or after.** The
  clock is not a constant: a single busy core has been measured at 350 MHz
  against a 1.4 GHz nominal (§Budgets). A µs figure quoted without the
  concurrent frequency reading cannot be checked by anyone, including its
  author, and the difference is up to 4×.
- **Size the probe to hold a core busy long enough to sample** — tens of
  seconds, not a burst. A probe that finishes before the governor could react
  reports the idle clock and nobody can tell.

Convert rather than re-measure where you can: the **~9× x86 → RB5009 factor**
in §Budgets was flat across three orders of magnitude, so a pinned dev-box
number usually answers the on-device question without building anything.

## Positioning

Beat AdGuard Home and Blocky on **both** axes:

- **Efficiency** — their steady-state RAM (roughly 100–200 MB and 50–100 MB
  respectively) is our ceiling territory; our target is below both.
- **Functionality** — streaming HTML rewriting powered by lol_html (Phase 4)
  filters inside pages, which neither does.
