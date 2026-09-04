# PERFORMANCE

Performance is the primary feature. This document is the contract: golden rules
every change must respect, and numeric budgets `cargo bench` verifies.

**Measured detail lives in `docs/code-review/`** — one file per task, carrying the
corpus, workload and device each figure came from. This file holds the targets
and the traps, not the archive.

## Golden rules

1. **Zero-copy where possible** — reference buffers, don't copy them.
2. **Streaming before buffering** — process incrementally; never load whole
   documents into memory.
3. **No runtime regex compilation** — and no regex on the hot path at all. Rules
   compile to hash/trie matchers at load time. SIMD `memchr` is the one
   single-byte primitive this leaves open.
4. **No GC, no hidden allocations** — the hot path is allocation-free;
   allocations happen at load/reload time and are served by **mimalloc**
   (`crates/fastadhunter/src/allocator.rs`), a deliberate choice on a static-musl
   artefact rather than a consequence of the target libc.
5. **No global locks** — atomic swap for ruleset/config, sharding for the cache,
   bounded channels between components.
6. **Cache-friendly layouts** — compact contiguous structures; pointer-chasing is
   the enemy on the RB5009.
7. **Bounded everything** — cache, ring buffers, channels, retention. Memory must
   not grow with traffic or uptime.
8. **Deterministic execution** — predictable latency beats occasional brilliance;
   avoid work with unbounded tails on the query path.
9. **Every feature justifies its runtime cost** — a change that touches the hot
   path states its cost.

## Budgets

Reference hardware: MikroTik RB5009 — Marvell Armada quad-core ARMv8, nominally
1.4 GHz, 1 GB RAM shared with RouterOS. Verified by criterion benches in
`benches/` and soaks on the device.

Budgets are written in decimal MB. On-device readings are reported in MiB, which
is ~4.9 % lower for the same bytes — compare like with like before declaring a
breach.

| Metric | Budget | Measured on the RB5009 |
|--------|--------|------------------------|
| RAM steady-state | ≤ 128 MB | 46.6–53.6 MiB (`dns+http`); `dns+http+https` TBD — must be measured during verification |
| RAM hard ceiling (container limit) | 256 MB | — |
| Compiled ruleset, 1M domains | ≤ 40 MB | 25.8 MiB at 799 k rules |
| Startup to serving, cached lists | < 3 s hard, ~1 s goal | 2.85 s at 1.15 M parsed |
| Container image size | ≤ 30 MB | 13.0 MiB |
| Blocked query, in-engine p99 | < 1 ms | — |
| `cache_hit` stage, in-engine p99 | < 1 ms | — |
| `forward` stage overhead added by engine, p99 | < 1 ms | not measured — see note under the table |
| **DNS** sustained throughput | ≥ 10 000 QPS | 20 k+ QPS |
| **HTTP** added latency, head path | < 1 ms | +161 µs min · +344 µs p50 |
| **HTTP** throughput, opaque body | ≥ 100 MiB/s | 271 MiB/s min · 208 MiB/s p50 at 1 MiB |
| **HTTP** request verdict (URL tier), p99 | < 1 ms | 569.5 µs at 8 KiB, EasyList + EasyPrivacy |
| **HTTP** concurrent connections | bounded by `[http] max_connections` (1024) | unmeasured |
| **HTTPS** SNI verdict + splice, added latency per connection | TBD — set from the RB5009 (loopback figures do not convert) | TBD — must be measured during verification |
| **HTTPS** splice throughput, steady state | ≥ 100 MiB/s (gigabit LAN is 119 MiB/s) | TBD — must be measured during verification |
| **HTTPS** interception handshake overhead vs splice | intercepted p50 ≤ 2 × spliced p50 | TBD — must be measured during verification |
| **HTTPS** intercepted h2 relay | ≥ 50 MiB/s | TBD — must be measured during verification |
| **HTTPS** minted-leaf cache hit rate, browsing load | ≥ 90 % (a real-session replay decides) | TBD — soak `https-sni` feed |
| **DoT** / **DoH** added latency vs UDP, p50 | TBD — set from the RB5009 (TLS/HTTP legs do not convert) | TBD — must be measured during verification |
| Cold `prewarm` per first-sight host (whole path incl. eviction scan, not raw keygen) | < 1 ms | **450.88 µs** — `certs_mint` on the device, criterion, 2026-09-04; 8.4× the dev box's 53.64 µs, so the ~9× factor holds. P5's end-to-end DoT increment misses the same budget at 1.389 ms: it tracks the CPU speed regime, not the mint, and is a recorded budget miss rather than a defect ([p3-06-testing-results.md](docs/code-review/phase3/p3-06-testing-results.md) §P5, §P5-regime) |
| CA generate / API-pair import wall time | < 100 ms / < 50 ms | **4.937 ms / 6.015 ms** — both inside, generate at ~5 % of its allowance (P6, 2026-09-04, median of 5 on `time_starttransfer − time_appconnect`, the client-observed request-processing time excluding the TLS handshake) |

