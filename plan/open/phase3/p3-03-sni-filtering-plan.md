# P3-03 — SNI Filtering — Implementation Plan

**Phase:** 3 · **Depends on:** phase2 (closed) · **Task:** `p3-03-sni-filtering.md`

## TASK START / CONTEXT

Read, in this order, before writing any code:

1. `plan/open/phase3/p3-03-sni-filtering.md` — the task file, completely.
2. `plan/open/phase3/CLAUDE.md` — the phase table.
3. PERFORMANCE.md §Golden rules (1–9) and §Budgets — hot-path and memory law.
4. `docs/code-review/phase2/p2-02-review.md` §Implementation Summary — the
   `HostResolver` port, the L1 egress guard (`DestinationPolicy`), and the
   **stream-generic** `serve_connection<S>` claim this task leans on.
5. `docs/code-review/phase2/p2-06-review.md` §Implementation Summary — the
   per-client policy snapshot (`PolicyState` / `context_for`) both pipelines
   share, reused verbatim for the SNI verdict.
6. `crates/fah-http/src/{claim.rs, proxy.rs, request.rs, server.rs}` — read the
   ranges that matter (destination parsing, the judge/emit core, the accept
   loop). The SNI path is a sibling of these, not a rewrite of them.
7. SECURITY.md §"Later phases" and §"Upstream privacy" — the ECH note lands
   here; §Data at rest is not touched (no key material on this path).

Do not read p3-01/p3-02/p3-04, the certificate machinery, or full phase-2
review files beyond the summaries above. **This task decrypts nothing** — no
rustls server/client config, no leaf minting, no CA. That is p3-04.

## What this task is, in one sentence

A second listener on the container (dst-nat target for LAN :443) reads the TLS
**ClientHello only**, asks the existing Rule Engine about the SNI hostname under
the client's policy, and then **closes** (block) or **byte-splices** (pass) —
never terminating TLS.

## Decisions settled by this plan

### 1. Placement — new modules in `fah-http`, no new crate

