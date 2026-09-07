# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-09-07

## Now

| | |
| --- | --- |
| Branch | `alloc-domains/http` at `144ef91`, pushed to **both** `origin` and `backup`. `main` at `0a716ec` (= `64be513` + the IP-literal fix), pushed to both. Tag `pre-alloc-domain-2026-09-06` = `64be513`, the rollback point before the HTTP allocation domains, pushed to both |
| Tree | uncommitted on the branch: `p2.6-11` closed in the phase table, the three `resoak-0.3.*` evidence folders and their predeclarations/audit deleted (47 files), this file |
| Tests | green at `6d591ad` (fmt, clippy `-D warnings`, `cargo test --all-features --workspace`, 33 suites); `144ef91` is docs only |
| Version | 0.3.1 (workspace, unchanged on the branch). Newest tags `pre-alloc-domain-2026-09-06`, `soak-p2.6-11`, `v0.2.19-phase2.5` |
| Deployed | production swapped **2026-09-07** (owner-run) from `fastadhunter-0.3.1` (`db2f9b2`) to image **`fastadhunter:alloc-6d591ad`** (`kingston/fastadhunter-alloc-6d591ad-rosready.tar`) on `veth1` / `172.17.0.2`, mount lists `fah-config,fah-data`, envlist `fah-env` now carrying **`FAH__RUNTIME__HTTP_RUNTIMES=2`** explicitly. **T0 = the container start time in the router log; day 7 = 2026-09-14** |
| Build ≠ tip | deployed code = `6d591ad` = branch tip minus one docs commit. `main` lacks the branch (by design until the soak verdict); the branch carries everything `main` has |
| Phase | **5 closed.** **2.6 in `plan/wip/phase2.6-adaptive-stage1`** — 12 of 13 `DONE` (`p2.6-11` closed 2026-09-07 by owner decision, its soak stopped at day 6 for the swap); `p2.6-12` `WAITING` |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 cleared; §5.7–14 gate Phase 3. S1-G2 tiers 1–3 met; S1-G4 and S1-G5 route 2 not validated and will not be |
| **Next** | the allocation-domain soak runs to **2026-09-14**; merge `alloc-domains/http` → `main` on its verdict. Meanwhile, off the critical path: `p2.6-12` on `main`, the TLS / lol_html capacity microbench on the probe, dashboard metadata for `runtime.http_runtimes` |

## HTTP allocation domains — soak in progress

What is deployed: each HTTP connection served end to end on one of N
`current_thread` runtimes on their own OS threads behind one acceptor, N=2, so
a connection's allocations are freed by the thread that made them. Decision
and cost: [ADR-0006](decisions/0006-http-allocation-domains.md); term:
CONTEXT.md "Allocation Domain"; config: CONFIGURATION.md `[runtime]` (boot
class); code review with findings 1–23:
[alloc-domains-http-review.md](code-review/phase2.6/alloc-domains-http-review.md).

Why N=2 — the RB5009 N sweep of 2026-09-07
([alloc-domains-n-sweep.md](code-review/phase2.6/alloc-domains-n-sweep.md)):

| | N=0 (old way) | N=2 | N=3 | N=4 |
| --- | --- | --- | --- | --- |
| new connections/s, keep-alive rps | 2341, 3297 | 1860, 2574 | 1963, 3237 | 1947, 3412 |
| HTTP p95 ms close / keep-alive | 15.9 / 46.9 | 27.8 / 45.3 | 49.1 / 53.5 | 50.7 / 53.3 |
| cores busy (close) | 3.63 | 2.21 | 2.58 | 2.65 |
| DNS p50 / p99 ms under HTTP load | 3.35 / 20.1 | 0.96 / 15.8 | 1.11 / 13.6 | 1.01 / 12.7 |
| held after 900 MiB WAN, +15 min | +56..+60 | +19 | +32 | +45 |

