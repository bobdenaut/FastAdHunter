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

**All six defects are now fixed** (`64c3e32`) and verified on-device across
several boots: port 53 bound via start-as-root then drop to uid 65532
(ADR-0004), list downloads resolved through the configured upstreams rather
than the empty `/etc/resolv.conf`, `last_status` decoupled from rule counts,
duplicate list sources rejected with 409.

**Startup at 1M rules is measured and inside budget.** 2 440 ms at 1 213 640
rules, down from 3 113 ms — `c61c00b` removed two allocations per rule after
`bench_startup_phases` showed parse at 81% of startup and disk read at 3%.
See PERFORMANCE.md.

A suspected list-persistence regression was investigated and **is not a code
defect**: the writes were lost by a previous container instance whose
`/config` mount was not effective, and died with `container/remove`. Verified
on the current instance — delete, restart, still gone.

**Remaining to close p1-11:**

**Cutover done 2026-07-19.** All six IPv4 rules on `172.17.0.3` (including the
two WireGuard `srcnat` accepts), `start-on-boot=yes`, `status=running`.
`GET /api/v1/queries` shows real household traffic from multiple LAN clients
with their own source addresses, blocked verdicts carrying rule and list
attribution, and durations of 0.013–0.148 ms against a 1 ms p99 budget. The
populated `rule` field is on-device confirmation that removing `ParsedRule.raw`
(`c61c00b`) did not break attribution — `decisive_rule` reconstructs it.

AdGuard stays running: it is the rollback target (`dns-adguard`) *and* the
IPv6 resolver the v6 `dstnat` points at. Stopping it would break IPv6 for the
whole LAN.

**Remaining:** run the 24h soak (§9 of deploy-rb5009.md), then measure
sustained throughput.

IPv6 DNS was repaired on 2026-07-19 (the `dstnat` pointed at a dead ULA;
retargeted to AdGuard's real address). It currently routes to AdGuard, **not**
to FastAdHunter, so the soak still measures IPv4 only — acceptable, since the
IPv6 path is now filtered rather than bypassing. Moving IPv6 onto FastAdHunter
needs the listener to bind `::` as well as `0.0.0.0`; veth2 already has
`2a02:2f04:5008:bb00::11/64`, so it is a bind-address change plus a one-line
`to-address` move. Not in Phase 1.

Two known metric defects do not gate closure but should be recorded in the
completion note: ruleset gauges are polled on a 10 s timer, so a scrape right
after a list change reads the previous ruleset (`GET /api/v1/lists` is live);
and `compile_duration_seconds` is hardcoded to zero.

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
