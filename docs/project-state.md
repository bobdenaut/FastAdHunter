# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-09-11

## Now

| | |
| --- | --- |
| Branch | `main` at `ad110d8` = `origin/main` = `backup/main`. `alloc-domains/http` merged 2026-09-07 (`e9dac79`, released as 0.3.2 `89aac76`); the branch is 0 commits ahead and can be deleted. Tag `pre-alloc-domain-2026-09-06` = `64be513` stays the rollback point before the allocation domains |
| Tree | uncommitted on `main`: `fa9451a` cherry-picked from `phase3-06` (`p2.6-12` — `adaptive` the only strategy, `fallback` deleted, conflicts in README/ROADMAP/this file resolved to `main`'s state, Phase-3-only test content dropped), the A/B harness `fah-dns/tests/strategy_ab.rs` (`#[ignore]`, now adaptive-only) and its measurement [strategy-ab-fallback-vs-adaptive.md](code-review/phase2.6/strategy-ab-fallback-vs-adaptive.md), `udp_inflight_cost.rs` and `shutdown_e2e.rs` moved off the removed strategy. Awaiting the commit go |
| Tests | green on that tree over `ad110d8`: fmt, clippy `-D warnings`, `cargo test --all-features --workspace`; F1/F2/F10 re-verified after the cherry-pick (fah-dns tcp/udp units, the ceiling wiring test, config/metrics/api round-trips, `shutdown_e2e` under `rust:1.96.0` in a Linux container) |
| Version | 0.3.3 (workspace, since `1c61f9c`), untagged. Newest tags `v0.3.2` (`89aac76`), `pre-alloc-domain-2026-09-06`, `soak-p2.6-11` |
| Deployed | production on **0.3.3** = `main` at `1c61f9c` (`7d03e95`…`857865d` are plan and docs) — HTTP allocation domains, N=2 (`FAH__RUNTIME__HTTP_RUNTIMES=2` on `fah-env`), `veth1` / `172.17.0.2`, mounts `fah-config,fah-data`. `GET /health` on 2026-09-11 23:13 local answered `0.3.3`, uptime 204 978 s (container start 2026-09-09T11:15Z) |
| Build ≠ tip | the running container predates the 2026-09-11 `main` commits — F10 stats flush (`32d7776`), F1 TCP bound (`ed28395`), F2 UDP ceiling (`b0b091e`), their close-out (`ad110d8`) — and the strategy removal: it still loses up to 300 s of stats on a stop and has no DNS-over-TCP ceiling. They ship with the next image. **Before that image boots, the router's TOML must say `strategy = "adaptive"`** (it does — the opt-in of 2026-08-25 set it); a config still saying `fallback` refuses to load |
| Phase | **0–2.6 and 5 closed** (2.6 closed 2026-09-07, all 13 tasks `DONE`; `p2.6-12` reached `main` on 2026-09-11 as the cherry-pick of `fa9451a` — `adaptive` is the only strategy, the `fallback` walk is deleted, a config naming it fails at load). `plan/wip` is empty; phases 3 and 4 are **parked** in `plan/open`, every task `WAITING`. No phase move was made |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 cleared; §5.7–14 gate Phase 3. S1-G2 tiers 1–3 met; S1-G4 and S1-G5 route 2 not validated and will not be |
| **Next** | (1) commit and push the strategy removal with its A/B evidence to both remotes. (2) F11 is the next review candidate (§Risk inventory close-out). (3) The F1 tuning soak once the F1 build is deployed. Off the critical path: the dashboard's `fallback` mode branch (`derive.ts`, `degraded-banner.tsx` and their tests) is now dead code and can go with its own frontend gate; dashboard settings metadata for `runtime.http_runtimes`; the TLS / lol_html capacity microbench on the probe |

## Risk inventory close-out — 2026-09-11

[project-risk-inventory.md](code-review/project-risk-inventory.md) surveyed
`main` at `baa2ecd`. Its three material findings — F1 (DNS-over-TCP had no
connection ceiling and allocated from the client's length prefix), F2 (UDP
in-flight queries unbounded) and F10 (no stats flush on a clean stop) — are
**closed**: fixed in `ed28395`, `b0b091e` and `32d7776`, verified by the
close-out audit, and moved to the inventory's §Closed with their evidence. No
material finding is open.

Follow-ups, neither a risk:

- **F1 soak — tuning only.** 1024 connections and 16 KiB per message are
  initial safety bounds. After 7 days on the RB5009 *with the F1 build*, read
  `counters.dns_tcp_connections.{peak,closed_oversize}` from
  `/api/v1/telemetry`, check the container fd budget, set the final
  `tcp_max_connections` default, and record corpus, workload and device under
  `docs/code-review/`. Cannot start before that build is deployed.
- **F10 history-write residual.** `fah-stats` `history/mod.rs` `append_line`
  writes a rollup line and its `\n` as two `write_all`s; a stop landing between
  them leaves a partial line that the reader skips — one completed hour lost.
  Pre-existing, a microsecond window once per 300 s; the fix is one combined
  write. Its own go.

Next review candidate: **F11** — long-lived task death is unobserved (a
`JoinSet` in the run loop's `select!` would surface it). F3–F9 and F12 are
record-only. N5 (`Semaphore::new` panics above `MAX_PERMITS`; neither
`max_connections` key has an upper bound) is recorded and excluded by owner
decision.

## HTTP allocation domains — merged

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

Rollback without a rebuild: `FAH__RUNTIME__HTTP_RUNTIMES=0` on `fah-env` +
restart (the 0.3.1 code path, same image). Rollback of the build:
`kingston/fastadhunter-arm64-0.3.1.tar`, or `main` at the tag.

Deferred, each its own go: 11b graceful shutdown of keep-alive connections
(finish the in-flight exchange instead of the whole transfer); dashboard
settings metadata for `runtime.http_runtimes` (review finding 3); the capacity
microbench (rustls AES-GCM and lol_html ms/MiB on the RB5009) that decides
whether N=2 clears 1 Gbit with TLS; IPv6 privacy-address rotation versus
address-exact client identity — reviewed 2026-09-07
([ipv6-privacy-rotation-review.md](code-review/phase2.6/ipv6-privacy-rotation-review.md)),
nothing built, direction is the owner's call.

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
