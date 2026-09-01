# P3-04 — TLS Interception — Implementation Plan

**Phase:** 3 · **Depends on:** `p3-01` (cert core), `p3-03` (SNI path) ·
**Task:** `p3-04-tls-interception.md`

## TASK START / CONTEXT

Read, in this order, before writing any code:

1. `plan/wip/phase3/p3-04-tls-interception.md` — the task file, completely.
2. **SECURITY.md in full** — §"Later phases" (interception is opt-in, per-client,
   never default; CA key never leaves `/config`; export public-only), §"Data at
   rest" (the redaction/no-Debug-on-key precedent), §"Container hardening"
   (webpki-roots because distroless has no system store). These are **hard law**,
   not guidance.
3. `plan/wip/phase3/p3-01-cert-core-plan.md` — the **seams this task consumes**:
   `Arc<CertStore>` with `prewarm(host)` (blocking, single-flighted, call from
   `spawn_blocking`) and `cached_leaf(host) -> Option<Arc<CertifiedKey>>` (the
   non-blocking read; `LeafCache` itself is private to `fah-certs`),
   and `fah_certs::MintingResolver` (the `ResolvesServerCert`
   implementation ships in `fah-certs` because p3-05's DoT listener needs the
   identical resolver and siblings cannot share code — p3-04 only wires it
   into a `ServerConfig`). When p3-01 lands, read its Implementation Summary
   to confirm the final names.
4. `plan/wip/phase3/p3-03-sni-filtering-plan.md` — the SNI parse + close-or-
   splice handler this task **branches from**: interception is a per-client
   branch taken *instead of* splicing, after the same `scan_client_hello`.
5. `docs/code-review/phase2/p2-04-review.md` §Implementation Summary — the
   verdict/block/event core (`judge` → `block::response` → `emit`,
   `matcher.lookup_http_in`) that is *"reused verbatim"*.
6. `crates/fah-http/src/proxy.rs` — the `handle`/`judge`/`emit` core and the
   stream-generic `serve_connection<S>`; the `LiteralConnector` rebind-hardening.

Do not read p3-02 (API), p3-05/06, or full phase-2 reviews beyond the summaries.
Do not re-litigate the SNI parser (p3-03) or the cert placement (p3-01).

## What this task is, in one sentence

For a client **explicitly listed** for interception and an SNI **not excluded**,
verify the upstream's real certificate first, then terminate TLS with a
p3-01-minted leaf, run the **existing** Phase-2 filtering pipeline over the
decrypted HTTP, and re-encrypt to the verified upstream — every other client and
every excluded domain still splices (p3-03).

## Decisions settled by this plan

### 1. Interception is a branch inside the p3-03 handler, not a new listener

The connection already arrived on the :443 listener and its ClientHello is
already parsed (p3-03). Interception replaces the *splice* leg with a
*terminate* leg. The decision point, in order:

1. `scan_client_hello` → SNI host (p3-03). No SNI → cannot intercept (no name to
   mint for, no name to verify upstream against) → fall through to p3-03's
   no-SNI handling. **Interception requires SNI.**
2. SNI verdict (`lookup_host_in`): `Block` → close (p3-03), never intercept a
   domain we would outright block.
3. `Pass`/`Allow` **and** client ∈ interception list **and** SNI ∉ exclusions →
   **intercept**. Otherwise → **splice** (p3-03).

- **Existing mechanism:** p3-03's `TlsProxy::serve_connection`.
- **Required change:** add the interception branch and the two gates below.
- **Reason:** the task's own framing — *"a per-client branch taken instead of
  splicing"*, *"fah-http does not grow a second pipeline"*.
- **Verification:** a non-listed client and an excluded domain both take the
  splice leg (observed as passthrough — the client validates the **origin's**
  cert, not ours).

### 2. Client gate — explicit list, default empty, reuse `AllowedNet`

- **Existing mechanism:** `fah_common::egress::AllowedNet` already parses
  IP/CIDR and matches an address; `peer.ip().to_canonical()` already canonical.
