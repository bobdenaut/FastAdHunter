# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-25

## Now

| | |
| --- | --- |
| Branch | `main`, `8924ca0`. Everything is on **both** origin and backup; nothing is local-only. Tag `v0.2.19-phase2.5` pushed; 0.2.20 is **not** tagged |
| Tree | clean. No runtime code change since p2.5-11 (`6417842`) other than phase-2.6 Stage 1 work through p2.6-09; every commit since is documentation, measurement data or the version bump |
| Tests | green — fmt/clippy/test at `1c430aa`: **1 056 passed, 0 failed, 8 ignored** |
| Version | 0.2.20 (workspace). **The version string still does not identify a build** — 0.2.19 named both the phase-2.5 production image and the Stage 1 probe image. 0.2.20 is unambiguous today, but discriminate by container `tag` or by whether `/telemetry`'s upstream entries carry the p2.6-06 fields (`state`, `penalty_round`, `penalties`, `penalized_seconds_total`, `probes`, `probe_successes`, `family`) |
| Deployed | **production**: container **`fah-next`** on `veth1` (`172.17.0.2`), 0.2.20, `root-dir /kingston/fastadhunter/root-0220`, **`strategy = "adaptive"`** via envlist `fah-optin`, `start-on-boot=yes`, soaking since 2026-08-25T07:57:02Z. **rollback**: the 0.2.19 container still exists as `comment="fastadhunter"`, stopped, `start-on-boot=no`. **probe**: `fah-probe` on `veth3` (`172.17.0.4`), `fastadhunter:stage1` at `f65386f`, stopped between arms |
| Selector trap | `[find comment="fastadhunter"]` now resolves to the **stopped 0.2.19 rollback container**. Every live-path command targets `[find comment="fah-next"]` until the post-soak cleanup renames it back |
| Phase | **2.5 closed**, tag `v0.2.19-phase2.5`. **2.6 in `plan/wip/phase2.6-adaptive-stage1`** — 13 tasks, **11 `DONE`**, p2.6-11 in progress |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 **cleared** — §5.1 p2.5-01, §5.2 p2.5-02, §5.4 p2.5-03, §5.5 p2.5-04, §5.6 p2.5-05 + p2.5-10. §5.7–14 gate Phase 3. **S1-G2 tiers 1, 2 and 3 all met**; **S1-G4 and S1-G5 are not validated and will not be** |
| **Next** | `p2.6-11` continues — L.4a/L.4b, then the soak closes 2026-09-01 |

## Phase 2.6 — Adaptive DNS Stage 1, in progress

**Done (11 of 13):** p2.6-01…07 built the Stage 1 config surface, health core,
selection and probing, outcome classification, pool integration, telemetry and
docs. p2.6-08 met S1-G2 tier 1 on the microbench. p2.6-09 passed S1-G3 on the
injected-failure bench. p2.6-13 built the on-device harness. p2.6-10 ran suite
S1-N and froze the tier-3 threshold.

**`p2.6-11` is in progress** — deploy done, pre-flip measurements done, soak
running. Full evidence:
[p2.6-11 review](code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md).

| Item | Result |
| --- | --- |
| 0.2.20 deployed, verified under `fallback`, then opted in | done |
| M.8 re-run — **closes p2.6-08 F1** | PASS. K = 8 pre-declared; 95 % t-CI of `mean(d_i)` `[−2.385 %, +1.893 %]`; worst pair +4.43 %; control −1.95 % |
| S1-G2 tier 2 | PASS — `attempts/miss = 1.000000`, zero penalties, zero probes, 6 repetitions |
| S1-G2 tier 3 | PASS — **+0.036 %** against the frozen 5.00 % |
| L.1s, the SWR arm | PASS — 114 000 refreshes/repetition, zero dropped, zero failed |
| S1-G4, S1-G5 | **NOT validated** — see below |
| L.3 soak | running, 2026-08-25T07:57:02Z → 2026-09-01 |
| L.4a / L.4b | not run |

