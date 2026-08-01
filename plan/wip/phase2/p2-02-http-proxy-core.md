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
FastAdHunter connects. `Host: 172.17.0.2:8443` reaches our own API,
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

## Architecture constraint — Phase 3 reuse

Decided before implementation starts, because both halves are cheap to build in
and expensive to retrofit.

**The proxy core must be transport-agnostic.** Connection handling operates on a
generic type parameter `S: AsyncRead + AsyncWrite + Unpin + Send + 'static`,
never on a concrete `TcpStream`, so Phase 3 can hand it a rustls-terminated
stream and reuse this pipeline unchanged after TLS termination. It must be a
**monomorphized type parameter, not a trait object** — `Box<dyn …>` is also "not
a `TcpStream`" but puts a virtual call on the per-read body path, which
PERFORMANCE.md forbids.

**The destination policy is shared; the claim validation is not.** The egress
guard above mixes two concerns, and only one of them generalises:

1. **Destination policy** — the address/port rules (loopback, link-local, ULA,
   RFC 1918, CGNAT, container subnet, router, non-intercepted port). Pure logic
   over an already-resolved `SocketAddr`, no I/O. HTTP and HTTPS share it
   verbatim: both derive their destination from an attacker-controlled claim
   (`Host`, then SNI) with no `SO_ORIGINAL_DST` to check it against, so this is
   **one implementation with one test suite**. Being pure and L1-shaped, it
   belongs in `fah-common` next to the resolver port — reachable from Phase 3
   whether HTTPS lands in `fah-http` or its own crate, with no layering
   violation. Not `fah-model`: it is business logic, not a data type (hard
   rule 2). Its allow-list config key must therefore **not** be scoped under
   `[http]` alone.
2. **Claim validation** — rejecting a missing or duplicated `Host`, and IP-literal
   hosts where policy expects names. This reads the HTTP message, not an address,
   and stays in `fah-http`. Phase 3 writes the SNI-shaped analogue (absent SNI,
   IP-literal SNI) against the same destination policy. Do **not** force these
   into one function to make them look shared.

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
- **Phase 3 reuse is proven, not asserted.** A test drives the proxy over a
  stream type that is *not* `TcpStream` (a `tokio::io::DuplexStream` is enough)
  — if the connection handler compiles against it, the rustls case will fit too.
  No `Box<dyn ...>` on the **per-byte path**: neither the connection I/O types
  nor the response body may be trait objects, since either would put a virtual
  call on every read. (Amended during implementation: the upstream connector's
  `Future` *is* boxed, because `tower_service::Service` needs a named future
  type and `TcpStream::connect`'s is opaque. That is one allocation per upstream
  connection, not per read, so it does not engage the rule's stated rationale —
  but the original wording forbade it outright and would have been quietly
  violated.)
- **The destination policy is exercised without HTTP.** Its tests call it with
  `SocketAddr`s directly, with no request and no proxy in sight; if that is
  awkward to write, the split in the architecture constraint above did not
  actually happen.
- Gates green.

## Out of scope

Filtering, blocking, HTML rewriting (Phase 4), HTTPS (Phase 3).

## Suggested prompt

> Read ARCHITECTURE.md HTTP pipeline (post p2-01), PERFORMANCE.md golden
> rules, SECURITY.md, and plan/wip/phase2/p2-02-http-proxy-core.md. Implement
> the streaming transparent proxy on hyper with the fast path, bounded
> resources, the post-resolution egress guard, the injected resolver port (no
> fah-dns dependency), and the tests + latency bench.
