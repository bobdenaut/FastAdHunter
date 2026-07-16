# P1-05 — DNS Cache

**Phase:** 1 · **Depends on:** p1-04 · **Model:** Sonnet

## Goal

Bounded, sharded in-memory cache of upstream answers with TTL clamps,
negative caching and serve-stale.

## Context

ARCHITECTURE.md §DNS Pipeline (cache stores upstream answers only, never
verdicts) + CONFIGURATION.md `[dns.cache]` + ADR-0001.

## Scope

- Bounded size from config (`max_entries`, default 10k, runtime-mutable),
  sharded by key hash across workers' access — no global lock; eviction
  LRU-ish or S3-FIFO (implementer's choice, justify in code comment).
- Key: (qname lowercase, qtype, qclass). TTL clamps: `min_ttl`, `max_ttl`.
- Negative caching per RFC 2308 (SOA minimum, capped by
  `negative_ttl_max_seconds`).
- Serve-stale per RFC 8767 minimal form: upstream failure ⇒ expired entry
  (≤24h stale) served with short TTL, flagged in QueryEvent.
- Tests: TTL expiry (mock clock), clamp behavior, negative entries, stale
  serving on upstream error, eviction at capacity, concurrent hammering.

## Acceptance criteria

- Memory bounded: filling beyond capacity evicts, never grows (test asserts).
- Cache hit adds < 1ms in-engine latency (bench added to `benches/`).
- Gates green.

## Out of scope

Persistence (cache is RAM-only by design), prefetch/optimistic refresh.

## Suggested prompt

> Read CONFIGURATION.md §[dns.cache], ARCHITECTURE.md pipeline properties, and
> plan/wip/phase1/p1-05-dns-cache.md. Implement the sharded bounded cache with
> RFC 2308 + RFC 8767 behavior and the listed tests + bench.
