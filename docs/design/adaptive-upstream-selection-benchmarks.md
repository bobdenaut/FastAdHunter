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
pinned to one core. Pure functions over a synthetic `Arc<[Health]>` — no
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
| M.8 `forward/udp_answered` | End-to-end `forward` against a loopback UDP mock, both strategies — the existing `crates/fah-dns/benches/upstream.rs` arm with an `adaptive` twin. **This is the `fallback` comparison**: `fallback` has no standalone selector to measure, its "selection" is entering the transport |
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
| M.1 | At or below the `select/noop` control within the CI — the structural claim, one relaxed load and early exit |
| M.8 | `adaptive` within the CI of `fallback`; > 10 % regression is a blocker (root CLAUDE.md) |
| M.9 | Counts equal between strategies |
| M.2, M.3, M.4 | < 111 ns x86 (< 1 µs RB5009-equivalent, 0.1 % of the 1 ms `forward` engine-overhead budget) |
| M.5 | < 22 ns x86 (< 200 ns RB5009-equivalent); must perform no store when the word is already Healthy with `consecutive_failures == 0` |
| M.6, M.7 | Cold paths; no budget beyond "does not spin" — M.7 winner count exactly 1 per batch |
| Control | Unchanged vs the pre-change checkout within ±2 %, or the session is discarded. The pre-change checkout has no `upstream_select`; the A/B applies to M.8 and the control |

### S1-M — what justifies proceeding

