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

| # | Task file | Outcome | STATUS |
|---|-----------|---------|--------|
| 1 | `p1-01-rule-parsers.md` | 4 formats parsed + classified (DNS-active vs inactive) | WAITING |
| 2 | `p1-02-compiled-matcher.md` | Verdict lookup: allow > block, subdomains, <1ms, 1M domains ≤40MB | WAITING |
| 3 | `p1-03-list-lifecycle.md` | Download, validate, atomic swap, /data cache, refresh, user rules | WAITING |
| 4 | `p1-04-dns-pipeline.md` | UDP/TCP listeners, EDNS(0), verdict → blocked-response synthesis | WAITING |
| 5 | `p1-05-dns-cache.md` | Bounded sharded cache, TTL clamps, negative, serve-stale | WAITING |
| 6 | `p1-06-upstreams.md` | UDP/TCP + DoT + DoH, parallel fallback, DNSSEC pass-through | WAITING |
| 7 | `p1-07-stats-querylog.md` | QueryEvent channel, aggregates, ring + SSD segments, retention | WAITING |
| 8 | `p1-08-metrics.md` | Prometheus /metrics: QPS, latency histograms, cache ratio | WAITING |
| 9 | `p1-09-api.md` | Full API.md surface: key auth, TLS, endpoints, WS events (heavy) | WAITING |
| 10 | `p1-10-benches-integration.md` | criterion benches vs budgets + end-to-end integration tests | WAITING |
| 11 | `p1-11-rb5009-deploy.md` | Deployment guide + on-device soak on the RB5009 | WAITING |

**Definition of done:** a phone pointed at the container's IP browses with ads
blocked; `GET /api/v1/stats` shows real counters; benches meet PERFORMANCE.md
budgets on dev hardware; container runs on the RB5009 through real household
traffic without exceeding the RAM budget.

**Key risks:** matcher memory/speed budget (mitigation: p1-02 benches early,
before anything depends on its internals); DoT/DoH upstream quirks behind
Hickory (mitigation: plain UDP path is the default and lands first);
RB5009 on-device validation depends on user's router access (p1-11 is last and
can wait without blocking the phase's code).