**S1-G4 closed without an answer, by owner decision.** The `fallback`
run-length window ended at the opt-in with **three closed runs, all of length 1,
zero of length ≥ 2, over 42 639 primary attempts (0.0070 %)**. That is far too
few to calibrate `penalty_failures`, which therefore stays at its compiled
default of **2 — provisional and empirically uncalibrated.** The alternative was
holding `fallback` another 12–17 days; it was put to the owner and declined.
S1-G5 route 1 is confirmed closed (partial failure exists, suite T); **route 2
stays open and is not decidable from this deployment.** Do not later describe
Stage 1's constants as calibrated on this deployment.

The window cannot be reopened without reverting the strategy: under `adaptive`,
`upstreams[].attempts` excludes `resolve_host`, so post-flip samples are a
different quantity and are not pooled.

**Remaining after p2.6-11:** `p2.6-12` (default flip) — and its precondition is
a gate set that now has two unvalidated members.

**The on-device measurement harness exists** — this is the expensive thing not
to rediscover:

- Probe container `fah-probe`, `veth3` / `172.17.0.4`, `root-dir
  /kingston/probe/root`, `mountlists probe-config,probe-data`, `envlists
  fah-env`, `logging=yes`, `start-on-boot=no`, no `memory-high`. Its config and
  API key live in `/kingston/probe/config` and **survive container removal**.
- Image `kingston/fastadhunter-stage1-f65386f.tar` is already on the store, so
  L.1/L.2 need no new build. L.3 is different — it is the **production**
  container with `adaptive` enabled through `FAH__DNS__UPSTREAMS__STRATEGY`,
  which needs a production deploy and separate approval.
- Mocks are native CoreDNS 1.14.7 on the dev box at `192.168.10.10:5301/5302`,
  `coredns.exe -conf Corefile.<port>`. **Docker port publishing must not be
  used** — its userland UDP relay wedges permanently under load and presents as
  every upstream timing out, which under `adaptive` is indistinguishable from
  Stage 1's own penalty behaviour.
- Tooling and both raw datasets are committed:
  [p2.6-10-session/](code-review/phase2.6/p2.6-10-session/) (`tools/`,
  `config/`, nine repetitions) and
  [p2.6-13-trial/](code-review/phase2.6/p2.6-13-trial/).
- **Flipping the probe's strategy** costs one router command, not a config
  edit: envlist `probe-optin` carries `FAH__DNS__UPSTREAMS__STRATEGY=adaptive`,
  and `/container/set [find comment="fah-probe"] envlists=fah-env,probe-optin`
  (or back to `fah-env` alone) plus a restart switches arms. **`fah-env` must
  never carry a strategy key** — production and the probe share it, so setting
  it there flips both and silently destroys the `fallback` control.
- **Cache settings decide what a probe arm can measure.** The probe runs
  `max_entries = 10 000`, `min_ttl_seconds = 0`. A unique-name flood at
  ~10 000 QPS evicts the whole cache about once a second, so no entry survives
  to go stale and SWR is structurally unmeasurable. L.1s works only at a low
  rate over a working set below `max_entries`.
- Teardown of `fah-probe` is **deliberately deferred** until p2.6-11 finishes;
  L.4a/L.4b still need it.

**S1-G2 tier 3 is frozen at 5.00 %** on total upstream attempts, from
N = 0.1094 % over a pre-declared K = 8 null A/B on the RB5009
([p2.6-10 review](code-review/phase2.6/p2.6-10-null-ab-review.md)). `forward`
p99 measured N = 156.10 % and is **descriptive only** — it is quantized to
histogram bucket bounds and cannot carry a gate. Do not substitute mean latency
for it; that was considered and rejected. The metric order is unchanged.

## Phase 2.6 — background

- Spec [adaptive-upstream-selection.md](design/adaptive-upstream-selection.md)
  and benchmark protocol
  [adaptive-upstream-selection-benchmarks.md](design/adaptive-upstream-selection-benchmarks.md)
  passed a full pre-implementation validation on 2026-08-23 (two blockers
  fixed: one-pass config-order selection so a recovered primary is probed;
  `Ignore` mode never claims a probe) and a benchmark-methodology audit
  (`adaptive_paying` excludes probe carriers; tax-free share per ratio; B.2 is
  classification only; B.5 cadence and reset tail; L.4 split into WAN
  black hole / LAN `EHOSTUNREACH`).
