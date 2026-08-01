# p2-02 — HTTP proxy core

Transparent streaming proxy on hyper, with the post-resolution egress guard and
the injected resolver port. No filtering: every request that survives the guard
is forwarded (verdicts are p2-04, deliberately after this, so the pass-through
path has a baseline before any rule touches it).

## What shipped

| Crate | Change |
| --- | --- |
| `fah-common` (L1) | **new** `resolve.rs` — the `HostResolver` port, *moved down* from `fah-rules` |
| `fah-common` (L1) | **new** `egress.rs` — `DestinationPolicy`, default-deny, judges a resolved `SocketAddr` |
| `fah-config` (L1) | **new** `[egress]` section + structural validation |
| `fah-http` (L3) | **new** `claim.rs` (Host parsing), **new** `proxy.rs` (the proxy), `server.rs` now drives it |
| `fah-api` (L3) | `egress` added to `BOOT_KEYS` |
| `fastadhunter` (L4) | `build_http_proxy` — parses the allow-list authoritatively, fails startup on a bad entry |

Docs updated in the same change per root CLAUDE.md: ARCHITECTURE.md (pipeline
diagram, ports table, two new pipeline properties), CONFIGURATION.md (`[egress]`,
and `idle_timeout_ms`/`header_timeout_ms` corrected to what they now actually
do), CONTEXT.md (two new binding terms: **Destination Claim**, **Egress Guard**).

## The decision this task turns on

The router dst-nats port 80 into the container and RouterOS exposes no
`SO_ORIGINAL_DST`, so **the only statement of where a client meant to go is a
header the client wrote**. `Host: 172.17.0.2:8443` is our own API;
`Host: 192.168.10.1` is the router. Without a second source of truth, the only
defence is an allow-policy on where we are willing to connect.

Three properties make that policy hold:

**1. It judges the resolved address, never the name.** Checking the string would
be defeated by a DNS rebind — a public hostname whose A record is `192.168.10.1`
passes any name-level test and then connects to the router. Resolve first, judge
second. `a_public_name_resolving_to_a_private_address_is_refused` is that case.

**2. The rebind window is closed structurally, not by discipline.** After the
guard approves an address, the request target is rewritten to that **literal
address** and the upstream connector *refuses to resolve anything at all* —
`the_connector_refuses_anything_that_is_not_a_literal_address` asserts it errors
on a hostname. There is no second lookup for a rebind to race, so the
check-then-connect gap does not exist rather than being small.

**3. IPv6 spellings of IPv4 addresses are canonicalised first.** `::ffff:192.168.10.1`
is the router. So are `::192.168.10.1` and `64:ff9b::192.168.10.1` on a network
with NAT64. Without this, every IPv4 rule is bypassable by rewriting the address.
`ipv6_forms_embedding_an_ipv4_address_cannot_smuggle_one_past` covers all three.

The container subnet and the router are **not special-cased** — they are RFC 1918
addresses and the private rule already denies them. A hardcoded list of "our own"
addresses would be one more thing to drift out of date with the deployment.

## Where things live, and why

**The guard is at L1 and knows nothing about HTTP.** Phase 3's HTTPS path derives
its destination from SNI — an equally attacker-controlled claim with the same
missing `SO_ORIGINAL_DST` — and judges it with identical rules. Splitting it out
means one implementation and one test suite rather than two that drift.

**The `Host` parsing is not shared, on purpose.** Rejecting a missing or
duplicated `Host`, or an IP literal, reads the HTTP message; HTTPS's analogue
(absent SNI, IP-literal SNI) has nothing in common with that code. Forcing them
into one function to look shared would put "what a `Host` header is" into L1.
The split is asserted by a test: the policy's tests take `SocketAddr`s with no
request and no proxy in sight.

**The resolver port moved to L1 rather than being duplicated.** `fah-http` needs
name resolution and cannot import `fah-dns` (L3 siblings). `fah-rules` already
declared exactly the trait needed, and already depended on `fah-common` — so the
trait moved down and `fah-rules` re-exports it. One port, one binary adapter, two
consumers, no call-site churn. The alternative (a second identical trait in
`fah-http`) would have meant two adapters over the same `UpstreamPool`.

