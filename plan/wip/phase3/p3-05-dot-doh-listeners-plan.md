# P3-05 — DoT and DoH Listeners — Implementation Plan

**Phase:** 3 · **Depends on:** p3-01 · **Task:** `p3-05-dot-doh-listeners.md`

## TASK START / CONTEXT

1. `plan/wip/phase3/p3-05-dot-doh-listeners.md` — the task file, completely.
2. `plan/wip/phase3/CLAUDE.md` — phase table; p3-01…p3-04 statuses.
3. `docs/code-review/phase3/p3-01-cert-core-review.md` — **Implementation
   Summary** (declared dependency): the actual TLS `ServerConfig` construction
   path for the API pair.
4. Implementation Summaries of p3-02/p3-03/p3-04 reviews **only where this
   task touches them**: p3-02 for the API-cert source, p3-04 only if it
   changed `fah-api` server wiring. Do not read their full findings.
5. ARCHITECTURE.md §Listeners, §Dependency Layering (Ports table), §Runtime
   Model — the wiring rules this task must not bend.
6. CONFIGURATION.md `[dns.listen]` and `[api]` sections only.
7. `docs/code-review/Global Architecture Review-Reconciled.md` §5 items 1
   (listener supervision — the template p3 listeners must NOT clone; the fix
   shipped in p2.5-01, so copy the *current* pattern) and 9 (DoH/DoT listener
   placement vs the admin surface).
8. Code, fully: `crates/fah-dns/src/tcp.rs` (the `Accept` trait +
   `handle_connection` this task generalizes), `src/server.rs`
   (bind/serve/fatal supervision), `src/udp.rs` (skim),
   `crates/fah-api/src/server.rs` (semaphore bound + handshake timeout
   pattern), `src/ports.rs` (port-trait precedent),
   `crates/fah-common/src/resolve.rs` (the boxed-future port shape),
   `crates/fah-common/src/listen.rs` (bind helpers),
   `crates/fah-model/src/query_event.rs` and `src/protocol.rs`.

Do not re-litigate: dual-stack binding (`fah_common::listen`), the
bind-then-drop privilege split (ADR-0004), the single event channel, or the
upstream `Protocol` enum (which is the *upstream* transport and must not be
reused for the client-side dimension — its doc comment says why).

## Decisions settled by this plan

1. **DoT reuses the TCP/53 message loop.** RFC 7858 is exactly RFC 1035
   2-byte framing over TLS; `tcp::handle_connection` is already generic over
   `AsyncRead + AsyncWrite`. It gains a `Transport` parameter (today it
   hardcodes `Transport::Tcp`) and `pub(crate)` visibility; DoT gets its own
   accept loop rather than forcing TLS through the `Accept` trait — the
   handshake must happen inside the spawned connection task with a timeout,
   not inside `accept()`, or one slow handshake stalls every accept.
2. **DoT connection bound: semaphore before accept, capacity 64 (compiled
   const `DOT_MAX_CONNECTIONS`),** the exact `fah-api` `MAX_CONNECTIONS`
   pattern including kernel-backlog queueing. Handshake timeout 10 s
   (`HANDSHAKE_TIMEOUT`, same value/rationale as `fah-api`). Idle timeout:
   reuse `TCP_IDLE_TIMEOUT` (10 s) — Android holds DoT connections open with
   keepalive queries; if soak evidence shows churn, raising it is a one-const
   change noted for p3-06.
