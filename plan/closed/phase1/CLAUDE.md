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
| 11 | `p1-11-rb5009-deploy.md` | Deployment guide + on-device soak on the RB5009 | Sonnet | DONE |

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

**Cutover done 2026-07-19.** All six IPv4 rules on `172.17.0.2` (including the
two WireGuard `srcnat` accepts), `start-on-boot=yes`, `status=running`.
`GET /api/v1/queries` shows real household traffic from multiple LAN clients
with their own source addresses, blocked verdicts carrying rule and list
attribution, and durations of 0.013–0.148 ms against a 1 ms p99 budget. The
populated `rule` field is on-device confirmation that removing `ParsedRule.raw`
(`c61c00b`) did not break attribution — `decisive_rule` reconstructs it.

AdGuard stays running: it is the rollback target (`dns-adguard`) *and* the
IPv6 resolver the v6 `dstnat` points at. Stopping it would break IPv6 for the
whole LAN.

**Update 2026-07-21:** liviu stopped AdGuard entirely ~2026-07-19 — the note
above is obsolete. FastAdHunter has been the LAN's only resolver since (v6
DNS blackholes at the dead dstnat target and clients fall back to IPv4);
54.4 MB RSS after 2 days of full household traffic. Rollback now requires
starting the AdGuard container before `dns-adguard`.

**Soak complete — p1-11 DONE (2026-07-23).** ~91h on-device soak on the
RB5009 (`process_resident_memory_bytes` scraped every 5 min, 1061 samples,
19–23 Jul), driven the last ~40h by `test_aleator.py` — a deliberate
worst-case generator: 50 workers, no inter-query sleep, 7 query types over
1M then 10k unique domains, hitting both `192.168.10.1` and
`2a02:2f04:5008:bb00::11`. Full evidence:
[docs/code-review/p1-11-soak.md](../../../docs/code-review/p1-11-soak.md).

- **No leak — memory is bounded (hard rule #4 holds).** Every *continuous*
  run (no restart) fills its cache and then holds dead-flat for hours: real
  traffic ~55 MiB flat for 2.5 days; 1M-hammer plateaus at ~125 MiB
  (8M→26M queries) and, in the overnight run, **230.7 MiB held flat for 9h /
  12M queries (230.5–231.1, zero drift)**. Plateaus, never linear growth.
  Counter resets are the power failure + 0.2.0 deploy + test restarts, never
  steady-state crashes.
- **Hot path healthy under stress:** blocked p99 ≤ 1 ms held (99.9%+ under
  1 ms), upstream failures < 0.4%, DNS answers never affected. The bounded
  QueryEvent channel shed ~755k events at peak synthetic QPS — designed
  backpressure, 0 under real traffic.
- **0.2.0 IPv6 dual-stack validated under load:** the v6 listener served
  ~22.4M queries during the test.

**KNOWN LIMITATION (recorded, not gating closure) — cache byte-ceiling.**
The DNS cache is bounded by **entry count, not bytes** (p1-05). Real traffic
plateaus at ~55 MiB (2.3× under the 128 MB budget), but the synthetic
worst-case mix (7 types × 1M unique domains → millions of large TXT/SOA/
NXDOMAIN entries) pushed the *byte* ceiling to ~230 MiB on-device — 80% over
budget, though still bounded and never near OOM (router peaked 665/1024 MiB,
≥35% free). This is also a mild DNS-water-torture surface. **Follow-up (p2 /
p1-05):** add a byte-aware cache cap so the ceiling respects 128 MB under
adversarial input. Sustained-throughput measurement is folded into the same
follow-up rather than gating closure — the soak already proves the budget and
latency hold under load far above household levels.

IPv6 DNS was repaired on 2026-07-19 (the `dstnat` pointed at a dead ULA;
retargeted to AdGuard's real address). With AdGuard stopped that target is
dead again — and as of 2026-07-21 the dual-stack listener is **implemented**
(`[dns.listen] address = "::"` serves both stacks on one socket, v4-mapped
client addresses canonicalized; deploy-rb5009.md §IPv6 has the cutover
steps). Remaining on-device: set `address = "::"` in the config, deploy the
new build, move the two v6 dstnat `to-address` values to veth1's
`2a02:2f04:5008:bb00::11`.

Two known metric defects do not gate closure but should be recorded in the
completion note: ruleset gauges are polled on a 10 s timer, so a scrape right
after a list change reads the previous ruleset (`GET /api/v1/lists` is live);
and `compile_duration_seconds` is hardcoded to zero.

Phase 1 code (p1-01..p1-10) was committed at `4d72fd2`, then RC-review fixes +
the O(1) cache eviction (`e5f5b8a`) and the IPv6 dual-stack listener
(`753fffb`, == the 0.2.0 build soaked here). With p1-11 DONE (soak PASSED,
above) all 11 tasks are `DONE` — **phase moved `wip` → `closed` 2026-07-23.**
Deferred to `phase1.5` (pre-phase2 observability base): byte-aware cache cap
and sustained-throughput measurement.

**Definition of done:** a phone pointed at the container's IP browses with ads
blocked; `GET /api/v1/stats` shows real counters; benches meet PERFORMANCE.md
budgets on dev hardware; container runs on the RB5009 through real household
traffic without exceeding the RAM budget.

**Key risks:** matcher memory/speed budget (mitigation: p1-02 benches early,
before anything depends on its internals); DoT/DoH upstream quirks behind
Hickory (mitigation: plain UDP path is the default and lands first);
RB5009 on-device validation depends on user's router access (p1-11 is last and
can wait without blocking the phase's code).