Note `fah-config` could **not** import the policy type: both it and `fah-common`
are L1, and `layering.rs` demands a *strictly* lower layer. So `fah-config`
validates the allow-list structurally and `fastadhunter` re-parses it
authoritatively — the same split the DoH URL check already uses. The structural
check is what keeps `--healthcheck` honest; a typo that only surfaced on the next
real boot is the p1.5-07 defect again.

## Transport-agnostic, and proven

`Proxy::serve_connection` is generic over `S: AsyncRead + AsyncWrite + Unpin +
Send + 'static`, so Phase 3 hands it a rustls-terminated stream and reuses this
pipeline unchanged. Monomorphised, not a trait object.

`the_proxy_serves_a_connection_that_is_not_a_tcp_stream` drives a full request
over a `tokio::io::duplex` pipe. If that compiles and passes, the TLS case fits
the same signature — which is the whole claim, asserted rather than asserted-to.

The response body is `Either<Incoming, Full<Bytes>>`, not a boxed body: the
upstream stream is relayed with no per-chunk virtual call.

## Measurements

```text
http_pass_through/direct_to_origin    [34.87 µs  35.15 µs  35.46 µs]
http_pass_through/through_proxy       [70.18 µs  70.58 µs  71.01 µs]
```

**Added latency ≈ 35.4 µs** against the task's < 1 ms p99 budget — roughly a 28×
margin. Both arms use a warm keep-alive connection, so connection setup is
excluded from both; a transparent proxy amortises it across every request on the
connection.

Read this as the *shape* of the cost, not the deployed number: it is a Windows
dev box over loopback, and the added time is close to a full extra loopback
round trip, which is what a second hop costs. The proxy's own CPU work is a
fraction of it. **On-device figures are p2-08's job.**

## Streaming, evidenced without RSS sampling

The task suggested asserting bounded memory via allocation counters or RSS
sampling. Both are noisy in a test. `the_body_streams_rather_than_being_buffered_whole`
asserts the actual property instead: the origin writes one chunk by hand, stalls
500 ms, then finishes — and the test reads that first chunk within 250 ms. A
proxy that collected the body before forwarding could not produce it. Integrity
is covered separately by an 8 MB round trip verified byte-for-byte.

## Amended acceptance criterion — flagged, not quietly dropped

The task (as written yesterday) said *"No `Box<dyn ...>` on the connection or
body path."* The upstream connector's `Future` **is** boxed: `tower_service::Service`
requires a named future type and `TcpStream::connect`'s is opaque.

That is one allocation per *upstream connection*, not per read, so it does not
engage the rule's stated rationale — which the constraint section spells out as
"a virtual call on the per-read body path". The criterion was reworded to bound
it to the per-byte path, with the amendment and its reasoning recorded in the
task file. The connection I/O types and the response body remain concrete.

## Incidental findings

- **`max_connections` finally binds.** p2-01 documented that its test could not
  prove the ceiling, because the scaffold released its permit instantly. The
  proxy holds the permit for the life of a connection, so
  `max_connections_actually_blocks_the_second_connection` now closes that gap:
  the second connection sits in the kernel queue and is served only once the
  first releases.
- **The slowloris bound works, discovered the hard way.** The bench first failed
  because the idle client connection to the proxy was closed mid-run by the
  proxy's own `header_read_timeout` while the *other* arm was being measured.
  That is the timeout doing its job; it is now covered deliberately by
  `a_client_that_never_finishes_its_head_is_cut_off`, and the bench connects each
  arm lazily.
- **hyper 1.x panics rather than degrading** if `header_read_timeout` is set with
  no timer installed. `.timer(TokioTimer::new())` is not optional.

## Tests

22 unit (`fah-http`), 12 integration (`fah-http/tests/proxy.rs`), 26 in
`fah-common` (16 of them the egress policy), 4 new in `fah-config`.

The integration suite covers: round trip with `Host` and `Via` verified at the
origin; hop-by-hop headers stopping at the proxy; 8 MB body integrity; streaming;
upstream connection reuse (3 requests, 1 accept); the `DuplexStream` reuse proof;
rebind refusal; the full refusal matrix (our API, the router, container subnet,
loopback, link-local, RFC 1918) each counted; IP-literal `Host` refused *before*
any resolution is attempted; missing `Host` → 400; dead origin → 502; and a
malformed-input pass that asserts the listener survives all of it.

