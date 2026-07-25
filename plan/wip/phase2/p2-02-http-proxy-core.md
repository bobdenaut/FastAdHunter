# P2-02 — HTTP Proxy Core

**Phase:** 2 · **Depends on:** p2-01 · **Model:** Opus

## Goal

`fah-http` transparently proxies plain-HTTP traffic with streaming bodies and
a pass-through fast path.

## Context

README HTTP pipeline: TCP → parser → headers → (rules later) → client. The
router dst-nats port 80 to the container, so this is a transparent proxy:
original destination comes from the Host header, because RouterOS containers
do not expose SO_ORIGINAL_DST-style metadata.

**That constraint is a security problem, not a footnote.** `Host` is
attacker-controlled: any LAN device — or malware on one — picks where
FastAdHunter connects. `Host: 172.17.0.3:8443` reaches our own API,
`Host: 192.168.10.1` reaches the router, and link-local or metadata addresses
follow the same way. With no SO_ORIGINAL_DST there is nothing to cross-check
the claim against, so the guard has to be an allow-policy on the resolved
address, and it belongs **in this task** — before any forwarding code exists,
not retrofitted in p2-04.

Performance golden rules apply hard here: streaming before buffering, zero-copy
where possible, bounded everything.

## Scope

- Hyper-based server + client: accept intercepted connections, parse request
  head, resolve upstream from the Host header, stream request/response bodies
  both directions without buffering full messages.
- **Resolution goes through our own DNS pipeline, but not by calling it.**
  `fah-http` and `fah-dns` are both L3 and siblings never import each other
  (hard rule 1); p2-01 pins `fah-http` to `fah-rules` + L1. So define a
  resolver **port** (trait) at L1, implement it in `fah-dns`, and let the
  binary inject it — same pattern as the existing ports. Not a direct call,
  and not loopback UDP either.
- **Egress guard on the resolved address** (not on the Host string — resolve
  first, then judge, or a DNS rebind walks straight past it):
  - deny loopback, link-local (169.254/16, fe80::/10), unique-local (fc00::/7),
    RFC 1918, CGNAT (100.64/10), the container subnet (172.17.0.0/24) and the
    router itself;
  - deny any port other than the intercepted one;
  - deny `Host` values that are IP literals when the LAN policy expects names,
    and reject a missing/multiple `Host` outright;
  - every refusal is counted and logged with the client, at `warn` — this is
    the signal that a LAN device is probing.
  Config knob to allow specific private destinations for the deliberate case
  (an internal HTTP service the user wants filtered), default deny.
- Keep-alive both sides, connection pooling upstream, bounded pools and
  per-connection memory; timeouts from `[http]` config.
- Pass-through fast path: until filtering lands, every request forwards with
  minimal header touch (add `Via`, strip hop-by-hop headers per RFC 9110).
- **The body is never parsed.** Decisions are taken on the head; images, ZIPs,
  PDFs, video, fonts and every other non-HTML body are relayed byte-for-byte
  with no inspection, no buffering and no rewriting. HTML is the sole exception
  and only from Phase 4. Bench must show the body path does no per-byte work
  beyond the copy — this is what makes the throughput row in p2-07 reachable.
- Graceful behavior for non-HTTP bytes on port 80: detect, close, count.
- Tests: local origin server round-trips (small + multi-MB streamed bodies,
  chunked, keep-alive reuse), latency bench (added overhead vs direct)
  in `benches/`.

## Acceptance criteria

- Multi-MB body proxied with bounded memory (RSS flat during transfer, test
  asserts via allocation counters or RSS sampling).
- Added latency for pass-through < 1ms p99 in-process (record numbers).
- Malformed request → clean 400/close, no panic (fuzz corpus).
- **The proxy is not an open relay.** Tests assert refusal for `Host` pointing
  at our own API, the router, RFC 1918, loopback, link-local and the container
  subnet — including the case where a *public* name resolves to a private
  address (rebind), which is why the guard runs post-resolution. Refusals are
  counted.
- `fah-http` compiles with no dependency on `fah-dns` (assert in the crate's
  Cargo.toml review — the resolver arrives as an injected port).
- Gates green.

## Out of scope

Filtering, blocking, HTML rewriting (Phase 4), HTTPS (Phase 3).

## Suggested prompt

> Read ARCHITECTURE.md HTTP pipeline (post p2-01), PERFORMANCE.md golden
> rules, SECURITY.md, and plan/wip/phase2/p2-02-http-proxy-core.md. Implement
> the streaming transparent proxy on hyper with the fast path, bounded
> resources, the post-resolution egress guard, the injected resolver port (no
> fah-dns dependency), and the tests + latency bench.