- Thirteen tasks, each with a `-plan.md`; the phase CLAUDE.md requires reading
  both. `p2.6-13` was appended rather than inserted — it runs **before** 10 and
  11 despite the higher `NN`, because renumbering would break every
  cross-reference in the phase's review files. Both deployment-tier owner
  decisions have now been taken, and both went the same way: `penalty_failures`
  was **not** derived from run-length data, and the S1-G4 window was **not**
  extended.
- **Pre-declaration is the phase's recurring failure mode.** p2.6-08 F1 existed
  because K was extended mid-session; it closed in p2.6-11 only by writing K = 8
  down first. Two further declarations in p2.6-11 were wrong when written —
  M.8's sample size, and an L.1s workload that could not have exercised SWR at
  all — and both were caught while setting the arm up, corrected before the
  first measurement, and recorded with their reason. A declaration that can be
  quietly edited is not a declaration.
- Timing terms fixed everywhere: `timeout_ms` per leg (`attempt_timeout` in
  code); `attempt_bound_ms = ATTEMPT_LEGS × timeout_ms`; `PENALTY_BASE = 10 ×
  attempt_bound_ms`; `PENALTY_MAX = 300 s`.
- Owner decisions already taken: 8-endpoint cap; `attempts`/`failures` stay
  on `Health`, `run_buckets` on the cold struct; `resolve_host` attempts stay
  uncounted under `adaptive`; `timeout_ms` validated `1..=10 000`.
- **"Return to the last clean Phase 2 state" is long done — not a pending
  step.** Phase 3 was opened once, cancelled back to a clean `phase3`, and
  phases 2.5 and 2.6 were inserted ahead of it. The cancellation predates any
  commit, so `git log` carries no trace of Phase 3 work; **do not read that
  silence as the decision never having been taken.** `7609075` restated the
  spec's "decided separately" as a gate on the 2.6 `wip` move, which sent this
  question round once already; the phase file and spec now say it plainly.
  Supersedes the caveat in `f176f47`.

## Sequencing

1. **Phase 2.6 — Adaptive Stage 1**: p2.6-01…09 on the dev box **done**;
   p2.6-13 (harness) and p2.6-10 (null A/B) **done**; p2.6-11 **in progress** —
   deployed and opted in, tiers 2–3, M.8 and L.1s all passed, soak closes
   2026-09-01, L.4a/L.4b outstanding. Then p2.6-12.
   **p2.6-12's precondition is now weaker than the plan assumed.** The default
   flip was gated on every deployment-tier gate passing; S1-G4 and S1-G5 route 2
   closed unvalidated instead. The spec's own narrow rejection route still
   stands — if this deployment produces only isolated single losses, Stage 1 at
   `penalty_failures = 2` never engages and **not flipping the default is the
   correct outcome**. Three length-1 runs are consistent with that route and
   equally consistent with too small a sample; nothing measured distinguishes
   them. That call belongs to p2.6-12 and needs an explicit owner decision, not
   an inference from "the other gates passed".
2. **Phase 3 — after 2.6 closes**, conditional on §5.7–14 (cert machinery
   home/ADR, connector redesign, DoH/DoT listener placement, telemetry
   taxonomy, memory caps per new state owner, 443 steering v4+v6, on-device
   TLS measurements, opt-in bound to stable identity).

## Open unknowns (reconciled §4)

- **S1-G4 is closed, not answered.** Final window in
  [s1g4-window/](code-review/phase2.5/s1g4-window/): three closed runs, all
  length 1, zero of length ≥ 2, over 42 639 primary attempts. Not reopenable
  without reverting to `fallback`. One procedural lesson, paid for once:
  `failure_runs` resets per process, so **capture immediately before any
  restart** — segment 02's ~13 h tail was lost to a deploy that skipped it.
- **The two failure rates are not comparable, and that is the finding.** Suite
  T sample 1 gave 0.072 %; this window gives 0.0070 %. Upstream composition
  moved underneath them — SWR 69.1 % → 47.0 %, `resolve_host` 20.1 % → 36.5 %,
  client forwards 10.3 % → 16.4 % — at nearly identical total volume (913 vs
  929 attempts/h). They are rates over differently composed denominators;
  comparing them directly is a category error, and the spec's "base rate
  documented — 0.072 %" is scoped to suite T's composition. **Unproven:** that
  the composition shift *causes* the 15× gap. Telemetry carries no per-class
  failure attribution by design (S1.12), so the failures cannot be assigned to
  the class that shrank. Ruled out: `timeout_ms` unchanged since phase 0;
  p2.5-05 added event fields without touching `failures` semantics.