In-engine latency excludes upstream RTT — we measure what we add. The served
`forward` histogram (`duration_forward`; `forward_p50/p99` on `/history/perf`)
is the exception: it is timed end-to-end, upstream round trip and the RFC 8767
failed attempt included, so the `forward` overhead row has no measured
counterpart until a timer isolates the engine's share around the upstream
await. The dashboard's forward tile carries no budget for that reason. Budgets are
compared against `main` on every perf-relevant change; a >10 % regression on a
hot-path bench needs an explicit justification ([CONTRIBUTING.md](CONTRIBUTING.md)).

Per-endpoint upstream RTT (p5-11) instruments the forward path itself: one
`Instant` pair plus three relaxed RMWs per **answered** attempt, measured at
~58 ns on the dev box (~0.5 µs at the 9× factor) with no A/B regression on
the forward or pipeline benches — corpus, trees and the mock-forwarder trap
in [docs/code-review/phase5/p5-11-upstream-rtt-review.md](docs/code-review/phase5/p5-11-upstream-rtt-review.md)
§Measurements. It carries no budget: the figure it serves is network time.

Nothing enforces a budget at runtime: the container runs `memory-high=unlimited`,
so exceeding one is a budget breach, not a failure.

### What the latency stages hold

The three stages partition every resolved query, and a figure is meaningless
without knowing which one it came from:

- `block` — verdict only, no cache, no network.
- `cache_hit` — every serve answered without waiting on the network, SWR stale
  serves included.
- `forward` — cache misses plus the RFC 8767 outage fallback
  (`StaleServe::AfterForwardFailure`, which carries the upstream timeout), and
  nothing else.

Figures do not cross the 0.2.13 boundary; earlier `forward` numbers are pooled
with cache reads and unusable
([`0.2.13-stale-serve-metrics.md`](docs/code-review/phase2/0.2.13-stale-serve-metrics.md)).

### Reading a memory figure

- **The `≤ 128 MB` row is steady-state.** A compile peak is a transient and is
  judged against the 256 MB ceiling.
- **The compile peak is a ratchet across compiles, not one compile's cost.** Boot
  costs ~118–122 MiB; the second and later compiles in a process climb toward a
  saturation point (181.4 MiB at `MIMALLOC_PURGE_DELAY=0`, 230.7 MiB at 100). Any
  single `process_peak_rss` reading is meaningless without knowing how many
  compiles preceded it.
- **Dead memory is returned on the instant at `PURGE_DELAY=0`.** A sawtooth that
  ratchets *across* compiles is the arena filling; one that never returns is a
  leak. `PURGE_DELAY=0` is unproven above ~0.5 qps — the check is
  `memory.minor_page_faults` climbing at flat RSS.
- **Freed memory goes back to mimalloc, not necessarily to the kernel**, so RSS
  lags `cache_estimated_bytes` (CONTEXT.md §Accounted/Residual).
