# P2-02 — HTTP Proxy Core

**Phase:** 2 · **Depends on:** p2-01 · **Model:** Opus

## Goal

`fah-http` transparently proxies plain-HTTP traffic with streaming bodies and
a pass-through fast path.

## Context

README HTTP pipeline: TCP → parser → headers → (rules later) → client. The
router dst-nats port 80 to the container, so this is a transparent proxy:
original destination comes from the Host header (RouterOS containers don't
expose SO_ORIGINAL_DST-style metadata — document this constraint). Performance
golden rules apply hard here: streaming before buffering, zero-copy where
possible, bounded everything.

## Scope

- Hyper-based server + client: accept intercepted connections, parse request
  head, resolve upstream from Host header (through our own DNS pipeline —
  in-process handle, not loopback UDP), stream request/response bodies both
  directions without buffering full messages.
- Keep-alive both sides, connection pooling upstream, bounded pools and
  per-connection memory; timeouts from `[http]` config.
- Pass-through fast path: until filtering lands, every request forwards with
  minimal header touch (add `Via`, strip hop-by-hop headers per RFC 9110).
- Graceful behavior for non-HTTP bytes on port 80: detect, close, count.
- Tests: local origin server round-trips (small + multi-MB streamed bodies,
  chunked, keep-alive reuse), latency bench (added overhead vs direct)
  in `benches/`.

## Acceptance criteria

- Multi-MB body proxied with bounded memory (RSS flat during transfer, test
  asserts via allocation counters or RSS sampling).
- Added latency for pass-through < 1ms p99 in-process (record numbers).
- Malformed request → clean 400/close, no panic (fuzz corpus).
- Gates green.

## Out of scope

Filtering, blocking, HTML rewriting (Phase 4), HTTPS (Phase 3).

## Suggested prompt

> Read ARCHITECTURE.md HTTP pipeline (post p2-01), PERFORMANCE.md golden
> rules, and plan/wip/phase2/p2-02-http-proxy-core.md. Implement the
> streaming transparent proxy on hyper with the fast path, bounded resources,
> and the tests + latency bench.
