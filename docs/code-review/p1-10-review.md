# p1-10 review — benches + end-to-end integration

RC review before Phase 2, 2026-07-21. Scope: what p1-10 shipped —
`crates/fastadhunter/benches/pipeline.rs`, `crates/fastadhunter/tests/e2e.rs`,
and the consolidation of the per-crate benches (`fah-rules/benches/matcher.rs`,
`fah-dns/benches/cache.rs`, `fah-metrics/benches/record.rs`). Reviewed as
Rust-reviewer work: ownership, premises, API design, allocations,
maintainability. Findings were applied the same day — see §Resolution.

## What was built

**`benches/pipeline.rs`** — the five PERFORMANCE.md budget rows no single
crate can answer, in the binary's package (the only crate allowed to see every
layer): `blocked_query`, `forwarded_query_overhead`, `sustained_throughput`,
`startup_from_cached_lists` (+ a `startup_phases` decomposition into
read/parse/build), and `report_memory` (a one-shot bytes report — criterion
times durations, this budget is bytes). The blocked bench asserts its premise
before *and* after the timed loop (the forwarder was never reached).

**`tests/e2e.rs`** — spawns the real executable against tempdir `/config` +
`/data` volumes and a mock UDP upstream, fully offline: blocked domain →
0.0.0.0 TTL 10, allowed domain → upstream answer carried through, the query on
the WebSocket with rule/list attribution, stats + query log via the API, a
verdict flipping live through `PUT /rules/user` with no restart, key rotation
invalidating the old key immediately. Kills the child via a `Drop` guard,
keeps the engine log retrievable, and retries boot on a lost ephemeral-port
race (only on a genuine port conflict — anything else fails loudly with the
log attached).

**Per-crate benches** — matcher size + lookup latency at 1M domains
(`fah-rules`), cache-hit in-engine latency with an exactly-once forwarder
premise (`fah-dns`), per-event record cost with a mixed-shape workload
(`fah-metrics`).

## Findings

### Critical

None.

### Major

**M1 — `forwarded_query_overhead` measured the cache-hit path.**
`FORWARD_DOMAINS` was 4096, but the default cache holds 10 000 entries — the
comment "more distinct domains than the cache holds" was simply false. After
the first pass every domain was cached, and the timed loop was cache hits over
a 4096-entry working set. The premise assertion (`calls > before`) could not
catch it: criterion's warmup forwards 4096 times before sampling starts, so
"the forwarder was reached" is satisfied even when zero *measured* iterations
forward. The recorded 2.44 µs was a cold-working-set cache-hit figure wearing
the forward path's name — and it hid the real steady-state forward cost, which
is an order of magnitude higher (see §Resolution). Fix before Phase 2:
Phase 2 optimizes against these baselines, so a mislabeled baseline
misdirects the work.

**M2 — `sustained_throughput`'s "fresh third" was fresh for one wave.** The 64
"fresh names that must be forwarded" were generated once and reused every
wave; 64 ≪ 10 000, so from the second wave on they were cache hits. The
measured steady state was ⅓ blocked + ⅔ cache hits, zero forwards — not the
documented mix — and this bench asserted no premise at all, the one thing the
file's own header says every bench must do. Two smaller honesty bugs rode
along: the `CONCURRENCY` doc said 64 in-flight while the code spawned 192
tasks per wave, and the runtime took the dev box's core count while the budget
is stated for four RB5009 cores. The 611 000 QPS figure was real arithmetic
over the wrong workload. Fix before Phase 2.

### Minor

**m3 — duplicated 1M-list setup inside `pipeline.rs`.** `bench_startup` and
`report_memory` carried verbatim copies of the write-blocklist +
`RulesConfig` block (~20 lines each). Same-file duplication with no
counter-argument — consolidate now.