M.1 at or below the control, M.8 within the CI of `fallback`, M.9 equal, and
M.3 under the RB5009-equivalent 1 µs budget. Then selection is free and the
design needs no simplification.

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
  attempts that are client forwards (suite T sample 1). Comparable across
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
| B.1 Black hole | Endpoint A bound but silent; endpoint B healthy at 5 ms. 10 000 forwarded queries, distinct names, issued **sequentially** — the "≤ `penalty_failures` pay" criterion counts queries dispatched after the penalty landed; in-flight concurrent queries also pay (design S1.3) and would blur the count |
| B.2 ICMP unreachable | Two arms. **UDP**: endpoint A on a closed UDP port (`ECONNREFUSED`) — Linux only (`cfg(target_os = "linux")`), because Windows reports the same ICMP as `ECONNRESET`, a hard failure (design S1.4 platform note); on-device in L.4. **DoT**: endpoint A on a closed TCP port (`ECONNREFUSED` on both platforms) — the dev-box arm. B healthy at 5 ms in both |
| B.3 Healthy control | Both endpoints healthy at 5 ms — must show no difference between arms |
| B.4 All dead | Both endpoints black-holed. Verifies the S1.3 invariant: queries are still sent |
| B.5 Recovery | Scripted, in units of `PENALTY_MAX` (P): A healthy 0.4 P → black hole 2 P → healthy 0.6 P → flapping 0.1 P up / 0.1 P down for 0.6 P. **B healthy throughout** — A is probed by the first query after its deadline although B is Healthy (design S1.5 one pass). `PENALTY_MAX / PENALTY_BASE` swept {2.5, 12.5, 37.5} — the shipped 300 s / 24 s is 12.5; 60 s and 900 s are the other two |
| B.6 RCODE isolation | A answers SERVFAIL to everything, transport-healthy. 1 000 queries |
| B.7 `resolve_host` isolation | Single-stack mock: A answers, AAAA silent. 100 `resolve_host` calls, no client queries. Second pass: A pre-penalized with its deadline passed, B healthy — `resolve_host` must not claim the probe (design S1.8) |
| B.8 SWR interaction | Stale entries expiring against a black-holed endpoint, `swr_workers = 3`. Run twice: with **no client queries** (every probe counted is SWR's) and with client queries only — this is how SWR-vs-client probe attribution is measured, since telemetry does not carry it (design S1.12) |

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
  each paid.
- Time-to-penalize, in queries and wall clock.
- `state`, `penalty_round`, `penalties`, `probes`, `probe_successes`,
  `penalized_seconds_total` over time.
- **Net timeout cost avoided** (design S1-G3):
  `(fallback_paying − adaptive_paying) × attempt_bound_ms − failed_probes × attempt_bound_ms`,
  and the same over measured paid time; computed **twice** — over client
  `forward`s (B.1/B.2/B.5) and over SWR refreshes (B.8) — with a cache-hit-aware
  line using the deployed composition (10 % client forwards, 69 % SWR).
- B.4: count of queries for which no packet was sent — **must be 0**; each
  endpoint's deadline before and after every forced attempt.
- B.6: `state` and `consecutive_failures` — **must be unchanged**.
- B.7: every counter and `state` — **must be unchanged**.
- B.8: probes in each of the two passes; dropped SWR jobs.

### S1-B — acceptance

| Arm | Criterion |
| --- | --- |
| B.1 `fallback` | ~10 000 × one leg (`timeout_ms`) — establishes the tax being removed |
| B.1 `adaptive` | ≤ `penalty_failures` sequential queries pay one leg (≤ `attempt_bound_ms`); the remainder answer at ~5 ms |
| B.2 `adaptive` | ≤ 1 query pays anything and the endpoint is Penalized after 1 attempt regardless of `penalty_failures` (UDP arm on Linux; DoT arm everywhere). The UDP arm on Windows is not a result |
| B.3 | Arms within noise of each other; no penalty ever applied |
| B.4 | Zero queries with no packet sent; **a forced attempt that lands on a Penalized word leaves that word's deadline and `penalty_round` unchanged** (design S1.3 hard invariant) — a forced attempt landing on a word another query has meanwhile claimed as Probing follows the Probing row and is excluded from this assertion; both endpoints probed at their original deadlines |
| B.5 | Healthy again within `PENALTY_MAX` + one query interval with B Healthy throughout; never more than one probe in flight; flapping phase shows growing backoff and no probe storm; `penalty_round` does **not** reset during a healthy phase shorter than `PENALTY_MAX` and **does** reset after one at least `PENALTY_MAX` long |
| B.6 | Zero state change, zero counter movement beyond `attempts` |
| B.7 | Zero state change, zero counter movement, including `attempts`; second pass: the due word byte-for-byte unchanged, `probes` unchanged |
| B.8 | SWR-only pass: probes > 0; dropped jobs bounded by queue depth; no unbounded refresh retry |
| Net cost | Reported in two rows with the formula's inputs; the raw aggregate is never presented as client benefit |

### S1-B — what justifies implementing

B.1 and B.2 showing the one-leg tax removed from ≥ 99.9 % of queries once the
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
- **B.2 taking more than 1 query on Linux, or on the DoT arm** → path-failure
  classification is not firing; ICMP errors are being seen as ordinary
  timeouts. The UDP arm on Windows taking `penalty_failures` attempts is the
  platform, not a finding (design S1.4).
- **B.5 recovery consistently taking the full `PENALTY_MAX` under a household
  traffic pattern with idle gaps** → on-path probing does not recover promptly
  enough without traffic to carry the probe. Reconsider the background prober,
  accepting the extra task and idle traffic.
- **The 2.5 ratio (60 s) performing as well as 12.5 (300 s) with no extra probe
  traffic** → a case for `PENALTY_MAX = 60 s`. That is a policy-constant
  change (design S1.6), proposed to the owner as a spec amendment and confirmed
  on-device in S1-L L.4 before it ships — not applied inside the bench task.

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
| L.2 | Same, `fallback`, same session — the probe container's own config flips the strategy; the production container's env is not touched for this arm |
| L.3 | **7-day** household-traffic soak, real upstreams, `adaptive` opt-in on the **production** container (design S1.14) — the one arm that is not in the probe container |
| L.4 | Failure injection against real upstreams: one entry pointed at a dead address, 1 h |
| **Control** | `cache_hit`-only load (all names pre-warmed) in both L.1 and L.2 |

### S1-L — configuration

Probe container per routeros-traps.md for L.1, L.2, L.4. L.3 is the production
container with `adaptive` enabled through the `FAH_DNS_UPSTREAMS_STRATEGY`
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
- L.3: false-penalty count per healthy endpoint; `state` over time.

### S1-L — acceptance

| Metric | Criterion |
| --- | --- |
| L.1 vs L.2, chosen timing metric | Within the threshold **S1-N derived**. No number until S1-N has run. If S1-N dropped tier 3, this row is reported without a pass/fail |
| Tier 2 invariants (design S1-G2) | Counts identical between arms — no threshold needed: penalties on a healthy link (0 or T's base rate), attempts per forward (1.000; L.1 vs L.2 only — L.3's `attempts` excludes `resolve_host` under `adaptive`, design S1.8), `state` never leaving Healthy, SWR attempts and outcomes comparable |
| L.1 sustained QPS | ≥ 10 000, no worse than L.2 |
| Control arm | Moves less than the measured arm, or the session is discarded |
| L.3 RSS slope, final third of each 24 h window | < 2 MB drift (mimalloc's purge band alone is ±6 MB) |
| L.3 state memory | constant — `penalties` may grow, state size may not |
| L.3 false penalties | **Measured and reported. No pre-set threshold** — none is currently justified by evidence, and imposing one would smuggle the S1.6 hypothesis back in as a criterion |
| L.3 duration | 7 days complete before the default flip is proposed (design S1.14) |
| L.4 | Matches S1-B's B.1/B.2 shape on real hardware and real upstreams; the `attempt_bound_ms` cap may be exercised here (encrypted endpoints, TC retries) where S1-B's UDP mocks cannot |

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

One file per suite, in the task's review file under
`docs/code-review/phase2.6/` (`p2.6-08` for S1-M, `p2.6-09` for S1-B,
`p2.6-10` for S1-N, `p2.6-11` for T's S1-G4/G5 reading and S1-L), per the
project convention: corpus, workload and device on every figure; tables not
prose; the control arm reported alongside every arm; the scale factor on every
scaled figure. A result that omits the control arm is not a result.

Suite T is reportable today and blocks nothing else.
