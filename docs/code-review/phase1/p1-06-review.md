# Code Review — p1-06 Upstream Resolvers

**Scope:** `crates/fah-dns/src/upstream/` (new: `mod.rs`, `plain.rs`,
`encrypted.rs`), `crates/fah-dns/src/forwarder.rs` (deleted — temporary
p1-04 forwarder removed), `crates/fah-dns/src/{lib,pipeline,server,tcp,udp}.rs`
(import rewiring), `crates/fah-config/src/lib.rs` (DoH validation fix),
`CONFIGURATION.md` (DoH hostname note),
`crates/fah-dns/tests/server_integration.rs` (full-pipeline test), workspace
`Cargo.toml`/`Cargo.lock` (hickory-net, rand, rustls, url; dev: rcgen,
tokio-rustls) ·
**Reviewer:** chief architect pass · **Date:** 2026-07-18 ·
**Status:** findings 1–3 fixed same day (see "Fixes applied"); 4–8 are notes
or recorded obligations. Gates green, network smoke green.

## What was delivered (implementation report)

`UpstreamPool` replaces the temporary `UdpForwarder` behind the same
`Forwarder` trait (pipeline, tests and the cache bench needed no changes) and
completes ARCHITECTURE.md's pipeline: Rule Engine → cache → upstream → cache
store.

- **Fallback strategy** (`mod.rs`): the configured `[[dns.upstreams.servers]]`
  are tried in order; a transport error or `timeout_ms` expiry advances to the
  next server, so a down primary costs at most one extra timeout window
  (asserted by a wall-clock test). DNS-level rcodes are answers, not failures —
  `SERVFAIL` fallback/serve-stale policy lives in the pipeline (p1-05 review
  finding 2), not here.
- **Plain UDP** (`plain.rs`): fresh socket per query, upstream message ID
  randomized (closes p1-05 review finding 8 — with the ephemeral port that is
  the full 32 bits an off-path spoofer must guess now that a cache exists),
  reply accepted only if well-formed + ID match + `Response` type; truncated
  replies retried over one-shot RFC 1035 §4.2.2 TCP framing to the same
  server.
- **DoT/DoH** (`encrypted.rs`): one lazily-connected, persistent, multiplexed
  hickory `DnsExchange` per upstream (hickory-net + rustls, aws-lc-rs provider
  matching the one already in-tree via reqwest, webpki-roots because the
  distroless image has no system cert store). Reconnect only after a
  transport error — never per query; a timeout is classified "upstream slow"
  (connection kept, pool falls back) by keeping the multiplexer's internal
  timeout above the attempt timeout. The connection slot's mutex is held
  across connects deliberately so concurrent first queries share one
  handshake.
- **DNSSEC pass-through**: the client's message is forwarded as-is (DO bit,
  EDNS intact); RRSIGs come back byte-identical (asserted via the
  no-dnssec-feature `RData::Unknown` raw-rdata path).
- **Failure accounting**: per-upstream `attempts` / `failures` /
  `consecutive_failures` / `tls_handshakes` counters exposed as
  `UpstreamPool::status()` for p1-08 metrics and p1-09 `/health`.
- **Phase0 correctness fix** (user-approved): fah-config demanded `hostname`
  for DoH while CONFIGURATION.md's documented example carries none. DoH now
  derives the certificate name from the URL host (`hostname` is an optional
  override for IP-literal URLs) and validation instead requires an `https://`
  address. CONFIGURATION.md comment updated; two validation tests replaced,
  two added.
- **Tests**: 21 new — fallback-on-timeout within one extra window, TCP retry
  on truncation (UDP+TCP mocks sharing one port), DO-bit + RRSIG byte-identical
  pass-through, ID randomization, mismatched-ID discard, consecutive-failure
  reset, address/URL parsing, a local rcgen-cert DoT server proving reuse
  (1 handshake / 3 queries, asserted from both ends) and transparent reconnect
  after upstream idle-close, fail-closed on untrusted cert, a full-pipeline
  integration test (listener → rules → cache → pool → mock upstream; second
  query is a cache hit, upstream hit exactly once), plus two `#[ignore]`
  network smoke tests against Cloudflare DoT/DoH (run once during the task:
  both pass, DoH connection reuse confirmed).

## Overall assessment

The architecture holds: the `Forwarder` seam absorbed the entire replacement
without touching the pipeline, layering is clean (`fah-dns` →
`fah-config`/hickory; no sibling imports), the hot path is untouched (the
only lock added sits behind DoT/DoH network I/O), memory stays bounded (one
connection per configured upstream, counters are scalars), and every
acceptance criterion has a test that would actually fail if the property
broke — including the reuse counter asserted from both the client and the
mock server side. Timeout-classification (slow ≠ dead) is the standout
design decision: it keeps the one-extra-window promise honest under partial
failures.

A dedicated re-review pass after completion found one real bug and two
avoidable allocations, all in the new code, all fixed same day.

## Findings

### 1. MEDIUM (correctness) — IPv6-literal DoH URLs could never connect

`UpstreamServer::new` took the DoH host via `url.host_str()`, which renders
IPv6 literals **bracketed** (`"[2606:4700:4700::1111]"`). That string fails
`IpAddr` parsing in `resolve()` (so it falls through to `lookup_host`, where
getaddrinfo rejects it) and fails rustls `ServerName` parsing in the h2
connector. Every `https://[…]/dns-query` upstream would error on first
connect, then on every retry — configuration accepted, resolution
permanently broken. **Fix:** match the typed `url.host()` enum and store
IPs bare; regression test covers IPv4-literal, IPv6-literal and
IPv6-literal-with-port URLs and asserts the stored host parses as a bare
`IpAddr`.

