# Rule Engine runs before the cache

The obvious ordering — cache lookup first, rules only on miss — makes verdicts
stale: a domain cached yesterday keeps resolving after you block it today, until
its TTL expires. We run the Rule Engine on **every** query, before the cache,
and the cache stores only upstream answers, never verdicts. Rule changes take
effect instantly with no cache flush and no invalidation machinery.

## Considered options

- **Cache first + full flush on rule change** — daily list refreshes would mean
  a daily cold cache.
- **Cache first + verdict-aware invalidation** — fastest steady-state, but
  cache invalidation is famously bug-prone and the win is negligible.

The cost of rules-first is one compiled-matcher lookup per query
(O(labels) hash probes, allocation-free, tens of nanoseconds) — noise next to
even a cache hit's reply path.