**m4 — nothing end-to-end exercised the real `CacheAdapter`.** The cache
admin endpoints (p1-09) are tested in `fah-api` against a `FakeCache`; the
binary's adapter maps `fah_dns::CacheStats` field-for-field into the port's
DTO, and a transposed pair of fields would ship green. The e2e test already
boots the real assembly — extending it with the cache endpoints closes the
only untested wiring. Fix now (cheap: one repeat resolve + three requests).

**m5 — filtered bench runs still pay full setup.** `report_memory`,
`bench_startup_phases` and the `fah-rules` matcher bench do their 1M-domain
generate/parse/build work even when a `--bench <filter>` argument excludes
every timed bench they own — criterion offers no "am I filtered" hook, so a
filtered invocation costs ~30–60 s of unrelated setup. Defer: the cost is
per-invocation developer time, the fix (peeking at `std::env::args`) is a
hack, and criterion upstream is the right place for it.

### Nitpick

**n6 — `is_port_conflict` needles are broad.** Bare `"10048"` and
`"socket address"` can match unrelated log text and misclassify a real
failure as a port race. Worst case is two wasted retries — the final attempt
always panics with the log attached — so correctness is preserved. Leave;
tightening risks re-introducing the flake the breadth was added to kill.

**n7 — cross-crate bench duplication.** `Lcg`, the synthetic-domain
generator, the instant mock forwarder and `encode_query` each exist twice
(`fah-rules`/`fastadhunter`, `fah-dns`/`fastadhunter`). Consolidating needs a
shared dev-dependency crate; ~60 duplicated lines across independent bench
targets does not justify an eleventh crate in Phase 1. Defer, revisit if a
third copy appears.

### Product observation (exposed by the fixed bench, not a p1-10 defect)

At steady state the cache is full, and then **every forwarded query pays
`evict_one`: two O(shard-len) scans** (a dead-entry `find`, then
`min_by_key(inserted_at)` over ~625 entries) **plus a `CacheKey` clone — an
allocation on the query path** (golden rule 4 is "no hidden allocations").
Measured honestly this puts forwarded overhead at ~27 µs on a pinned x86
core — still ~35× inside the 1 ms budget even allowing 3–5× for the RB5009,
so nothing gates on it, but it is the single biggest cost on the forward path
and the obvious first target if Phase 2 wants one: an insertion-order ring per
shard makes FIFO eviction O(1) and allocation-free. The old bench could never
have seen this because it never filled the cache. **Resolved same day — see
§O(1) eviction below.**

## What's deliberately fine

- **Spawn-per-query in the throughput bench** — mirrors `udp.rs`, which
  spawns a task per datagram; the overhead measured is overhead the product
  pays.
- **`report_memory` as a non-timed criterion function** — the budget is
  bytes; printing + asserting alongside the timed benches keeps it in the
  `cargo bench` workflow without faking a duration.
- **The accept-any TLS verifier in e2e** — the appliance cert is self-signed
  by design (SECURITY.md); the bypass is explicit, local, and test-only.
- **One monolithic e2e test** — booting the binary costs seconds and the
  assertions are about accumulated state; the module doc argues it and the
  argument holds.
- **Ephemeral-port pick-and-release with retry-on-conflict** — inherently
  racy, but the alternative (fixed ports) collides with whatever the dev box
  runs; losing the race is detected and retried, other failures stay loud.

## Resolution (2026-07-21)

All fix-now findings applied; gates green (fmt, clippy `-D warnings`, full
workspace tests).

- **M1** — `FORWARD_DOMAINS` raised to 16 384 (> the 10 000-entry cache;
  with oldest-first eviction, cyclic access over a larger set misses every
  time). The workload cursor is a single `AtomicU64` that never rewinds —
  the first fix attempt kept a per-closure `mut i`, and the new premise
  assertion itself caught that criterion re-invokes the closure per phase,
  resetting the walk into just-cached entries (51% hits). The premise is now
  asserted hard: ≥ 90% of *timed iterations* must reach the forwarder,
  filter-safe, immune to warmup satisfying it.