### 2. LOW (efficiency) — blanket 64 KiB recv buffer per UDP upstream query

`udp_round_trip` allocated `vec![0u8; 65535]` for every forwarded query,
inherited from the p1-04 stub. The upstream can only legitimately send what
our forwarded EDNS advertises (512 bytes without EDNS; 1232–4096 typical
with), so the buffer is now sized by the existing
`response::max_udp_payload(request)` — ~50× smaller in the common case, one
fewer large allocation per cache miss. A non-compliant upstream's oversized
datagram loses its tail in the kernel, fails to parse, and runs into the
timeout — the same outcome as any garbage reply (documented at the decision
site).

### 3. LOW (efficiency) — unconditional double `Message` clone on the DoT/DoH path

`ExchangeConn::query` built the `DnsRequest` up front **and** cloned it
again in case the dead-connection retry needed it — two deep copies of the
message per query, one wasted on the overwhelmingly common single-send path.
**Fix:** a `as_request()` closure builds the request per attempt: one clone
normally, two only when a reconnect-retry actually happens. Same pass also
replaced `"0.0.0.0:0".parse().unwrap()` per query with const-constructed
`SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))` — no string parsing or
unwrap on the forward path.

### 4. NOTE (semantics, documented) — `tls_handshakes` counts attempts, not completions

The counter increments when a connect starts, so a flapping upstream shows
handshake attempts even when none complete. That is the more useful signal
for the health/metrics consumers it feeds (and the doc comment says
"attempted"), but p1-08 should label the metric accordingly
(`…_handshakes_attempted_total` or similar) rather than implying established
sessions. No code change.

### 5. NOTE (no change) — "ordered parallel fallback" implemented sequentially

ARCHITECTURE.md's phrase says "parallel"; the implementation is strictly
sequential, exactly as the task scope specifies ("primary first; on timeout
or error, next server") and as the acceptance criterion budgets ("one extra
timeout window"). True hedged/racing queries are a different strategy with
different duplicate-traffic tradeoffs — that belongs to the backlogged
load-balancing strategies, not this task. Recorded so the wording mismatch
doesn't read as a gap later.

### 6. NOTE (edge, accepted) — DoH URL query strings are dropped

`url.path()` ignores `?dnssec=1`-style query parameters; a DoH URL carrying
one would silently lose it. RFC 8484 templates in the wild are plain paths
(`/dns-query`), so this is accepted as-is; revisit only if a real resolver
needs it.

### 7. NOTE (obligation on p1-08/p1-09) — `status()` exists but nothing consumes it yet

Failure accounting is delivered as `UpstreamPool::status()` returning
`UpstreamStatus` snapshots. p1-08 must map it to Prometheus series and p1-09
must derive the `/health` degraded state from `consecutive_failures` —
recorded here so the seam doesn't sit unwired.

### 8. NOTE (obligation on p1-09/p1-11) — the binary still doesn't construct the pool

`fastadhunter` doesn't wire `Server`/`Pipeline`/`UpstreamPool` yet; the DNS
engine is only reachable from tests. Expected — API wiring is p1-09 and
deployment is p1-11 — but the pool's constructor (`from_config`) is the
binary's entry point when that lands.

## Verdict

The upstream layer is complete against its task scope: all three protocols,
ordered fallback inside the promised time budget, connection reuse proven
from both ends, DNSSEC pass-through proven byte-identical, the p1-04
temporary forwarder gone, and the cache-store seam exercised end-to-end by
an integration test with no doubles. The re-review caught one genuine bug
(IPv6 DoH — finding 1) before anything could depend on it, and trimmed the
forward path's allocations (findings 2–3). Findings 4, 7 and 8 are
obligations on p1-08/p1-09/p1-11, recorded so they don't evaporate; 5 and 6
are accepted deviations with their reasoning on file.

## Fixes applied (2026-07-18)

Findings 1–3 fixed the same day; 4–8 need no code change here.

1. **IPv6-literal DoH hosts stored bare** (`upstream/mod.rs`) — typed
   `url::Host` match replaces `host_str()`; `Domain` passes through, `Ipv4`/
   `Ipv6` render unbracketed so both `resolve()`'s `IpAddr` fast path and
   rustls `ServerName` accept them. New test
   `doh_ip_literal_urls_are_accepted_including_ipv6` (plus a test-only
   `ExchangeConn::target()` accessor).
2. **EDNS-sized UDP recv buffer** (`upstream/plain.rs`) — buffer sized by
   `response::max_udp_payload(request)` instead of a constant 64 KiB;
   `MAX_UDP_REPLY` removed.
3. **Per-attempt request clone + const bind addresses**
   (`upstream/encrypted.rs`, `upstream/plain.rs`) — `DnsRequest` built per
   attempt via closure; bind addresses const-constructed.

**Verification:** `cargo fmt --check` clean;
`cargo clippy --workspace --all-targets -- -D warnings` clean;
`cargo test --workspace` all green — fah-dns 57 unit + 7 integration
(+2 `#[ignore]` network smoke, run once: both pass, DoT and DoH each complete
with a single TLS handshake across repeated queries).