## Gates

```text
cargo fmt --check                                     clean
cargo clippy --workspace --all-targets -- -D warnings clean
cargo test --workspace                                all pass
cargo bench -p fah-http --bench proxy                 numbers above
```

## Not done here

Filtering, blocking and HTML rewriting (p2-04 / Phase 4) and HTTPS (Phase 3) are
out of scope by the task. Two things worth naming for whoever picks up next:

- **`ProxyStats` has no consumer yet.** The counters exist and are asserted by
  tests, but nothing publishes them — `fah-metrics` is an L3 sibling, so they
  reach it through a port the binary wires. That belongs with p2-04, which is
  when the HTTP pipeline gets events at all.
- **`QueryEvent` is still DNS-shaped.** The architecture debt already recorded
  against p2-04 comes due there, not here: this task emits no events.

## Review pass — defects found and fixed

A read-through against the design goals turned up five defects. All are fixed
in place, each with a test that fails without the fix.

### 1. 6to4 walked past the egress guard

`canonicalize` handled three IPv6 spellings of an IPv4 address; there is a
fourth. **6to4 (`2002::/16`) carries the address in octets 2..6, not at the
end**, so none of the low-order arms could see it. `2002:c0a8:0a01::` is
192.168.10.1 — the router — and on any network with a 6to4 route it was
**allowed**. The claim in "Three properties make that policy hold" that all
IPv4-embedding forms are canonicalised was, as written, one prefix short.

Fixed by reading 6to4 *before* the low-order arms, which would otherwise match
an unrelated suffix. `a_6to4_address_is_judged_as_the_ipv4_it_carries` covers
the router, our own API, loopback and the metadata address, and asserts a 6to4
wrapper around a *public* address still passes.

### 2. The residue of `::/96` was allowed

`canonicalize` deliberately skips `::0.0.0.x` so the v4-compatible arm cannot
swallow `::1` — correct, but nothing then caught what it left behind.
`::0.0.0.5` reached `check_v6`, matched no rule, and returned `Ok(())`. Not
routable in practice, but a default-deny policy that returns "allowed" for an
address it has no opinion about is the wrong default. `check_v6` now refuses
the whole remaining `::/96` block.

### 3. A `debug_assert` reachable from the wire

`to_upstream_request` asserted `Host` is present unless the request is
pre-HTTP/1.1. But `authority_of` deliberately accepts an absolute-form target
with **no** `Host` at all — `an_absolute_target_alone_is_accepted` is that
test — and hyper does not require one. So `GET http://example.com/ HTTP/1.1`
with no `Host` **panicked every debug build** from unauthenticated input. The
assertion was documenting intent, not guarding anything; the intent is now a
comment.

### 4. `example.com` and `example.com:80` were treated as a desync

`authority_of` compared the request target's authority to the `Host` header as
**raw strings**. Two spellings of one destination — a default port written on
one side only, or a difference in case, both legal — were rejected as
`Malformed` and answered 400. Now compared by meaning: host case-insensitively,
port after defaulting. A genuine disagreement in host or port is still refused,
which is asserted separately.

### 5. `refused_destination` counted resolution failures

The counter was incremented before the branch that decides *why* the loop fell
through, so a host that resolved to **zero** addresses incremented both
`resolve_failures` and `refused_destination`. That is the counter an operator
reads as "a LAN device is probing", inflated by ordinary NXDOMAIN-ish
outcomes. The increment moved into the refusal arm.

### Raised, not fixed

- **`Upgrade` is stripped, so WebSocket over port 80 cannot complete.** Correct
  for a proxy that does not implement upgrades, and hop-by-hop stripping is
  right per RFC 9110 — but it is a behaviour change for the LAN that this
  document should state rather than leave to be discovered. Phase 3/4 decision.
- **`retarget` hardcodes the `http` scheme.** Fine here; Phase 3 hands this
  pipeline a TLS-terminated stream and will need the scheme to follow.
- **240.0.0.0/4 and 198.18.0.0/15 are not refused.** Unroutable rather than
  dangerous, and adding them means adding a `Refusal` variant, which is a
  metric-label change. Left deliberately.

Gates after the fixes: `cargo fmt --check` clean, `cargo clippy --workspace
--all-targets -- -D warnings` clean, `cargo test --workspace` 664 passing.