3. **DoT certificate — CA-minted per SNI when a CA exists, API pair as the
   fallback.** The self-signed API pair alone cannot satisfy Android Private
   DNS hostname mode: its SANs are the bind/probed *addresses*
   (`fah-api/src/tls.rs` `san_entries`), it names no hostname, and it chains
   to nothing a device trusts — so "serve the API cert and document it" would
   leave the phase's own definition of done undeliverable. Two client routes,
   both must work:
   - **Imported real certificate** (p3-01/p3-02): hostname SAN + a chain the
     device already trusts — the shared API pair serves DoT as-is, no CA
     install.
   - **FAH CA route**: the device installs the exported CA (p3-06
     walkthrough), and DoT presents a leaf for the hostname the phone was
     pointed at: the DoT `ServerConfig` uses `fah_certs::MintingResolver`
     (p3-01) when a CA exists — leaf minted for the SNI the client sends,
     hostname SAN correct by construction, chained to the installed CA — with
     the API pair as the static fallback when no CA exists or the hello
     carries no SNI. Minting for arbitrary SNI is harmless (the leaf only
     means something to a device that installed our CA) and the p3-01 cache
     bound (512) caps the state.
   The binary builds **one dedicated `Arc<ServerConfig>` for DoT** (the API
   server keeps its own — the minting resolver lives on 853 only). No new
   config key: the operator picks the hostname (a DNS record plus the phone
   setting); the resolver serves whatever name arrives.
   **Bootstrap:** while the phone validates, the Private DNS hostname must
   resolve to the container over plain DNS — a local answer for that name
   (e.g. a `$dnsrewrite` rule mapping it to the container address) is part of
   the p3-06 walkthrough, not code.
   **Explicitly-tested assumption (recorded in p3-06 either way):** the test
   device consults the user CA store when validating Private DNS. Vendor
   behaviour varies; if it fails on the household's devices, the
   imported-real-cert route is the remaining path and the walkthrough says so.
4. **DoH is a route on the existing API server** (`/dns-query`, RFC 8484,
   GET + POST wireformat), reached through a **new port trait in
   `fah-api/src/ports.rs`** implemented by the binary over
   `Arc<fah_dns::Pipeline<F>>` — the `StatsSource`/`HostResolver` crossing,
   no sibling import. **`/dns-query` is auth-exempt**: DNS clients cannot
   carry bearer keys or cookies. This amends SECURITY.md's "two exemptions"
   sentence and is called out as a doc change; the route is outside `/api/v1/`
   precisely so the admin-surface auth statement stays clean (GAR §5.9).
   Two consequences of sharing the listener, stated because GAR §3.5 flagged
   them: **(a)** DoH connections draw from the API server's 64-permit
   semaphore (`fah-api/src/server.rs:33`), so browsers holding persistent h2
   DoH sessions share capacity with the dashboard — acceptable at household
   scale, but it is a coupling, so p3-06's soak records peak concurrent DoH
   sessions against the ceiling and a const bump / separate semaphore is the
   named escape hatch if measurement demands it (measure before tuning);
   **(b)** h2 needs no work — the API `ServerConfig` already offers ALPN
   `[h2, http/1.1]` (`fah-api/src/tls.rs:226`) and hyper-util's auto builder
   serves both, so RFC 8484's SHOULD-h2 is met; record it as verified, not
   assumed.
5. **Config:** `[dns.listen]` gains `dot_enabled` (default **true**),
   `dot_port` (default **853**), `doh_enabled` (default **true**). All three
   boot-class. Rationale for enabled-by-default: the phase's definition of
   done is zero-setup encrypted DNS; exposure is LAN-side and bounded exactly
   like :53; an untrusted cert makes an unused listener, not a vulnerability;
   and a disabled default would need a config edit, which the working
   agreement says features must not require. `doh_enabled = false` means the
   route is **absent** — the request falls through to the router's merged SPA
   fallback (`web::mounted()`), so a GET returns the dashboard shell and a
   POST is refused; the observable invariant is that no
   `application/dns-message` response exists, not a 404. Mirrors p2-01's
   "don't bind what you won't serve"; `dot_enabled = false` means the socket
   is never bound.
6. **Transport dimension:** new `fah-model` enum
   `ClientTransport { Udp, Tcp, Dot, Doh }`, serialized lowercase, carried as
   a new `transport` field on `QueryEvent`. It is deliberately **not**
   `fah_model::Protocol` (that is the observed *upstream* transport; the doc
   comment on it forbids exactly this reuse). `fah-dns`'s internal
   `pipeline::Transport` gains `Dot`/`Doh` variants and maps 1:1 into the
   model enum where the `QueryEvent` is built. UDP payload sizing
   (`max_udp_payload`) applies to `Udp` only; the three stream transports use
   `u16::MAX`.
7. **Client IP for policy resolution = TCP/TLS peer address** for DoT, and
   the connection's `ConnectInfo<SocketAddr>` peer for DoH (nothing proxies
   this listener — same argument SECURITY.md already makes for `Host`-derived
   origins).
