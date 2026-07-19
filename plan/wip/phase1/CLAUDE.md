# Phase 1 — DNS + REST API + Docker

**Objective:** the complete network-wide DNS ad blocker per ROADMAP.md Phase 1:
Rule Engine with all four list formats, DNS pipeline (rules → cache → upstream),
stats/query log, Prometheus metrics, the full authenticated HTTPS API, and a
deployable image validated on the RB5009.

**Why this order:** Rule Engine first (pure logic, no I/O — testable in
isolation), then the DNS engine that consumes its verdicts, then the observers
(stats, metrics), then the API that exposes everything, and finally proof:
benches against PERFORMANCE.md budgets and the on-device soak.

**Always select the first task whose `STATUS` is `WAITING`.**

| # | Task file | Outcome | MODEL | STATUS |
|---|-----------|---------|-------|--------|
| 1 | `p1-01-rule-parsers.md` | 4 formats parsed + classified (DNS-active vs inactive) | Sonnet | DONE |
| 2 | `p1-02-compiled-matcher.md` | Verdict lookup: allow > block, subdomains, <1ms, 1M domains ≤40MB | Opus | DONE |
| 3 | `p1-03-list-lifecycle.md` | Download, validate, atomic swap, /data cache, refresh, user rules | Sonnet | DONE |
| 4 | `p1-04-dns-pipeline.md` | UDP/TCP listeners, EDNS(0), verdict → blocked-response synthesis | Sonnet | DONE |
| 5 | `p1-05-dns-cache.md` | Bounded sharded cache, TTL clamps, negative, serve-stale | Sonnet | DONE |
| 6 | `p1-06-upstreams.md` | UDP/TCP + DoT + DoH, parallel fallback, DNSSEC pass-through | Sonnet | DONE |
| 7 | `p1-07-stats-querylog.md` | QueryEvent channel, aggregates, ring + SSD segments, retention | Sonnet | DONE |
| 8 | `p1-08-metrics.md` | Prometheus /metrics: QPS, latency histograms, cache ratio | Sonnet | DONE |
| 9 | `p1-09-api.md` | Full API.md surface: key auth, TLS, endpoints, WS events (heavy) | Opus | DONE |
| 10 | `p1-10-benches-integration.md` | criterion benches vs budgets + end-to-end integration tests | Sonnet | DONE |
| 11 | `p1-11-rb5009-deploy.md` | Deployment guide + on-device soak on the RB5009 | Sonnet | WAITING |

**p1-11 status (2026-07-19):** deployment guide complete and validated against
the live RB5009 — container runs, blocking works, and every measured budget
passes at 1.19M rules (49.6 MB RSS, 37.7 MB ruleset, 0.042 ms blocked p99,
12 MB image). Six defects found on-device, recorded in
[docs/code-review/p1-11-review.md](../../../docs/code-review/p1-11-review.md).

Defect 1 (list mutations not persisted) is **fixed** — `POST/PATCH/DELETE
/api/v1/lists` now write `[[rules.lists]]` back through `ConfigStore` before
mutating the engine, with a regression test that reparses the TOML from disk.
That unblocks the soak and the two unmeasured budgets (startup-to-serving at
1M rules, sustained throughput), all of which need the ruleset to survive a
restart.

**Remaining to close p1-11:** rebuild and redeploy the image to the RB5009,
re-add the 1M-rule lists (they will persist this time), measure startup@1M and
sustained throughput, then run the 24h soak. Defects 2–6 stay open and do not
gate it.

Phase 1 code (p1-01..p1-10) is committed at `4d72fd2`; the phase stays in `wip`
until p1-11 closes.

**Definition of done:** a phone pointed at the container's IP browses with ads
blocked; `GET /api/v1/stats` shows real counters; benches meet PERFORMANCE.md
budgets on dev hardware; container runs on the RB5009 through real household
traffic without exceeding the RAM budget.

**Key risks:** matcher memory/speed budget (mitigation: p1-02 benches early,
before anything depends on its internals); DoT/DoH upstream quirks behind
Hickory (mitigation: plain UDP path is the default and lands first);
RB5009 on-device validation depends on user's router access (p1-11 is last and
can wait without blocking the phase's code).