- **M2** — the throughput mix is now real: a fixed blocked third, a hot
  cached third, and a fresh third drawn from a 16 384-name pool through a
  cursor shared across waves, so fresh names never repeat within the cache's
  memory. Runtime pinned to `worker_threads(4)` to match the budget's core
  count; `CONCURRENCY` replaced by an honest `WAVE = 192`. Premise asserted
  from both sides: forwards ≥ 90% of the fresh third *and* ≤ the fresh third
  plus 10% slack (blocked/cached thirds must not leak upstream).
- **m3** — shared `cached_1m_blocklist()` helper; both setups now one call.
- **m4** — e2e extended: a repeat resolve must be a cache hit visible in
  `GET /api/v1/cache` (`entries ≥ 1`, `hits ≥ 1`, all fresh),
  `POST /api/v1/cache/clean` removes nothing from an all-fresh cache, and
  `GET /api/v1/debug/memory` reports a non-empty ruleset, a non-zero cache
  estimate, and a present-even-when-null `process_rss`.
- **m5, n6, n7** — deferred with rationale above.

### Re-measured (pinned per PERFORMANCE.md §Measuring reliably)

| Budget row | Target | Was (mislabeled) | Now (honest) | Headroom |
| ---------- | ------ | ---------------- | ------------ | -------- |
| Forwarded query overhead | < 1 ms | 2.44 µs | **27.7 µs** | ~36× |
| Sustained throughput (4 cores) | ≥ 10 000 QPS | 611 000 QPS | **209 000 QPS** | 21× |

The code did not regress — the benches stopped lying. The forwarded figure
now includes the full-cache eviction scan every steady-state forward pays;
the throughput figure now includes a genuinely forwarded third. Every budget
still passes with wide margin. Unchanged rows (blocked 1.83 µs, cache hit
1.87 µs, startup 300 ms dev / 2 440 ms device, ruleset 28.3 MiB) stand.

One environmental note: the extended e2e failed twice in the first
post-rebuild workspace run (output lost to a pipeline, cause consistent with
first-execution stalls on freshly compiled binaries), then passed six
consecutive full-workspace runs. If it recurs, the failing assert's message
carries the JSON context needed to attribute it.

### O(1) eviction (follow-up, same day)

The product observation was acted on immediately at liviu's request. Each
shard now keeps an insertion-order `VecDeque<(CacheKey, seq)>`; eviction pops
the head instead of scanning the map. A per-shard `seq` counter ties queue
nodes to entries (not `Instant` — a paused test clock can hand two inserts
the same timestamp); a refresh re-inserts under a new seq, turning the old
node into a ghost that is skipped at pop time and swept by an amortized-O(1)
compaction once the queue outgrows 2× the shard bound — so the queue itself
obeys hard rule 4. The estimator counts the queue (slab + cloned domain
strings). The dead-first eviction preference is gone with the scans; dead
weight now leaves by aging to the queue head, TTL turnover, or
`POST /cache/clean`. Caching semantics — what is stored, TTL clamps,
negative caching, serve-stale, lookup — are untouched, and their tests were
not modified. Three new tests pin the mechanics: FIFO order, ghost-tolerant
refresh, queue bound under refresh churn.

| Budget row | Before | After | Delta |
| ---------- | ------ | ----- | ----- |
| Forwarded query overhead | 27.7 µs | **~13 µs** | −53% |
| Sustained throughput (4 cores) | 209 000 QPS | **241 000 QPS** | +15% |

The remaining ~13 µs is the genuine churn of an insert at capacity — the
evicted entry's drop (several hickory frees), probes in a high-load table,
two key clones — not a scan. Caveat recorded honestly: these runs landed in
a noisy window (a pinned A/B of the untouched cache-hit path swung ±30%
against itself, old code indistinguishable from new at p = 0.15), so the
deltas' *direction* is structural but the exact figures are soft; the
numbers that count are the RB5009's in p1-11. Gates after the change: fmt
clean, clippy `-D warnings` clean, 371 tests / 0 failures.