8. **Port 853 binds before the privilege drop**, alongside 53, inside
   `fah_dns::Server::bind` — ADR-0004's split already separates bind from
   serve; DoT rides it. Bind failure on 853 when enabled is a startup error
   naming `[dns.listen] dot_port` / `FAH__DNS__LISTEN__DOT_PORT`, via the
   parameterized `bind_error`.

## Detailed implementation plan

### Step 1 — config (`crates/fah-config/src/schema/dns/listen.rs`)

- `DnsListenConfig` += `dot_enabled: bool` (true), `dot_port: u16` (853),
  `doh_enabled: bool` (true), serde defaults per the existing pattern;
  `deny_unknown_fields` keeps typos fatal.
- CONFIGURATION.md `[dns.listen]` section text (proposed edit): three keys,
  all boot, with the Android-trust caveat one line long.

### Step 2 — model (`crates/fah-model`)

- New `client_transport.rs`: `ClientTransport` as decided, `as_str()`,
  lowercase serde, `Display`. Include `Tcp` — the dimension is
  `udp|tcp|dot|doh` per the task, and TCP/53 exists today.
- `QueryEvent` += `pub transport: ClientTransport`; constructor threading.
  QueryEvents are not persisted per-row (SECURITY.md: aggregates only), so
  the only compatibility surface is the WS JSON: `transport` becomes a field
  of the **DNS record shape** (`fah-api`'s `QueryRecord`/wire mapping), always
  present on `kind: dns` events; HTTP-side record shapes are untouched — the
  API.md event-shape note says which kind carries the key.

### Step 3 — pipeline (`crates/fah-dns/src/pipeline.rs`)

- `enum Transport { Udp, Tcp, Dot, Doh }`; the two new variants behave as
  `Tcp` for payload sizing. Map to `ClientTransport` at the single point the
  `QueryEvent` is built. Every existing `handle` call site names its variant
  already, so the change is additive.

### Step 4 — DoT listener (`crates/fah-dns/src/dot.rs`, new)

- `pub async fn run(listener: TcpListener, acceptor: TlsAcceptor,
  pipeline: Arc<Pipeline<F>>) -> ListenerDied`:
  - `RetryPolicy` accept-loop skeleton copied from `tcp::run` (the p2.5-01
    resilient template — accept errors retry with backoff, `Fatal` returns
    `ListenerDied` into the supervision channel).
  - Semaphore permit **before** `accept()` (decision 2); permit moves into
    the task.
  - Spawned task: `timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream))`;
    failure → `debug!`, slot freed. Success →
    `tcp::handle_connection(tls_stream, &pipeline, client_ip, Transport::Dot)`
    with the same disconnect-classification logging tcp.rs has.
- `tcp::handle_connection` change: takes `transport: Transport`; `tcp::run`
  passes `Tcp`. No other body change — framing, idle timeout, malformed-close
  semantics are shared by construction (principle 4).
- tokio-rustls moves from dev-dependency to dependency of `fah-dns`
  (rustls already is one), pinned like `fah-api`'s:
  `default-features = false, features = ["aws_lc_rs", "tls12"]` — one crypto
  provider across the workspace.

### Step 5 — DoT wiring (`crates/fah-dns/src/server.rs`, `crates/fastadhunter/src/main.rs`)

- `Server::bind(listen: &DnsListenConfig)` additionally binds a TCP listener
  on `(address, dot_port)` when `dot_enabled` (via `fah_common::listen`,
  dual-stack), stores it beside the existing pair; `dot_addr()` accessor for
  tests.
- `Server::serve` gains `dot_tls: Option<Arc<rustls::ServerConfig>>`; when the
  socket exists it spawns `dot::run` with its own `fatal_tx` clone — DoT death
  reaches the same supervision path as UDP/TCP.
- `main.rs`: build the **dedicated DoT `Arc<ServerConfig>`** per decision 3 —
  `fah_certs::MintingResolver` over the `CertStore` when a CA exists, its
  `fallback` slot (p3-01's constructor parameter) holding the API pair's
  `CertifiedKey` from **`CertStore::api_certified_key()`** — p3-01 delivers it
  so no key material is re-parsed outside `fah-certs` — so a no-SNI hello still
  handshakes; plain API pair otherwise —
  and hand it to `Server::serve`.
- **Pre-warm the DoT hostname at startup** (p3-01 M5 resolution):
  `MintingResolver::resolve` never mints — it is a synchronous cache read, so
  minting there would block a tokio worker. Call
  `spawn_blocking(move || store.prewarm(&dot_hostname))` once during startup for
  the hostname clients will send as SNI; the `fallback` covers the no-SNI case.
  `CertStore::prewarm` is blocking and single-flighted inside `fah-certs`; never
  call it directly from an async task. A resolve that misses the warm cache
  increments `LeafCacheStats::unwarmed_misses`, so a forgotten pre-warm shows up
  in `GET /api/v1/certificates` rather than as a silent fallback.
- When `api.tls = false` **and** DoT is
  enabled, the config is still built from the same cert pair (DoT without TLS
  does not exist); only if the cert pair itself cannot load does DoT fail
  startup, explicitly (rule 11: explicit failure over silent downgrade —
  never a plaintext fallback on 853).

### Step 6 — DoH port and route (`crates/fah-api`)

- `ports.rs`: 
  `pub type Resolving = Pin<Box<dyn Future<Output = Option<Vec<u8>>> + Send>>;`
  `pub trait DnsWireSource: Send + Sync + 'static { fn resolve(&self, message: Vec<u8>, client: IpAddr) -> Resolving; }`
  — the `HostResolver` boxed-future shape (implementors clone an `Arc` into
  the future). `None` = drop, exactly the pipeline's contract.
- `AppState`/builder: `pub doh: Option<Arc<dyn DnsWireSource>>` — `None` when
  `doh_enabled = false` or the engine mode lacks a DNS pipeline handle; the
  routes are added only when `Some` (decision 5).
- `routes.rs`: `/dns-query` (GET + POST) **outside** the `/api/v1` nest and
  outside the auth layer.
- New `crates/fah-api/src/doh.rs` handlers:
  - POST: require `Content-Type: application/dns-message`; body cap 65 535
    bytes (the protocol's own maximum; oversized → 413); empty → 400.
  - GET: `?dns=` base64url-unpadded (RFC 8484 §4.1); decode failure → 400.
  - Both: peer IP from `ConnectInfo<SocketAddr>`; call the port; `None` →
    `502` with `application/dns-message`-less problem body is wrong — RFC
    behaviour: answer nothing meaningful exists, so return 500-class only on
    internal failure; a pipeline `None` (undecodable query) → 400.
  - Response: `200`, `Content-Type: application/dns-message`,
    `Cache-Control: no-store` (deliberately conservative; deriving `max-age`
    from the answer TTL means parsing the response again — noted as a
    possible later refinement, not built).
- Binary (`main.rs`): implement `DnsWireSource` over `Arc<Pipeline<F>>`
  calling `pipeline.handle(&message, client, Transport::Doh)`.

### Step 7 — observability

- `QueryEvent.transport` flows through the existing single event channel to
  stats/WS untouched — no new channel, no new counters in this task. Any
  per-transport telemetry counter set is p3-06's call if a budget needs it
  (GAR §5.10 taxonomy work stays minimal here); mark as diagnostic.
- API.md WS §`WS /api/v1/events` event shape gains the `transport` key
  (proposed doc edit).

## Ownership / concurrency summary

Pipeline stays the single shared `Arc`; each listener owns its socket and its
accept loop; TLS config is an immutable `Arc<ServerConfig>` cloned at spawn
(a cert import needs a restart — consistent with p3-02's decision, one story
everywhere). DoT connection state is one task + one permit per connection,
bounded at 64; DoH rides the API server's existing 64-connection semaphore.
No locks on the query path beyond what exists today.

## Performance contract

| Metric | Class | Value |
| ------ | ----- | ----- |
| Verdict parity | hard gate | same domain/client/rules ⇒ byte-equivalent verdict across udp/tcp/dot/doh (test matrix) |
| DoT/DoH added latency vs UDP (in-engine) | target now, budget row in p3-06 | TBD — must be measured during verification (dev box + ~9× factor; handshake excluded from per-query figures, reported separately) |
| TLS handshake cost | diagnostic here | TBD — on-device measurement is p3-06 (GAR §5.13) |
| DoT concurrent connections | hard gate | ≤ 64, enforced before accept |
| Memory | hard gate | per-connection state × 64 bound; no per-query allocation added beyond the existing TCP read buffer |
| RSS steady-state | hard gate | ≤ existing 128 MB budget with listeners idle — re-affirmed in p3-06 soak |

No invented numbers: every latency figure is `TBD — must be measured during
verification`.

## Tests

### Unit

- `ClientTransport` serde spelling (lowercase, four variants) and the
  pipeline→model mapping.
- DoH request parsing: base64url decode (padded input rejected per RFC),
  content-type enforcement, body cap, empty body.
- Config defaults: `dot_enabled`/`doh_enabled` true, `dot_port` 853;
  env override `FAH__DNS__LISTEN__DOT_PORT` works.

### Integration

- **Verdict parity matrix (the acceptance test):** one blocked and one
  allowed domain, same client, over UDP, TCP, DoT, DoH against ephemeral
  listeners (port 0) — verdicts and answers identical; each event carries the
  right `transport`. DoT client: `tokio-rustls` client (or hickory's DoT
  client, already a dev-dependency pattern in `upstream/encrypted.rs`)
  trusting the test cert. DoH client: reqwest/hyper POST + GET forms.
- Policy resolution uses the TLS peer address: a per-client policy assigned to
  the test client IP applies over DoT and DoH.
- Concurrent-connection bound: 65 simultaneous DoT connects — the 65th queues
  (does not error, does not get served while 64 are held).
- Handshake timeout: a TCP connect to 853 that never speaks TLS is dropped
  after the timeout, slot freed.
- `dot_enabled = false` ⇒ nothing listens on 853 (connect refused);
  `doh_enabled = false` ⇒ `/dns-query` never answers
  `application/dns-message` (a GET falls to the SPA shell, a POST is
  refused) and everything else serves.
- `/dns-query` requires no auth; `/api/v1/*` still does (exemption is exactly
  one route wide).

### E2E

- Extend `crates/fastadhunter/tests/e2e.rs`: full binary boot with defaults —
  DoT and DoH answer a query; blocked domain blocked over both. (Windows
  WSAEACCES trap from `docs/project-state.md` §Known-good gate note applies —
  environmental, do not attribute.)

### Regression

- UDP/TCP listener tests unchanged; WS event shape: existing keys byte-stable,
  `transport` added; existing `fah-api` auth tests green (the exemption did
  not widen); `mode_sync.rs` / layering green.

### Security

- DoT never serves plaintext: a plain-TCP DNS query on 853 gets no DNS answer
  (handshake failure closes it).
- The DoH route cannot reach any admin handler: auth-exemption test above plus
  a route-table assertion that `/dns-query` is the only unauthenticated
  addition.
- No cert/key material in DoT error logs (handshake failures log error kind
  only).

### Performance

- Criterion or harness measurement of DoT/DoH added latency vs UDP on the dev
  box — recorded in the review file as the seed for p3-06's budget rows;
  diagnostic here.

## Verification / Gates

- **Mandatory:** `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --all-features --workspace`.
- **Mandatory:** parity matrix, bound, and auth-exemption tests green.
- **Recommended:** dev-box latency measurement recorded (feeds p3-06).
- **Diagnostic:** Android Private DNS against a real device is p3-06 (needs
  the router and the user; the working agreement forbids touching the RB5009
  from here).

## Doc changes (proposed — owner approval required)

- CONFIGURATION.md `[dns.listen]`: three new keys.
- API.md: `/dns-query` (unauthenticated, RFC 8484 subset), WS event
  `transport` key.
- SECURITY.md §API access: the exemption list gains `/dns-query` (with the
  one-line rationale); §Later phases DoT/DoH bullet becomes present tense.
- ARCHITECTURE.md §Listeners: DoT/DoH rows move from "later phase" to real.
- Finish code first, list the edits, wait for the yes.

## Non-goals

- DoH over HTTP/3, DNS-over-QUIC (backlog, per the task).
- Padding (RFC 7830), TTL-derived DoH `max-age`, per-transport telemetry
  counters (noted diagnostic, built only if p3-06 needs them).
- Any router configuration.

## Acceptance criteria (from the task file)

- Same domain, same client, same verdict across UDP/DoT/DoH (matrix green;
  TCP included).
- Android Private DNS setup documented (text here, walked with the user in
  p3-06).
- CONFIGURATION.md + API.md updated in the same approved change.
- Gates green.
