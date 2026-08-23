# Adaptive upstream selection — benchmark specification

Companion to [adaptive-upstream-selection.md](adaptive-upstream-selection.md).
Grouped **by stage**, not by scenario, so Stage 1 can be accepted or rejected
without any Stage 2 or Stage 3 bench existing.

| Suite | Runs when | Decides | Gate tier | Phase 2.6 task |
| --- | --- | --- | --- | --- |
| [T](#t-telemetry-extraction) — telemetry extraction | **Running**, sample 1 in | `penalty_failures`; whether Stage 1's problem occurs here at all | deployment (S1-G4, S1-G5) | `p2.6-11` reads it |
| [S1-M](#s1-m-stage-1-microbenchmarks) — microbench | Stage 1 implemented | Selector and state-update cost — **tier 1 of gate S1-G2** | merge | `p2.6-08` |
| [S1-N](#s1-n-null-ab-noise-baseline) — null A/B | **Before S1-L**, no Stage 1 code needed | The harness noise band; freezes S1-G2 tier 3 | deployment | `p2.6-10` |
| [S1-B](#s1-b-stage-1-behavioural) — behavioural | Stage 1 implemented | The win exists (gate S1-G3) | merge | `p2.6-09` |
| [S1-L](#s1-l-stage-1-live-rb5009) — live RB5009 | Stage 1 implemented, S1-N reported | No regression on the real device (S1-G2 tiers 2–3); the 7-day soak | deployment | `p2.6-11` |
| [S2](#s2-stage-2-gate-benchmarks) | Only if the Stage 2 gate opens | Whether RTT ordering is worth building | — | none |
| [S3](#s3-stage-3-gate-benchmarks) | Only if the Stage 3 gate opens | Whether hedging is worth building | — | none |

The gate tiers are the design document's (§Stage 1 acceptance gates): the
**merge** tier lets `adaptive` ship opt-in; the **deployment** tier lets it
become the default. **S2 and S3 do not run to close Stage 1 and are not
prerequisites for it.**

Terminology, from design S1.6: `timeout_ms` bounds one **leg**;
`attempt_bound_ms = ATTEMPT_LEGS × timeout_ms` (3 × 800 = 2 400 ms at the
default) bounds one **attempt**. A plain-UDP attempt against a black hole costs
one leg. "Paying" below means latency ≥ `timeout_ms`; the measured time paid is
reported beside the bound, never replaced by it.

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

`crates/fah-dns/tests/support/mock_upstream.rs`, created by `p2.6-09` and
shared with bench targets via `#[path]`. Live public resolvers cannot be made
to fail on cue and cannot be blamed when they do. Scaled penalty constants are
injected through the pool's `with_policy` seam (design S1.6: the policy struct
is what makes minute-scale recovery benchable in milliseconds).

| Knob | Purpose |
| --- | --- |
| `latency: Distribution` | fixed, uniform or lognormal delay before answering |
| `loss: f32` | fraction of requests silently dropped |
| `refuse: bool` | bind, record the port, drop the socket — a closed port. The kernel answers `ECONNREFUSED` on Linux and `ECONNRESET` on Windows (design S1.4 platform note) |
| `rcode: ResponseCode` | answer SERVFAIL/REFUSED while staying transport-healthy |
| `script: Vec<Phase>` | timed transitions — recovery needs "dead for 60 s, then alive" |

There is no `unreachable` knob: `ENETUNREACH` / `EHOSTUNREACH` come from
routing and ARP, which loopback cannot produce — "never bind" is just a closed
port, the same thing as `refuse`. The `classify` unit tests cover those kinds
with synthetic `io::Error`s (p2.6-04); real ICMP host-unreachable is exercised
on-device in S1-L L.4b.

The harness is plain UDP. A DoT endpoint on a closed TCP port needs no TLS
mock — the connect is refused before any handshake — so
`UpstreamPool::from_config` with a `dot` entry at a dropped `TcpListener`'s
address is the whole setup (the `closed_tcp_addr` shape in `encrypted.rs`).

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
reset by the next success, and at an 800 ms leg timeout no cadence catches a
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
- The counters are cumulative **per process** and the window spans at least two
  restarts (0.2.16 → 0.2.18 → the 2.6 binary under `fallback`). Read the
  distribution as the sum of per-process deltas from `/history/perf`, never as
  `end − start` across a restart.

The `max_consecutive_failures` candidate in the design document is superseded
and not needed.

**Window boundary once `adaptive` is opt-in.** Under `adaptive`, `failure_runs`
measures throttled attempts, not outage duration (design S1.12): a penalized
endpoint is not attempted, so one outage yields one short run. The S1-G4
distribution is therefore read **only from the `fallback` window** — from the
p2.5-06 deploy to the opt-in flip. Samples after the flip are a different
quantity and must not be pooled with it.

---

## S1-M. Stage 1 microbenchmarks

`crates/fah-dns/benches/upstream_select.rs`, criterion, `harness = false`,
pinned to one core — except M.7, pinned to **four**: four threads racing one
CAS on one core serialize on the scheduler and measure nothing contended. M.7
is a CAS microbench, not the multi-threaded client/mock/pipeline harness the
no-pin rule is about. Pure functions over a synthetic `Arc<[Health]>` — no
sockets, no runtime, no RTT state (none exists in Stage 1).

### S1-M — workload

| Bench | Arm |
| --- | --- |
| M.1 `select/2_healthy` | 2 endpoints, both Healthy — the shipped default config, and the steady state |
| M.2 `select/8_healthy` | 8 endpoints, all Healthy — early exit on the first |
| M.3 `select/8_first_7_penalized` | Worst case: 7 Penalized with future deadlines before the Healthy one — the one pass decodes 7 deadlines and reads the clock closure once (design S1.5, lazy) |
| M.4 `select/8_all_penalized` | The S1.3 invariant path — earliest-deadline scan |
| M.5 `update/success` | Success outcome: relaxed load, compare, early return when already Healthy with `consecutive_failures == 0` |
| M.6 `transition/penalize` | `compare_exchange_weak` Healthy → Penalized |
| M.7 `transition/claim_probe` | Contended `Penalized → Probing` claim, 4 threads calling `select(.., claim = true)`, exactly one winner. The claim is advisory by design (a lost CAS continues the scan, it does not retry — design S1.7, `p2.6-03`), so this measures one claim, not a loop |
| M.8 `forward/udp_answered` | End-to-end `forward` against a loopback UDP mock, both strategies — the existing `crates/fah-dns/benches/upstream.rs` arm with an `adaptive` twin. **This is the `fallback` comparison**: `fallback` has no standalone selector to measure, its "selection" is entering the transport. It is a **gross check**: the arm is syscall-dominated (bind, connect, send, recv — tens of µs) and cannot see a 100 ns selector change; the precision claim is M.1 against the control |
| M.9 `forward/allocations` | Counting allocator around one `forward` per strategy; the S1-G2 tier 2 "allocations added = 0" invariant, measured here where it is exact |
| **Control** | `select/noop` — reads the same memory, returns index 0 |

The `now` closure handed to `select` returns a constant; no clock is read in
any arm. The all-Healthy arms (M.1, M.2) never evaluate it; M.3 and M.4
evaluate it once (design S1.5, lazy read at the first non-Healthy word).

### S1-M — configuration

None. Stage 1 has one tunable (`penalty_failures`) and it does not affect these
paths' cost. Synthetic Penalized/Probing words are built through a bench-only
constructor if one is needed; the results file names it.

### S1-M — metrics

Wall-clock per operation. For M.7, additionally: winner count must be exactly
1. For M.9: allocation count per strategy.

### S1-M — acceptance

| Bench | Criterion |
| --- | --- |
| M.1 | `M.1 ≤ noop + 10 ns` x86 **and** `< 111 ns` x86 (the M.2–M.4 budget). "At or below the control" is unattainable by construction — `select` does strictly more than a noop read, and at 1–2 ns a 1 % CI is narrower than the decode. The "one relaxed load, early exit" claim is the instruction count, reported from the disassembly (`cargo asm` / `objdump`) or `perf stat`, not from the timing |
| M.8 | `adaptive` within the CI of `fallback`; > 10 % regression is a blocker (root CLAUDE.md) |
| M.9 | Counts equal between strategies |
| M.2, M.3, M.4 | < 111 ns x86 (< 1 µs RB5009-equivalent, 0.1 % of the 1 ms `forward` engine-overhead budget) |
| M.5 | < 22 ns x86 (< 200 ns RB5009-equivalent); must perform no store when the word is already Healthy with `consecutive_failures == 0` |
| M.6, M.7 | Cold paths; no budget beyond "does not spin" — M.7 winner count exactly 1 per batch |
| Control | Unchanged vs the pre-change checkout within ±2 %, or the session is discarded. The pre-change checkout has no `upstream_select`; the A/B applies to M.8 and the control |

### S1-M — what justifies proceeding

M.1 within 10 ns of the control and under 111 ns, M.8 within the CI of
`fallback`, M.9 equal, and M.3 under the RB5009-equivalent 1 µs budget. Then
selection is free and the design needs no simplification.

### S1-M — what justifies rejecting or simplifying

- **M.3 over 1 µs RB5009-equivalent** → the packed-word decode is too expensive.
  Split `state` into a separate `AtomicU8` read first, so the scan skips
  penalized endpoints without decoding the deadline.
- **M.5 over its budget** → the success path is doing more than an early-exit
  compare. It must not write when the state word is already correct.
- **M.7 showing more than one winner per batch** → the CAS is not on the
  single packed word, or the claim preserves the wrong bits. Correctness, not
  tuning.
- Any remedy above that changes the word layout or the selector is a design
  change: it goes to the owner as a spec amendment, not into the bench task.

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
  attempts that are client forwards (suite T sample 1). Attempts-per-forward
  is `(Σ upstreams[].attempts − (swr.completed + swr.failed)) / cache_misses`,
  every term a delta over the run — SWR refreshes produce attempts without a
  client forward, so the raw ratio is never 1.000. Comparable across
  strategies only where no `resolve_host` traffic exists — L.1/L.2 in the probe
  container — because `adaptive` does not count `resolve_host` attempts
  (design S1.8).
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

**Metric choice is a fixed order, not a judgment:** total upstream attempts
(covers client and SWR) if its N ≤ 10 %; else `forward` p99 if its N ≤ 10 %;
else tier 3 is dropped. "Adequate" means exactly N ≤ 10 % on that metric.

### S1-N — what justifies proceeding

An N small enough that the resulting threshold would catch a regression worth
catching. Since Stage 1's healthy path is provably one relaxed load and zero
allocations, the live gate only needs to catch a *gross* regression — precision
lives in S1-M.

### S1-N — what justifies dropping the live timing gate

- **N > 10 % on both candidate metrics** → the harness cannot resolve
  anything useful. Drop tier 3, rest the healthy-path claim on S1-M and the
  tier 2 invariants, and say so explicitly in the result. This is an honest
  outcome, not a failure.
- Precision improves as √K while cost is linear in K. If K = 12 still leaves N
  wide, the answer is to drop the gate, **not** to run more repetitions.

## S1-B. Stage 1 behavioural

The suite that shows the win. Dev box, mock upstreams, tokio test runtime, not
pinned.

Every arm is run twice in the same session: `strategy = "fallback"` and
`strategy = "adaptive"`. Assertions about `state`, deadlines, `penalty_round`
and probes (B.4's deadline rows, B.5, B.7's second pass, B.8's probe counts)
are `adaptive`-only by nature — no such state exists under `fallback`; the
`fallback` run of those arms reports attempts and latency and nothing else.

### S1-B — workload

| Arm | Setup |
| --- | --- |
| B.1 Black hole | Endpoint A bound but silent; endpoint B healthy at 5 ms. 10 000 forwarded queries, distinct names, issued **sequentially** — the "≤ `penalty_failures` non-probe queries pay" criterion counts queries dispatched after the penalty landed; in-flight concurrent queries also pay (design S1.3) and would blur the count. A stays dead for the whole run, so one probe-carrying query pays per penalty window — those are `failed_probes`, identified by the `probes` delta around each query |
| B.2 ICMP unreachable | **Classification check, not a win scenario**: a refused endpoint costs ~0 under `fallback` too, so B.2 is excluded from the net-cost rows. Two arms. **UDP**: endpoint A on a closed UDP port (`ECONNREFUSED`) — Linux only (`cfg(target_os = "linux")`), because Windows reports the same ICMP as `ECONNRESET`, a hard failure (design S1.4 platform note); real host-unreachable is L.4b. **DoT**: endpoint A on a closed TCP port (`ECONNREFUSED` on both platforms, no TLS mock needed) — the dev-box arm. B healthy at 5 ms in both |
| B.3 Healthy control | Both endpoints healthy at 5 ms — decided on **counts**, not timing: an unpinned tokio runtime cannot resolve a timing difference and would reject on noise. Timing is reported; the precision claim is S1-M |
| B.4 All dead | Both endpoints black-holed, queries sequential. Verifies the S1.3 invariant: queries are still sent |
| B.5 Recovery | Scripted, in units of `PENALTY_MAX` (P): A healthy 0.4 P → black hole 2 P → healthy 0.6 P → flapping 0.1 P up / 0.1 P down for 0.6 P → **healthy 1.2 P → black hole 0.1 P** (the tail that exercises the round reset: the next penalty must land at `penalty_round == 1`). Queries **sequential at 20 QPS** (scaled; one query interval = 50 ms), so "one query interval" in the recovery criterion is 50 ms and every forced or probe attempt lands on a word no other query is touching. **B healthy throughout** — A is probed by the first query after its deadline although B is Healthy (design S1.5 one pass). `PENALTY_MAX / PENALTY_BASE` swept {2.5, 12.5, 37.5} — the shipped 300 s / 24 s is 12.5; 60 s and 900 s are the other two |
| B.6 RCODE isolation | A answers SERVFAIL to everything, transport-healthy. 1 000 queries |
| B.7 `resolve_host` isolation | Single-stack mock: A answers, AAAA silent. 100 `resolve_host` calls, no client queries. Second pass (`adaptive` only): A pre-penalized with its deadline passed, B healthy — `resolve_host` must not claim the probe (design S1.8) |
| B.8 SWR interaction | Stale entries expiring against a black-holed endpoint, `swr_workers = 3`. Run twice: **SWR-only** — stale entries expire, no client queries, every probe counted is SWR's; **client-only** — distinct never-cached names so no stale hit ever enqueues a refresh, every probe counted is a client's. This is how SWR-vs-client probe attribution is measured, since telemetry does not carry it (design S1.12). The SWR path is driven from `tests/` through `Pipeline::new(..)` + `Pipeline::spawn_swr_workers()` (`pipeline.rs:158`, public — the `server_integration.rs` construction shape); `SwrPool::spawn_workers` itself is `pub(crate)` and is not called directly |

### S1-B — configuration

`timeout_ms` **scaled down** (e.g. 50 ms) with the policy scaled in proportion
through `with_policy`, so that `PENALTY_BASE / attempt_bound_ms` stays 10 and
`PENALTY_MAX / PENALTY_BASE` is the swept ratio; the shipped shape is preserved
and the suite fits `cargo test`. `penalty_failures` swept {1, 2, 3}. The scale
is stated on every figure. The mocks are plain UDP and never truncate, so one
attempt costs one leg here; the `attempt_bound_ms` cap is the worst case and is
not exercised by this suite.

### S1-B — metrics

- Client-visible p50/p99, and the first 10 queries reported separately.
- Count of queries that paid (latency ≥ `timeout_ms`) and the **measured** time
  each paid, split into **non-probe paying queries** and **probe-carrying
  paying queries** (`failed_probes`). With sequential issue the `probes`
  delta around each query identifies the carrier exactly.
- Time-to-penalize, in queries and wall clock.
- `state`, `penalty_round`, `penalties`, `probes`, `probe_successes`,
  `penalized_seconds_total` over time.
- **Net timeout cost avoided** (design S1-G3):

  ```text
  adaptive_paying = non-probe paying queries          (probe carriers excluded)
  probe_cost      = failed_probes × attempt_bound_ms
  net_avoided     = (fallback_paying − adaptive_paying) × attempt_bound_ms − probe_cost
  ```

  and the same over measured paid time. `adaptive_paying` **excludes** the
  probe carriers — counting them there and in `probe_cost` subtracts every
  failed probe twice. Computed **twice** — over client `forward`s (B.1 and
  B.5; B.2 contributes nothing, its refusal is ~free under `fallback` too)
  and over SWR refreshes (B.8) — with a cache-hit-aware line using the
  deployed composition (10 % client forwards, 69 % SWR).
- **Tax-free share** per arm: queries that paid nothing ÷ queries after the
  penalty landed, stated **per `PENALTY_MAX / PENALTY_BASE` ratio** — it
  depends on how many penalty windows the run spans.
- B.3: `penalties`, `attempts` per endpoint, `state` — the decision inputs;
  p50/p99 reported, not gated.
- B.4: count of queries for which no packet was sent — **must be 0**; each
  endpoint's deadline before and after every forced attempt.
- B.5: `penalty_round` at the first penalty after the 1.2 P healthy tail;
  the mock's **high-water mark of concurrently outstanding requests at A**
  while A is Penalized or Probing (the in-flight-probe measurement —
  trivially ≤ 1 under sequential issue, so the concurrent claim rests on
  G1 #8); `probes` delta per phase against the number of penalty windows
  elapsed in that phase; p99 of the flapping phase and of the black-hole
  phase, separately.
- B.6: `state` and `consecutive_failures` — **must be unchanged**.
- B.7: every counter and `state` — **must be unchanged**.
- B.8: probes in each of the two passes; `swr.enqueued`, `swr.dropped`,
  `swr.failed` deltas; the number of distinct stale keys the pass created.

### S1-B — acceptance

| Arm | Criterion |
| --- | --- |
| B.1 `fallback` | ~10 000 × one leg (`timeout_ms`) — establishes the tax being removed |
| B.1 `adaptive` | ≤ `penalty_failures` **non-probe** sequential queries pay one leg (≤ `attempt_bound_ms`); thereafter only probe-carrying queries pay, one per penalty window, reported as `failed_probes`; every other query answers at ~5 ms. Tax-free share stated per ratio — at 12.5 the run spans ~6 windows, at 2.5 ~13 — never as one percentage |
| B.2 `adaptive` | Endpoint Penalized after 1 attempt regardless of `penalty_failures`, and ≤ 1 query pays anything (UDP arm on Linux; DoT arm everywhere). The UDP arm on Windows is not a result. Not a net-cost input |
| B.3 | `penalties == 0` on both endpoints, `attempts` equal between arms, `state` Healthy throughout — **count-based**; p50/p99 reported beside it and not gated |
| B.4 | Zero queries with no packet sent; **a forced attempt that lands on a Penalized word leaves that word's deadline and `penalty_round` unchanged** (design S1.3 hard invariant) — sequential issue keeps every forced attempt on a Penalized word; a forced attempt landing on a word another query has meanwhile claimed as Probing follows the Probing row and is excluded from this assertion; both endpoints probed at their original deadlines |
| B.5 | Healthy again within `PENALTY_MAX` + 50 ms (one query interval at 20 QPS) of the "healthy 0.6 P" phase start, with B Healthy throughout; the mock's outstanding-request high-water mark at A while Penalized/Probing ≤ 1; flapping phase: `penalty_round` strictly increases across consecutive penalties until the cap, `probes` delta ≤ penalty windows elapsed in the phase (no probe storm), `p99_flapping ≤ 1.1 × p99_black_hole` (scaled legs are 10× the mock RTT, so noise is not the limit); `penalty_round` does **not** reset during the 0.6 P healthy phase and **does** reset after the 1.2 P tail — the penalty in the final 0.1 P lands at `penalty_round == 1` |
| B.6 | Zero state change, zero counter movement beyond `attempts` |
| B.7 | Zero state change, zero counter movement, including `attempts`; second pass: the due word byte-for-byte unchanged, `probes` unchanged |
| B.8 | SWR-only pass: probes > 0. `swr.dropped` reported (a count is not bounded by a depth). No unbounded refresh retry: `swr.enqueued ≤ stale_keys × ceil(pass_duration / REFRESH_FAILURE_COOLDOWN)` — each key re-enqueues at most once per cooldown |
| Net cost | Reported in two rows with the formula's inputs; the raw aggregate is never presented as client benefit |

### S1-B — what justifies implementing

B.1 showing the one-leg tax removed from every query after the penalty except
the one probe carrier per penalty window, **with B.3's counts identical**. The
tax-free share is reported per ratio — at the shipped 12.5 a ~50 s scaled run
spans ~6 windows (~99.9 %), at 2.5 ~13 (~99.87 %) — and at production scale
(300 s windows, 0.68 QPS) it is one carrier per ~200 queries while the
endpoint stays dead. Against the measured 5 ms healthy path the p99 on the
failure scenario improves ~160× (one 800 ms leg → 5 ms) at zero cost on the
healthy one.

### S1-B — what justifies rejecting or simplifying

- **B.3 showing any count difference between arms** — a penalty applied, an
  attempt not made, `state` leaving Healthy → Stage 1 is not free on a
  healthy link. Fix or reject; there is no acceptable trade here, because the
  healthy link is ~100 % of normal operation. A timing difference on this
  unpinned runtime is noise, not a finding; timing is S1-M's.
- **B.6 or B.7 showing any state change** → the classification or the isolation
  is wrong. Both are correctness failures (gates S1-G1.5 and S1-G1.7), not
  tuning findings.
- **B.4 showing a query with no packet sent** → the S1.3 invariant is violated.
  Hard stop.
- **B.2 taking more than 1 query on Linux, or on the DoT arm** → path-failure
  classification is not firing; ICMP errors are being seen as ordinary
  timeouts. The UDP arm on Windows taking `penalty_failures` attempts is the
  platform, not a finding (design S1.4).
- **L.3 (not B.5 — B.5's 20 QPS sequential load has no idle gaps) showing
  recovery consistently at the full `PENALTY_MAX`**: `probe_successes`
  landing one full window after the deadline because household idle gaps
  carried no query to claim the probe → on-path probing does not recover
  promptly enough without traffic. Reconsider the background prober,
  accepting the extra task and idle traffic. Report-only in this phase.
- **The 2.5 ratio (60 s) performing as well as 12.5 (300 s) with no extra probe
  traffic** → a case for `PENALTY_MAX = 60 s`. That is a policy-constant
  change (design S1.6), proposed to the owner as a spec amendment and confirmed
  on-device in S1-L L.4a before it ships — not applied inside the bench task.

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
| L.1 | 10 000 QPS synthetic forward load, 2 healthy mock upstreams, 10 min, `adaptive`. The mocks are whatever served p2.5-09's on-device forward load — the same tool and the same host, named in the results file; if p2.5-09 used real upstreams for that arm, L.1 does too and says so (the tier 2 invariants are counts and do not depend on the mock) |
| L.2 | Same, `fallback`, same session — the probe container's own config flips the strategy; the production container's env is not touched for this arm |
| L.3 | **7-day** household-traffic soak, real upstreams, `adaptive` opt-in on the **production** container (design S1.14) — the one arm that is not in the probe container |
| L.4a | Failure injection, **WAN black hole**: one entry pointed at a routable address nothing answers on (timeout → hard failure, B.1 shape), 1 h |
| L.4b | Failure injection, **LAN host-unreachable**: one entry pointed at an unused address inside the LAN prefix — ARP fails and the kernel returns `EHOSTUNREACH` on the connected UDP socket (path failure, penalized after 1 attempt). The only arm in the whole plan where the UDP path-failure row meets real ICMP/ARP, which the Windows dev box cannot produce (design S1.4). 1 h |
| **Control** | `cache_hit`-only load (all names pre-warmed) in both L.1 and L.2 |

### S1-L — configuration

Probe container per routeros-traps.md for L.1, L.2, L.4a, L.4b. L.3 is the production
container with `adaptive` enabled through the `FAH__DNS__UPSTREAMS__STRATEGY`
environment override (never a TOML edit inside the container), binary deployed
first, opt-in second, confirmed via `GET /api/v1/config`. `penalty_failures` at
whatever value T produced, or the compiled default if T's window is still open.
Every router command is proposed to the owner and run by the owner.

### S1-L — metrics

- `forward`-stage p50/p99 from `/api/v1/telemetry`. The stage partition is
  binding: `forward` is misses plus the RFC 8767 fallback, nothing else.
- Sustained QPS against the ≥ 10 000 budget.
- RSS from the 60 s perf samples (`/history/perf`); slope over the final third
  of a 24 h window inside L.3, for each day of the soak.
- All Stage 1 telemetry fields (S1.12), daily read-only snapshots.
- L.3: **false-penalty** count per endpoint — a `penalties` delta in a 60 s
  perf sample during which the *other* endpoint's `failures` did not move
  (the link was up; the penalized endpoint alone failed `penalty_failures`
  times). `penalties` deltas in samples where every endpoint's `failures`
  moved are outage penalties, reported separately. `state` over time — with
  the caveat that a probe (≤ `attempt_bound_ms`) is invisible to the 10 s
  poll; `probes` is the probe count, `state` is not.

### S1-L — acceptance

| Metric | Criterion |
| --- | --- |
| L.1 vs L.2, chosen timing metric | Within the threshold **S1-N derived**, on the metric S1-N's fixed order chose. No number until S1-N has run. If S1-N dropped tier 3, this row is reported without a pass/fail |
| Tier 2 invariants (design S1-G2), L.1 vs L.2 | Counts, each a delta over the run: `penalties == 0` on both endpoints in both arms (the mocks never fail); attempts per forward `(Σ attempts − (swr.completed + swr.failed)) / cache_misses == 1.000` in both arms (L.1/L.2 only — L.3's `attempts` excludes `resolve_host` under `adaptive`, design S1.8); `penalties` and `probes` deltas == 0 in the `adaptive` arm (the 10 s poll cannot see a probe, so `state` is decided from the counters); `swr.failed == 0` and `swr.dropped == 0` in both arms, `swr.completed` reported — exact SWR equality between two runs is not a criterion |
| Tier 2 on L.3 | `penalties` is **report-only** here: real upstreams do fail, and this count is G4 row 5 (false-penalty rate) under its definition above, not a pass/fail |
| L.1 sustained QPS | ≥ 10 000 absolute. The relative half ("no worse than L.2") uses the S1-N threshold on QPS when tier 3 is alive and is dropped with it otherwise |
| Control arm | Moves less than the measured arm, or the session is discarded |
| L.3 RSS slope, final third of each 24 h window | < 2 MB drift (mimalloc's purge band alone is ±6 MB) |
| L.3 state memory | constant — `penalties` may grow, state size may not |
| L.3 false penalties | **Measured and reported, by the definition above. No pre-set threshold** — none is currently justified by evidence, and imposing one would smuggle the S1.6 hypothesis back in as a criterion |
| L.3 duration | 7 days complete before the default flip is proposed (design S1.14) |
| L.4a | Matches S1-B's B.1 shape on real hardware and real upstreams: `penalty_failures` non-probe queries pay, then one probe carrier per window; the `attempt_bound_ms` cap may be exercised here (encrypted endpoints, TC retries) where S1-B's UDP mocks cannot |
| L.4b | Penalized after exactly 1 attempt with `failures == 1` on the dead entry — the path-failure row confirmed with a real `EHOSTUNREACH`; if the kernel reports a timeout instead, the results file says so and the row is recorded as unconfirmed on this platform |

### S1-L — what justifies implementing

L.1 within the S1-N-derived threshold of L.2 with a flat control arm, every
tier 2 invariant intact, plus L.4a reproducing the behavioural win on the real
device and L.4b confirming the path-failure row. That is "costs nothing when
nothing is wrong, and pays when something is".

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

One file per suite, in the task's review file under
`docs/code-review/phase2.6/` (`p2.6-08` for S1-M, `p2.6-09` for S1-B,
`p2.6-10` for S1-N, `p2.6-11` for T's S1-G4/G5 reading and S1-L), per the
project convention: corpus, workload and device on every figure; tables not
prose; the control arm reported alongside every arm; the scale factor on every
scaled figure. A result that omits the control arm is not a result.

Suite T is reportable today and blocks nothing else.