- Reducing the refresh transient is
  [`p2-12`](plan/wip/phase2/p2-12-compile-transient-structural.md);
  the structural decomposition is
  [`p2-11`](docs/code-review/phase2/p2-11-compile-transient.md).

### Converting dev-box numbers

**Budget from the factor, never from the reported clock.** The RB5009 governor
boosts between 350 MHz and 1400 MHz, and the reported frequency does not predict
throughput — measured three times, most tightly in p2-08, where four runs
reporting 350, 700, 1400 and 700 MHz agreed within 6 %. Sample
`/system/resource/print` during a run to document conditions, never to scale a
result.

The **~9× x86 → RB5009 factor** is the durable input: 8.25–10.0× across twelve
arms spanning three orders of magnitude and two corpora, median ~9.05, flat
across URL lengths — so the gap is CPU throughput, not memory bandwidth, and a
pinned dev-box bench usually answers the on-device question without building a
probe container.

**It converts CPU-bound work only.** p2-08's HTTP arms — syscall- and copy-bound,
across two OS network stacks — came out **4.55–10.09×**. Anything dominated by
socket I/O needs a probe container, not a conversion
([p2-08](docs/code-review/phase2/p2-08-review.md) §Findings). TLS handshake,
splice and interception figures do not convert either (p3-06): the dev box
resolves them only as diagnostics, and the Phase 3 budget rows fill from the
RB5009 runbook — dev-box figures, corpus and pinning per row in
[docs/code-review/phase3/p3-06-phase3-verification-review.md](docs/code-review/phase3/p3-06-phase3-verification-review.md)
§Measurements and §Post-review work C.

Whether all-cores load behaves differently is **untested**.

## Design costs worth knowing

Each of these is a hot-path or memory trade already paid; the review file holds
the full measurement.

- **Rule deduplication saves ~30 bytes per duplicate and shortens probe chains.**
  A domain carried by two lists occupies one slot instead of two that hash to the
  same place — **−46 %** on shared-domain lookups, −22 % on single-list hits. It
  costs compile time only, and only where there is nothing to collapse: 1 M
  *unique* rules move the build phase 41.6 → 93.3 ms, ~19 % of a whole compile,
  paid once per compile and never per query. Two non-overlapping 1 M lists
  compile to 57.7 MiB, **past the 40 MB budget** (p1.5-05).
- **Per-client policy resolution costs +8.5 ns per query, and a deployment with
  no policies pays it too.** Schedules are evaluated on a 20 s tick and swapped
  atomically, so the query path does no time arithmetic and no name lookup. 15
  assignments walked to the end add a further 10.7 ns. Documented rather than
  claimed as zero (p2-06).
- **All policies share one compiled ruleset** behind a 16-bit per-rule visibility
  mask: **+2.03 MiB flat** at deployed scale instead of ~+17 MiB per policy
  (p2-05).
- **A blocked HTTP request costs 48–55 % less than a forwarded one** — the verdict
  is taken on the head, so it never resolves and never opens an upstream
  connection. This is not a claim that blocking makes the router faster in
  absolute terms; load still rises with traffic.
- **The DNS cache is bounded twice** — `max_entries` and `max_bytes` (default
  64 MiB), both enforced by the same O(1) amortized FIFO eviction, which runs
  until *both* hold. Entry count alone did not bound memory: an adversarial
  large-answer mix plateaued at ~230 MiB, 80 % over budget (p1.5-05).
- **A stale cache hit is answered from cache, and the refresh runs on a fixed
  pool** of `[dns.cache] swr_workers` detached tasks — golden rule 8 applied to
  the one cache state with an unbounded tail on the query path (ADR-0005). The
  pool never back-pressures: enqueue is `try_send`, and a full queue drops the
  refresh rather than delaying a client. Sustained growth in
  `counters.swr.dropped` means the pool is undersized, not that anything failed.
  Cost: one `Option<Instant>` per `Entry` (~16 B), charged per *bucket* — ~262 KB
  at 10 000 entries, ~2.6 MB at 100 k. Expect `cache_hit` to rise and forwards to
  fall; that is queries moving between buckets, not the cache becoming more
  efficient.
