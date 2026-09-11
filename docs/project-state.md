# Project state

Where the work is right now. **Rewrite this file — never append.** History
belongs in `git log`, `docs/code-review/` and the phase tables; this file is only
what is true today.

**Last updated:** 2026-09-07

## Now

| | |
| --- | --- |
| Branch | `phase3-06` = `main` (`857865d`, 0.3.3) + Phase 3, with the HTTPS listener re-homed on the allocation domains (merge `e0c6071`, 2026-09-07). Local: the merge, two docs commits and this docs pass are not pushed. Tag `pre-alloc-domain-2026-09-06` = `64be513` stays the rollback point before the allocation domains |
| Tree | clean after the two N3 follow-up commits of 2026-09-11 on `phase3-06`: engine, telemetry and docs, then the dashboard line; the untracked `p3-06-probe/smoke-20260911T0700Z/` capture predates them and is not committed |
| Tests | green on the working tree 2026-09-11: fmt, clippy `-D warnings`, `cargo test --all-features --workspace`; vitest 58 files / 1047; bundle 137 074 B gzip (89.2 %) |
| Version | 0.3.3 (workspace). Newest tags `v0.3.2`, `pre-alloc-domain-2026-09-06`, `soak-p2.6-11` |
| Deployed | production on **0.3.3** — HTTP allocation domains, N=2 (`FAH__RUNTIME__HTTP_RUNTIMES=2` on `fah-env`) — since the owner-run swap of **2026-09-07** on `veth1` / `172.17.0.2`, mount lists `fah-config,fah-data`; the 0.3.1 soak was stopped on day 6 for it. Soak **restarted 2026-09-09** — the run from the 09-07 swap was invalidated by the p3-06 query flood of 2026-09-08. **T0 = 2026-09-09T11:15:15Z** (container start in the router log); day 7 = **2026-09-16**. The p3-06 probe (`fah-probe` on `veth3` / 172.17.0.4) was torn down 2026-09-11 evening with its config, data and image tars; `veth3` remains, no test firewall rule or address list remains |
| Build ≠ tip | deployed code = `main` at `1c61f9c` (`7d03e95`…`857865d` are plan and docs). `phase3-06` is not deployed: Phase 3 waits on this soak's verdict and then its own 24 h full-mode soak |
| Phase | **5 closed. 2.6 closed 2026-09-07** — `adaptive` shipped opt-in on 2026-08-25 and is the compiled-in default since `p2.6-12` (on `phase3-06`, 2026-09-07); the `fallback` walk is deleted. **3 in `plan/wip/phase3`** — `p3-01`…`p3-05` `DONE`, `p3-06` `AWAITING SOAK`, `p3-07`…`p3-09` `DONE` on `phase3-06` 2026-09-10 (ADR-0008: Interception Document, 525 classification, rejection view + editor). **N3 follow-up closed 2026-09-11 as a technical experiment, not promoted** — the client alert names the TLS stack, not the cause; counters and the per-client account landed on `phase3-06`; HTTP/3 must be refused for intercepted clients ([p3-06-n3-alert-ab.md](code-review/phase3/p3-06-n3-alert-ab.md)) |
| Gate | [Global Architecture Review-Reconciled.md](code-review/Global%20Architecture%20Review-Reconciled.md): §5.1–6 cleared; §5.7–14 gate Phase 3. S1-G2 tiers 1–3 met; S1-G4 and S1-G5 route 2 not validated and will not be |
| **Next** | the 0.3.3 soak runs to **2026-09-16**; its verdict is ADR-0006's plateau. Then, on `phase3-06`: remeasure N with TLS on the RB5009 (ADR-0006 revisit trigger), the `p3-06` 24 h full-mode soak, and only then `phase3-06` → `main`. Off the critical path: dashboard settings metadata for `runtime.http_runtimes`, the TLS / lol_html capacity microbench on the probe. **Before Phase 3 goes live:** the router refuses UDP 443 LAN→WAN (deploy-rb5009.md §5c), HTTP/3 bypasses the steer otherwise |

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
HTTP shutdown.

## Known-good gate note

`cargo test -p fastadhunter --test e2e` fails on the Windows dev box with
`WSAEACCES` (10013) when WinNAT reserves the ephemeral port block.
Environmental — do not attribute to a change.

p2.5-09 V3b (live healthcheck column) is a RouterOS 7.21.5 platform
limitation, not a build defect — documented as a trap in
[routeros-traps.md](routeros-traps.md) §Container configuration.
