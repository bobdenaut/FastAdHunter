# Adaptive DNS Upstream Selection — Stage 1 specification

Status: **Stage 1 shipped as the default** — `adaptive` is the compiled-in
default and the `fallback` path is deleted (p2.6-12, first build after 0.3.3, on
`phase3-06`). Stages 2 and 3 are candidate designs only and are not accepted;
each carries an explicit benchmark gate that must pass before it may be
specified in detail.

Sequencing, settled before this document was written: Phase 3 had been opened
once and was cancelled back to a clean `phase3`, and phases 2.5 and 2.6 were
inserted ahead of it. Nothing remains to revert. From that baseline: implement
Stage 1 → benchmark and close Stage 1 → decide Stage 2 and Stage 3 from that
data → close Adaptive DNS → restart Phase 3.

Benchmarks: [adaptive-upstream-selection-benchmarks.md](adaptive-upstream-selection-benchmarks.md).

---

## The defect

[`UpstreamPool::forward`](../../crates/fah-dns/src/upstream/mod.rs#L182) walks
`servers` in config order on every query and remembers nothing between queries.
`UpstreamStatus::consecutive_failures` is documented as explicitly **not** a
liveness signal, and nothing reads it.

Consequence: an endpoint that stops answering costs up to `attempt_bound_ms`
= `ATTEMPT_LEGS × timeout_ms` (2 400 ms at the default; one leg, 800 ms, for
a plain UDP attempt that never truncates) on **every** query, indefinitely.
There is no state in which FAH knows an endpoint is down.

Stage 1 fixes exactly that, and nothing else.

## Deployment reality

Measured facts. Each is a reading taken at a stated time on a stated device; none
licenses a prediction about future behaviour.

### Upstream reachability, 2026-08-16

Router-side `/ping`, n=5 per target. The container holds
`2a02:2f04:5304:c901::2/64` on `veth1` (`GS`, `from-pool=ipv6-pool`).

| Target | Family | min | avg | max | loss |
| --- | --- | --- | --- | --- | --- |
| `2606:4700:4700::1111` | v6 | 2.182 ms | 2.299 ms | 2.366 ms | 0 % |
| `2606:4700:4700::1111`, src `2a02:2f04:5304:c901::2` | v6 | 2.270 ms | 2.394 ms | 2.559 ms | 0 % |
| `2620:fe::fe` | v6 | 2.323 ms | 2.452 ms | 2.533 ms | 0 % |
| `1.1.1.1` | v4 | 2.172 ms | 2.299 ms | 2.375 ms | 0 % |
| `9.9.9.9` | v4 | 2.261 ms | 2.379 ms | 2.571 ms | 0 % |

**What this establishes:** the container's v6 source address is routable; both
families reached all candidate resolvers at that moment; a healthy path RTT
floor of ~2.4 ms exists against a `timeout_ms` of 800 ms, a ratio of ~330×.

**What this does not establish:** DNS-level RTT (this is ICMP echo; resolver
processing and cold recursive lookups sit on top), any distribution (n=5, one
moment), that the families remain equal over time, or any value for a hedge
delay beyond a floor.

### Other measured facts

| Fact | Source |
| --- | --- |
| Household query rate ≈ 0.68 QPS mean | [measurement-traps.md](../measurement-traps.md) |
| IPv6 is 22 % of *client-side* traffic — the listener, not the upstream path | [routeros-traps.md](../routeros-traps.md) |
| `veth1` re-addresses `from-pool` on a PPPoE redial, automatically | RouterOS config, 2026-08-16 |
| Default upstreams are IP literals (`1.1.1.1`, `9.9.9.9`), not hostnames | [`upstreams.rs:42`](../../crates/fah-config/src/schema/dns/upstreams.rs#L42) |

### Upstream traffic composition, 2026-08-16

Suite T sample 1, deployed build v0.2.16, 45.7 h uptime. Attempt attribution is
arithmetic over one telemetry snapshot, not an instrumented breakdown — the
residual is assigned to list fetches by elimination.

| Source | Attempts | Share |
| --- | --- | --- |
| SWR refreshes (`swr.enqueued`) | 28,809 | **69.1 %** |
| `resolve_host` (HTTP `HostResolver`, 4,192 requests × A+AAAA) | 8,384 | **20.1 %** |
| Client DNS forwards (`cache_misses`) | 4,279 | 10.3 % |
| List-fetch bootstrap (residual) | ~238 | 0.6 % |
| **Total** | **41,710** | |

This composition drives S1.8, S1.9 and S1-G2 and was not anticipated when they
were first written. It is one snapshot of one deployment; the observation window
tests whether it is stable.

### Observed failure data, 2026-08-16

Same snapshot.

| Endpoint | attempts | failures | rate |
| --- | --- | --- | --- |
| `1.1.1.1` | 41,710 | 30 | **0.072 %** |
| `9.9.9.9` | 30 | 3 | — |

- **27 partial-failure events, 3 total-failure events, over 45.7 h** (~14 partial
  events/day).
- **`9.9.9.9`'s 30 attempts equal `1.1.1.1`'s 30 failures exactly.** Direct
  confirmation of the current ordered-fallback behaviour: the secondary is
  contacted only when the primary has already failed, so it carried 27 queries
  in 45.7 hours.
- The measured 0.072 % primary failure rate is **7× below** the lower of the two
  illustrative rates used in S1.6.

**What this settles:** S1-G5's first rejection condition — "no partial upstream
failure over the observation window" — **is already false**. The population
Stage 1 serves exists.

**What this does not settle:** whether those 30 failures were isolated single
losses or runs of two or more. `penalty_failures = 2` fires only on runs of ≥ 2,
so this distinction decides whether Stage 1 ever engages. Under an independence
assumption at the measured rate, runs of ≥ 2 would occur roughly once per
87 days — but independence is the wrong model for a real outage, and only the
observed distribution can settle it. `penalty_failures = 2` therefore remains
provisional (S1.6, gate S1-G4).

### First DNS-level latency observation

`latency.dns.forward` = 102.15 s over 4,279 forwards → **mean ≈ 23.9 ms**, about
10× the 2.4 ms ICMP floor.

**Contextual evidence only.** It is a mean over cache misses including cold
recursive lookups, aggregated across both endpoints, with no per-endpoint
attribution and no distribution. It must not be used to define any Stage 2 or
Stage 3 constant.

### Not measured

| Quantity | Blocks | How to close |
| --- | --- | --- |
| **Failure clustering / run-length distribution** | Justifying `penalty_failures = 2` — gate S1-G4 | See below; **collected since p2.5-06** as `upstreams[].failure_runs` — needs deploy time, not new code |
| Stability of the traffic composition above | S1.8, S1.9, S1-G2 framing | Suite T differenced series |
| Whether total failures land only on SWR refreshes | How urgent Stage 1 is for *clients* | Suite T: do `swr.failed` and the secondary's `failures` move together? |
| DNS-level `srtt` / `rttvar` per endpoint | Any Stage 2 or Stage 3 constant | Requires Stage 2 instrumentation |

#### Failure clustering — the blocking open measurement

Builds before p2.5-06 could not report run length: `consecutive_failures` is
reset by the next success, and at an 800 ms attempt timeout no polling cadence
catches a run mid-flight. Suite T's differenced series can **bound** clustering
— by whether failures arrive spread across sampling intervals or bunched into a
few — but cannot measure the distribution directly.

**p2.5-06 closes it in the build.** Every endpoint carries four cumulative
counters, `upstreams[].failure_runs` = `[len 1, len 2, len 3, len >= 4]`,
published live on `/telemetry` and persisted per 60 s perf sample, so
`/history/perf` deltas give the distribution for any window without a polling
race. A run is consecutive transport failures on one endpoint closed by that
endpoint's next transport success; an RCODE is a success (S1.4, pinned by
p2.5-04). Two properties the S1-G4 analysis must carry:

- **Open runs are absent.** A run is bucketed only when it closes, so a run
  still in progress (and one open at process exit) is not counted. In-flight it
  is visible as `consecutive_failures`.
- **Concurrency splits runs.** `forward` runs concurrently; a success dispatched
  before an outage but returning after k failures were counted resets the
  counter mid-run, so one outage of n can appear as runs of k and n−k. The bias
  is toward *shorter* runs — conservative for S1-G4, since it works against
  `penalty_failures = 2`. If runs of ≥ 2 still dominate the measured
  distribution, they dominate in reality too.
- **Counters reset on every restart.** `failure_runs` is cumulative per
  process. The window spans at least two restarts (0.2.16 → 0.2.18 → the
  2.6 binary deployed under `fallback`), so the distribution is the **sum of
  per-process deltas** read from `/history/perf`, never `end − start` across a
  restart — that undercounts by every run closed before the restart.

Gate S1-G4 therefore needs deploy time, not new code. The earlier candidate
remedy — a `max_consecutive_failures` high-water mark, an eighth `AtomicU64`
that would break the 64 B assertion in S1.2 and force `timestamp_ms` down from
50 to 42 bits — is **superseded and not needed**.

---

# Stage 1 specification

Self-contained. Implementable with no part of Stage 2 or Stage 3 present.

## S1.1 Scope

**In:** health state per endpoint, penalty on repeated transport failure,
skipping penalized endpoints during selection, on-path recovery probing,
transport-only failure classification, `resolve_host` health isolation, SWR
interaction, bounded state, telemetry, backward compatibility.

**Out, and not to be introduced by implication:** RTT measurement, EWMA,
`rttvar`, RTT bands, scored selection, family preference or bias, delayed
hedging, a total query deadline, and every configuration key belonging to those.

Selection order within the Healthy set remains **config order**, unchanged from
today.

## S1.2 Endpoint model

One `[[dns.upstreams.servers]]` entry = one endpoint. No endpoint fans out to
multiple addresses. `EndpointId(u8)` is the config index, stable for process
lifetime; all `[dns.upstreams]` keys are boot-only, so the endpoint set is
immutable after boot and there is no reload path.

State is split into two parallel arrays indexed by `EndpointId`. Aligning a
struct that also contains `Transport` — whose encrypted variant holds a
`tokio::sync::Mutex<Option<DnsExchange>>`, a `TokioRuntimeProvider` and three
`Arc<str>` — would put cold data on the hot cache line and buy nothing.

```rust
// Hot: written per query, read per selection. One cache line, asserted.
#[repr(align(64))]
struct Health {
    state: AtomicU64,            // packed, see S1.3
    attempts: AtomicU64,
    failures: AtomicU64,
    penalties: AtomicU64,
    probes: AtomicU64,
    probe_successes: AtomicU64,
    penalized_ms_total: AtomicU64,
}
const _: () = assert!(std::mem::size_of::<Health>() == 64);
```

Seven `u64` = 56 B, padded to 64 by the alignment. **An eighth field doubles the
struct to 128 B**; the assertion exists to make that a compile error rather than
a silent regression.

Cold array: `address: String`, `protocol`, `family` (informational in Stage 1
— nothing reads it for selection; derived once at construction from the
configured address, `None` for a hostname DoH URL), `transport`. Unchanged
from today's `UpstreamServer` minus the counters that moved.

**Cap: 8 endpoints**, enforced by `fah-config` validation as it already enforces
non-empty. `EndpointId` is a `u8` and the selection scan is linear.

### Memory

| Component | Size | Grows with |
| --- | --- | --- |
| `Health` array | 8 × 64 B = **512 B**, statically asserted | nothing |
| Cold array | unchanged from today; `Transport` dominates | nothing |

Fixed for process lifetime, independent of traffic, uptime, client count and
query names — hard rule 4. No map is keyed by anything a query can influence.

## S1.3 State machine

```text
              consecutive transport failures ≥ penalty_failures
        ┌───────────────────────────────────────────────────────┐
        │                                                       ▼
   ┌─────────┐                                            ┌───────────┐
   │ Healthy │◄──────── success answered ─────────────────│ Penalized │
   └─────────┘                                            └───────────┘
        ▲                                                       │
        │                                          deadline passed and a
        │                                          query claims the probe
        │                                                       ▼
        │                                                 ┌──────────┐
        └──────────────── probe answered ─────────────────│ Probing  │
                                                          └──────────┘
                            probe failed → Penalized, penalty_round += 1
```

Packed `state: AtomicU64`:

| Bits | Field | Notes |
| --- | --- | --- |
| 0–1 | `state` | 0 Healthy · 1 Penalized · 2 Probing · 3 never written — `unpack` maps it to Healthy (fail-open, consistent with the hard invariant) behind a `debug_assert` |
| 2–5 | `penalty_round` | saturating at 15 |
| 6–13 | `consecutive_failures` | saturating at 255 |
| 14–63 | `timestamp_ms` | ms since pool epoch (`Instant`), 50 bits ≈ 35 000 years |

`timestamp_ms` carries **two meanings, disambiguated by the state bits read
atomically in the same word**: while Healthy it is `healthy_since`; while
Penalized or Probing it is the penalty deadline. This is what lets the
`penalty_round` reset rule (S1.6) exist without a second word.

Two timing conventions, pinned: a deadline **has passed** when
`deadline <= now`; and **every subtraction on `timestamp_ms` is saturating**.
Threads sample `Instant::now()` independently, so a success recorded by another
thread can carry a `healthy_since` later than this thread's `now` — a plain
`now - healthy_since` would underflow (a debug-build panic, a spurious
`penalty_round` reset in release).

### Transition rule

The next word is a pure function of **(current word, outcome, now, policy)**,
computed inside a `compare_exchange_weak` loop and applied to the state
**observed at record time, never the state at dispatch time**. `forward` runs
concurrently, so an outcome can land on a state other than the one its attempt
was dispatched under; the function decides from what it sees.

| Observed state | Outcome | Next |
| --- | --- | --- |
| Healthy | success | Healthy, `consecutive_failures = 0` |
| Healthy | hard failure | `consecutive_failures += 1` (saturating); at `penalty_failures` → Penalized, deadline and round per S1.6 |
| Healthy | path failure | Penalized immediately |
| **Penalized** | **success** | **Healthy**, `consecutive_failures = 0`, `healthy_since = now`, `penalty_round` kept — the probe-success rule. An in-flight attempt dispatched before the penalty is valid transport evidence; discarding it would keep a live endpoint out for up to `PENALTY_MAX`. If the success was stale, at worst `penalty_failures` *new* queries pay up to `attempt_bound_ms` once and the endpoint is penalized again at round + 1 |
| Penalized | any failure | `consecutive_failures += 1` (saturating) **only. Deadline and `penalty_round` unchanged.** This is the forced-use case of the hard invariant below: extending the deadline on every forced failure would push the probe out forever during a total outage |
| Probing | success | Healthy, as above |
| Probing | failure | Penalized, `consecutive_failures += 1` (saturating — the open run extends, S1.12), `penalty_round += 1`, new deadline (S1.7). This is a penalty: `penalties += 1` and `penalized_ms_total` accrues the new nominal duration |

A probe whose endpoint a stale success already moved to Healthy therefore
lands as an ordinary Healthy-row outcome: a failure counts one and does not
bump `penalty_round`. No transition is keyed on what the attempt *was*, only on
what the word *is*.

### Hard invariant

**The selector always returns a candidate while endpoints exist.** If the
pass selects nothing — every endpoint is Penalized with a future deadline, or
Probing — the endpoint with the earliest `timestamp_ms` among the Penalized
and Probing words is used anyway, without claiming a probe and without altering
its deadline. A transient total blip must never become a self-inflicted outage
in which FAH refuses to send a packet. This rule overrides the deadline check in
S1.5 and is the single most dangerous behaviour to get wrong.

The forced attempt is recorded under the row of the state observed when it
lands, like every other outcome. Landing on a **Penalized** word: `attempts`,
`failures` and `consecutive_failures` move, deadline and `penalty_round` do
not, a success restores Healthy. Landing on a **Probing** word (the forced
endpoint had a probe in flight, or a probe was claimed after the forced
attempt was dispatched): the Probing row applies — a failure bumps
`penalty_round` and sets a new deadline, a success restores Healthy.
Deadline-and-round invariance is a property of the Penalized row, not of the
forced attempt; S1-G1 #6 and bench B.4 assert it on that row. No `Probing`
claim is made by a forced attempt and `probes` does not move.

## S1.4 Failure classification

**DNS RCODEs never affect health. Only transport outcomes do.** No Stage 1
transition can be triggered by NXDOMAIN, SERVFAIL or REFUSED.

| Outcome | Class | Effect |
| --- | --- | --- |
| Response with any RCODE, including NXDOMAIN | Success | `consecutive_failures = 0`; Probing or Penalized → Healthy (S1.3 transition rule) |
| Response, SERVFAIL | Success (transport) | as above |
| Response, REFUSED | Success (transport) | as above |
| Timeout | Hard failure | `consecutive_failures += 1` |
| `ENETUNREACH` / `EHOSTUNREACH` | Path failure | penalize immediately |
| `ECONNREFUSED` (ICMP port unreachable) | Path failure | penalize immediately |
| TLS handshake / certificate failure | Path failure | penalize immediately |
| Idle-close → transparent reconnect → answer | Success | `consecutive_failures = 0` |
| TC → TCP retry succeeds | Success | `consecutive_failures = 0` |
| Malformed or ID-mismatched datagram | ignored, wait continues | resolves as timeout |
| `ECONNRESET` on any transport | Hard failure | a mid-connection reset is transient; the encrypted transport reconnects inside the attempt |
| Local socket error — `EMFILE`, `ENOBUFS`, `EADDRNOTAVAIL`, bind failure | Hard failure | not an endpoint property, but not distinguishable per endpoint without a taxonomy; every endpoint fails together, all get penalized, the hard invariant keeps sending one packet per query |
| `rustls::Error` after the handshake (record decrypt) on an encrypted transport | Path failure | the transport maps every rustls error to `InvalidData`; handshake and post-handshake are not distinguished |

Path failures penalize at first occurrence because they are unambiguous
kernel/library signals rather than statistics. "Immediately" applies to an
endpoint eligible for the transition — Healthy or Probing. A forced attempt
against an already-Penalized endpoint (S1.3 hard invariant) records the path
failure in `failures` and `consecutive_failures` like any failure, and leaves
deadline and `penalty_round` unchanged.

Classification is by `io::ErrorKind` and is **transport-specific** in one
place: `InvalidData` is a path failure on an encrypted transport (its only
producer is a `rustls::Error` in the chain) and a hard failure on plain
(a TCP-leg wire-decode failure).

**Platform note.** `ECONNREFUSED` for a UDP upstream is a Linux property of a
`connect()`ed socket; Windows reports the same ICMP port-unreachable as
`ECONNRESET`, a hard failure. The dev box is Windows, so the UDP path-failure
row is exercised only on Linux — bench B.2's UDP arm (`ECONNREFUSED`) and
S1-L L.4b (real `EHOSTUNREACH` from an ARP failure on the LAN) — and the
dev-box path-failure tests use a DoT endpoint on a closed TCP port, which
yields `ECONNREFUSED` on both platforms.

**Why SERVFAIL must not penalize.** SERVFAIL usually means the *name* is broken
(DNSSEC failure, dead zone), not the endpoint. Distinguishing "this endpoint
SERVFAILs everything" from "this name SERVFAILs everywhere" needs per-name
state — unbounded, attacker-influenced, forbidden by hard rule 4.

**Deliberate consequence:** an upstream that SERVFAILs *everything* keeps
primacy forever. This is identical to today's behaviour — the ordered walk
returns the first `Ok`, and SERVFAIL is an `Ok` — so it is not a regression.
[`pipeline.rs:380`](../../crates/fah-dns/src/pipeline.rs#L380) treats upstream
SERVFAIL as an outage signal for serve-stale; that stays, and the two
definitions of "the upstream failed" deliberately differ. Do not "fix" one to
match the other.

### Attacker-triggerable transition

[`plain::query`](../../crates/fah-dns/src/upstream/plain.rs#L73) discards
non-matching UDP replies and keeps waiting until the timeout. An off-path
flood that guesses an endpoint's ephemeral port therefore produces a *timeout*,
which is a hard failure, which can drive that endpoint to Penalized.

Impact is bounded: every configured upstream is operator-chosen and equally
trusted, so the achievable effect is "steer between trusted resolvers", not
"inject an answer". The existing per-datagram checks are unchanged —
`connect()`ed socket, matching random ID, `MessageType::Response`. Recorded here
so it is a known property rather than a surprise.

## S1.5 Selection algorithm

**One pass** over config order, from the caller's `start` index. No sort, no
score, no allocation, no lock. Each endpoint is judged where the scan meets
it:

```text
for each endpoint from `start`, in config order:
    Healthy                           → use it, stop
    Penalized, deadline <= now        → CAS Penalized → Probing,
                                        use it as a probe (S1.7), stop
    Penalized, deadline > now         → skip
    Probing                           → skip
nothing selected                      → earliest `timestamp_ms` among the
                                        Penalized and Probing words from
                                        `start`: use it, claim nothing
                                        (S1.3 hard invariant)
```

The clock is read **lazily, at the first non-Healthy word, at most once per
call**; a pass that meets a Healthy word first never reads it. The selector
takes `claim: bool`. With `claim = false` a due endpoint is treated as not due
— skipped, nothing stored — so the pass falls through to the next Healthy
endpoint or to the forced candidate. The caller passes `false` in Ignore mode
(S1.8) and once a probe has already been claimed in the current query.

**Why one pass and not "Healthy first, then due".** A due endpoint ahead of a
Healthy one is probed by the next query that reaches it — that is what returns
a recovered primary to primacy, and it is the only way a penalized endpoint is
ever probed while another endpoint answers. A due endpoint *behind* the first
Healthy one is reached only when that Healthy endpoint fails on the same query;
its recovery is lazy and costs nothing, since config order would not select it
anyway.

On failure of the chosen endpoint, the scan continues from the next endpoint
under the same rules, exactly as today's walk continues — but skipping
Penalized endpoints. **A query claims at most one probe**: the caller keeps a
local `probed` flag and passes `claim = false` for the rest of that query once
a probe has been claimed, so `[due, due, Healthy]` costs one probe leg, not
two.

**Worst-case attempts per query is therefore ≤ today's**, since every call
returns an index ≥ `start` and Stage 1 tries a subset of what the current walk
tries plus at most one probe. No new deadline key is needed and none is added.

**Boot behaviour is bit-identical to today**: every endpoint starts Healthy, so
the pass selects the first configured server on the first query and on every
query thereafter while it answers.

Cost: ≤ 8 relaxed loads with an early exit on the first Healthy — in the
steady state, exactly one, and no clock read. Strictly less work than today's
loop, which re-enters the transport for every server until one answers.

## S1.6 Penalty

### Policy, not state machine

`penalty_failures`, `PENALTY_BASE`, `PENALTY_MAX` and the jitter are
**policy**: inputs to the S1.3 transition function, not part of it. They live
in one plain `Copy` struct built once at boot from `[dns.upstreams]`
(`PENALTY_BASE` is `10 × attempt_bound_ms`, below). No trait, no dispatch, no
second implementation. The transitions themselves are value-free — success, hard
failure, path failure, deadline passed — so changing a constant changes no
transition.

Two things follow. S1-G1 tests parameterize on the threshold and on scaled
penalty constants, so the value S1-G4 eventually derives changes zero tests;
and S1-G3's recovery criterion is benchable with a millisecond `PENALTY_MAX`
instead of a 300 s wait.

### Threshold

`penalty_failures` — consecutive **hard or path** failures before Penalized.
Path failures penalize at 1 regardless (S1.4).

**Shipped default: 2. Provisional, and explicitly not frozen.** The base rate is
now measured — 0.072 % on the primary, suite T sample 1 — but the base rate is
**not the quantity this constant depends on**. `penalty_failures = 2` fires only
on runs of two or more consecutive failures, so what settles it is the
**run-length distribution**, which the build collects since p2.5-06 as
`upstreams[].failure_runs` and which needs deploy time to accumulate (see
[Failure clustering](#failure-clustering--the-blocking-open-measurement)).

The value stays provisional until the observation window closes.
[Gate S1-G4](#s1-g4-constant-derivation) makes that a condition of closing
Stage 1.

Sensitivity at the measured rate, assuming independence — an assumption known to
be wrong for real outages, and shown here only to bound the isolated-loss case:

| `penalty_failures` | Expected firings at p = 7.19 × 10⁻⁴, isolated loss | Cost when genuinely dead |
| --- | --- | --- |
| 1 | ~14/day — every partial-failure event | up to 1 × `attempt_bound_ms` |
| 2 | ~1 per 87 days | up to 2 × `attempt_bound_ms` |
| 3 | negligible | up to 3 × `attempt_bound_ms` |

Read this the right way round: **if the observed failures are isolated single
losses, `penalty_failures = 2` almost never engages and Stage 1 saves almost
nothing.** If instead a real outage produces a sustained run, it engages
immediately and saves the full tax. The observation window decides which
deployment this is, and it is the single most decision-relevant measurement
outstanding.

### Duration and backoff

```text
attempt_bound_ms = ATTEMPT_LEGS × timeout_ms     (2 400 ms at the default 800 ms)
penalty(round)   = min(PENALTY_BASE << (round - 1), PENALTY_MAX) ± 25 % jitter
PENALTY_BASE     = 10 × attempt_bound_ms          (24 s at the default)
PENALTY_MAX      = 300 s                          (rounds: 24, 48, 96, 192, 300 s)
```

`attempt_bound_ms` is the wall-clock bound of one attempt —
[`UpstreamServer::query`](../../crates/fah-dns/src/upstream/mod.rs#L371) wraps
the `ATTEMPT_LEGS` (3) legs an attempt can take (UDP then TCP retry; or slot
wait, connect, exchange) in `timeout_ms × ATTEMPT_LEGS`. `timeout_ms` itself
bounds one leg. A plain UDP attempt against a black hole costs one leg; the
bound is what penalty timing and cost accounting use, so they hold in the
worst case. The code names the per-leg value `attempt_timeout`; this document
never uses that name for the bound.

| Constant | Basis |
| --- | --- |
| `PENALTY_BASE` | **Derived.** The penalty must exceed one attempt's bound by enough that a retry landing on a still-dead endpoint is negligible against the penalty. Any factor of roughly 5–20 of `attempt_bound_ms` satisfies this; 10 is the round number inside that range (30× the typical one-leg attempt), and the value is not measurement-sensitive |
| `PENALTY_MAX` | **A policy choice, not a derived value.** It states how long a stale penalty is tolerable with no operator action. 300 s is chosen because an operator restarts the container over anything longer. Nearby existing intervals for scale: `REFRESH_FAILURE_COOLDOWN` 30 s, `FailureAlarm::WARN_INTERVAL` 30 s |
| Jitter ±25 % | **Arbitrary and admitted.** Taken from the low 8 bits of `now_ms` when a penalty is applied (a cold path) — deterministic, no RNG, no cryptographic requirement. It desynchronises one endpoint's successive penalty windows; two endpoints penalized in the same millisecond share the jitter and keep synchronised deadlines, which is accepted: every probe is one query's primary attempt and ≤ 8 endpoints cannot storm |

`penalty(round)` is defined for `round ≥ 1` only; `next_round` never yields 0
(a reset yields 1), and the implementation `debug_assert`s it.

`penalty_round` restarts at round 1 only when the endpoint has been continuously
Healthy for `PENALTY_MAX`, checked from `healthy_since` at the moment the *next*
penalty is applied — so it costs the query path nothing. The stored round is
written only when a penalty is applied, so recovery never clears it: 0 is the
value an endpoint carries until its first penalty, and nothing returns it there. Without the rule a flapping
endpoint oscillates forever at `PENALTY_BASE`; without the reset it ratchets to
`PENALTY_MAX` and stays there. The check is `now.saturating_sub(healthy_since)
>= PENALTY_MAX` (S1.3, concurrent `now` sampling).

`timeout_ms` is validated to **1..=10 000** (S1.13). At 0 every penalty is
zero-length and every query probes; above 10 000 `PENALTY_BASE` exceeds
`PENALTY_MAX` and the backoff disappears. Neither is a configuration this
design supports.

## S1.7 Recovery probing

**On-path. No background task, no clock inside `fah-dns`.** Principle 6: the
library holds pure logic; the binary owns timers.

A probe is a **primary attempt**, not a hedge. A query whose selection pass
(S1.5) meets a Penalized endpoint whose deadline has passed — before any
Healthy endpoint — claims that endpoint (`Penalized → Probing`, one
`compare_exchange_weak`, so exactly one probe is in flight per endpoint) and
sends its query there. A lost CAS is not retried: the loser continues its pass
as if the endpoint were Penalized with a future deadline.

- Probe answered → Healthy, `consecutive_failures = 0`, `healthy_since = now`,
  `penalty_round` kept.
- Probe failed → Penalized, `consecutive_failures += 1`, `penalty_round += 1`,
  new deadline; counted as a penalty (S1.3).

Both apply to the state observed when the probe lands (S1.3 transition rule).
A probe is an ordinary attempt for every counter — `attempts`, `failures`,
`failure_runs` — plus `probes`, incremented at the claim, and
`probe_successes`, at the answer. Only a Record-mode query claims (S1.8), and
at most one per query (S1.5).

**Invariant: a Record-mode `forward` future is never dropped before it
completes.** `Probing` has no timeout; the claim is released only by the
outcome landing, and a word left in Probing with no probe in flight is skipped
by every pass and reached only as a forced candidate — the endpoint is out
until the next restart. Today's Record callers satisfy the invariant: the UDP
listener spawns one task per datagram and never aborts it, the TCP listener
awaits the pipeline inline, SWR workers are aborted only at shutdown. A future
client-side deadline (`select!` or `timeout` around `forward`) must not be
added without a claim-release path.

On an encrypted endpoint the probe may have to pay a new handshake, because the
previous connection was idle-closed during the penalty; that handshake failing
is a probe failure. Penalizing never closes a connection — the existing
idle-close path does (S1.17).

**Cost, stated numerically:** one query pays up to `attempt_bound_ms` per
penalty window per endpoint. At `PENALTY_MAX` = 300 s and 0.68 QPS that is one
query of at most 2.4 s (800 ms for plain UDP) per five minutes, ≈ 0.03 % of
queries.

**Stage 3 relationship, if Stage 3 is ever built:** Stage 3 may *relocate* the
probe onto its hedge leg so that no client waits on a suspect endpoint. That is
an optimisation of this mechanism, not its definition. Stage 1 is complete
without it.

Rejected alternative — a background prober: one extra task, a clock in the
library, and upstream traffic while idle, in exchange for detecting recovery
when there is no traffic to serve. Recovery matters when traffic exists.

## S1.8 `resolve_host` health isolation

**Required, not optional, and load-bearing.**
[`UpstreamPool::resolve_host`](../../crates/fah-dns/src/upstream/mod.rs#L115)
issues A and AAAA concurrently through the same `forward`, and on a single-stack
path one leg is *designed* to fail — that is the p1-11 defect-2 behaviour pinned
by `resolve_host_succeeds_when_only_one_address_family_answers`.

**Measured share: ~20 % of all upstream attempts** (8,384 of 41,710, suite T
sample 1). This is not a list-fetch bootstrap edge case. The dominant caller is
the HTTP pipeline's `HostResolver`, which resolves every proxied destination —
two lookups per request — so `resolve_host` is one upstream attempt in five
whenever the HTTP engine carries traffic.

Without isolation, a single-stack condition would inject a transport failure
against a perfectly healthy endpoint on that same one-in-five cadence, which at
the observed volume would dwarf the real failure signal by two orders of
magnitude (8,384 spurious versus 30 genuine over 45.7 h).

Specification: `forward` takes an internal health mode.

| Caller | Mode | Rationale |
| --- | --- | --- |
| `Forwarder::forward` — the pipeline | Record | Real client queries are the health signal |
| SWR workers (S1.9) | Record | Real queries against real endpoints |
| `resolve_host` → `lookup` | **Ignore** | A deliberately-failing family leg is not endpoint evidence |

In Ignore mode no counter moves, no state transition occurs, and **no probe is
claimed**: the selector runs with `claim = false` (S1.5), so a due endpoint is
treated as not due and the pass falls through to the next Healthy endpoint or
to the forced candidate, writing nothing. A claim without a recorded outcome
would leave the word in Probing with no probe in flight (S1.7 invariant).
Selection still skips Penalized endpoints, so a bootstrap benefits from health
learned elsewhere without contributing to it. No new trait and no second pool.

**Consequence for `attempts` and `failures`:** under `adaptive` they exclude
`resolve_host` traffic — about 20 % of attempts on the measured mix — while
`fallback` counts it. The two strategies' counters are therefore not
comparable across the opt-in flip, suite T's `failures / attempts` base rate is
a `fallback`-window quantity, and p2.6-11 states this beside every `attempts`
figure. `tls_handshakes` is a connection counter and still moves in Ignore
mode (S1.17).

## S1.9 SWR interaction

[SWR workers](../../crates/fah-dns/src/swr.rs) call `Forwarder::forward` with a
clone of the same pool. Under Stage 1 that means background refreshes both
**record health** and **may claim probes**.

**SWR is the dominant producer of upstream health observations and probe
claims** — not a supplementary source. Measured share: **~69 % of all upstream
attempts** (28,809 of 41,710, suite T sample 1), against 10 % for client DNS
forwards. Stage 1's health state will therefore be learned mostly from
background refreshes, at all times, not merely during quiet hours.

The intended behaviour is unchanged by that correction, and both parts remain
right:

- A refresh failure is a real transport failure against a real endpoint.
  Excluding the 69 % majority of attempts would discard most of the available
  signal and leave health learning to a 10 % minority.
- A refresh claiming a probe is strictly better than a client query claiming it:
  the client is already answered from stale cache and waits on nothing
  ([ADR-0005](../decisions/0005-serve-stale-while-refresh.md)). At this traffic
  mix most probes will be claimed by SWR, which means the S1.7 probe cost is
  largely paid off the client path.

Consequence for interpretation: a health transition will usually be triggered by
traffic no client is waiting on. That is desirable, and it means client-visible
latency is a poor proxy for how much health learning is happening.

Bounds already in place, so this adds no unbounded work: at most
`[dns.cache] swr_workers` refreshes are in flight (default 3); enqueue is
`try_send` on a bounded queue and drops rather than blocking; and
`REFRESH_FAILURE_COOLDOWN` (30 s) already suppresses repeat refreshes of a key
whose refresh failed.

**Consequence to expect in a soak:** during an outage, `attempts` and `failures`
grow far more slowly than today, because penalized endpoints stop being
attempted. A flattening counter is the feature working, not the outage ending.

## S1.10 Concurrency and thread-safety

- `Arc<[Health]>` and `Arc<[Endpoint]>`, both immutable after boot. No `Mutex`,
  no `RwLock`, no `ArcSwap` on the query path.
- **All loads and stores are `Ordering::Relaxed`.** These values are hints, not
  synchronisation for other data: nothing here publishes a pointer or guards an
  initialisation, no happens-before relationship is required, and a marginally
  stale read selects a marginally different endpoint, which is harmless.
- Transitions use `compare_exchange_weak` on the single packed word. They are
  cold — once per outage, once per probe — never per query.
- **Every mutation of the packed word is a CAS loop with saturation. Never
  `fetch_add` on it.** `consecutive_failures` saturates at 255 and
  `penalty_round` at 15; a `fetch_add(1 << 6)` at 255 carries into bit 14 and
  corrupts `timestamp_ms`. The six plain counters are independent `AtomicU64`s
  and keep `fetch_add`. S1-G1 #11 is the concurrent saturation test.
- The existing `ExchangeConn::slot` mutex is unchanged and still contended only
  on (re)connect.
- **No timers are added.** `Instant::now()` is read lazily — at the first
  non-Healthy word a selection pass meets, and once more when an outcome is
  recorded off the clean-Healthy path (S1.5, S1.11). The all-Healthy steady
  state reads no clock. A clock jump cannot affect anything — `Instant` is
  monotonic.

## S1.11 Hot-path requirements

| Path | Allocations added | Locks | Syscalls added | Timers added |
| --- | --- | --- | --- | --- |
| Blocked query | 0 | none | 0 | 0 |
| Cache hit | 0 | none | 0 | 0 |
| Forward, all endpoints Healthy | **0** | none | 0 | 0 |
| Forward, a non-Healthy endpoint met or an outcome recorded off the clean path | **0** | none | ≤ 2 `clock_gettime` (vDSO, no kernel entry) | 0 |

Stage 1 adds no allocation anywhere, and the steady-state selection is one
relaxed load. It removes work rather than adding it: a penalized endpoint is
skipped instead of being dialled.

Budget: `forward`-stage p99 must not regress against `strategy = "fallback"` on
a healthy link. `cache_hit` and `block` stages are untouched by construction and
serve as the control arm.

## S1.12 Telemetry

`UpstreamStatus` → `fah_model::UpstreamSample` → `/api/v1/telemetry`, pulled by
the existing 10 s poll ([`main.rs:874`](../../crates/fastadhunter/src/main.rs#L874)).
No push path changes.

| Field | New? | Answers |
| --- | --- | --- |
| `state` | yes | `healthy` \| `penalized` \| `probing` — **the authoritative liveness signal under `adaptive`**; under `fallback` every endpoint reports `healthy` and the field carries no information |
| `penalty_round` | yes | how deep the backoff is |
| `penalties` | yes | how often this endpoint has been penalized — a failed probe counts (S1.3) |
| `penalized_seconds_total` | yes | **scheduled** unavailability — see below |
| `probes`, `probe_successes` | yes | whether recovery works; a probe is an ordinary attempt for every other counter |
| `family` | yes | informational; nothing selects on it in Stage 1; `null` for a hostname DoH URL |
| `attempts`, `failures` | existing | changed *rate* (S1.9) **and changed coverage**: under `adaptive` `resolve_host` attempts are not counted (S1.8) |
| `failure_runs` | existing (p2.5-06) | run length in **attempts**; under `adaptive` attempts are throttled, so a run is not a duration |
| `consecutive_failures` | existing | **demoted to a diagnostic** |
| `tls_handshakes` | existing | unchanged |

### Probe and penalty semantics

- A probe counts in `attempts`, `failures` and `failure_runs` like any attempt;
  `probes` moves at the claim, `probe_successes` at the answer. A probe failure
  increments `consecutive_failures` and therefore extends the open run, and the
  run definition — closed by that endpoint's next transport success — stays
  literally true. A probe failure is also a penalty: `penalties += 1` and
  `penalized_seconds_total` accrues the new nominal duration (S1.3).
- **Under `adaptive`, `failure_runs` measures throttled attempts, not elapsed
  outage time**: a ten-minute outage costing two failures and three failed
  probes is one run of 5. The S1-G4 window is read on the `fallback` build and
  is unaffected.
- `penalized_seconds_total` is **scheduled unavailability**: the nominal
  `penalty(round)` accrued when the penalty is applied, on the cold path, with
  no extra state. The packed word holds the deadline, not `penalized_since`,
  and an eighth field breaks the 64 B assertion. Actual unavailability can
  exceed the figure — no traffic claims the probe after the deadline — or fall
  short of it — a stale in-flight success restores Healthy early (S1.3).
- **Who claimed a probe — SWR or a client query — is not reported.** It would
  be an eighth field; the 64 B budget does not permit it.

Two semantic changes that must be documented in the same change (`.md` edits
require owner approval per the working agreement):

1. **`consecutive_failures` freezes** once an endpoint stops being attempted, so
   it is no longer "how broken is this right now". Its doc comment currently
   says "**Not a liveness signal**", which stays true — but `state` is now the
   signal, and `/health` degraded should be restated from "every server has
   `consecutive_failures > 0`"
   ([`adapters.rs:234`](../../crates/fastadhunter/src/adapters.rs#L234)) to
   "**no endpoint Healthy**" — every endpoint Penalized or Probing — which is
   what it was trying to express. Not "every endpoint Penalized": a probe in
   flight is not recovery, and the literal reading would flip `/health` back to
   ok for one attempt bound on every probe during an outage. Under `fallback`
   the existing rule stays.
2. [measurement-traps.md](../measurement-traps.md) §Traffic and rates — the
   "Upstream `failures` as client timeouts" entry gains the S1.9 case: counters
   grow more slowly during an outage because penalized endpoints are skipped,
   and `failure_runs` counts throttled attempts, not time.

## S1.13 Configuration surface

**Exactly one new enum variant and one new key.** `deny_unknown_fields` on the
root `Config` makes later key *removal* a boot failure with no migration — the
trap recorded in [project-state.md](../project-state.md) — so every speculative
key added now is permanent. Stage 1 ships only what Stage 1 uses.

```toml
[dns.upstreams]
strategy = "adaptive"      # boot — the only value since p2.6-12; "fallback" was the default until 0.3.3
timeout_ms = 800           # boot — unchanged, per leg; an attempt is bounded at ATTEMPT_LEGS × timeout_ms
penalty_failures = 2       # boot — consecutive transport failures before penalty
```

Not exposed, and not to be added "while we are in there":

| Not a key | Because |
| --- | --- |
| `penalty_base_ms` | Derived: `10 × ATTEMPT_LEGS × timeout_ms` (S1.6) |
| `penalty_max_ms` | Policy constant, 300 s (S1.6) |
| `query_deadline_ms` | Unrelated behavioural change; Stage 1's worst case is already ≤ today's |
| Every `hedge_*`, `preferred_family`, band width | Stage 2/3. Adding them inert now makes them permanent |

`penalty_failures` needs an `env.rs` override entry
([`env.rs:79`](../../crates/fah-config/src/env.rs#L79)) and a CONFIGURATION.md
`[dns.upstreams]` section update — section only, per principle 19.

`timeout_ms`, currently unbounded, gains a validated range of **1..=10 000**
(S1.6). This is the one change to an existing key: a config above 10 s fails
validation on the new binary. None is known; the default is 800.

## S1.14 Backward compatibility

- **`strategy = "fallback"` remains the default and keeps the existing code
  path, untouched.** It is not re-expressed as "adaptive with penalties
  disabled" — a compatibility mode that is a rewrite is not a compatibility
  mode. Every existing upstream test keeps its assertions and its observed
  behaviour. The one permitted edit: the nine `DnsUpstreamsConfig { .. }`
  struct literals across the workspace gain the `penalty_failures` field,
  because the type gains it (p2.6-01 lists the sites).
- **Flipping the default to `adaptive` is a separate, explicitly-approved
  step**, gated on the deployment tier of the acceptance gates below — S1-G2
  tiers 2–3, S1-G4, S1-G5 — plus a 7-day soak. Until then `adaptive` is
  opt-in.
- The `fallback` path is deleted (principle 14) only after that flip, not before.
- **Executed as p2.6-12** (review: `docs/code-review/phase2.6/p2.6-12-default-flip-review.md`):
  default `adaptive`, `fallback` deleted, a config naming it rejected at load.
- `penalty_failures` takes `#[serde(default)]`, so a config written for the
  current release boots unchanged on the new binary.
- **The reverse bricks the resolver.** Deploy order is fixed: **binary first,
  config second.**
- `UpstreamSample` gains fields; the JSON is additive and the dashboard does not
  exist yet.
- Wire behaviour is unchanged: same query, same randomised IDs, DNSSEC still
  pass-through, no answer synthesised, compared or reordered.

## S1.15 Failure scenarios

| Scenario | Behaviour |
| --- | --- |
| **All endpoints Penalized, no deadline passed** | Earliest-deadline endpoint used anyway, claiming nothing (S1.3 invariant). Never "refuse to send" |
| Primary dead, secondary healthy | `penalty_failures` queries pay up to `attempt_bound_ms` each; every later query skips the dead endpoint at one relaxed load — except one probe-carrying query per penalty window, which pays up to `attempt_bound_ms` while the primary stays dead (S1.7 cost). When the primary returns, the first Record-mode query after its deadline probes it (S1.5 one pass — the Healthy secondary does not suppress the probe) and it regains primacy |
| Primary dead with ICMP unreachable | 1 query pays; the rest skip |
| `resolve_host` meets a due endpoint | Not probed (Ignore never claims, S1.8); answered by the next Healthy endpoint or the forced candidate |
| Full WAN outage | Every endpoint penalized; one probe per `PENALTY_BASE` per endpoint; clients live on cache and serve-stale; `FailureAlarm` still fires |
| Endpoint flapping | `penalty_round` climbs, backoff reaches `PENALTY_MAX`; reset needs `PENALTY_MAX` of continuous health |
| Upstream SERVFAILs everything | Keeps primacy — **not solved**, identical to today (S1.4) |
| Upstream slow but answering | Keeps primacy — **out of Stage 1 scope**; this is what Stage 2/3 would address |
| Delegated-prefix rotation | Nothing. `veth1` re-addresses `from-pool`; UDP upstreams bind `UNSPECIFIED` and pick the new source on the next query. A persistent v6 DoT/DoH connection reconnects, which is not a failure |
| List refresh on a single-stack link | No health effect — S1.8 |
| SWR refresh storm during an outage | Bounded by `swr_workers`, `try_send` and `REFRESH_FAILURE_COOLDOWN` — S1.9 |
| Clock jump | No effect; all timing is `Instant` |
| Config of 8 dead endpoints | 512 B of state, ≤ 8 attempts per query (≤ today's), one probe claim at a time |

## S1.16 Security and resource exhaustion

- **No attacker-influenced key anywhere.** State is indexed by config position;
  no query, client or name can create an entry. This is why Stage 1 has no
  per-name and no per-client dimension, and why — unlike Unbound's
  `infra_cache`, keyed by address × zone and needing its own size cap — it needs
  no cache bound at all.
- **No amplification.** Stage 1 never sends more than one query at a time and
  its worst-case attempt count is ≤ today's. Upstream query volume can only fall.
- **No new file descriptors, no new timers, no new tasks.**
- **State memory is fixed at 512 B**, statically asserted.
- Attacker-triggerable penalty via a port-guessing flood: bounded impact,
  documented in S1.4.
- No new `unsafe`, no new crypto, no new dependency.

## S1.17 Connection establishment vs. health

Origin: p2.5-03 review finding F2
([review](../code-review/phase2.5/p2.5-03-encrypted-reconnect-review.md)).

**Invariant: health reads only attempt outcomes.** A TLS handshake or
certificate failure is an attempt outcome — a path failure, S1.4. Idle-close,
reconnect and the `ExchangeConn` slot state are connection lifecycle, not
health observations, and move no counter. Penalizing an endpoint does not close
its connection; idle-close does. A probe on an encrypted endpoint may pay a new
handshake, and that handshake failing is a probe failure.

- Selection must skip a Penalized endpoint **before** connection
  establishment: no query may wait on a per-endpoint connect for an endpoint
  already known unhealthy.
- Connection creation itself stays **single-flight**: one handshake per
  endpoint at a time; concurrent queries adopt the connection it produces,
  never duplicate it.
- The current `ExchangeConn` slot lock (held across `connect()`) already
  satisfies the single-flight half and is kept. Stage 1 supplies the skip;
  it must not replace the lock with connect-outside-lock or connect-ahead.

Tests (Stage 1 acceptance):

| Scenario | Assertion |
| --- | --- |
| N concurrent queries against `[penalized-dead, live]` | all answer within one `attempt_bound_ms` of the live endpoint; the dead endpoint's `tls_handshakes` stays unchanged after penalization |
| N concurrent first queries against one live endpoint | exactly 1 handshake |
| Idle-close, transparent reconnect, answer | no counter moves, `state` unchanged |

---

# Stage 1 acceptance gates

Stage 1 is closed only when **all five gates are decided and S1-G5 is not
triggered**. They split by when they can be decided:

| Tier | Gates | Decides |
| --- | --- | --- |
| **Implementation / merge** | S1-G1, S1-G2 tier 1, S1-G3 on the injected-failure bench | `adaptive` ships opt-in (S1.14) |
| **Deployment / default flip** | S1-G2 tiers 2–3, S1-G4, S1-G5, plus the 7-day soak of S1.14 | `adaptive` becomes the default; `fallback` is deleted |

Nothing in the first tier waits on deploy time; nothing in the second is
decidable from the dev box. Each gate is decidable independently of Stages 2
and 3.

## S1-G1 Correctness

All must hold; none is a measurement. Tests parameterize on `penalty_failures`
and on scaled penalty constants (S1.6, policy), so a later change to the
shipped value changes no test.

| # | Gate |
| --- | --- |
| 1 | gates green — fmt, clippy `-D warnings`, `test --workspace` |
| 2 | `crates/fastadhunter/tests/layering.rs` passes unchanged |
| 3 | `const _: () = assert!(size_of::<Health>() == 64)` compiles |
| 4 | Every existing upstream test passes under `strategy = "fallback"` with its assertions and observed behaviour unmodified; the only edit is the `penalty_failures` field added to `DnsUpstreamsConfig` struct literals (S1.14) |
| 5 | Test: NXDOMAIN, SERVFAIL and REFUSED each classify as success (p2.6-04, `classify` over the three `rcode_udp_server` results) **and** leave `state` and `consecutive_failures` unchanged end-to-end under `adaptive` — owned by bench B.6 (p2.6-09, merge tier) |
| 6 | Test: with every endpoint Penalized and no deadline passed, a query is still sent to the earliest-deadline endpoint; no `Probing` claim, `probes` unchanged; after the forced attempt fails **on the Penalized word**, deadline and `penalty_round` are unchanged and `consecutive_failures` moved. A forced failure landing on a Probing word follows the Probing row (S1.3) and is not asserted here |
| 7 | Test: `resolve_host` with one family black-holed moves no counter and no state (S1.8) |
| 8 | Test: concurrent queries against a due endpoint produce exactly one `Probing` claim — with no other endpoint (the rest are forced onto it) **and** with a Healthy endpoint configured behind it (the rest answer from the Healthy one; the due endpoint ahead of it is still probed exactly once — S1.5 one pass) |
| 9 | Test: a probe failure increments `consecutive_failures` and `penalty_round` and counts a penalty; a probe success restores Healthy |
| 10 | Test: `penalty_round` resets only after `PENALTY_MAX` of continuous health; `now.saturating_sub(healthy_since)` with `healthy_since > now` does not reset (S1.3) |
| 11 | Test: `consecutive_failures` at 255 under N concurrent failures saturates with `timestamp_ms` and the state bits intact; `penalty_round` at 15 stays 15 across a sequential claim → probe-failure cycle repeated 16 times (S1.10) |
| 12 | Test: Penalized + success → Healthy, `consecutive_failures = 0`, `healthy_since = now`, `penalty_round` kept (S1.3) |
| 13 | Test: a probe landing after a stale success moved the endpoint to Healthy is an ordinary outcome — failure gives `consecutive_failures = 1`, `penalty_round` unchanged (S1.3) |
| 14 | Test: idle-close, reconnect, answer moves no counter and leaves `state` unchanged — end-to-end through `forward` under `adaptive` (S1.17) |
| 15 | Test: `penalized_seconds_total` grows by the nominal `penalty(round)` at application, not at recovery (S1.12) |
| 16 | Test: Ignore never claims — `[due, Healthy]` and a `resolve_host` call: answered by the Healthy endpoint, the due word byte-for-byte unchanged, `probes` unchanged (S1.8). `[due]` alone: the attempt is sent forced, the word unchanged |
| 17 | Test: at most one probe per query — `[due, due, Healthy]` with the first probe failing: exactly one `Probing` claim, the second due endpoint skipped, the Healthy one answers (S1.5) |

## S1-G2 No regression

**The threshold is not yet frozen.** An earlier draft set it at ±2 % on
healthy-link `forward` p99. Two findings invalidate that as written, and the
gate is restructured rather than renumbered.

### Why ±2 % was withdrawn

This project's own recorded control-arm behaviour already exceeds it:

| Evidence | Figure | Source |
| --- | --- | --- |
| `http_pass_through/direct_to_origin` — an arm that never touches the proxy — session-to-session drift | **+8.8 %** | [measurement-traps.md](../measurement-traps.md) §Carry a control arm |
| Deployed corpus at 8 KiB, two on-device sessions | **−4.6 %** | same |
| Unpinned dev-box benches, consecutive runs of an unmodified binary | **6×** | PERFORMANCE.md §Measuring reliably |
| Pinning is forbidden for multi-threaded harness benches | ~500× spread when misapplied | same |

The gate's metric is a **tail statistic**, under a multi-threaded Tokio load,
network-I/O-bound, in a probe container sharing a 1 GB box with the production
container and postgres, and structurally un-pinnable. Two quieter arms on this
project already drifted 4.6 % and 8.8 %. A ±2 % gate sits below the demonstrated
noise floor and would either fail spuriously or be waved through — worse than no
gate. Quantile sampling error is not the constraint: at 10 000 QPS for 10 min
there are ~6 M forwards and ~60 k tail samples, so sampling error is likely
sub-1 %. Session-to-session systematic drift dominates.

### Why the metric's coverage is also wrong

Client DNS forwards are **~10 % of upstream attempts** (Upstream traffic
composition, above). `forward`-stage p99 therefore observes a tenth of the
traffic Stage 1 actually acts on, and none of the SWR path where ~69 % of health
transitions and probe claims will occur.

### Structure of the replacement

**Tier 1 — precision, at the microbench.** S1-M is pinned, single-core and
CPU-bound, and holds a CI under 1 % by the project's proven recipe. Stage 1's
healthy-path claim is structural — one relaxed load, early exit on the first
Healthy endpoint, zero allocations — and is falsifiable there at nanosecond
resolution. **This is where "not slower than `fallback`" is decided.**

**Tier 2 — zero-variance invariants, on the live run.** Counts, not timings, so
they have no noise floor and no threshold to derive:

| Invariant | Criterion |
| --- | --- |
| Penalties applied on a healthy link | `penalties == 0` on every endpoint in L.1 and L.2 (mock upstreams that never fail). On L.3, with real upstreams, the count is **report-only** — it is S1-G4's false-penalty row, not a pass/fail here |
| Upstream attempts per forwarded query | `(Σ attempts − (swr.completed + swr.failed)) / cache_misses == 1.000` in both arms, every term a delta over the run. Comparable only between L.1 and L.2, which carry no HTTP traffic: under `adaptive` `attempts` excludes `resolve_host` (S1.8) |
| Allocations added on the forward path | 0 — measured at S1-M M.9 and `tests/forward_alloc.rs`, where it is exact |
| `state` during the healthy arm | never leaves Healthy — decided from `penalties` and `probes` deltas == 0, since the 10 s poll cannot see a ≤ `attempt_bound_ms` probe |
| SWR outcomes | `swr.failed == 0` and `swr.dropped == 0` in both arms; `swr.completed` reported. Exact equality of SWR counts between two runs is not achievable and not a criterion |
| RSS slope over the final third of a 24 h soak | < 2 MB half-to-half drift ([measurement-traps.md](../measurement-traps.md); mimalloc's purge band alone is ±6 MB) |

A timing wobble with identical counts is the box, not a Stage 1 defect. These
catch every behavioural regression exactly.

**Tier 3 — gross-regression check, threshold derived, not chosen.** Frozen only
after suite S1-N (the null A/B) reports the harness's noise band. Until then the
gate carries no number.

### Dimensions that must be validated before the threshold is frozen

| Dimension | Why |
| --- | --- |
| Noise band **N** from a null A/B — one binary, labels only, K ≥ 6 alternating repetitions | The smallest effect this harness can resolve. Anything below N is unmeasurable by construction |
| Which metric carries the gate | `forward` p99 covers 10 % of upstream traffic. Total upstream attempts, or attempts-per-forward, cover all of it and are far quieter |
| Whether the SWR path needs its own arm | 69 % of attempts are invisible to the client-latency metric |
| Control-arm drift across the same null runs | If `cache_hit` moves more than `forward`, the box drifted and the session is void |
| Repetition count vs achievable resolution | Precision improves as √K, cost as K. If K = 12 still leaves N wide, the answer is to drop the live timing gate, not to run more |

Freezing rule, once S1-N has run: threshold = **max(2 × N, 5 %)**. The 2× keeps
a single excursion from failing the gate; the 5 % floor exists because a quieter
arm on this project already drifted 4.6 %, so claiming finer resolution on a
noisier one would not be credible. The metric is chosen by a **fixed order,
not a judgment**: total upstream attempts if its N ≤ 10 %, else `forward` p99
if its N ≤ 10 %, else **tier 3 is dropped** and the healthy-path claim rests
on tiers 1 and 2 — an honest outcome, not a failure.

Throughput: **≥ 10 000 QPS sustained**, absolute. The relative half — "no
worse than `fallback`" — uses the S1-N threshold on QPS while tier 3 is alive
and is dropped with it.

On a healthy link Stage 1 and today's walk are identical by construction, so
**this gate can only detect a regression — it can never show a gain.** Gains
live in S1-G3.

## S1-G3 Behavioural win

Measured against `strategy = "fallback"` in the same session.

| Scenario | Criterion |
| --- | --- |
| Black-holed endpoint, healthy alternative | With queries issued **sequentially**, ≤ `penalty_failures` **non-probe** queries pay up to `attempt_bound_ms`; thereafter only probe-carrying queries pay, one per penalty window while the endpoint stays dead, and they are reported as `failed_probes`, never as the tax. Every other query answers at the healthy endpoint's RTT. The tax-free share is stated **per tested `PENALTY_MAX / PENALTY_BASE` ratio** — it is a function of run length over the backoff schedule, not a constant. Concurrent queries dispatched before the penalty lands also pay (S1.3) and are outside this count |
| ICMP-unreachable endpoint | Penalized after 1 attempt regardless of `penalty_failures`, and ≤ 1 query pays anything. **A classification check, not a latency win**: a refused endpoint costs ~0 under `fallback` too (immediate error, then the next endpoint), so this scenario contributes nothing to the net-cost rows. Decidable for UDP on Linux only (S1.4 platform note); on the dev box the arm is a DoT endpoint on a closed TCP port |
| Recovery after the endpoint returns | Healthy again within `PENALTY_MAX` + one query interval of the endpoint's return — the interval is bench B.5's stated cadence (sequential, 20 QPS scaled: 50 ms) — with the alternative endpoint Healthy throughout (S1.5 one pass) |
| Concurrent probe claims | never more than one in flight per endpoint: the mock's high-water mark of outstanding requests at the endpoint while Penalized/Probing ≤ 1 (B.5); the concurrent claim itself is S1-G1 #8 |
| Flapping endpoint | `penalty_round` strictly increases across consecutive penalties until the cap; `probes` delta ≤ penalty windows elapsed in the phase (no probe storm); `p99_flapping ≤ 1.1 × p99_black_hole` on the same run |
| Net timeout cost avoided | reported in two rows, client-visible and SWR — see below |

### Net timeout cost avoided

"Queries that pay the attempt bound" is the wrong unit on its own: it counts a
probe and a client timeout alike. Report

```text
adaptive_paying = paying queries that did NOT carry a probe
failed_probes   = probe-carrying queries whose probe failed (each paid once)
probe_cost      = failed_probes × attempt_bound_ms
net_avoided     = (fallback_paying − adaptive_paying) × attempt_bound_ms − probe_cost
```

**`adaptive_paying` excludes probe-carrying queries.** A query that paid
because it carried a failed probe is counted once, in `probe_cost`; counting
it in `adaptive_paying` as well subtracts it twice and understates the win
by one attempt bound per probe. With sequential issue the `probes` delta
around each query identifies the carrier exactly. A successful probe answers
at the endpoint's RTT and pays nothing.

State the inputs with `attempt_bound_ms` as the cap (S1.6) and the
**measured** time paid beside it — a plain-UDP black hole costs one leg, so
the measured figure is the honest one and the bound is the worst case —

in **two rows, never one**: client-visible forwards (black hole and recovery
scenarios; the ICMP-unreachable scenario contributes nothing, above) and SWR
refreshes. SWR is ~69 % of attempts (Upstream traffic composition) and a
refresh timeout is not client latency
([ADR-0005](../decisions/0005-serve-stale-while-refresh.md)), so the aggregate
overstates the client benefit by up to that share. Interpret cache-hit-aware,
as Stage 3's amplification figure must be: paying queries are a fraction of
forwards, which are a fraction of client queries. The raw attempt count is not
a client-visible benefit.

## S1-G4 Constant derivation

`penalty_failures = 2` is **provisional** and may not be frozen without this
gate.

| Gate | Status | Criterion |
| --- | --- | --- |
| Base rate documented | **Done** — 0.072 % primary, suite T sample 1 | Confirmed over the full window, not one snapshot |
| Partial-failure population exists | **Done** — 27 events / 45.7 h | — |
| **Run-length distribution** | **Open — blocking, now collecting** | Whether the observed failures are isolated losses or sustained runs. This, not the base rate, is what `penalty_failures` depends on. Source: `upstreams[].failure_runs` (p2.5-06), read as the sum of per-process deltas over the `fallback` window (counters reset on restart); the analysis states the concurrency-splitting caveat |
| `penalty_failures` chosen from the run-length data | Open | Not from the illustrative table in S1.6 |
| False-penalty rate on a healthy link | Open | Measured and stated. No pre-set threshold is imposed, because none is justified by evidence |

The instrumentation gap that used to threaten this gate is closed (p2.5-06), so
the `max_consecutive_failures` high-water mark is no longer a candidate. What
remains is deploy time: if the window closes with too few closed runs to read,
the choice is between shipping `penalty_failures` as an admitted guess and
extending the window. Prefer extending it.

## S1-G5 Rejection

Stage 1 does **not** ship if both hold:

1. Telemetry over the observation window shows no *partial* upstream failure —
   no interval in which one endpoint failed while another answered; and
2. S1-G3 confirms the win exists only in the injected-failure benches.

In that case Stage 1 solves a problem that does not occur on this deployment,
and the correct outcome is to record the measurement and stop.

**Condition 1 is already false.** Suite T sample 1 records 27 partial-failure
events over 45.7 h. This rejection route is closed unless the full window
contradicts the sample.

A *second*, narrower rejection route is now open in its place, and it is the
live one: if the observation window shows the partial failures are **isolated
single losses with no runs of ≥ 2**, then Stage 1 at `penalty_failures = 2`
never engages on this deployment. The response is then to choose between
lowering the threshold — which S1.6 shows costs ~14 penalties/day of a healthy
endpoint, plainly unacceptable — and not shipping. Not shipping would be the
correct call.

---

# Stage 2 — candidate, not accepted

**Idea:** RTT EWMA per endpoint (RFC 6298 α = 1/8, β = 1/4), banded so
near-equal endpoints tie, plus a family-preference bias, ordering the Healthy
set by measured latency instead of config order.

**Why it is not accepted:**

- The measured v4/v6 separation on this link is 0.15 ms across four targets,
  inside run-to-run spread. There is currently nothing for RTT ordering to
  resolve.
- The natural "unmeasured sorts worst" rule locks unmeasured endpoints out
  permanently — they are never selected, so they are never measured. With four
  endpoints, #3 and #4 would keep a synthetic worst-case estimate for process
  lifetime, making the RTT dimension inert exactly where it was supposed to
  help.
- A `srtt == 0` sentinel collides with a legal sub-microsecond sample against a
  loopback mock. Any Stage 2 spec must use an explicit `Option` or `u32::MAX`.
- Family preference expressed as a score bias **breaks first-query config
  ordering** whenever the endpoint set is mixed-family: at boot all RTT bands
  tie, so the bias alone decides, and a v6 endpoint configured second wins over
  a v4 endpoint configured first. Any Stage 2 spec must resolve this explicitly.

### Stage 2 gate

Stage 2 may be specified in detail only if, from Stage 1 telemetry plus a live
dual-stack arm:

1. Some pair of healthy endpoints shows a **durable** DNS-level `srtt`
   separation larger than one candidate band, sustained over hours — not a
   transient; **and**
2. A banding width exists that both suppresses primary switching on a healthy
   link and still resolves that separation.

If (1) fails, the correct outcome is a static family-preference ordering rule —
config ordering with an explicit preferred family — and **no RTT machinery at
all**.

# Stage 3 — candidate, not accepted

**Idea:** a Happy-Eyeballs-inspired delayed second attempt against a different
endpoint, first success wins, delay derived from RFC 6298 rather than RFC 8305's
250 ms constant, bounded by a token-bucket budget.

**Why it is not accepted:**

- **Blocking prerequisite.** Dropping a hickory `send(...).first_answer()`
  future must provably remove the in-flight entry from the `DnsExchange`
  multiplexer. A leaked entry per hedged query is an unbounded leak. This must
  be settled by a test before Stage 3 is scheduled, not treated as a risk to
  manage during implementation.
- **The obvious demotion rule converges the wrong way.** Inflating the primary's
  `srtt` on a hedge win also raises the derived hedge delay, so the next hedge
  fires later, wins become rarer, and the mechanism damps toward "stop hedging"
  rather than "demote the primary". Any Stage 3 spec needs an explicit counter
  instead.
- **First-success-wins has behavioural consequences beyond latency.** Two
  resolvers can answer differently for geo/CDN names, and whichever wins the
  race is what gets cached, so answer stability across queries falls. With
  DNSSEC pass-through, a validating stub can see RRSIGs from one resolver and
  not the other.
- **Amplification** is the largest risk. A budget caps it, but the cap applies
  to forwarded queries only — a minority after cache hits — so the figure must
  be reported cache-hit-adjusted rather than as a bare multiplier.
- **Stage 1 may already have taken the win.** Once an endpoint is penalized
  after `penalty_failures` queries, there is nothing left for a hedge to rescue
  except the *slow-but-answering* case.

### Stage 3 gate

Stage 3 may be specified in detail only if all four hold:

1. The hickory cancellation prerequisite is proven by test.
2. Stage 1 telemetry shows a real population of **slow-but-answering** queries —
   an endpoint whose answers arrive far above its own typical latency without
   ever failing. Without that population there is nothing to hedge.
3. Client p99 with hedging on beats hedging off, same workload and same session,
   by a margin agreed before the run. **`hedges_won / hedges_started` is a
   diagnostic, not a decision metric** — a hedge that wins by 0.5 ms is a win
   worth nothing, and the true quantity (primary latency minus hedge latency) is
   unmeasurable because the primary future is dropped.
4. The amplification cost, stated cache-hit-adjusted, is accepted explicitly.

---

# Prior art

Stage 1 only. Stage 2/3 comparisons belong with their specifications if those
are ever written.

| System | Mechanism | Stage 1 |
| --- | --- | --- |
| **Unbound** | Exponential timeout backoff, blacklisting a server after repeated timeouts, later probing it back | **Adopted** — S1.6, S1.7 |
| **Unbound** | `infra_cache` keyed by (address, zone), with its own size cap | **Rejected** — FAH is a forwarder with no zone dimension; the key would be attacker-influenced |
| **Unbound** | Lameness detection, EDNS-capability probing | **Rejected** — meaningless against a recursive forwarder target |
| **Unbound** | Constants sized for hundreds of authoritative servers (`USEFUL_SERVER_TOP_TIMEOUT` 120 s, `RTT_MAX_TIMEOUT` 120 s) | **Rejected** — an order of magnitude too slow for a 2-endpoint household forwarder |
| **dnsmasq** | `--all-servers` fans out to every server | **Rejected** — Stage 1 sends one query at a time |
| **AdGuard Home** | `parallel` / `fastest_addr` race every query | **Rejected** — same reason |
| **BIND 9** | Per-server SRTT with decay and quantisation | Stage 2 territory, not Stage 1 |
| **Knot Resolver** | ε-greedy exploration (~5 % of queries to a non-best server) | **Rejected** — a permanent latency tax for estimate warmth the probe path provides free |
| **systemd-resolved** | Linear fallback with feature-level downgrade | The behaviour Stage 1 replaces |
| **RFC 8305** | Happy Eyeballs v2 | Entirely Stage 3; nothing from it appears in Stage 1 |

# Open measurements

| # | Question | Blocks | Status |
| --- | --- | --- | --- |
| 1 | Household transport-failure base rate per endpoint | S1-G4 | **Answered** — 0.072 % primary, suite T sample 1. Confirm over the full window |
| 2 | Frequency of partial upstream failure | S1-G5 | **Answered** — 27 events / 45.7 h. Rejection route 1 closed |
| 3 | **Failure clustering / run-length distribution** | **S1-G4, and whether Stage 1 ships at all** | **Open, blocking — but collecting.** `upstreams[].failure_runs` since p2.5-06; the window starts at that build's deploy. See S1.6 and the clustering note above |
| 4 | Is the traffic composition (69 % SWR / 20 % `resolve_host` / 10 % client) stable? | S1.8, S1.9, S1-G2 framing | Open — suite T differenced series |
| 5 | Do total failures land only on SWR refreshes? | How urgent Stage 1 is for *clients* | Open — `swr.failed` was 3 and the secondary's `failures` was 3 in sample 1; test whether they move together |
| 6 | Harness noise band **N** for the live timing gate | Freezing S1-G2 tier 3 | Open — suite S1-N, runs before S1-L |
| 7 | DNS-level `srtt`/`rttvar` per endpoint, and whether any durable separation exists | Stage 2 gate | Open — needs Stage 2 instrumentation. The 23.9 ms aggregate forward mean is context, not a substitute |
| 8 | Does dropping a hickory `first_answer()` future release the in-flight entry? | Stage 3 gate | Open — one test |
| 9 | Does a slow-but-answering population exist in real traffic? | Stage 3 gate | Open — Stage 1 telemetry plus latency-per-endpoint |
