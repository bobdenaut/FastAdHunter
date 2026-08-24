# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-24

## Now

| | |
| --- | --- |
| Branch | `main`, `5657aa0`. Everything is on **both** origin and backup; nothing is local-only. Tag `v0.2.19-phase2.5` pushed |
| Tree | clean. No runtime code change since p2.5-11 (`6417842`) other than phase-2.6 Stage 1 work through p2.6-09; the four most recent commits are documentation and measurement data only |
| Tests | green — fmt/clippy/test at the last code-bearing task (p2.6-09) |
| Version | 0.2.19 (workspace). **The version string does not identify a build** — the deployed phase-2.5 image and the Stage 1 image both report `0.2.19`. Discriminate by container `tag`, or by whether `/telemetry`'s upstream entries carry the p2.6-06 fields (`state`, `penalty_round`, `penalties`, `penalized_seconds_total`, `probes`, `probe_successes`, `family`) |
| Deployed | **production**: 0.2.19 phase-2.5 build on `veth1` (`172.17.0.2`) since 2026-08-23T14:59:52Z, carrying p2.5-01…11, `strategy = "fallback"`. **probe**: container `fah-probe` on `veth3` (`172.17.0.4`) running `fastadhunter:stage1` built at `f65386f`, also `fallback`, `start-on-boot=no` — a measurement container, not a service |
| Phase | **2.5 closed**, tag `v0.2.19-phase2.5`. **2.6 in `plan/wip/phase2.6-adaptive-stage1`** — 13 tasks, **11 `DONE`** |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 **cleared** — §5.1 p2.5-01, §5.2 p2.5-02, §5.4 p2.5-03, §5.5 p2.5-04, §5.6 p2.5-05 + p2.5-10. §5.7–14 gate Phase 3. **S1-G2 tier 1 met** (p2.6-08); **tier 3 frozen at 5.00 %** (p2.6-10) |
| **Next** | `p2.6-11-optin-deploy-soak` |

## Phase 2.6 — Adaptive DNS Stage 1, in progress

**Done (11 of 13):** p2.6-01…07 built the Stage 1 config surface, health core,
selection and probing, outcome classification, pool integration, telemetry and
docs. p2.6-08 met S1-G2 tier 1 on the microbench. p2.6-09 passed S1-G3 on the
injected-failure bench. p2.6-13 built the on-device harness. p2.6-10 ran suite
S1-N and froze the tier-3 threshold.

**Remaining:** `p2.6-11` (opt-in deploy, 7-day soak, G2 tiers 2–3, G4, G5),
then `p2.6-12` (default flip).

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
- Teardown of `fah-probe` is **deliberately deferred** until p2.6-11 finishes.

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
  cross-reference in the phase's review files. Deployment tier keeps two
  explicit owner decisions: `penalty_failures` from the observed run-length
  data, and extending a short S1-G4 window rather than reading it.
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
   p2.6-13 (harness) and p2.6-10 (null A/B) on the device **done**; next
   p2.6-11 (opt-in deploy, 7-day soak, S1-G4, S1-G5); p2.6-12 default flip only
   if every deployment-tier gate passes. If the run-length window shows only
   isolated single losses, not flipping the default is the correct outcome.
2. **Phase 3 — after 2.6 closes**, conditional on §5.7–14 (cert machinery
   home/ADR, connector redesign, DoH/DoT listener placement, telemetry
   taxonomy, memory caps per new state owner, 443 steering v4+v6, on-device
   TLS measurements, opt-in bound to stable identity).

## Open unknowns (reconciled §4)

- **S1-G4 run-length distribution — the gate that decides whether Stage 1
  ships.** Captures and procedure:
  [s1g4-window/](code-review/phase2.5/s1g4-window/) (owner says
  "capture S1-G4"). Read as the sum of per-process `failure_runs` deltas;
  counters reset on restart, so a segment survives only if captured before its
  process ends. Running total: **one closed run, of length 1**, over 18,949
  primary attempts. Live bucketing is confirmed working (it was unobserved at
  p2.5-09 V5c). Segment 02 is open on 0.2.19.
- **The failure rate does not match the earlier sample.** This window gives
  0.0053 % (1 / 18,949). Suite T sample 1 recorded 0.072 % and 27 partial
  failure events over 45.7 h — an order of magnitude busier. Unexplained, and
  it decides whether S1-G4 can ever accumulate enough closed runs to read.
- **Three of four endpoints produce no data under `fallback`.** `9.9.9.9` one
  attempt, both v6 endpoints zero, across both segments. Stage 1 cannot
  penalize or probe an endpoint that is never selected.
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
- **Whether the SWR path needs its own arm is unanswered.** The L.1 workload
  produces zero SWR activity by construction — unique forward names are never
  re-queried and control names stay fresh — so ~69 % of upstream attempts stay
  unmeasured. Carried to p2.6-11.
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
