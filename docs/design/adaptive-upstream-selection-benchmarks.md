# Adaptive upstream selection — benchmark specification

Companion to [adaptive-upstream-selection.md](adaptive-upstream-selection.md).
Grouped **by stage**, not by scenario, so Stage 1 can be accepted or rejected
without any Stage 2 or Stage 3 bench existing.

| Suite | Runs when | Decides |
| --- | --- | --- |
| [T](#t-telemetry-extraction) — telemetry extraction | **Running**, sample 1 in | `penalty_failures`; whether Stage 1's problem occurs here at all |
| [S1-M](#s1-m-stage-1-microbenchmarks) — microbench | Stage 1 implemented | Selector and state-update cost — **tier 1 of gate S1-G2** |
| [S1-N](#s1-n-null-ab-noise-baseline) — null A/B | **Before S1-L**, no Stage 1 code needed | The harness noise band; freezes S1-G2 tier 3 |
| [S1-B](#s1-b-stage-1-behavioural) — behavioural | Stage 1 implemented | The win exists (gate S1-G3) |
| [S1-L](#s1-l-stage-1-live-rb5009) — live RB5009 | Stage 1 implemented, S1-N reported | No regression on the real device |
| [S2](#s2-stage-2-gate-benchmarks) | Only if the Stage 2 gate opens | Whether RTT ordering is worth building |
| [S3](#s3-stage-3-gate-benchmarks) | Only if the Stage 3 gate opens | Whether hedging is worth building |

**S2 and S3 do not run to close Stage 1 and are not prerequisites for it.**

## Common methodology

Binding, from [measurement-traps.md](../measurement-traps.md) and
PERFORMANCE.md §Measuring reliably:

- **A control arm in every session.** Here the `cache_hit` and `block` stages,
  which no part of this design touches. A caveat on a weak comparison is not a
  substitute for a control arm.
- **A/B against a real pre-change checkout**, never criterion's stored baseline.
  Criterion's `change:` line compares against *the previous run, whatever that
  was*. `rm -rf target/criterion` when establishing a baseline.
- **Pin CPU-bound microbenches** to one core; pin throughput benches to four,
  matching the RB5009. **Do not pin** any bench hosting client, mock upstream
  and pipeline in one multi-threaded runtime — a 32.6 µs arm once read
  `[423 µs 7.49 ms 16.1 ms]` that way.
- **Convert dev-box figures with the measured ~9× x86 → RB5009 factor.** Never
  with `cpu-frequency` or `scaling_cur_freq`.
- Trust a delta only when its interval is narrow relative to the change it
  reports.
- Quote **absolutes** when comparing variants.
- Mock latencies are anchored to the one measured figure available — a ~2.4 ms
  ICMP path floor — plus resolver processing, giving a 5 ms median. That anchor
  is a floor, not a distribution; state it wherever a mock latency is quoted.

### Mock-upstream harness

`crates/fah-dns/tests/support/mock_upstream.rs`. Live public resolvers cannot be
made to fail on cue and cannot be blamed when they do.

| Knob | Purpose |
| --- | --- |
| `latency: Distribution` | fixed, uniform or lognormal delay before answering |
| `loss: f32` | fraction of requests silently dropped |
| `refuse: bool` | close the socket so the kernel returns `ECONNREFUSED` |
| `unreachable: bool` | never bind, so the address is dead |
| `rcode: ResponseCode` | answer SERVFAIL/REFUSED while staying transport-healthy |
| `script: Vec<Phase>` | timed transitions — recovery needs "dead for 60 s, then alive" |

Binds `127.0.0.1:0` or `[::1]:0`, so a dual-stack pair exists on the dev box
when a later stage needs one.

---

## T. Telemetry extraction

**No code. Runs before Stage 1 is written.** This is the cheapest suite here and
it can reject Stage 1 outright.

### T — workload

Read `GET /api/v1/telemetry` from the **currently deployed build** on a cadence
that captures per-endpoint counters over the observation window. Household
traffic only; no synthetic load — synthetic queries swamp the household sample
(a real 51.8 % block rate once read as 0.242 % for exactly this reason).

### T — metrics

| Metric | Derived from |
| --- | --- |
| Per-endpoint transport-failure base rate | `failures / attempts`, differenced between samples |
| Distribution of failure *runs* | `failure_runs` per endpoint (p2.5-06), differenced between samples: bucket `[len 1, len 2, len 3, len >= 4]` of runs **closed** in the interval. Cumulative and monotonic, so no cadence can miss a run |
| **Partial-failure intervals** | Any interval where one endpoint's `failures` rose while another's did not, and queries still succeeded |
| Total-failure intervals | Intervals where every endpoint's `failures` rose together |

### T — execution

- Sampler: `suite-t-sample.sh`, read-only `GET /api/v1/telemetry`, appends one
  JSONL row per run. The API key is read from `.vscode/settings.json` where it
  already lives and is never copied into the script, the task definition or the
  series.
- Cadence **10 min**, window **14 days**. At the observed ~0.65 upstream
  failures/hour that is ~0.1 failures per interval, keeping individual events
  separable in the differenced series. 14 days yields ~200 failure events —
  enough to see whether any clustered. A window under ~7 days risks concluding
  "no sustained failures" from a window that simply did not contain one.

### T — sample 1, 2026-08-16 (v0.2.16, 45.7 h uptime)

| Endpoint | attempts | failures | rate |
| --- | --- | --- | --- |
| `1.1.1.1` | 41,710 | 30 | 0.072 % |
| `9.9.9.9` | 30 | 3 | — |

- 27 partial-failure events, 3 total-failure events.
- `9.9.9.9` attempts (30) = `1.1.1.1` failures (30) exactly — ordered fallback
  confirmed; the secondary served 27 queries in 45.7 h.
- Attempt attribution: SWR 69.1 %, `resolve_host` 20.1 %, client forwards
  10.3 %, list bootstrap ~0.6 %.
- `latency.dns.forward` mean ≈ 23.9 ms. **Context only** — an aggregate over
  cache misses including cold recursive lookups, no per-endpoint attribution.
  Not usable for any Stage 2 or Stage 3 constant.

Full interpretation in the design document under *Observed failure data*.

### T — acceptance

Not pass/fail. The output feeds gates S1-G4 and S1-G5 directly.

### T — what justifies proceeding

A non-zero count of **partial-failure intervals** — **already satisfied** by
sample 1 — *and* evidence that at least some failures arrive in runs of ≥ 2.
The second half is still open, and is now read straight off `failure_runs`
buckets 2-4 rather than inferred.

### T — what justifies rejecting Stage 1

Two routes, one now closed:

1. ~~Zero partial-failure intervals over the window~~ — **closed.** Sample 1
   records 27.
2. **Open and live:** partial failures occur but are **all isolated single
   losses**, with no runs of ≥ 2. Then `penalty_failures = 2` never engages, and
   the alternative — lowering it to 1 — would penalize a healthy endpoint ~14
   times a day at the measured rate. Not shipping is the correct call in that
   case.

Note the asymmetry: total outages are *also* improved slightly (a penalized
endpoint is probed rather than dialled every query), but the client is on
serve-stale either way, so the gain is not worth the code on its own.

### T — instrumentation limit, and what closed it

Builds before p2.5-06 could not report run length: `consecutive_failures` is
reset by the next success, and at an 800 ms attempt timeout no cadence catches a
run mid-flight — the differenced series could only **bound** clustering.

p2.5-06 adds `upstreams[].failure_runs`, so the distribution is read directly
and the sampling cadence no longer matters for it. The window for this metric
starts at the deploy of the first build carrying it; earlier samples have no
`failure_runs` field (and perf rows read back as four zeros). Two residual
limits the analysis must state:

- Only **closed** runs are counted — a run in progress at sample time, or open
  when the process exits, is absent.
- Concurrent in-flight queries can split one outage into two shorter runs,
  biasing the distribution toward short runs. That bias works *against*
  `penalty_failures = 2`, so a distribution dominated by runs of ≥ 2 is a
  conservative result.

The `max_consecutive_failures` candidate in the design document is superseded
and not needed.

---

## S1-M. Stage 1 microbenchmarks

`crates/fah-dns/benches/upstream_select.rs`, criterion, `harness = false`,
pinned to one core. Pure functions over a synthetic `Arc<[Health]>` — no
sockets, no runtime, no RTT state (none exists in Stage 1).

### S1-M — workload

| Bench | Arm |
| --- | --- |
| M.1 `select/2_healthy` | 2 endpoints, both Healthy — the shipped default config, and the steady state |
| M.2 `select/8_healthy` | 8 endpoints, all Healthy — early exit on the first |
| M.3 `select/8_first_7_penalized` | Worst case: scan all 8 before finding the Healthy one |
| M.4 `select/8_all_penalized` | The S1.3 invariant path — earliest-deadline scan |
| M.5 `update/success` | Success outcome: relaxed load, compare, early return when already Healthy with `consecutive_failures == 0` |
| M.6 `transition/penalize` | `compare_exchange_weak` Healthy → Penalized |
| M.7 `transition/claim_probe` | Contended `Penalized → Probing` claim, 4 threads, exactly one winner |
| **Control** | `select/noop` — reads the same memory, returns index 0 |

### S1-M — configuration

None. Stage 1 has one tunable (`penalty_failures`) and it does not affect these
paths' cost.

### S1-M — metrics

Wall-clock per operation. For M.7, additionally: winner count must be exactly 1.

### S1-M — acceptance

| Bench | Criterion |
| --- | --- |
| M.1 | **Not slower than the current `fallback` loop's per-query selection overhead.** Stage 1 does strictly less work — one relaxed load versus entering the transport — so anything slower is a defect, not a trade |
| M.2, M.3, M.4 | < 111 ns x86 (< 1 µs RB5009-equivalent, 0.1 % of the 1 ms `forward` engine-overhead budget) |
| M.5 | < 22 ns x86 (< 200 ns RB5009-equivalent) |
| M.6, M.7 | Cold paths; no budget beyond "does not spin" — M.7 must not show a CAS loop retrying unboundedly |
| Control | Unchanged vs the pre-change checkout within ±2 %, or the session is discarded |

### S1-M — what justifies proceeding

M.1 at or below the `fallback` equivalent, and M.3 under the RB5009-equivalent
1 µs budget. Then selection is free and the design needs no simplification.

### S1-M — what justifies rejecting or simplifying

- **M.3 over 1 µs RB5009-equivalent** → the packed-word decode is too expensive.
  Split `state` into a separate `AtomicU8` read first, so the scan skips
  penalized endpoints without decoding the deadline.
- **M.5 over its budget** → the success path is doing more than an early-exit
  compare. It must not write when the state word is already correct.
- **M.7 spinning** → the claim is contended more than expected; make the probe
  claim advisory (lost races simply do not probe) rather than a retry loop.

---

## S1-N. Null A/B noise baseline

**Runs before S1-L, and needs no Stage 1 code.** Its only output is the number
that lets gate S1-G2 tier 3 be frozen. Without it that gate carries no threshold.

### S1-N — workload

The S1-L L.1 workload, run K ≥ 6 times against **one binary and one config** —
the current `fallback` build. The arms are labels only; there is no code
difference between them. Labels alternate A/B/A/B… so monotone drift (thermal,
cache warming, production-traffic variation) splits across both labels rather
than loading one.

Identical duration, corpus, query rate and cold-start delta per repetition.

### S1-N — configuration

Probe container on the RB5009, **not the dev box** — the gate applies to the
device, and production traffic variation is part of the noise floor the gate
will actually face. Production container untouched.

### S1-N — metrics

- `forward` p50 and p99 per repetition.
- The `cache_hit` control arm per repetition.
- Sustained QPS per repetition.
- **Total upstream attempts and attempts-per-forward** per repetition — candidate
  replacement metrics, since `forward` p99 observes only the ~10 % of upstream
  attempts that are client forwards (suite T sample 1).
- SWR attempts and outcomes per repetition — the ~69 % the client metric misses.

### S1-N — output

| Quantity | Definition |
| --- | --- |
| **N** | Half-width of the interval containing 95 % of the pairwise A−B deltas on the chosen metric |
| Per-metric N | Computed for `forward` p99, attempts-per-forward and total attempts, so the quietest adequate metric can carry the gate |
| Control-arm drift | Across the same runs. If `cache_hit` moves more than `forward`, the box drifted and the session is void |

### S1-N — freezing rule

Threshold = **max(2 × N, 5 %)** on whichever metric is chosen. The 2× keeps a
single excursion from failing the gate; the 5 % floor exists because a quieter
arm on this project already drifted 4.6 %, so claiming finer resolution on a
noisier one would not be credible.

### S1-N — what justifies proceeding

An N small enough that the resulting threshold would catch a regression worth
catching. Since Stage 1's healthy path is provably one relaxed load and zero
allocations, the live gate only needs to catch a *gross* regression — precision
lives in S1-M.

### S1-N — what justifies dropping the live timing gate

- **N above ~10 % on every candidate metric** → the harness cannot resolve
  anything useful. Drop tier 3, rest the healthy-path claim on S1-M and the
  tier 2 invariants, and say so explicitly in the result. This is an honest
  outcome, not a failure.
- Precision improves as √K while cost is linear in K. If K = 12 still leaves N
  wide, the answer is to drop the gate, **not** to run more repetitions.

## S1-B. Stage 1 behavioural

The suite that shows the win. Dev box, mock upstreams, tokio test runtime, not
pinned.

Every arm is run twice in the same session: `strategy = "fallback"` and
`strategy = "adaptive"`.

### S1-B — workload

| Arm | Setup |
| --- | --- |
| B.1 Black hole | Endpoint A bound but silent; endpoint B healthy at 5 ms. 10 000 forwarded queries, distinct names |
| B.2 ICMP unreachable | Endpoint A never bound (`ECONNREFUSED`/`EHOSTUNREACH`); B healthy at 5 ms |
| B.3 Healthy control | Both endpoints healthy at 5 ms — must show no difference between arms |
| B.4 All dead | Both endpoints black-holed. Verifies the S1.3 invariant: queries are still sent |
| B.5 Recovery | Scripted: A healthy 2 min → black hole 10 min → healthy 15 min → flapping 30 s up / 30 s down for 3 min. 5 QPS. `PENALTY_MAX` swept {60 s, 300 s, 900 s} |
| B.6 RCODE isolation | A answers SERVFAIL to everything, transport-healthy. 1 000 queries |
| B.7 `resolve_host` isolation | Single-stack mock: A answers, AAAA silent. 100 `resolve_host` calls, no client queries |
| B.8 SWR interaction | Stale entries expiring against a black-holed endpoint, `swr_workers = 3` |

### S1-B — configuration

`timeout_ms = 800` (shipped default), `penalty_failures` swept {1, 2, 3}.

### S1-B — metrics

- Client-visible p50/p99, and the first 10 queries reported separately.
- Count of queries that paid a full `timeout_ms`.
- Time-to-penalize, in queries and wall clock.
- `state`, `penalty_round`, `penalties`, `probes`, `probe_successes`,
  `penalized_seconds_total` over time.
- B.4: count of queries for which no packet was sent — **must be 0**.
- B.6: `state` and `consecutive_failures` — **must be unchanged**.
- B.7: every counter and `state` — **must be unchanged**.
- B.8: probes claimed by SWR workers versus by client queries; dropped SWR jobs.

### S1-B — acceptance

| Arm | Criterion |
| --- | --- |
| B.1 `fallback` | ~10 000 × 800 ms — establishes the tax being removed |
| B.1 `adaptive` | ≤ `penalty_failures` queries pay 800 ms; the remainder answer at ~5 ms |
| B.2 `adaptive` | ≤ 1 query pays anything |
| B.3 | Arms within noise of each other; no penalty ever applied |
| B.4 | Zero queries with no packet sent; both endpoints probed at their deadlines |
| B.5 | Healthy again within `PENALTY_MAX` + one query interval; never more than one probe in flight; flapping phase shows growing backoff and no probe storm; `penalty_round` does **not** reset during the 15 min healthy phase at `PENALTY_MAX = 300 s` |
| B.6 | Zero state change, zero counter movement beyond `attempts` |
| B.7 | Zero state change, zero counter movement, including `attempts` |
| B.8 | SWR-claimed probes > 0; dropped jobs bounded by queue depth; no unbounded refresh retry |

### S1-B — what justifies implementing

B.1 and B.2 showing the 800 ms tax removed from ≥ 99.9 % of queries once the
endpoint is penalized, **with B.3 flat**. Against the measured 5 ms healthy
path that is a ~160× p99 improvement on the failure scenario at zero cost on the
healthy one.

### S1-B — what justifies rejecting or simplifying

- **B.3 showing any difference between arms** → Stage 1 is not free on a healthy
  link. Fix or reject; there is no acceptable trade here, because the healthy
  link is ~100 % of normal operation.
- **B.6 or B.7 showing any state change** → the classification or the isolation
  is wrong. Both are correctness failures (gates S1-G1.5 and S1-G1.7), not
  tuning findings.
- **B.4 showing a query with no packet sent** → the S1.3 invariant is violated.
  Hard stop.
- **B.2 taking more than 1 query** → path-failure classification is not firing;
  ICMP errors are being seen as ordinary timeouts.
- **B.5 recovery consistently taking the full `PENALTY_MAX` under a household
  traffic pattern with idle gaps** → on-path probing does not recover promptly
  enough without traffic to carry the probe. Reconsider the background prober,
  accepting the extra task and idle traffic.
- **`PENALTY_MAX = 60 s` performing as well as 300 s with no extra probe
  traffic** → use 60 s and recover five times faster.

---

## S1-L. Stage 1 live RB5009

On-device, in a **probe container**
([routeros-traps.md](../routeros-traps.md) §On-device measurement needs its own
container). The production container is not stopped, not reconfigured, not
restarted. No router change: failure arms are produced by pointing a
`[[dns.upstreams.servers]]` entry at an address nothing answers on, in the probe
container's own config.

### S1-L — workload

| Arm | Description |
| --- | --- |
| L.1 | 10 000 QPS synthetic forward load, 2 healthy mock upstreams on the probe host, 10 min, `adaptive` |
| L.2 | Same, `fallback`, same session |
| L.3 | 24 h household-traffic soak, real upstreams, `adaptive` |
| L.4 | Failure injection against real upstreams: one entry pointed at a dead address, 1 h |
| **Control** | `cache_hit`-only load (all names pre-warmed) in both L.1 and L.2 |

### S1-L — configuration

Probe container per routeros-traps.md. `penalty_failures` at whatever value T
produced.

### S1-L — metrics

- `forward`-stage p50/p99 from `/api/v1/telemetry`. The stage partition is
  binding: `forward` is misses plus the RFC 8767 fallback, nothing else.
- Sustained QPS against the ≥ 10 000 budget.
- `process_rss` from `/debug/memory`; slope over the final third of L.3.
- All Stage 1 telemetry fields (S1.12).
- L.3: false-penalty count per healthy endpoint.

### S1-L — acceptance

| Metric | Criterion |
| --- | --- |
| L.1 vs L.2, chosen timing metric | Within the threshold **S1-N derived**. No number until S1-N has run |
| Tier 2 invariants (design S1-G2) | Counts identical between arms — no threshold needed |
| L.1 sustained QPS | ≥ 10 000, no worse than L.2 |
| Control arm | Moves less than the measured arm, or the session is discarded |
| L.3 RSS slope, final third | < 2 MB drift (mimalloc's purge band alone is ±6 MB) |
| L.3 state memory | constant — `penalties` may grow, state size may not |
| L.3 false penalties | **Measured and reported. No pre-set threshold** — none is currently justified by evidence, and imposing one would smuggle the S1.6 hypothesis back in as a criterion |
| L.4 | Matches S1-B's B.1/B.2 shape on real hardware and real upstreams |

### S1-L — what justifies implementing

L.1 within the S1-N-derived threshold of L.2 with a flat control arm, every
tier 2 invariant intact, plus L.4 reproducing the behavioural win on the real
device. That is "costs nothing when nothing is wrong, and pays when something
is".

### S1-L — what justifies rejecting or simplifying

- **L.1 worse than L.2 beyond the S1-N-derived threshold, with a flat control
  arm** → Stage 1 is not free on the real device.
- **Any tier 2 invariant broken** → a behavioural regression, which no timing
  threshold can excuse.
- **L.3 false-penalty count high enough that a healthy endpoint spends a
  meaningful fraction of the day Penalized** → raise `penalty_failures` and
  re-run. What "meaningful" is must be decided *from* the L.3 number, not before
  it.
- **L.3 RSS slope above the noise band** → state is not bounded as claimed;
  re-check the `Health` array and the counter set.

---

## S2. Stage 2 gate benchmarks

**Do not run these to close Stage 1.** They exist only if the Stage 2 gate opens
— that is, if Stage 1 telemetry shows a durable DNS-level RTT separation between
healthy endpoints, sustained over hours.

Required arms, sketched only:

| Arm | Question |
| --- | --- |
| Healthy dual-stack, matched latencies | Does banding suppress primary switching below a rate agreed before the run? |
| Healthy dual-stack, latencies separated by a stated margin | Does the same band width still resolve it? |
| Unmeasured-endpoint behaviour | Does an endpoint that is never selected stay permanently unmeasured, and does that matter? |
| Mixed-family config with the preferred family configured second | Does the first query still follow config order? |
| Live four-resolver dual-stack on the RB5009 | Does any durable separation exist at all on this link? |

**Rejection is the expected outcome and must be easy to reach.** If no durable
separation exists, or no band width satisfies both of the first two arms, Stage 2
reduces to a static family-preference ordering rule with no RTT machinery — and
the benchmark must be able to say so.

No thresholds are stated here. Every candidate figure that appeared in earlier
drafts — switch rate, band width, separation margin — was invented, and none may
be reintroduced without evidence from Stage 1 telemetry.

## S3. Stage 3 gate benchmarks

**Do not run these to close Stage 1.** They exist only if all four Stage 3 gate
conditions hold.

Prerequisite, before any bench: a test proving that dropping a hickory
`send(...).first_answer()` future removes the multiplexer's in-flight entry. A
leak here is unbounded and vetoes Stage 3 regardless of any latency result.

Required arms, sketched only:

| Arm | Question |
| --- | --- |
| Slow-but-answering endpoint | Does client p99 with hedging on beat hedging off, same workload, same session? |
| Jittery-but-not-slow endpoint | Same question where the median gives no reason to switch — the only case Stage 1 and Stage 2 both miss |
| Amplification | Upstream queries per client query, reported **cache-hit-adjusted** |
| Budget binding | Does the token bucket actually cap amplification when the tail is sustained? |
| Answer stability | Do two resolvers return different answers for the same name, and how often does the winner alternate? |

**The decision metric is client p99 on/off, not `hedges_won / hedges_started`.**
A hedge that wins by 0.5 ms is a win worth nothing. The true quantity — primary
latency minus hedge latency — is unmeasurable, because the primary's future is
dropped. `hedges_won / hedges_started` is a diagnostic for whether the mechanism
fires at all.

No thresholds are stated here. The margin that would justify shipping must be
agreed **before** the run, from Stage 1's observed latency distribution.

---

## Reporting

One file per suite in `docs/code-review/`, per the project convention: corpus,
workload and device on every figure; tables not prose; the control arm reported
alongside every arm. A result that omits the control arm is not a result.

Suite T is reportable today and blocks nothing else.