- **The expiry sweep runs on the blocking pool**, one shard lock at a time, every
  `[dns.cache] cleanup_interval_seconds` (default 360). `clean` is synchronous and
  O(entries) — exactly the unbounded tail golden rule 8 keeps off the query path.
  It is **not** a bound; `max_entries`/`max_bytes` are, and they hold with the
  sweep disabled. It costs 79 µs for a full 16-shard walk at 409 entries, ~19 ms
  of CPU per day. At default settings it usually reclaims nothing, because
  `serve_stale = true` makes an entry sweepable only 24 h past TTL — read
  `counters.cache_cleanup.bytes_freed` near zero as normal.
- **Inspected content (HTML) is budgeted separately and does not exist yet.**
  Phase 4 rewrites HTML through `lol_html`; the HTTP rows above are the opaque
  path and must not be read as covering it.
- 10 k QPS is ~100× a busy household's peak. The headroom is the proof of
  efficiency, and it is what keeps p99 flat at real loads.

## Measuring reliably

The hot-path benches resolve sub-microsecond work, below the noise floor of a
loaded desktop. An unpinned `cargo bench` on a busy dev machine has been observed
swinging **6×** between consecutive runs of an unmodified binary — enough to
manufacture a "+159 % regression" that does not exist. Before believing any
regression, re-measure pinned to one core:

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

Two pinning rules, by what the bench measures. **CPU-bound microbenches**
(matcher, cache, pipeline, certs) run on **one core** (`ProcessorAffinity = 4`,
`taskset -c 2`) — that is where the sub-1 % intervals come from.
**Throughput and socket-bound benches** (`fah-http/benches/*`: pass-through,
opaque body, splice, handshake, h2) run on **four distinct physical cores with
the runtime sized to them**: `TOKIO_WORKER_THREADS=4` plus an affinity mask
that names one logical CPU per physical core — on the dev box's i9-13980HX
that is `ProcessorAffinity = 0x55` (CPUs 0, 2, 4, 6; `taskset -c 0,2,4,6`).
Two measured traps (p3-06 F8, 2026-09-02): a mask alone leaves tokio spawning
one worker per *machine* CPU inside the mask, and `ProcessorAffinity = 15` on a
hyper-threaded part is two physical cores, not four — together they turned
`http_pass_through/direct_to_origin` from 31.7 µs ± 0.6 % into 70 µs ± 20 %.
Done right, the pinned means match the unpinned ones with ~5× tighter
intervals ([docs/measurement-traps.md](docs/measurement-traps.md) §Calibration).

**Building the `fastadhunter` bench target.** `cargo bench -p fastadhunter`
does not compile as is: the crate's dev-dependency on `fah-api` carries
`test-harness`, cargo unifies it into the bench profile, and `fah-api`'s
`compile_error!` refuses that feature outside `debug_assertions` (p3-06 review
X2, a p5-04 leftover). Until a follow-up moves the feature off the bench
target, build it with
`CARGO_PROFILE_BENCH_DEBUG_ASSERTIONS=true cargo bench --no-run -p fastadhunter --bench pipeline`.
Every recorded `full_pipeline` A/B (p3-06, S3) used that override on **both**
arms, so the comparison is fair, but its absolutes are not shipped codegen
and must not become a budget row.

Pinned, the same benches hold a confidence interval under 1 %. Trust a criterion
delta only when its interval is narrow relative to the change it reports:
`[366.0 ns 366.8 ns 367.5 ns]` is a measurement, `[737 ns 882 ns 1.04 µs]` is
noise wearing a number's clothes.

Four traps, each of which has already produced a wrong number:

- **Criterion's `change:` line compares against the *previous run*, whatever that
  was** — a different corpus, or another session. Quote **absolutes** when
  comparing variants, A/B against a real pre-change checkout, and
  `rm -rf target/criterion` when establishing a baseline.