- **Three of four endpoints produce no data.** `9.9.9.9` gets one attempt per
  primary failure and nothing else; both v6 endpoints stay at zero, across
  every segment. Stage 1 cannot penalize or probe an endpoint that is never
  selected, and `adaptive` does not change this — selection is config-order,
  first-healthy-wins, not latency-aware. Making those endpoints observable
  needs deliberate probing of non-selected endpoints, which is Stage 2 work and
  is not designed.
- **The deployed build is unsoaked.** V6 passed on 0.2.18 over 15.81 h;
  0.2.19 shipped after it. The delta is a WS event key and a counter on a 10 s
  poll, so no soak was run — but the soak evidence describes a build that is
  no longer running.
- **Latency percentiles are unusable as gate metrics at the current histogram
  resolution.** `quantile()` returns a bucket *bound*, over eleven bounds
  spanning 0.1 ms to 100 ms, so in the region a real `forward` p99 occupies the
  grid step is 2.0–2.5×. Measured on-device over K = 8: N = 156.10 %, and
  `cache_hit_p99` never left its first bucket in 161 perf rows, making the
  control arm's p99 half structurally unable to fire. Any future gate wanting
  latency resolution needs a code change, not a methodology change.
- ~~Whether the SWR path needs its own arm.~~ **Answered in p2.6-11: it did,
  and Stage 1 is clean on it.** L.1's `swr.*` zeroes were vacuous — its
  workload enqueues nothing, so passing `swr.failed == 0` was arithmetic. The
  L.1s arm sustains 114 000 refreshes per repetition with zero dropped, zero
  failed, and no penalty or probe on the `adaptive` arm. Two limits stand: L.1
  is ~100 % client forwards and L.1s is 97–100 % SWR, so the deployed ~69/10
  blend is measured by neither, and their interaction is untested; and
  `probes == 0` is evidence about refresh traffic against **healthy**
  endpoints only — SWR against a failing endpoint is B.8's scenario, covered
  in-process at 100 stale keys and never on-device at load.
- **The deployed build has no soak evidence of its own yet.** 0.2.20 is the
  first build to run `adaptive` in production; its 7-day window closes
  2026-09-01. Until then the only soak evidence on file describes 0.2.18.
- IPv6 upstream forwarding: first on-device evidence 2026-08-22
  (`2606:4700:4700::1111` at index 0, 37 attempts / 0 failures, 0.2.17;
  [p2.5-08 review](code-review/phase2.5/p2.5-08-hygiene-review.md) §Side
  finding). Defaults stay v4 literals; no soak yet.
- DoH bootstrap via the container's OS resolver unverified (possible self-loop).
- Upstream TCP/53 reachability mock-tested only. UDP ICMP path failure
  (`ECONNREFUSED`/`EHOSTUNREACH`) is a Linux-only signal — the Windows dev
  box reports `ECONNRESET` — so it is exercised only on-device (S1-L L.4b).
- HTTP path under concurrency unmeasured; 1024-permit ceiling never exercised.
- TLS on RB5009: handshake, interception CPU+memory, cert-mint, splice — probe
  containers, not dev benches.
- `MIMALLOC_PURGE_DELAY=0` above idle load; cache byte-cap eviction never fired
  on-device.

## Deferrable (reconciled §6)

Type mirrors (`fah_config`/`fah_model`), `CacheStats` identity-DTO; compile
transient (peak 181.4 MiB deployed, monitored via `peak_rss`); policy
fail-open window and name-assignments-on-LRU; SWR no-EDNS truncation tax;
fah-common scope creep; `blocking_mode` inert; histogram 100 ms ceiling;
p2.5-10 n1 (test-helper readability in `fah-api`, test-only).

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (10013) when WinNAT reserves the ephemeral port block.
Environmental — do not attribute to a change.

p2.5-09 V3b (live healthcheck column) is a RouterOS 7.21.5 platform
limitation, not a build defect — documented as a trap in
[routeros-traps.md](routeros-traps.md) §Container configuration.