`fah-http` is already "the HTTP/HTTPS engine" (its `lib.rs`: *"Phase 3 reuses
this pipeline unchanged by handing it a TLS-terminated stream"*). The SNI path
shares this crate's `HostResolver` port, `DestinationPolicy`, `Ruleset`,
`PolicyState` and event channel. New files:

- `crates/fah-http/src/sni.rs` — the ClientHello SNI frame parser (no TLS
  stack, bounded, allocation-bounded).
- `crates/fah-http/src/https.rs` — the HTTPS connection handler: peek hello →
  verdict → close-or-splice.
- `crates/fah-http/src/tls_server.rs` — the `[https.listen]` accept loop.

No dependency-layering change: `fah-http` stays L3, gains no new sibling import.
No new third-party crate for p3-03 (raw byte parsing only; `tokio` `copy_*`
already available).

### 2. The accept loop is shared, not duplicated

`server.rs`'s `accept_loop` (permit-before-accept, `set_nodelay`, spawn) is
**byte-identical** to what the HTTPS listener needs. Extract it into a private
generic helper rather than copy it (engineering principle 4):

- **Existing mechanism to reuse:** `Server` + `accept_loop` in `server.rs`.
- **Required change:** lift the loop body into
  `fn accept_loop<F, Fut>(listener, permits, on_conn: F)` where `on_conn:
  Fn(TcpStream, SocketAddr) -> Fut`, keep `Server` calling it with the proxy,
  add `TlsServer` calling it with the HTTPS handler. `Semaphore`/`JoinHandle`
  bind/serve/shutdown shape is copied structurally (small, and the two configs
  differ), but the loop body is not.
- **Reason:** a divergence between the two accept loops (e.g. one forgets the
  permit-for-the-whole-transfer fix p2-02 landed) is a silent capacity bug.
- **Verification:** `max_connections` binding test mirrored for the HTTPS
  listener; the shared helper has one test.

Lower-churn alternative (documented, not recommended): a second standalone
accept loop in `tls_server.rs`. Rejected — reintroduces the exact drift the
`fah_common::listen` shared bind exists to prevent (`server.rs:5-7`).

### 3. Host-only verdict — reuse the domain tier, add one thin entry point

The matcher already answers HTTP over both tiers via `lookup_http_in`, and its
private `lookup_domain(host, None, ctx)` is exactly the domain-tier walk the
p2 architecture note reserved for *"HTTPS not intercepted → host-only match over
the same domain index"* (`plan/closed/phase2/CLAUDE.md`).

- **Existing mechanism:** `Matcher::lookup_domain` (private) + `context_for`.
- **Required change:** add one public line to `fah-rules`:

  ```rust
  pub fn lookup_host_in(&self, host: &str, ctx: &ClientContext<'_>) -> MatchDecision {
      self.lookup_domain(host, None, ctx)
  }
  ```

  and a `lookup_host(host)` convenience over `ClientContext::default()`.
- **Reason:** the alternative — building a synthetic `HttpRequest { url:
  format!("https://{host}/"), … }` and calling `lookup_http_in` — **allocates a
  String per handshake** and drags in the URL tier, which cannot match anything
  on a path-less SNI observation. A host-only entry point is allocation-free and
  semantically exact (`$dnstype`/`$dnsrewrite` correctly excluded because
  `qbit = None`, same as the HTTP path).
- **Verification:** unit test asserting `||ads.example.com^` blocks the SNI
  `ads.example.com`, an `@@` exception overrides it, and a `$client`-scoped
  rule only fires for the scoped IP — reusing the p2-06 fixtures.

### 4. Destination — resolve the SNI, reuse the egress guard (no other option)

Measured on-device (RB5009, 2026-08-31), not inferred: `getsockopt(SO_ORIGINAL_DST)`
on a dst-nat'd connection **inside the container** returns **ENOENT**. The option
itself exists — the errno is `ENOENT`, not `ENOPROTOOPT` — but the RouterOS-side
dst-nat conntrack lives in a **different network namespace**, so the FAH container
**cannot recover the pre-DNAT destination through this mechanism** (the test is in
§5). After the router's dst-nat, the SNI hostname is therefore the *only*
statement of where the client was going — the direct analogue of the `Host`
header on the :80 path (the same reason `claim.rs:3-8` gives for HTTP).

- **Existing mechanism:** `HostResolver::resolve(host)` +
  `DestinationPolicy::check(SocketAddr)` (shared, judges the *resolved* address;
  `egress.rs` already documents it as shared with the HTTPS/SNI path).
- **Required change:** on pass, resolve the SNI host, build
  `SocketAddr::new(ip, 443)`, run `policy.check` (open-relay guard: refuses LAN/
  loopback/link-local unless allow-listed), connect to the first approved
  address, splice. Reuse `build_http_proxy`'s exact egress construction with
  port 443.
- **Reason:** identical trust boundary to the HTTP path (an attacker-controlled
  destination claim judged against the resolved address); reusing the guard is
  what stops the SNI proxy becoming an open relay into the router.
- **Verification:** blocked SNI never reaches `resolve`/`connect` (asserted with
  a connection-counting origin at 0); an SNI resolving to `192.168.x` is refused
  by the guard.

### 5. `no_sni` is a **classification** switch, and the ECH/no-SNI limit is real

Because the container cannot recover the original destination (decision 4,
measured) **and** there is no SNI to resolve, a no-SNI or ECH ClientHello that
reaches the dst-nat'd :443 has **no recoverable destination** — it cannot be
spliced, whatever `[https.sni] no_sni` says.

- `no_sni = "pass"` (default, per the task): close the connection, emit a
  `https-sni` event with verdict `pass` (unfiltered/undeliverable), do **not**
  count it as a block.
- `no_sni = "block"`: close, emit verdict `block`.

Both close; the switch controls only how the connection is *classified* in the
event feed and metrics. This is stated plainly in CONFIGURATION.md and SECURITY.md
(the ECH note), and it is a headline risk (see Risk register). The DNS layer
still catches the bad domains behind ECH — the phase mitigation on record.

**Measured evidence (on-device, RB5009, 2026-08-31):** a LAN host
(`192.168.10.10`) aimed at `1.1.1.1:4443`; RouterOS dst-nat'd it to the probe
container (`accept_local = 172.17.0.4:4443`, proving the redirect fired);
`getsockopt(SO_ORIGINAL_DST)` returned **`ENOENT` (errno 2)**. The probe was a
throwaway libc `getsockopt` listener container, not kept in-tree. This turns the
transport limit from architectural inference
into a **verified constraint**: with no way to recover the pre-DNAT destination,
no-SNI/ECH HTTPS cannot be forwarded by this transparent proxy. `ENOENT` (not
`ENOPROTOOPT`) is the telling detail — the option is supported; the container's
netns simply holds no conntrack record of the RouterOS-side NAT. If a future
RouterOS/kernel exposed the pre-DNAT tuple to the container's netns, no-SNI could
splice to it — out of scope now.

### 6. Events — add `EventKind::HttpsSni` (wire `https-sni`), reuse `RequestEvent`

The task asks for *"RequestEvent with kind: https-sni, host, verdict"*. The
honest single-source model is a new `Event`/`EventKind` variant carrying a
`RequestEvent`, not a `Host`-string overload of `Event::Http`.

- **Existing mechanism:** `fah_model::Event` (`#[serde(tag="kind")]`),
  `EventKind`, and the fan-out that already routes `Event::Http` into
  `metrics.record_http` / `stats.record_http` (both take `&RequestEvent`).
- **Required change (`fah-model`):**
  - `Event::HttpsSni(Box<RequestEvent>)` with `#[serde(rename = "https-sni")]`.
  - `EventKind::HttpsSni` + `as_str`/`FromStr`/`Display` arm `"https-sni"`.
  - extend `Event::kind/client_ip/timestamp/verdict/policy` match arms (the
    compiler forces each — the repo's preferred failure mode).
  - `Event::https_sni(RequestEvent)` constructor.
- **Required change (binary + observers):** `spawn_event_fanout` gains the
  `Event::HttpsSni` arm → **reuse** `metrics.record_http` and
  `stats.record_http` (a `RequestEvent` is a `RequestEvent`). The events socket
  serializes the new `kind` automatically (serde tag). The `?kind=` API filter
  (`EventKind::from_str`) accepts `https-sni` for free once the enum arm exists.
- **RequestEvent field mapping for an SNI observation:** `host` = SNI host,
  `path` = `""`, `method` = `""`, `resource_type = Unknown`, `status` = `0`
  (no HTTP status — connection-level outcome), `bytes` = upstream→client bytes
  from `copy_bidirectional` on pass / `0` on block, `duration` = hello→close,
  `policy` = the deciding policy id.
- **Reconciliation with the task text ("query-log … extended"):** p2-09
  **removed** the persisted query log and `GET /queries`
  (`plan/closed/phase2/CLAUDE.md` #9). The live feed is `WS /api/v1/events`;
  that is the surface this task extends. No query-log code exists to touch.
- **Metrics dimension:** reuse the aggregate `requests_pass/allow/block` +
  duration families (record_http) rather than inventing a per-protocol counter
  set now. If the owner wants http vs https-sni split in `/telemetry`, that is a
  small additive `protocol`-labelled counter — flagged, not built, to keep this
  task minimal. State the choice in the review file.

### 7. Operating mode — `dns+http+https` only

`EngineMode::DnsHttpHttps` already exists (`schema/engine.rs`). Add
`fn https_enabled(mode)` beside `http_enabled` in `main.rs`, exhaustive-matched
(`Dns | DnsHttp => false`, `DnsHttpHttps => true`) so a future mode fails to
compile until someone decides. The `[https]` section is inert in every other
mode, mirroring how `[http]` is inert without `http` (`schema/http.rs:3-6`).

## Detailed implementation plan

### Step 1 — config surface (`fah-config`)

New `crates/fah-config/src/schema/https.rs`, added to `Config` as `pub https:
HttpsConfig`, `#[serde(default)]` so existing configs parse unchanged:

```rust
#[serde(deny_unknown_fields, default)]
pub struct HttpsConfig {
    pub listen: HttpsListenConfig,   // default "::" / 8443 (dst-nat target of :443)
    pub max_connections: usize,      // default 1024, mirrors [http]
    pub hello_timeout_ms: u64,       // ClientHello read deadline; default 10_000
    pub idle_timeout_ms: u64,        // spliced-session idle close; default 60_000
    pub sni: SniConfig,              // { no_sni: NoSni }  (Pass | Block), default Pass
}
```

- `[https.listen] port` default **8443**, not 443: the container is unprivileged
  after ADR-0004's drop; the router dst-nats 443 here, exactly as 80 → 8080
  (`schema/http.rs:66-71`).
- `[egress]` is **not** duplicated — reused at port 443 (`schema/egress.rs:5-8`
  already reserves it for this path).
- Validation in `fah-config/src/lib.rs`: reject `max_connections == 0`
  (mirror the existing `http.max_connections` check).
- Config classification: `listen`/`max_connections` are `boot`; timeouts follow
  `[http]`'s `boot` precedent (no live-reload consumer).

### Step 2 — ClientHello SNI parser (`fah-http/src/sni.rs`)

Pure function over bytes, no async, unit-testable in isolation:

```rust
pub enum HelloScan {
    Sni(Box<str>),   // extracted server_name
    NoSni,           // valid ClientHello, no SNI extension (or ECH-only)
    NotTls,          // first byte is not a TLS handshake record
    Incomplete,      // need more bytes (caller reads more, up to the cap)
}
pub fn scan_client_hello(buf: &[u8]) -> HelloScan;
```

Frame walk (no allocation beyond the returned host `Box<str>`):

1. Record layer: byte 0 == `0x16` (handshake) else `NotTls`; read 2-byte
   version, 2-byte length. Reassemble across records only up to the cap.
2. Handshake layer: type `0x01` (ClientHello); 3-byte length.
3. Body: `legacy_version(2)`, `random(32)`, `session_id`, `cipher_suites`,
   `compression_methods`, then extensions. Every length is bounds-checked
   against the slice; any overrun → `NotTls` (malformed).
4. Extensions: find `server_name` (type `0x0000`); read the first `host_name`
   (name type `0x00`); return it. No SNI extension → `NoSni`. (An
   `encrypted_client_hello` extension present with no plaintext SNI → `NoSni`;
   we make no attempt to read ECH.)

**Bounded** (hard rule 4): the caller reads into a `Vec` capped at
`MAX_HELLO_BYTES = 16 KiB` (comfortably fits ClientHellos incl. ECH/PQ key
shares; TLS plaintext record max is 16 384). More than one record is
reassembled only within that cap; exceeding it → treat as `NoSni` (we could not
find SNI in a bounded read). No per-connection allocation beyond the one read
buffer and the host string.

### Step 3 — HTTPS connection handler (`fah-http/src/https.rs`)

A `TlsProxy` struct holding the same shared handles the `Proxy` holds
(`Arc<dyn HostResolver>`, `DestinationPolicy` at port 443, `Option<Arc<dyn
Ruleset>>`, `Arc<PolicyState>`, `Option<mpsc::Sender<Event>>`, counters,
`hello_timeout`, `idle_timeout`, `no_sni: NoSni`). Built in the binary beside
`build_http_proxy`.

`async fn serve_connection(self: Arc<Self>, stream: TcpStream, peer: SocketAddr)`:

1. Canonicalize peer (`peer.ip().to_canonical()` — same as `proxy.rs:277`, for
   v4-mapped-v6 and one log identity).
2. Read the ClientHello under `hello_timeout` into a bounded buffer; run
   `scan_client_hello`. `NotTls` → `non_tls` counter, close. `Incomplete` past
   the cap → treat per `NoSni`.
3. Verdict:
   - SNI present: `matcher.lookup_host_in(&host, &ctx)` with `ctx =
     matcher.context_for(peer.ip(), &policies.current())`. `Block` → close now,
     `blocked` counter, emit `https-sni`/`block`, **return before any resolve or
     connect** (acceptance criterion: zero bytes upstream).
   - SNI present, `Pass`/`Allow`: go to splice (step 4).
   - No SNI: honor `no_sni` (decision 5) — close, emit `pass` or `block`. No
     splice is possible (no destination).
4. Splice (pass): `resolve(host)` → for each address `policy.check((ip,443))` →
   first approved → `TcpStream::connect`, `set_nodelay`. Write the **buffered
   ClientHello bytes** to upstream first (they were consumed from the client),
   then `tokio::io::copy_bidirectional_with_sizes(client, upstream, BUF, BUF)`
   under an idle deadline. On close, emit `https-sni`/`pass` with `bytes` =
   upstream→client total. Resolve/connect failures → close, `resolve_failures`/
   `upstream_failures` counters (reuse the `ProxyCounters` names; see Step 5).

`copy_bidirectional` buffer size `BUF` (e.g. 16 KiB each direction) is bounded
and per-connection; total splice memory is `2 × BUF × active_splices`, and
`active_splices ≤ https.max_connections` (semaphore) — bounded by config, not
traffic (hard rule 4).

### Step 4 — listener (`fah-http/src/tls_server.rs`)

`TlsServer` mirroring `Server`: `bind(&HttpsConfig)` via
`fah_common::listen::{listen_addr, bind_tcp, bind_error}` (shared dual-stack
bind, `IPV6_V6ONLY` off — the listener is dual-stack for free, so the IPv6 dst-nat
story is p2-14's, already solved); `serve(Arc<TlsProxy>)` spawning the shared
`accept_loop` (decision 2); `shutdown()` aborting. `PORT_SETTING = "[https.listen]
port, or FAH__HTTPS__LISTEN__PORT"`.

### Step 5 — counters and `lib.rs` exports

- Reuse `ProxyCounters` fields where they mean the same thing (`requests`,
  `resolve_failures`, `upstream_failures`, `refused_destination`,
  `dropped_events`, `blocked`) and add `non_tls` (garbage on :443, analogue of
  `non_http`). Decide in review whether `TlsProxy` gets its own
  `ProxyCounters` instance (recommended — one snapshot per listener keeps
  `/telemetry` legible) or shares the HTTP one. Recommended: separate instance,
  same type.
- `lib.rs`: `pub use https::TlsProxy; pub use tls_server::TlsServer; pub use
  sni::{scan_client_hello, HelloScan};` and re-export the `NoSni` config echo
  if the handler needs it typed locally.

### Step 6 — binary wiring (`fastadhunter/src/main.rs`)

- `https_enabled(config.engine.mode)` gate (decision 7).
- Bind `TlsServer::bind(&config.https)` **before** the privilege drop (beside
  the HTTP bind, ~line 385) — 8443 needs no privilege, but the ordering leaves
  443 usable where a runtime permits it, exactly the reasoning at
  `main.rs:376-384`.
- Build `TlsProxy` after the drop, `.with_rules(Arc::clone(&rules))`,
  `.with_policies(Arc::clone(&policy_state))`, `.with_events(events_tx.clone())`
  — the **same** ruleset, snapshot and channel the HTTP proxy and DNS pipeline
  use, so one device is judged identically across DNS/HTTP/HTTPS.
- `HTTPS_ORIGIN_PORT = 443` const (mirror `HTTP_ORIGIN_PORT`); egress policy
  `DestinationPolicy::new(443, exceptions)` from the same `[egress]` parse.
- `tls.serve(...)` after the drop, in the `dns.serve` / `http.serve` block.
- Feed `TlsProxy`'s counters into `spawn_telemetry_poll` like the HTTP ones.

## Public API / compatibility

- `fah_model::Event`/`EventKind` gain a variant — **additive**; existing serde
  round-trips unchanged (new tag value only). Any exhaustive external match on
  `Event` recompiles (all in-tree).
- `WS /api/v1/events` gains `kind: "https-sni"`; a client that filters by kind
  and does not know it simply never selects it. No breaking change to existing
  message shapes.
- `Matcher` gains `lookup_host`/`lookup_host_in` — purely additive.
- New `[https]` config section, `#[serde(default)]` — old configs parse.

## Security model / trust boundaries

- **No decryption, no key material, no CA on this path.** The bytes past the
  ClientHello are relayed uninspected. Nothing here reads or writes `/config`.
- **Attacker-controlled input** is (a) the ClientHello bytes — parsed with
  strict bounds, never trusted for length; and (b) the SNI hostname — judged,
  then handed to `resolve` and the **egress guard**, which is what prevents the
  proxy relaying to the router or the API (open-relay guard, `egress.rs`).
- **ECH / no-SNI:** documented limitation (decision 5), SECURITY.md note; the
  DNS layer remains the backstop.
- **Fail-closed on garbage:** non-TLS or malformed hellos are closed and
  counted, never forwarded.

## Performance / memory

| Metric | Class | Value |
| ------ | ----- | ----- |
| Per-connection heap (steady splice) | hard gate | 2 × `BUF` copy buffers + one host `Box<str>`; bounded, freed on close |
| Hello parse | target | bytes-only frame walk, no crypto, no alloc beyond the returned host |
| SNI verdict | target | one `lookup_host_in` — domain-tier walk, allocation-free (matcher hot-path contract) |
| Splice throughput vs direct | diagnostic | bench in this task; budget row set in p3-06 with the ~9× factor |
| DNS hot path | hard gate | untouched — nothing on this path runs per DNS query |
| Steady RSS | hard gate | grows only with concurrent HTTPS connections, capped by `https.max_connections` |

No number is invented as a gate; the throughput budget row lands in
PERFORMANCE.md via p3-06 from measured data (dev box A/B, then RB5009 probe, per
`docs/measurement-traps.md` — splice is I/O-bound, so follow the p2-08 opaque-body
convention, **no core-pinning**).

## Tests

### Unit (`fah-http/src/sni.rs`)

- Real ClientHello bytes (captured from a `tokio-rustls` client in a dev-dep
  test, or a checked-in fixture) → correct SNI extracted.
- No-SNI hello → `NoSni`; hello with `encrypted_client_hello` and no plaintext
  SNI → `NoSni`.
- Truncated hello → `Incomplete` until complete, then `Sni`.
- Every length field overrun (fuzz a few hundred mutations) → `NotTls`, never a
  panic, never an out-of-bounds read.
- Oversized (> `MAX_HELLO_BYTES`) → `NoSni`, bounded.

### Integration (`fah-http/tests/`, new `sni.rs`)

- **Splice round-trip (the acceptance test):** a real `tokio-rustls` client
  handshakes to a real `tokio-rustls` origin **through** the `TlsProxy` splice;
  handshake completes end-to-end and an application payload is byte-identical.
  (This proves we relay, not terminate.)
- **Blocked SNI → zero bytes upstream:** connection-counting origin (reuse the
  `origin()` pattern in `tests/filtering.rs`) stays at 0 accepts; client sees a
  closed connection.
- **Egress guard:** SNI resolving to a private address → refused, counted.
- **Garbage on :443:** random bytes → closed, `non_tls` incremented, listener
  survives (mirror `non_http_bytes_are_closed_and_counted`).
- **`no_sni` matrix:** no-SNI hello under `pass` → event `pass`; under `block` →
  event `block`; both close.
- **Event kind:** a spliced and a blocked connection each surface on the event
  channel as `kind = https-sni` with the right verdict (assert via the mpsc, as
  `filtering.rs` does).

### Regression

- `fah-model` serde round-trip extended to the new variant.
- `crates/fastadhunter/tests/layering.rs` unchanged (no new crate).
- `http_e2e` and existing proxy/filtering suites unchanged.

### Bench

- Criterion or an `httpbench`-style splice arm measuring added latency and
  throughput vs a direct TCP splice. Diagnostic in this task; budget in p3-06.

## Verification / Gates

1. **Compile/check:** `cargo fmt --all -- --check`, `cargo clippy --workspace
   --all-targets -- -D warnings`.
2. **Tests:** `cargo test --all-features --workspace` green, including the
   splice round-trip and the zero-bytes-upstream assertion.
3. **Security invariants:** blocked SNI never resolves/connects (asserted);
   egress guard refuses private destinations; garbage closed not forwarded.
4. **Perf/memory:** splice bench recorded in the review file; per-connection
   memory shown bounded by `max_connections`.
5. **API/docs consistency:** `EventKind` spelling test covers `https-sni`;
   proposed doc edits listed and matched to code.
6. **RB5009/e2e:** deferred to p3-06 (dst-nat 443, on-device splice throughput,
   real phone/browser). Mark the task `AWAITING SOAK` if the dev box cannot
   produce the on-device throughput row the acceptance criterion names.

## Doc changes (proposed — owner approval required before editing any `.md`)

- CONFIGURATION.md: new `[https]`, `[https.listen]`, `[https.sni]` section; note
  `[egress]` is shared at 443; classification table rows.
- SECURITY.md: the **ECH / no-SNI limitation** (decision 5) under a Phase-3 SNI
  note; reaffirm "SNI path decrypts nothing".
- API.md §`WS /api/v1/events`: `kind` may be `https-sni`; the field mapping for
  an SNI observation (empty method/path, status 0).
- ARCHITECTURE.md §HTTP Pipeline (or a new §HTTPS): the SNI listener and the
  close-or-splice split; the shared accept loop.
- ROADMAP.md: mark SNI filtering delivered when the phase closes.
- Finish the code first, then list these edits and wait (working agreement). The
  task's own review file `docs/code-review/phase3/p3-03-sni-filtering-review.md`
  needs no approval.

## Non-goals / deferred

- **Decryption/MITM** — p3-04 (this task's splice path is the branch p3-04
  replaces for listed clients).
- **QUIC / HTTP-3** — documented limitation, backlog (UDP :443 is a separate
  transport we do not touch).
- **Forwarding no-SNI/ECH traffic** — impossible: the container cannot recover
  the original destination (decision 5, measured `ENOENT` on-device); documented,
  not attempted.
- **Per-protocol metric counters** — aggregate reuse now; additive later if the
  owner asks.

## File-by-file implementation checklist (dependency order)

1. `crates/fah-config/src/schema/https.rs` — new config types + defaults +
   tests.
2. `crates/fah-config/src/schema/mod.rs` + `Config` — wire `https`, re-export.
3. `crates/fah-config/src/lib.rs` — `max_connections != 0` validation.
4. `crates/fah-rules/src/matcher.rs` — `lookup_host` / `lookup_host_in` (+ test).
5. `crates/fah-model/src/request_event.rs` — `Event::HttpsSni`,
   `EventKind::HttpsSni`, constructors, match arms, serde rename (+ tests).
6. `crates/fah-http/src/sni.rs` — parser + unit tests.
7. `crates/fah-http/src/https.rs` — `TlsProxy`, close-or-splice, counters.
8. `crates/fah-http/src/server.rs` — extract the shared generic `accept_loop`.
9. `crates/fah-http/src/tls_server.rs` — `TlsServer`.
10. `crates/fah-http/src/lib.rs` — module decls + `pub use`.
11. `crates/fastadhunter/src/main.rs` — `https_enabled`, bind/build/serve wiring,
    telemetry poll, `HTTPS_ORIGIN_PORT`.
12. `crates/fastadhunter/src/main.rs` fan-out — `Event::HttpsSni` arm reusing
    `record_http`.
13. `crates/fah-http/tests/sni.rs` — integration + splice round-trip.
14. Bench arm (proxy bench or httpbench splice mode).

## Risk register

| Risk | Severity | Mitigation |
| ---- | -------- | ---------- |
| **No-SNI/ECH HTTPS is undeliverable** on RouterOS (container cannot recover the original dst — measured `ENOENT` on-device, decision 5) — enabling `dns+http+https` could break the rare no-SNI client | High (correctness/UX) | Document loudly (SECURITY.md, CONFIGURATION.md); default `no_sni = pass` logs rather than blocks; DNS layer catches the domains; ECH still nascent. Constraint is now measured, not inferred. |
| ClientHello parser panics/over-reads on hostile bytes | High (DoS/security) | Strict bounds on every length; fuzz suite; `NotTls` on any overrun; never `unwrap` on wire data. |
| Splice memory unbounded under connection floods | Medium | `https.max_connections` semaphore + fixed `BUF`; per-connection memory is `O(1)`, total `O(max_connections)`. |
| Accept-loop drift from the HTTP one (capacity bug) | Medium | Shared generic `accept_loop` (decision 2); mirrored `max_connections` binding test. |
| Adding `EventKind` variant breaks a surface that assumed two kinds | Low | Additive; compiler-forced exhaustive updates; spelling test covers the new value. |
| Second resolution races the client's own (CDN/geo) | Low | Same trade the :80 path already accepts; SNI-splicing to any IP serving that SNI is correct; egress guard bounds the target. |

## Verification matrix

| Acceptance criterion (task) | Gate | Evidence |
| --------------------------- | ---- | -------- |
| Blocked domain: zero bytes to upstream | 2, 3 | connection-counting origin at 0; verdict taken before resolve/connect |
| Spliced HTTPS byte-identical + budget-fast | 2, 4 | round-trip payload equality; splice bench (budget in p3-06) |
| No-SNI / ECH handled per config | 2 | `no_sni` matrix test; documented limitation |
| Events carry `kind: https-sni`, host, verdict | 2, 5 | mpsc assertion; `EventKind` spelling test; API.md |
| Docs updated (CONFIGURATION, SECURITY ECH, API kinds) | 5 | proposed edits listed, owner-approved |
| Gates green | 1, 2 | fmt/clippy/test |
| RB5009 dst-nat 443 + on-device throughput | 6 | p3-06 (may hold task at `AWAITING SOAK`) |