N=2 carries the tested connection-rate workload with a third less CPU per
request than N=0, better DNS latency under load, and a third of the old way's
held memory; N=3/4 buy keep-alive rate at the cost of p95 and memory. The LAN
transfer pass is **not** a 1 GbE test (router forwarding path caps it at
~67–70 MiB/s). Untested: TLS termination and HTML rewriting — the N decision
is re-measured when Phase 3/4 exist.

**Predeclared soak verdict (7 days, household traffic):**

- RSS plateau flat, against 0.3.1's +0.4 MiB/h drift and its +8..+37 MiB
  evening steps. Read `process_rss` from the 6-min history samples; `cpu_*_ms`
  from `/api/v1/debug/memory` (diagnostic, not in `/telemetry`).
- HTTP p95 and CPU per MiB relayed not above the A/B figures.
- DNS p99 not above the 0.3.1 series.
- Zero restarts; every stop inside RouterOS's 10 s (measured 5.6 s with three
  transfers in flight; the 5 s drain then aborts them — `WARN` by design).

Rollback without a rebuild: `FAH__RUNTIME__HTTP_RUNTIMES=0` on `fah-env` +
restart (the 0.3.1 code path, same image). Rollback of the build:
`kingston/fastadhunter-arm64-0.3.1.tar`, or `main` at the tag.

Deferred, each its own go: 11b graceful shutdown of keep-alive connections
(finish the in-flight exchange instead of the whole transfer); dashboard
settings metadata for `runtime.http_runtimes` (review finding 3); the capacity
microbench (rustls AES-GCM and lol_html ms/MiB on the RB5009) that decides
whether N=2 clears 1 Gbit with TLS.

**A bug found on the way, fixed on both branches (`0a716ec` / `6d591ad`):**
with `[egress] allow_ip_literal_hosts = true`, a bare-IP `Host` was accepted
and then handed to the DNS resolver, so every such request was a 502. Since
`f7ae186`; production has the flag off and never hit it.

## Phase 5 — Web Dashboard, closed 2026-09-01

Thirteen screens served from `/web` by `fah-api` on one origin, one binary, no
second container and no new port. Bundle **128 730 B gzip** (83.8 % of the
150 KB budget); arm64 rootfs **14.07 MiB** against the 30 MB budget; no Node in
the runtime image. Verified in two stages —
[p5-10 review](code-review/phase5/p5-10-phase5-verification-review.md).

Four rows deferred, not passed (review §13.7), wanting a deploy window:
concurrent Argon2id peak RSS; RSS deltas above the drift floor (needs repeated
attach/detach cycles); `/cache` at a second occupancy; the real-phone leg of
the mobile pass (owner-run; likely needs a LAN→container dstnat that
[routeros-traps.md](routeros-traps.md) records as absent).

`constants.ts` verdict: no refresh default moves; a **30 s option** for
`/telemetry` and `/cache` is cleared by measurement and not applied (own
approval). Surfaces to re-review later: certificate UI and per-client
interception controls after Phase 3; the **Lists** rule partition after
Phase 4.

## Phase 2.6 — Adaptive DNS Stage 1, in progress

**Done (12 of 13):** p2.6-01…07 built the Stage 1 config surface, health core,
selection and probing, outcome classification, pool integration, telemetry and
docs. p2.6-08 met S1-G2 tier 1 on the microbench. p2.6-09 passed S1-G3 on the
injected-failure bench. p2.6-13 built the on-device harness. p2.6-10 ran suite
S1-N and froze the tier-3 threshold. **p2.6-11 closed 2026-09-07.** Evidence:
[p2.6-11 review](code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md);
repair plan [plan/resoak-orchestration.md](../plan/resoak-orchestration.md).

| Soak | Build | Outcome |
| --- | --- | --- |
| L.3 | 0.2.20 | superseded — an audit found F1/F2/F3; no acceptance |
| re-soak 1 | 0.3.0 | terminated at T0+59 h to change the measurement method; no verdict |
| re-soak 2 | 0.3.1 | T0 2026-09-01T07:27:49Z, **stopped 2026-09-07 at day 6** for the allocation-domain swap. Carried the `adaptive` acceptance rows below, G5a and p5-10 Stage B. The day-7 RSS acceptance was not taken; its memory findings live in [resoak-0.3.1-memory-diagnosis.md](code-review/phase2.6/resoak-0.3.1-memory-diagnosis.md) (allocator retention, not a leak; the mechanism the allocation domains address). Raw pulls and the predeclarations were deleted 2026-09-07 |