- **The default corpus can hide the regression the real one shows.**
  `crates/fah-rules/benches/url_matcher.rs` falls back to a synthetic
  EasyList-shaped corpus that compiles **zero** unindexed rules. Set
  `FAH_URL_CORPUS` before believing a URL-tier number.
- **Subtract the harness's own cost before attributing a stage.** A per-stage
  profile put header stripping at 1.34 µs; the setup alone (`HeaderMap::clone`)
  was 1.28 µs of it. The real figure was ~170 ns.
- **Pinning is for CPU-bound microbenches only.** It must not be applied to
  `fah-http/benches/proxy.rs`, which hosts client, proxy and origin in one
  multi-threaded runtime — pinned, a 32.6 µs arm read `[423 µs 7.49 ms 16.1 ms]`.
  Run those unpinned and take the range across several runs.
- **A steady background load produces reproducible wrong numbers, not noisy
  ones.** With a video playing, `matcher_lookup` read −8 % across two mirrored
  passes with CIs under 3 %, on a path the change did not touch; on an idle box
  it is flat. Mirrored ordering and narrow intervals do not detect a confounder
  that is constant across every arm. Bench an idle machine, and treat a delta on
  untouched code as proof the session is invalid
  ([p1-01](docs/code-review/phase1/p1-01-review.md) §Two wrong numbers).
- **Code placement alone moves this suite by more than most real changes.**
  Adding one never-called `pub fn` to `fah-rules` — identical behaviour — moved
  `startup_phases/3_build_matcher` **+7.9 %**, `blocked_query` +11.7 % and
  `forwarded_query_overhead` +15.0 %, while `2_parse_rule_list` and
  `startup_from_cached_lists` held inside ±1.2 %. Build that third arm before
  believing any single-digit delta on the µs benches or on `3_build_matcher`.

Throughput is the other exception: restrict it to four cores
(`ProcessorAffinity = 15` / `taskset -c 0-3`) so the figure is shaped like the
RB5009's quad-core budget.

A **control arm** — one the change cannot possibly affect, run in the same
session — is the cheapest noise detector available, and the only thing that
separates "the code got faster" from "the box was in a different state".

Put it **inside the crate under test**, not in the harness. A control that lives
in the probe reports only on the machine; it cannot see a codegen or placement
effect in the crate being changed, and it read a reassuring −1.4 % through the
session that produced both wrong numbers above. The control that works for that
is the third build: same commit, one semantically null edit.

### Measuring on the RB5009

The device cannot be benched the way the dev box can: the image is distroless and
RouterOS exposes no `docker exec`, so an on-device measurement ships as **its own
throwaway container** with the measurement as the entrypoint, reporting through
`/log print where topics~"container"`. `Dockerfile.probe` with
`crates/fah-rules/examples/urlbench.rs`, and `Dockerfile.httpprobe` with
`crates/fah-http/examples/httpbench.rs`, are the working examples — note they
bake their corpus in rather than mounting `/data`.

- **Carry a control arm through every on-device comparison.** It is what caught
  the frequency field misleading us.
- **Size the probe to hold a core busy long enough to sample** — tens of seconds,
  not a burst.
- **Read all three estimators, never one alone.** `min` is the intrinsic cost,
  `p50` what a client typically experiences, `p99` the tail. On the HTTP head path
  the added cost at p50 is **2.1× the min** — quoting only `min` claims 6.2×
  headroom where the user sees 2.9×.
- **Percentages against a loopback baseline are the harshest possible reading.**
  The same +344 µs is under 3 % of a request to a real origin at 10–50 ms RTT.

## Positioning

Beat AdGuard Home and Blocky on **both** axes:

- **Efficiency** — their steady-state RAM (roughly 100–200 MB and 50–100 MB
  respectively) is our ceiling territory; our target is below both.
- **Functionality** — streaming HTML rewriting powered by lol_html (Phase 4)
  filters inside pages, which neither does.