- **Required change:** `[https.interception] clients: Vec<String>` → parsed to
  `Vec<AllowedNet>` at proxy build (fail startup on a bad entry, like
  `build_http_proxy` does for egress). A helper `intercepts(peer_ip) -> bool`
  is an any-match over the list. **Default empty ⇒ nobody is intercepted.**
- **Reason:** SECURITY.md hard law (opt-in, per-client, never default). Reusing
  `AllowedNet` avoids a second CIDR parser (principle 4) and gives CIDR-scoped
  households (`192.168.10.0/24`) for free.
- **Verification:** config-default test asserts the list is empty; a fuzz over
  arbitrary configs keeps `intercepts` false whenever `clients` is empty; a
  branch test proves a non-listed IP is never terminated.

Per-policy interception flag (task's "or per-policy flag"): **deferred.** It
couples interception to the `PolicyState` schema; the `clients` list satisfies
the acceptance criteria with less surface. Flagged as optional follow-up.

**GAR §5.14 ("opt-in bound to stable identity, not bare IPs") — owner decision
required at task start.** The container's only per-connection identity is the
source IP: the client MAC is not readable from an unprivileged TCP accept, and
no other stable identifier crosses the socket. An IP list therefore satisfies
§5.14 only under an operational precondition: **every listed client holds a
static DHCP lease (or static address) on the router.** Present that to the
owner as the decision — (a) accept IP/CIDR with the static-lease precondition,
documented in CONFIGURATION.md's `[https.interception]` text and verified per
device in the p3-06 walkthrough, or (b) defer interception until a stronger
binding exists. A DHCP reassignment otherwise silently moves interception to
whichever device inherits the address — the exact failure §5.14 names. Record
the decision in the review file; p3-06 confirms §5.14 closed or carries it as
a finding.

### 3. Exclusions — shipped baseline + user list, matched on SNI before terminate

- **Existing mechanism:** none reusable directly; the domain matcher is
  overkill and list-lifecycle-coupled for a tiny static set.
- **Required change:** a small `ExclusionSet` in `fah-http` — a set matched by
  exact host **and** parent-suffix (`api.bank.com` excluded by `bank.com`).
  Built from a **compiled-in baseline** const (`BASELINE_EXCLUSIONS`: known
  cert-pinned families — banking, OS/app update hosts) **merged** with
  `[https.interception] exclude_domains` (user-extendable). The SNI is checked
  against it **before** any TLS termination; a hit takes the splice leg.
- **Reason:** cert-pinned apps break under interception (the phase's headline
  risk); shipping a baseline means the failure mode is off by default without
  the user curating a list. "Every config key ships with a production-ready
  compiled-in default" (working agreement) — here the *value itself* is
  compiled in.
- **Verification:** an excluded SNI on a listed client splices (client sees the
  origin cert); baseline entries are present without any user config.

### 4. Upstream verification happens **before** we present our cert (the core invariant)

The task forbids ever presenting a locally-signed success for an upstream we
could not verify. A lazy pooled upstream connect (the plaintext proxy's model)
would complete our downstream handshake first and only discover a bad upstream
cert on the first request — after the client already trusted our leaf. That is
the exact thing forbidden.

**Ordering (per intercepted connection):**

1. Resolve SNI host, `DestinationPolicy::check((ip, 443))` (reuse the egress
   guard — same open-relay protection as splice).
2. **Connect + TLS-handshake to the upstream first**, with `tokio-rustls`
   `connect(ServerName = sni_host, tcp_to_approved_ip)`. Critically, the
   **ServerName is the hostname, the socket is the pre-approved IP** — this
   keeps p2-02's rebind-hardening (we connect to the IP the policy approved)
   *and* verifies the certificate against the **name** (webpki-roots).
3. Upstream verification fails ⇒ **close the client TCP without completing our
   TLS handshake.** The client sees a TLS/connection failure (as if the site
   were unreachable), **never our cert.** Emit a `https`/`upstream-cert-failure`
   event.
4. Upstream verified ⇒ pre-warm the leaf (`spawn_blocking(store.prewarm(sni))`;
   the resolver then serves it from `cached_leaf`), accept the downstream TLS,
   and bridge (step below).

- **Reason:** this is the single most important security property of the task.
  Verifying first and closing on failure means "we never present a valid cert
  for an upstream we couldn't verify" is *structurally* true, not merely tested.
- **This step discharges GAR §5.8** (retarget/connector redesign for upstream
  TLS hostname verification): the plaintext `LiteralConnector` only ever sees
  an IP literal and can never hostname-verify — the confirmed design conflict.
  `connect_verified_upstream` is the redesign: socket to the policy-approved
  IP, `ServerName` = the SNI hostname, webpki verification against the name.
  `LiteralConnector` remains plaintext-only and is not touched. State this in
  the review file so p3-06's gate map can point at it.
- **Verification:** a self-signed upstream + listed client ⇒ the client's TLS
  handshake to us errors (assert the `tokio-rustls` client error); no minted
  leaf is served for that session.

Note the trade: interception pays an upstream handshake before the downstream
one completes (added connect latency). Acceptable — interception is opt-in and
low-volume. Stated in Performance.

### 5. Bridge — reuse the verdict pipeline verbatim, dedicated verified upstream

After the downstream TLS accept we hold a TLS-terminated stream `S:
AsyncRead+AsyncWrite`, and (from step 4) an **already-verified upstream TLS
connection**. Serve the downstream with hyper (auto h1/h2) whose service runs the
**same** verdict/block/emit core as the plaintext proxy, forwarding each allowed
request over the established upstream connection.

- **Existing mechanism:** `serve_connection<S>` is already stream-generic
  (`proxy.rs:264-272`); `judge` → `block::response` → `emit` +
  `matcher.lookup_http_in` is the p2-04 pipeline.
- **Required change:**
  - Extract the reusable core (`judge`, `emit`, the block short-circuit) so both
    the plaintext `handle` and the intercepted service call it — targeted, not a
    rewrite. Concretely, `judge`/`emit` become reachable by the intercepted
    handler (same-crate; either `pub(crate)` or a shared `fn filter(...) ->
    Judged` used by both).
  - The **forwarder differs**: instead of `self.client` (plaintext pooled
    `LiteralConnector`), the intercepted connection forwards over the
    per-connection **verified upstream TLS** connection (scheme `https`,
    ServerName already validated). Use `hyper::client::conn::{http1,http2}` (or
    `auto`) handshaked over the upstream TLS stream once, reused for every
    request on this client connection.
  - ALPN: offer `[h2, http/1.1]` downstream (leaf resolver's `ServerConfig`);
    negotiate `[h2, http/1.1]` upstream independently. Because we bridge at the
    HTTP layer (reconstruct requests), downstream and upstream ALPN need **not**
    match — hyper reframes h2⇄h1. Pick the upstream client protocol from the
    upstream's negotiated ALPN.
- **Reason:** the task's "reuses the Phase 2 filtering pipeline verbatim" ==
  reuse the *filtering/verdict/block/event* logic; the TLS transport is
  necessarily interception-specific. A dedicated 1:1 verified upstream (no shared
  pool) is what makes decision 4's verify-before-present hold without a wasted
  second handshake, and keeps ALPN/session coherent per client.
- **Verification:** an ad URL **inside** HTTPS on a listed+trusting client is
  blocked (block response over TLS) and a normal page loads; both h1 and h2.

Lower-reuse alternative (documented, rejected): give `Proxy` a TLS-capable
pooled `client` and set scheme by a field, reusing `handle` unchanged. Rejected
because the pooled/lazy upstream connect defers verification past our handshake,
breaking decision 4. Reuse of the *verdict* core is preserved either way; the
transport is where they must diverge.

### 6. Certificate/TLS specifics (consuming p3-01)

- **`fah_certs::MintingResolver`** (p3-01's `ResolvesServerCert` over the
  `CertStore` — shipped at L2 because p3-05's DoT listener needs the identical
  resolver and siblings cannot import each other): p3-04 **consumes** it with
  `fallback: None`, no local implementation — an unwarmed host aborts the
  handshake, fail-closed (the fallback slot exists for p3-05's DoT). rustls
  sees the replayed ClientHello (see RewindStream below), so its SNI == ours.
- **`resolve()` never mints — pre-warm is mandatory** (p3-01 M5 resolution).
  rustls' `ResolvesServerCert::resolve` is synchronous, so minting inside it
  would block a tokio worker for ~0.5 ms per first-sight host on the RB5009.
  `MintingResolver::resolve` is a pure cache read; the mint happens **before**
  the stream reaches the acceptor:

  ```text
  accept → peek ClientHello (already done for the SNI verdict)
         → spawn_blocking(move || store.prewarm(&host)).await
         → TlsAcceptor::accept(rewound stream)   // guaranteed cache hit
  ```

  `CertStore::prewarm` is **blocking** (P-256 keygen + signature) and must be
  called from `spawn_blocking`, never directly from an async task.
  `CertStore::cached_leaf` is the non-blocking read the resolver uses. p3-01
  owns the single-flight: concurrent `prewarm` calls for the same host collapse
  to one mint (`std::sync::Condvar`, no tokio in `fah-certs`), so a burst of
  connections to one new host costs one keygen, not N. p3-04 adds **no**
  minting, caching or coalescing logic of its own.
- **Skipping the pre-warm is observable, not silent:** a resolve that misses the
  cache increments `LeafCacheStats::unwarmed_misses`, which p3-02 serializes and
  p3-06 asserts is zero. `inflight` is exposed as a gauge — it is bounded by
  this task's concurrent-connection cap, which is why `fah-certs` carries no
  separate in-flight limit.
- **Downstream `ServerConfig`** built **once** per `TlsProxy`
  (`Arc<ServerConfig>`): the `MintingResolver`, ALPN `[h2, http/1.1]`, no client
  auth, aws-lc-rs provider (the one workspace backend, SECURITY.md). Cheap to
  share; the per-host cost is the leaf mint, cached.
- **Upstream `ClientConfig`** built once (`Arc<ClientConfig>`): roots =
  `webpki-roots` (distroless has no system store — same reason `hickory-net`
  uses it, root `Cargo.toml:44`), ALPN `[h2, http/1.1]`. Per connect,
  `ServerName::try_from(sni_host)`.
- **RewindStream** (`fah-http`, p3-04): p3-03 consumed the ClientHello bytes off
  the client socket to parse SNI. rustls' acceptor must read them too. Wrap the
  client socket in a small `AsyncRead`/`AsyncWrite` adapter that yields the
  buffered hello bytes first, then the live socket. (Splice does not need this —
  it writes the buffer to upstream; only the terminate leg re-presents it to
  rustls.)
- **Key material lifetime:** leaves live only in the p3-01 LRU (`Arc`-shared,
  bounded 512, 7-day validity), in memory, never persisted by p3-04. The CA key
  stays in `CertStore`/`/config` (p3-01) — p3-04 never touches it. No new
  Debug/Display over any key type (SECURITY.md redaction precedent); the
  `CertifiedKey` is opaque and never logged. No zeroization beyond p3-01's
  posture — leaves are per-host, short-lived, and not secrets in the CA sense.
- **HSTS transparent:** our leaf chains to the client-trusted CA, so HSTS
  behaves normally; documented in SECURITY.md §interception (task requirement).

### 7. Events — add `EventKind::Https` (wire `https`), reuse `RequestEvent`

Mirrors p3-03's `HttpsSni` addition. Intercepted requests are **full** HTTP
requests, so they carry a complete `RequestEvent` (host, path, method,
resource_type, status, bytes) exactly like `kind: http` — only the tag differs.

- `Event::Https(Box<RequestEvent>)` + `EventKind::Https` (`"https"`), constructor
  `Event::https(...)`, match arms, `as_str`/`FromStr`/`Display` — additive, same
  shape as p3-03. If p3-03 already landed the enum-widening machinery, this is
  one more arm.
- Fan-out routes `Event::Https` into `metrics.record_http` / `stats.record_http`
  (reuse — a `RequestEvent`). Events socket serializes `kind: https`
  automatically; `?kind=https` filter works once the enum arm exists.
- An `upstream-cert-failure` outcome (decision 4) is surfaced as **both** a
  counter and an event, encoding settled now: `upstream_cert_failures` counter
  on the proxy's counter set, plus a `https` event with verdict `Pass`,
  synthetic status **526** (the invalid-upstream-certificate convention),
  `bytes = 0`. Not `Block` — `Verdict::Block(DecisiveRule)` carries a deciding
  rule (`fah-model/src/verdict.rs:30-34`) and no rule fired here; fabricating
  one would corrupt rule attribution in stats. API.md's event note names 526.

## Detailed implementation plan

### Step 1 — config (`fah-config`)

Extend `HttpsConfig` (from p3-03) or add `crates/fah-config/src/schema/
interception.rs`:

```rust
#[serde(deny_unknown_fields, default)]
pub struct InterceptionConfig {
    pub clients: Vec<String>,          // IP/CIDR; default empty ⇒ off
    pub exclude_domains: Vec<String>,  // merged with the compiled-in baseline
}
```

- Nested as `[https.interception]`. `#[serde(default)]`; default all-empty.
- A default-off test (`the_interception_client_list_is_empty`) mirrors
  `the_default_allow_list_is_empty` — a security property guarded by a failing
  test, not a convention.

### Step 2 — dependencies (`fah-http/Cargo.toml`)

Add runtime deps (workspace-pinned): `tokio-rustls` (async accept/connect over
rustls 0.23), `rustls` (already workspace), `webpki-roots` (upstream roots).
`hyper` gains `http2`; `hyper-util` gains `server`, `server-auto` and `http2`
(it carries only `client`, `client-legacy`, `http1`, `tokio` today — the
downstream auto h1/h2 builder needs the server half; mirror `fah-api`'s
feature set). `fah-certs` (p3-01, L2) added as a dependency (L3→L2,
legal). aws-lc-rs is the provider via rustls default features already in-tree.

### Step 3 — cert/TLS glue (`fah-http/src/tls.rs`, new)

- `Arc<ServerConfig>` builder (over `fah_certs::MintingResolver`),
  `Arc<ClientConfig>` builder (webpki-roots), `RewindStream<S>`, and
  `connect_verified_upstream(client_config, sni, approved_ip) -> Result<TlsStream>`.
- All crypto via rustls/tokio-rustls/webpki-roots — no hand-rolled anything
  (SECURITY.md). The leaf comes from `fah_certs::CertStore` (p3-01).

### Step 4 — the interception branch (`fah-http/src/https.rs`)

Extend `TlsProxy` with `Arc<ServerConfig>`, `Arc<ClientConfig>`,
`Vec<AllowedNet>` (clients), `ExclusionSet`, `Arc<CertStore>`. In
`serve_connection`, after the SNI verdict `Pass`/`Allow`:

```text
if self.intercepts(peer.ip()) && !self.exclusions.contains(sni) {
    // decision 4: verify upstream FIRST
    let upstream = match connect_verified_upstream(&self.client_config, sni, approved_ip).await {
        Ok(u) => u,
        Err(_) => { emit https/upstream-cert-failure; close; return; }
    };
    // decision 5: terminate downstream, bridge over `upstream`
    let tls = acceptor(self.server_config).accept(RewindStream::new(hello_buf, client)).await?;
    serve_intercepted(tls, upstream, peer).await;   // reuses judge/block/emit
} else {
    splice(...);   // p3-03
}
```

`serve_intercepted` runs `hyper` auto server over `tls`, its service calling the
shared verdict core and forwarding over `upstream`.

**p3-03 carry-over (review finding m8):** while this step reworks
`serve_connection`, split the `non_tls` counter. Today it counts three things
— non-TLS bytes, client EOF before a hello, and the `hello_timeout` deadline —
so browser preconnects that close unused dominate it, and the "garbage on
:443" reading API.md §telemetry gives it is diluted; the HTTP twin `non_http`
counts parse errors only. Keep `non_tls` for `NotTls`, add `hello_timeouts`
(EOF or deadline before a complete hello), note both in API.md §telemetry.
p3-06 reads the two during the soak.

### Step 5 — reuse the verdict core (`fah-http/src/proxy.rs`)

Make `judge`/`emit`/the block short-circuit reachable by the intercepted handler
(same crate). Minimal extraction: a `pub(crate) fn filter(&self, req, peer) ->
Judged` and reuse of `block::response` + `emit`, called by both `handle` and
`serve_intercepted`. `HttpRequest` is built from the decrypted request with
scheme `https` in the reconstructed URL (`absolute_url` gains an `https` variant
or a scheme param — targeted change; URL patterns match `https://host/...`).

### Step 6 — binary wiring (`fastadhunter/src/main.rs`)

- Build `Arc<CertStore>` at startup (p3-01 delivers this seam; `LeafCache` is
  private to `fah-certs` — the store *is* the handle. p3-04 is its first leaf
  consumer; the cache is empty until now, per p3-01's RSS note).
  **The store already exists as `Option<Arc<CertStore>>`** — p3-02 wired it
  and `main.rs` turns a failed `CertStore::open` (corrupt, non-CA or
  key-mismatched `ca-cert.pem`) into a logged `None`, not a boot error
  (p3-02 final review, §Deferred items). Interception **must** treat `None`
  exactly like "no CA": listed clients are spliced, never MITM'd, one `warn!`
  at startup naming the cause, and `GET /api/v1/certificates` remains the
  operator's signal. Do not add a second `open`, and do not make a missing
  store fatal — DNS must keep resolving.
- Parse `[https.interception].clients` → `Vec<AllowedNet>` (fail startup on bad
  entry); build the `ExclusionSet` (baseline ∪ user).
- Build the `ServerConfig`/`ClientConfig` once; hand them + the `Arc<CertStore>`
  to `TlsProxy`.
- Log at startup when the interception list is non-empty (a deliberate, auditable
  posture change — mirror the egress allow-list log at `main.rs:630-637`).

## Public API / compatibility

- `fah_model::Event`/`EventKind` gain `Https` — additive (as p3-03).
- `fah-http` public surface gains the TLS types; `Proxy`'s verdict core becomes
  `pub(crate)`-reachable — no external break.
- `[https.interception]` config is new, default-off — old configs parse and are
  **not** intercepting anyone.
- `absolute_url` gains a scheme parameter/variant — internal to `fah-http`.

## Security model / trust boundaries

- **Opt-in, per-client, never default** — enforced by decision 2's empty
  default + the default-off test + the config-fuzz invariant.
- **Never present a cert for an unverifiable upstream** — enforced structurally
  by decision 4's verify-before-present ordering, not just by a test.
- **CA key never leaves `/config`; export public-only** — untouched here (p3-01
  owns the CA; p3-04 only reads minted leaves through the LRU seam).
- **Exclusions ship a baseline** so pinned apps (banking, OS update) keep working
  without user curation; matched on SNI before any decryption.
- **Redaction** — no Debug/Display on key-bearing types; `CertifiedKey` opaque;
  passphrases/keys never logged (SECURITY.md §Data at rest precedent).
- **Egress guard** still bounds the upstream target (open-relay protection).

## Performance / memory

| Metric | Class | Value |
| ------ | ----- | ----- |
| Per intercepted connection | — | 1 downstream + 1 upstream TLS session + hyper h1/h2 buffers; **heavier than splice**, only for listed clients |
| Leaf mint (cache miss) | diagnostic | p3-01's ECDSA-P256 mint cost; measured in p3-01, budget row in p3-06 |
| Downstream handshake | diagnostic | aws-lc-rs P-256 server handshake; RB5009 cost measured in p3-06 |
| Added connect latency | inherent | upstream verify precedes downstream accept (decision 4) — opt-in, accepted |
| Streaming | hard gate | bodies relayed, never buffered (reuse p2-02's streaming discipline) |
| DNS hot path | hard gate | untouched |
| Steady RSS | hard gate | bounded by concurrent intercepted connections (`https.max_connections`) + leaf LRU (≤ ~1.5 MB, p3-01) |

Interception is CPU-heavier than every other path (two TLS sessions + framing);
that cost is bounded to opt-in clients and measured on-device in p3-06 with the
~9× factor (`docs/measurement-traps.md`; HTTP work does **not** convert at 9× —
p2-08 found 4.5–10× — so p3-06 measures directly).

## Tests

### Unit

- `ExclusionSet`: exact + suffix match; baseline present with no user config.
- `intercepts`: empty list ⇒ false for every IP; CIDR membership.
- `RewindStream`: reads yield buffered bytes then socket bytes, in order.
  (`MintingResolver` unit coverage lives in p3-01 with the type.)

### Integration (`fah-http/tests/interception.rs`)

- **Intercepted + CA trusted:** `tokio-rustls` client trusting a test CA →
  through `TlsProxy` → real TLS origin. An ad URL inside HTTPS is **blocked**
  (block response over TLS); a normal URL loads (origin payload byte-identical).
  Both **HTTP/1.1 and HTTP/2** (ALPN both).
- **Non-listed client always splices:** the branch is proven — a non-listed IP
  gets the origin's real cert (validate against the origin's CA, not ours).
- **Excluded domain splices** even for a listed client (origin cert observed).
- **Bad upstream cert never yields a locally-signed success:** self-signed
  upstream + listed client ⇒ the client's TLS handshake to us **errors**; assert
  no minted leaf served and an `upstream-cert-failure` event/counter.
- **Config fuzz:** arbitrary configs with empty `clients` ⇒ `intercepts` false.

### Regression

- p3-03 splice tests unchanged (splice remains the default leg).
- `fah-model` serde round-trip extended to `Event::Https`.
- `layering.rs` green with `fah-http → fah-certs` (L3→L2).

### Bench / on-device

- Handshake + first-byte latency for intercepted vs spliced; leaf-mint hit/miss.
  Diagnostic here; budgets in p3-06 on the RB5009.

## Verification / Gates

1. **Compile/check:** fmt, clippy `-D warnings`.
2. **Tests:** workspace green incl. all four security-property tests.
3. **Security invariants:** non-listed never intercepted (branch + fuzz);
   upstream-verify-before-present (self-signed upstream ⇒ client TLS error, no
   leaf served); exclusions splice; CA key untouched.
4. **Perf/memory:** intercepted-path cost recorded; leaf LRU bound shown; DNS
   hot path untouched.
5. **API/docs:** `EventKind` spelling covers `https`; proposed SECURITY.md +
   CONFIGURATION.md edits listed.
6. **RB5009/e2e:** deferred to p3-06 (real phone/browser with CA installed, ad
   blocked inside HTTPS, banking/pinned app still works via exclusions,
   handshake budgets). Likely `AWAITING SOAK` until on-device evidence exists.

## Doc changes (proposed — owner approval required before editing any `.md`)

- SECURITY.md §interception: opt-in/per-client/never-default reaffirmed; the
  verify-before-present invariant; HSTS transparency; the shipped exclusion
  baseline and why; CA-key-stays-in-`/config` restated for the interception path.
- CONFIGURATION.md: `[https.interception]` (`clients`, `exclude_domains`), the
  compiled-in baseline exclusions listed, default-off stated.
- API.md §events: `kind: https`; the `upstream-cert-failure` encoding.
- ROADMAP.md: interception delivered when the phase closes.
- Task review file `docs/code-review/phase3/p3-04-tls-interception-review.md`
  needs no approval.

## Non-goals / deferred

- **HTML rewriting** — Phase 4; flows through this pipe when it lands.
- **QUIC / HTTP-3** — UDP :443, not touched.
- **Per-policy interception flag** — deferred; the `clients` list suffices.
- **No-SNI interception** — impossible (no name to mint/verify); falls to p3-03's
  no-SNI handling.
- **Upstream client-cert / mTLS origins** — out of scope.

## File-by-file implementation checklist (dependency order)

*(assumes p3-01 and p3-03 landed; if p3-03's `Event`-widening is not yet in,
its steps precede these.)*

1. `crates/fah-config/src/schema/{https.rs|interception.rs}` — `[https.
   interception]` + default-off test.
2. `crates/fah-model/src/request_event.rs` — `Event::Https`, `EventKind::Https`
   (additive; skip if p3-03 generalized the machinery — still add the arm).
3. `crates/fah-http/Cargo.toml` — `tokio-rustls`, `webpki-roots`, `fah-certs`,
   hyper/hyper-util `http2` + `server-auto`.
4. `crates/fah-http/src/tls.rs` — server/client config builders (resolver
   imported from `fah_certs`), `RewindStream`, `connect_verified_upstream`.
5. `crates/fah-http/src/exclusions.rs` — `ExclusionSet` + `BASELINE_EXCLUSIONS`.
6. `crates/fah-http/src/proxy.rs` — extract the reusable verdict core
   (`filter`/`emit` reachable); `absolute_url` scheme param.
7. `crates/fah-http/src/https.rs` — the interception branch + `serve_intercepted`;
   the `non_tls` / `hello_timeouts` split (p3-03 m8).
8. `crates/fah-http/src/lib.rs` — module decls + `pub use`.
9. `crates/fastadhunter/src/main.rs` — build `CertStore`/configs/`ExclusionSet`,
   parse `clients`, wire into `TlsProxy`, fan-out `Event::Https` arm, startup log.
10. `crates/fah-http/tests/interception.rs` — the four security-property tests +
    h1/h2.

## Risk register

| Risk | Severity | Mitigation |
| ---- | -------- | ---------- |
| Cert-pinned app breaks under interception | High (UX) | Interception opt-in per client; **shipped** exclusion baseline; SNI matched before terminate; splice is the default leg. |
| We present a valid cert for an unverifiable upstream | Critical (security) | Verify-before-present ordering (decision 4) makes it structurally impossible; self-signed-upstream test proves the client handshake errors. |
| A non-listed client gets intercepted | Critical (security) | Empty-default + default-off test + config-fuzz invariant + branch test. |
| CA/leaf key exposure | Critical (security) | CA key never leaves `/config` (p3-01); leaves in-memory LRU only; no Debug on key types; nothing logged. |
| h2/h1 + ALPN bridging bugs (framing, streaming) | Medium | hyper reframes; both-ALPN tests; bodies streamed, never buffered. |
| Interception CPU cost on RB5009 | Medium | Opt-in/low-volume; measured on-device in p3-06 (no 9× conversion for HTTP work). |
| RewindStream mis-orders replayed hello vs socket | Medium | Unit test asserts byte order; interception e2e would fail loudly otherwise. |

## Verification matrix

| Acceptance criterion (task) | Gate | Evidence |
| --------------------------- | ---- | -------- |
| Non-listed client can never be intercepted | 3 | branch test (splice, origin cert) + config fuzz |
| Upstream verification failure never yields a locally-signed success | 3 | self-signed upstream ⇒ client TLS error; no leaf served; event/counter |
| Ad URL inside HTTPS blocked, page loads (listed + CA trusted) | 2 | e2e block-over-TLS + normal-load, h1 and h2 |
| Excluded domain splices for intercepted client | 2, 3 | origin cert observed on a listed client |
| SECURITY.md + CONFIGURATION.md updated same change | 5 | proposed edits listed, owner-approved |
| Gates green | 1, 2 | fmt/clippy/test |
| Banking/pinned app keeps working; handshake budgets | 6 | p3-06 on-device (may hold at `AWAITING SOAK`) |
| GAR §5.8 discharged — hostname-verified upstream connector | 3 | decision 4 (`connect_verified_upstream`); stated in review file |
| GAR §5.14 decision recorded — static-lease precondition or deferral | 5, 6 | decision 2 owner decision; verified per device in p3-06 walkthrough |