| Item | Result |
| --- | --- |
| 0.2.20 deployed, verified under `fallback`, then opted in | done |
| M.8 re-run — closes p2.6-08 F1 | PASS. K = 8 pre-declared; 95 % t-CI of `mean(d_i)` `[−2.385 %, +1.893 %]`; worst pair +4.43 %; control −1.95 % |
| S1-G2 tier 2 | PASS — `attempts/miss = 1.000000`, zero penalties, zero probes, 6 repetitions |
| S1-G2 tier 3 | PASS — +0.036 % against the frozen 5.00 % |
| L.1s, the SWR arm | PASS on the fixed generator — 114 000 refreshes/repetition, zero dropped, zero failed |
| S1-G4, S1-G5 route 2 | **NOT validated** — see below |
| G5a, 0.3.1 | satisfied at T0+4 s — `dyndns` 304. Do not re-litigate |
| L.4a — WAN black hole, 1 h | PASS — 2 attempts arm the penalty, then 14 probe carriers, one per window; 16 of 3 365 945 queries paid the dead endpoint (~804 ms each); p50 0.996 ms |
| L.4b — LAN host-unreachable, 1 h | SPLIT. Window behaviour PASS (14 probes, 14 windows). Path-failure classification UNCONFIRMED on this platform — the unused LAN address produced a timeout, not `EHOSTUNREACH`; the router drops silently when its ARP fails |
| `EHOSTUNREACH` → `PathFailure` | unit-test coverage only. Deferred to topology-specific validation; gates nothing in p2.6-11 |

**S1-G4 closed without an answer, by owner decision.** The `fallback`
run-length window ended at the opt-in with three closed runs, all of length 1,
over 42 639 primary attempts (0.0070 %) — too few to calibrate
`penalty_failures`, which stays at its compiled default of **2, provisional and
empirically uncalibrated.** S1-G5 route 1 is closed (partial failure exists,
suite T); route 2 stays open and is not decidable from this deployment. The
window cannot be reopened without reverting the strategy: under `adaptive`,
`upstreams[].attempts` excludes `resolve_host`, so post-flip samples are a
different quantity.

**Remaining:** `p2.6-12` (default flip), whose precondition is a gate set with
two unvalidated members (§Sequencing).

**The on-device measurement harness** — the expensive thing not to rediscover:

- Probe container today: **`fah-alloc3`** on `veth3` / `172.17.0.4`, image
  `alloc-6d591ad`, mount lists `h1buf-config,h1buf-data`
  (`/kingston/fah-h1buf/{config,data}`), envlist `h1buf-env` (mimalloc keys +
  `FAH__RUNTIME__HTTP_RUNTIMES`, the arm switch), `start-on-boot=no`, stopped
  after the N sweep. Its config carries the probe-only egress exception
  `192.168.10.10/32` with `allow_ip_literal_hosts = true` and **its own API
  key** (generated at first boot; a file scp'd through RouterOS arrives
  root-owned and the service user cannot read it — let the app generate). The
  key is on the dev box at `E:/fah-diag/probe/config/apikey-alloc2`.
- One veth carries one container: a new image means remove + add on `veth3`
  reusing the mount lists and the envlist. `/container/envs` keys are addressed
  by `list=`.
- HTTP load rig (dev box): `E:/fah-diag/tools/connrate.py` (6 processes × 8
  threads, raw sockets, server-closes-first so Windows' 16k dynamic ports never
  exhaust), `dnsload.py` (UDP, cached/blocked/uncached mix), `alloc-run.sh` /
  `arm.sh` (drivers + `/debug/memory` sampling), `h1buf-ab.sh` (transfers).
  Origin: `static-web-server` native on port 80 serving `E:/fah-diag/origin`.
  **Docker port publishing must not be used** for any load path — its userland
  relay caps TCP and wedges UDP.
