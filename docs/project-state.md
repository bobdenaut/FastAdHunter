# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-23

## Now

| | |
| --- | --- |
| Branch | `main`, `4f45208` pushed to origin and backup; the Stage 1 benchmark-audit edits (spec, benchmarks, p2.6 plans, ROADMAP, README, this file) are in the tree, uncommitted |
| Tree | no code change since p2.5-11 (`6417842`); docs only |
| Tests | green at p2.5-11 — fmt/clippy/test, 982 passed, 3 ignored, 42 suites |
| Version | 0.2.18 (workspace) |
| Deployed | **0.2.18** on the RB5009 since 2026-08-23 01:19 EEST (p2.5-09 V2) — carries p2.5-01…08: `counters.dns.answers` and `failure_runs` live; **S1-G4 run-length collection running since that moment.** p2.5-10 (WS `endpoint`) and p2.5-11 (`counters.http.refused`) are merged, not deployed |
| Phase | 2.5 `plan/wip/phase2.5-hardening` — p2.5-01…08 and p2.5-10 DONE; p2.5-09 open on V6 (≥ 12 h soak, evaluable from 2026-08-23 10:20 UTC) and V7; p2.5-11 code merged, review status pending in the table |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): PASS WITH REQUIRED CHANGES; §5.1–6 are this phase |
| **Next** | close p2.5-09 (V6 read, V7 records) and mark p2.5-11; close Phase 2.5; then `plan/open/phase2.6-adaptive-stage1` — the phase move is the owner's call |

## Phase 2.6 — Adaptive DNS Stage 1, frozen for implementation

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

## Sequencing

1. Close Phase 2.5 (p2.5-09 V6/V7, p2.5-11 marked).
2. **Phase 2.6 — Adaptive Stage 1**: p2.6-01…09 on the dev box → `adaptive`
   opt-in; p2.6-10 (null A/B) and p2.6-11 (opt-in deploy, 7-day soak, S1-G4,
   S1-G5) on the device; p2.6-12 default flip only if every deployment-tier
   gate passes. If the run-length window shows only isolated single losses,
   not flipping the default is the correct outcome.
3. **Phase 3 — after 2.6 closes**, conditional on §5.7–14 (cert machinery
   home/ADR, connector redesign, DoH/DoT listener placement, telemetry
   taxonomy, memory caps per new state owner, 443 steering v4+v6, on-device
   TLS measurements, opt-in bound to stable identity).

## Open unknowns (reconciled §4)

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
- S1-G4 run-length distribution: collecting since the 0.2.18 deploy; read as
  the sum of per-process `failure_runs` deltas over the `fallback` window
  (counters reset on restart); only closed runs bucketed; concurrent queries
  can split one outage — lower bound on clustering. No run closed in the first
  15 min (p2.5-09 V5c); live bucketing under real failures still unobserved.
- p2.5-09 V3b (live healthcheck column) is a RouterOS 7.21.5 platform
  limitation, not a build defect.

## Deferrable (reconciled §6)

Type mirrors (`fah_config`/`fah_model`), `CacheStats` identity-DTO; compile
transient (peak 181.4 MiB deployed, monitored via `peak_rss`); policy
fail-open window and name-assignments-on-LRU; SWR no-EDNS truncation tax;
fah-common scope creep; `blocking_mode` inert; histogram 100 ms ceiling.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (10013) when WinNAT reserves the ephemeral port block.
Environmental — do not attribute to a change.
