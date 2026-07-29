# p1-11 — On-device soak results (RB5009)

**Window:** 2026-07-19 17:12Z → 2026-07-23 12:09Z (~91 h), 1061 `/metrics`
samples at 5-min cadence, plus paired `/api/v1/stats`. Target: the
FastAdHunter container on the RB5009 (veth1, `172.17.0.2`, 1.19M rules).

**Load:** light real household traffic first, then ~40 h under
`plan/wip/phase1/test_aleator.py` — a deliberate worst-case generator:
50 async workers, no inter-query sleep, 7 query types (A/AAAA/MX/NS/TXT/SOA/
CNAME), per-worker chunk of 1M (then 10k) unique domains, split across
`192.168.10.1:53` (IPv4) and `[2a02:2f04:5008:bb00::11]:53` (IPv6). This is a
cache-saturation / near-zero-locality stressor, **not** representative load.

## Verdict: bounded, no leak — p1-11 PASS

Every *continuous* run (between restarts) climbs, fills its cache, then holds
flat. A leak grows linearly without bound; this reaches a plateau and stays.

| Run | Build / regime | Plateau (process RSS) | Held flat across |
| --- | --- | --- | --- |
| Segment A | v0.1.0, real traffic | **~55 MiB** | 2.5 days, +0.9 MiB drift |
| Run 1 | v0.2.0, light | ~55 MiB | hours |
| Run 2 | v0.2.0, 1M hammer | **~125 MiB** | 8M → 26M queries, flat |
| Run 4 | v0.2.0, 1M hammer (overnight) | **~230.7 MiB** | **9 h / 12M queries, 230.5–231.1, zero drift** |

Distinct plateau *levels* reflect load and answer-size mix, not growth — each
level is stable for hours. Counter resets across the window map to the power
failure (~07-21), the 0.2.0 deploy (07-21 21:57Z), and test restarts; none
occurred while sitting at a plateau (no steady-state crash / OOM).

## Hot path under stress

- **Blocked p99 ≤ 1 ms held:** 99.9%+ of blocked verdicts under 1 ms
  throughout (budget: 1 ms p99).
- **Upstream failures < 0.4%** of attempts; DNS answers never affected.
- **QueryEvent channel** shed ~755k events at peak synthetic QPS — the
  bounded channel's designed backpressure protecting the hot path; **0 drops
  under real traffic**.
- **Cache hit rate** tracked locality: 1.4% during the 1M-unique hammer
  (expected — every key distinct), 74% under the 10k regime.

## 0.2.0 IPv6 dual-stack — validated under load

`test_aleator.py` drove both stacks. Stats show the v6 client address served
**~22.4M queries** over the test, confirming the single-socket `::`
dual-stack listener (v4-mapped clients canonicalized) works on-device under
full load.

## Router-total memory (RouterOS graph) — reconciled

The RouterOS "Memory Usage" graph (1024 MiB total) read ~588 MiB (57%),
max 665 MiB (65%) during the test. This is **RouterOS base + container RSS +
reclaimable page cache** (millions of querylog writes to `/data` during the
hammer). It is a step-to-plateau tracking test load — flat baseline for weeks
on the monthly graph, one bounded step during the test window — never runaway,
always ≥35% free. Consistent with the directly-measured process RSS above.

## Known limitation (recorded — does not gate closure)

The DNS cache is bounded by **entry count, not bytes** (p1-05). Under the
pathological mix the *byte* ceiling reached ~230 MiB — 80% over the 128 MB
budget — while real traffic stays ~55 MiB (2.3× under). Still bounded, never
near OOM. It is also a mild DNS-water-torture surface. **Follow-up (phase 2 /
p1-05):** add a byte-aware cache cap so the worst-case ceiling respects the
128 MB budget under adversarial input; fold sustained-throughput measurement
into the same work.