- DNS mocks for Stage 1 arms: native CoreDNS 1.14.7 on the dev box at
  `192.168.10.10:5301/5302`. Tooling and raw datasets for p2.6-10/13 are
  committed under [p2.6-10-session/](code-review/phase2.6/p2.6-10-session/)
  and [p2.6-13-trial/](code-review/phase2.6/p2.6-13-trial/).
- **`fah-env` must never carry a strategy key** — production and a probe may
  share it. The probe's own list (`h1buf-env`) is where arms are switched.
- Cache settings decide what a probe arm can measure: a unique-name flood at
  ~10 000 QPS evicts a 10 000-entry cache about once a second, so SWR is
  structurally unmeasurable there.

**S1-G2 tier 3 is frozen at 5.00 %** on total upstream attempts, from
N = 0.1094 % over a pre-declared K = 8 null A/B on the RB5009
([p2.6-10 review](code-review/phase2.6/p2.6-10-null-ab-review.md)). `forward`
p99 measured N = 156.10 % and is descriptive only — quantized to histogram
bucket bounds, it cannot carry a gate.

## Reading a soak pull

Two rules cost real time when broken:

- **`?fields=` is passed explicitly, 15 fields, `upstreams` excluded.** Omitting
  it selects `PerfFields::ALL`, whose full-row parse injects an RSS excursion
  into the series being gated — +6.9 MiB on a gate pull.
- **Every extra pull and every dashboard session is logged when taken.** An
  unlogged pull is a method violation, not an attribution.

The polled endpoints (`/health`, `/telemetry`, `/cache`) inject no measurable
excursion; the cost is the fat `/history/perf` path.

## Phase 2.6 — background

- Spec [adaptive-upstream-selection.md](design/adaptive-upstream-selection.md)
  and benchmark protocol
  [adaptive-upstream-selection-benchmarks.md](design/adaptive-upstream-selection-benchmarks.md)
  passed a full pre-implementation validation on 2026-08-23 and a
  benchmark-methodology audit.
- Thirteen tasks, each with a `-plan.md`. `p2.6-13` runs before 10 and 11
  despite the higher `NN`; renumbering would break cross-references. Both
  deployment-tier owner decisions went the same way: `penalty_failures` was
  not derived from run-length data, and the S1-G4 window was not extended.
- **Pre-declaration is the phase's recurring failure mode.** A declaration
  that can be quietly edited is not a declaration. The same rule carried into
  the allocation-domain work: its memory criterion (floor ±6 MB by +3 min) was
  predeclared, **not met**, and recorded as such in ADR-0006; adoption is the
  owner's decision on the measured trade, not a re-read of the criterion.
- Timing terms fixed everywhere: `timeout_ms` per leg (`attempt_timeout` in
  code); `attempt_bound_ms = ATTEMPT_LEGS × timeout_ms`; `PENALTY_BASE = 10 ×
  attempt_bound_ms`; `PENALTY_MAX = 300 s`.
- Owner decisions already taken: 8-endpoint cap; `attempts`/`failures` stay on
  `Health`, `run_buckets` on the cold struct; `resolve_host` attempts stay
  uncounted under `adaptive`; `timeout_ms` validated `1..=10 000`.
- "Return to the last clean Phase 2 state" is long done. Phase 3 was opened
  once, cancelled back to a clean `phase3`, and phases 2.5 and 2.6 were
  inserted ahead of it; `git log` carries no trace of Phase 3 work — do not
  read that silence as the decision never having been taken.

## Sequencing

1. **Allocation-domain soak** to 2026-09-14, verdict, merge to `main`.
2. **Phase 2.6 — `p2.6-12`** (default flip) can run on `main` in parallel.
   Its precondition is weaker than the plan assumed: S1-G4 and S1-G5 route 2
   closed unvalidated. If this deployment produces only isolated single losses,
   Stage 1 at `penalty_failures = 2` never engages and **not flipping the
   default is the correct outcome**; that call needs an explicit owner
   decision, not an inference from the other gates.
