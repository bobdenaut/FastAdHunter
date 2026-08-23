# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-23

## Now

| | |
| --- | --- |
| Branch | `main`, `f176f47`. `1c976d1` and tag `v0.2.19-phase2.5` are on origin and backup; `cdf4167` and `f176f47` are **local only** |
| Tree | clean. No code change since p2.5-11 (`6417842`) — the 0.2.19 bump is a version string, `Cargo.lock` moved by nothing else |
| Tests | green — fmt/clippy/test, 982 passed, 3 ignored, 42 suites, re-run at the 0.2.19 bump |
| Version | 0.2.19 (workspace) |
| Deployed | **0.2.19** on the RB5009 since 2026-08-23T14:59:52Z — carries p2.5-01…11. `counters.http.refused` verified live (0 → 2 on two refusals, one 10 s poll); refusals log nothing at production level |
| Phase | **2.5 closed**, tag `v0.2.19-phase2.5`. **2.6 in `plan/wip/phase2.6-adaptive-stage1`** — all 12 tasks `WAITING` |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 **cleared** — §5.1 p2.5-01, §5.2 p2.5-02, §5.4 p2.5-03, §5.5 p2.5-04, §5.6 p2.5-05 + p2.5-10. §5.7–14 gate Phase 3 |
| **Next** | `p2.6-01-config-surface` |

## Phase 2.6 — Adaptive DNS Stage 1, open

- Spec [adaptive-upstream-selection.md](design/adaptive-upstream-selection.md)
  and benchmark protocol
  [adaptive-upstream-selection-benchmarks.md](design/adaptive-upstream-selection-benchmarks.md)
  passed a full pre-implementation validation on 2026-08-23 (two blockers
  fixed: one-pass config-order selection so a recovered primary is probed;
  `Ignore` mode never claims a probe) and a benchmark-methodology audit
  (`adaptive_paying` excludes probe carriers; tax-free share per ratio; B.2 is
  classification only; B.5 cadence and reset tail; L.4 split into WAN
  black hole / LAN `EHOSTUNREACH`).
- Twelve tasks, each with a `-plan.md`; the phase CLAUDE.md requires reading
  both. Merge tier (p2.6-01…09) fully mechanical. Deployment tier keeps two
  explicit owner decisions: `penalty_failures` from the observed run-length
  data, and extending a short S1-G4 window rather than reading it.
- Timing terms fixed everywhere: `timeout_ms` per leg (`attempt_timeout` in
  code); `attempt_bound_ms = ATTEMPT_LEGS × timeout_ms`; `PENALTY_BASE = 10 ×
  attempt_bound_ms`; `PENALTY_MAX = 300 s`.
- Owner decisions already taken: 8-endpoint cap; `attempts`/`failures` stay
  on `Health`, `run_buckets` on the cold struct; `resolve_host` attempts stay
  uncounted under `adaptive`; `timeout_ms` validated `1..=10 000`.
- **Unrecorded precondition.** The phase file requires the spec's "return to
  the last clean Phase 2 state" sequencing decision to be taken *before* the
  `wip` move. No decision exists — no ADR, no entry here, no review. It looks
  satisfied de facto (tree tagged, gates green, no Phase 3 code, `phase3`
  never left `plan/open/`), and is recorded as an assumption in `f176f47`.

## Sequencing

1. **Phase 2.6 — Adaptive Stage 1**: p2.6-01…09 on the dev box → `adaptive`
   opt-in; p2.6-10 (null A/B) and p2.6-11 (opt-in deploy, 7-day soak, S1-G4,
   S1-G5) on the device; p2.6-12 default flip only if every deployment-tier
   gate passes. If the run-length window shows only isolated single losses,
   not flipping the default is the correct outcome.
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
