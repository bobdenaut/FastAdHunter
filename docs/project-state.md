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
