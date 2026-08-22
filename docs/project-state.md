# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-08-22

## Now

| | |
| --- | --- |
| Branch | `main`, p2.5-07 committed and pushed to origin and backup |
| Tree | p2.5-08 in progress (token untracked, guard widened, docs reconciled) |
| Tests | green — fmt/clippy/test, 979 across 42 binaries |
| Version | 0.2.17 (workspace) |
| Deployed | **0.2.17** on the RB5009 (owner-confirmed 2026-08-22; no soak record yet — last recorded soak is `soak-0.2.16-72h`) — carries p2.5-01…07, so `failure_runs` (S1-G4) is collecting since that deploy |
| Phase | 2.5 `plan/wip/phase2.5-hardening` — p2.5-01…07 DONE, p2.5-08 in progress, p2.5-09 WAITING |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): PASS WITH REQUIRED CHANGES; §5.1–6 are this phase |
| **Next** | finish p2.5-08; p2.5-09 — deploy, listener-death drill, S1-G4 collection running |

Phase 2 closed (`plan/closed/phase2`). Phase 3 reset: the Phase 3 decision/ADR
package is out of this phase.

## Sequencing after 2.5

1. **Adaptive DNS Stage 1 — GO**, ships behind §5.3–6 (S1-G4 run-length data,
   encrypted reconnect, `io::ErrorKind` fidelity, SERVFAIL-served + endpoint
   attribution) — all landed in p2.5-03…07; S1-G4 needs on-device days.
2. **Phase 3 — GO after Adaptive closes**, conditional on §5.7–14 (cert
   machinery home/ADR, connector redesign, DoH/DoT listener placement,
   telemetry taxonomy, memory caps per new state owner, 443 steering v4+v6,
   on-device TLS measurements, opt-in bound to stable identity).

## Open unknowns (reconciled §4)

- IPv6 upstream forwarding: first on-device evidence 2026-08-22
  (`2606:4700:4700::1111` at index 0, 37 attempts / 0 failures, 0.2.17;
  [p2.5-08 review](code-review/phase2.5/p2.5-08-hygiene-review.md) §Side
  finding). Defaults stay v4 literals; no soak yet.
- DoH bootstrap via the container's OS resolver unverified (possible self-loop).
- Upstream TCP/53 reachability mock-tested only; ICMP visibility on connected
  UDP.
- HTTP path under concurrency unmeasured; 1024-permit ceiling never exercised.
- TLS on RB5009: handshake, interception CPU+memory, cert-mint, splice — probe
  containers, not dev benches.
- `MIMALLOC_PURGE_DELAY=0` above idle load; cache byte-cap eviction never fired
  on-device.
- S1-G4 run-length distribution: starts at the first deploy carrying p2.5-06;
  only closed runs bucketed, concurrent queries can split one outage — lower
  bound on clustering.

## Deferrable (reconciled §6)

Type mirrors (`fah_config`/`fah_model`), `CacheStats` identity-DTO; compile
transient (peak 181.4 MiB deployed, monitored via `peak_rss`); policy
fail-open window and name-assignments-on-LRU; SWR no-EDNS truncation tax;
fah-common scope creep; `blocking_mode` inert; histogram 100 ms ceiling.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (10013) when WinNAT reserves the ephemeral port block.
Environmental — do not attribute to a change.