3. **Phase 3 — after 2.6 closes**, conditional on §5.7–14 (cert machinery
   home/ADR, connector redesign, DoH/DoT listener placement, telemetry
   taxonomy, memory caps per new state owner, 443 steering v4+v6, on-device
   TLS measurements, opt-in bound to stable identity). The allocation-domain N
   is re-measured with TLS on; ADR-0006 lists it as a revisit trigger.

## Open unknowns (reconciled §4)

- **S1-G4 is closed, not answered.** Final window in
  [s1g4-window/](code-review/phase2.5/s1g4-window/): three closed runs, all
  length 1, over 42 639 primary attempts. `failure_runs` resets per process —
  **capture immediately before any restart**.
- **The two failure rates are not comparable, and that is the finding.** Suite
  T gave 0.072 %; the S1-G4 window 0.0070 %, over differently composed
  denominators (SWR 69.1 % → 47.0 %, `resolve_host` 20.1 % → 36.5 %). Unproven
  that the composition shift causes the gap; telemetry carries no per-class
  failure attribution by design (S1.12).
- **Three of four endpoints produce no data.** `9.9.9.9` gets one attempt per
  primary failure; both v6 endpoints stay at zero. Selection is config-order,
  first-healthy-wins; observing non-selected endpoints is Stage 2 work.
- **No accepted 7-day soak exists for any running build yet.** 0.3.1's was
  stopped at day 6; the allocation-domain soak is the one in flight.
- **Latency percentiles are unusable as gate metrics at the current histogram
  resolution** (`quantile()` returns a bucket bound; grid step 2.0–2.5× where a
  real `forward` p99 sits). Any future latency gate needs a code change.
- The SWR path had its own arm (L.1s) and Stage 1 is clean on it; the deployed
  ~69/10 blend of client forwards and SWR is measured by neither L.1 nor L.1s,
  and SWR against a failing endpoint is covered in-process only.
- **IPv6 client identity inflates the client count** (486 active clients on a
  household LAN, privacy-address rotation). Unchased.
- IPv6 upstream forwarding: first on-device evidence 2026-08-22; defaults stay
  v4 literals; no soak yet.
- DoH bootstrap via the container's OS resolver unverified (possible
  self-loop). With allocation domains, the DoT/DoH exchange is pinned to the
  runtime that built the pool (review finding 21, `06a263f`); dormant on the
  UDP production config.
- Upstream TCP/53 reachability mock-tested only; UDP ICMP path failure is
  Linux-only and L.4b did not reach it either (silent ARP drop).
- **HTTP path under concurrency: measured 2026-09-07** — 1.9k new
  connections/s and 2.6–3.4k keep-alive rps on the RB5009 at 48 concurrent
  connections; the 1024-permit ceiling is still never exercised.
- TLS on RB5009: handshake, interception CPU+memory, cert-mint, splice — probe
  containers, not dev benches. The rustls / lol_html ms/MiB microbench is the
  first step and is not written.
- `MIMALLOC_PURGE_DELAY=0` is on both envlists; cache byte-cap eviction never
  fired on-device.

## Deferrable (reconciled §6)

Type mirrors (`fah_config`/`fah_model`), `CacheStats` identity-DTO; compile
transient (peak 141.3 MiB on 0.3.1, monitored via `peak_rss`); policy
fail-open window and name-assignments-on-LRU; SWR no-EDNS truncation tax;
fah-common scope creep; `blocking_mode` inert; histogram 100 ms ceiling;
p2.5-10 n1 (test-helper readability in `fah-api`, test-only); 11b graceful
HTTP shutdown; `spawn_perf_sampler`'s always-`None` `https_connections`
parameter (Phase 3 hook).

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (10013) when WinNAT reserves the ephemeral port block.
Environmental — do not attribute to a change.

p2.5-09 V3b (live healthcheck column) is a RouterOS 7.21.5 platform
limitation, not a build defect — documented as a trap in
[routeros-traps.md](routeros-traps.md) §Container configuration.
