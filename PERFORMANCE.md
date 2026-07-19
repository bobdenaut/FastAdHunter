# PERFORMANCE

Performance is the primary feature. This document is the contract: golden
rules that every change must respect, and numeric budgets that `cargo bench`
verifies.

## Golden rules

1. **Zero-copy where possible** — reference buffers, don't copy them.
2. **Streaming before buffering** — process incrementally; never load whole
   documents into memory.
3. **No runtime regex compilation** — and no regex on the hot path at all.
   Rules compile to hash/trie matchers at load time.
4. **No GC, no hidden allocations** — the hot path is allocation-free;
   allocations happen at load/reload time.
5. **No global locks** — atomic swap for ruleset/config, sharding for the
   cache, bounded channels between components.
6. **Cache-friendly layouts** — compact contiguous structures; pointer-chasing
   is the enemy on a 1.4 GHz ARM core.
7. **Bounded everything** — cache, ring buffers, channels, retention. Memory
   must not grow with traffic or uptime.
8. **Deterministic execution** — predictable latency beats occasional
   brilliance; avoid work with unbounded tails on the query path.
9. **Every feature justifies its runtime cost** — a PR that touches the hot
   path states its cost in its description.

## Budgets (acceptance targets)

Reference hardware: MikroTik RB5009 — Marvell Armada quad-core ARMv8 @ 1.4 GHz,
1 GB RAM shared with RouterOS. Verified with criterion benches in `benches/`
(`cargo bench`) and soak tests on the device.

| Metric | Budget |
|--------|--------|
| RAM steady-state, 1M blocked domains loaded | ≤ 128 MB |
| Compiled ruleset for 1M domains | ≤ 40 MB |
| RAM hard ceiling (container limit) | 256 MB |
| Verdict + cache hit, in-engine p99 | < 1 ms |
| Blocked query, in-engine p99 | < 1 ms |
| Forwarded query overhead added by engine, p99 | < 1 ms |
| Sustained throughput on RB5009 | ≥ 10 000 QPS |
| Startup to serving (cached lists, 1M-domain parse) | 1–3 s (< 3 s hard, ~1 s goal) |
| Container image size | ≤ 30 MB |

Notes:

- In-engine latency excludes upstream RTT — we measure what we add.
- 10k QPS is ~100× a busy household's peak; the headroom is the proof of
  efficiency, and it's what keeps p99 flat at real loads.
- Budgets are compared against `main` on every perf-relevant change; a >10%
  regression on a hot-path bench needs an explicit justification
  (see [CONTRIBUTING.md](CONTRIBUTING.md)).

## Measuring reliably

The hot-path benches resolve sub-microsecond work, which is below the noise
floor of a loaded desktop. An unpinned `cargo bench` on a busy dev machine has
been observed swinging **6×** between consecutive runs of an unmodified binary
— enough to manufacture a "+159% regression" that does not exist. Before
believing any regression, re-measure with the process pinned to one core:

```powershell
# Windows: run the bench executable directly, one core, high priority
$p = Start-Process -FilePath 'target\release\deps\<bench>-<hash>.exe' `
     -ArgumentList '--bench','--sample-size','200' -NoNewWindow -PassThru
$p.ProcessorAffinity = 4; $p.PriorityClass = 'High'; $p.WaitForExit()
```

```sh
# Linux
taskset -c 2 nice -n -5 cargo bench -p <crate> --bench <bench>
```

Pinned, the same benches hold a confidence interval under 1%. Trust a criterion
delta only when its interval is narrow relative to the change it reports — a
result quoted as `[366.0 ns 366.8 ns 367.5 ns]` is a measurement; one quoted as
`[737 ns 882 ns 1.04 µs]` is noise wearing a number's clothes.

Throughput is the exception to core-pinning: restrict it to four cores
(`ProcessorAffinity = 15` / `taskset -c 0-3`) so the figure is shaped like the
RB5009's quad-core budget rather than a dev box's full core count.

## Positioning

Beat AdGuard Home and Blocky on **both** axes:

- **Efficiency** — their steady-state RAM (roughly 100–200 MB and 50–100 MB
  respectively) is our ceiling territory; our target is below both.
- **Functionality** — streaming HTML rewriting powered by lol_html (Phase 4)
  filters inside pages, which neither does.
