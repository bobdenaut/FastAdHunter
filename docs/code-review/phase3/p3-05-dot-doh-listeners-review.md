# P3-05 — DoT and DoH Listeners — Review

**Task:** `plan/wip/phase3/p3-05-dot-doh-listeners.md` · **Plan:**
`p3-05-dot-doh-listeners-plan.md` · **Status:** implementation complete, review
not started.

## Implementation Summary

Two new front doors into the unchanged `fah-dns` pipeline: a DoT listener on
`[dns.listen] dot_port` (853) bound beside :53 before the privilege drop, and
an RFC 8484 `/dns-query` route on the API server reached through a new
`fah-api` port. Every `QueryEvent` now names the client transport
(`udp|tcp|dot|doh`), and the WS `kind: dns` record carries it as `transport`.

| Area | Where |
| --- | --- |
| `ClientTransport` (L1) | `crates/fah-model/src/client_transport.rs` (new); `QueryEvent.transport`, constructor parameter |
| Config keys `dot_enabled`, `dot_port`, `doh_enabled` | `crates/fah-config/src/schema/dns/listen.rs`, validation + env overrides in `lib.rs` / `env.rs` |
| DoT listener, `DotTls`, connection bound, handshake deadline | `crates/fah-dns/src/dot.rs` (new) |
| Shared framing loop generalised over `Transport` | `crates/fah-dns/src/tcp.rs` (`handle_connection`, `report_connection_end`) |
| 853 bind, `serve(pipeline, Option<DotTls>)`, `dot_addr()` | `crates/fah-dns/src/server.rs` |
| `Transport::{Dot, Doh}` → `ClientTransport` at the one event-build site | `crates/fah-dns/src/pipeline.rs` |
| `DnsWireSource` / `WireResolving` port, `AppState.doh` | `crates/fah-api/src/ports.rs`, `state.rs` |
| `/dns-query` handlers (GET + POST) | `crates/fah-api/src/doh.rs` (new); route in `routes.rs` outside `/api` and outside the auth layer |
| WS record `transport` key | `crates/fah-api/src/wire.rs` |
| `DnsWireAdapter`, DoT TLS assembly, API-pair loading for DoT | `crates/fastadhunter/src/adapters.rs`, `main.rs` (`dot_tls`) |
| Manual request suite | `requests/dns-query.http`, `requests/dns-query.bin` (new) |

Release-binary dependency delta: none. `Cargo.lock` gains four edges only
(`fah-dns → fah-certs, tokio-rustls`; `fah-api → base64`; `fah-dns` dev
`x509-parser`), all crates already linked.

## Decisions

- **Leaf minted at the handshake, not pre-warmed on a timer (deviation from
  plan Step 5).** The plan pre-warms "the DoT hostname" at boot and re-warms
  it every 24 h, but also decides "no new config key" — nothing in the
  binary knows which hostname clients will send, so the ticker had nothing to
  warm. `dot.rs` instead peeks the ClientHello with
  `tokio_rustls::LazyConfigAcceptor`, runs `spawn_blocking(store.prewarm(sni))`
  when a CA exists (p3-04's exact pattern, `intercept.rs:129`), then completes
  the handshake through the shared `MintingResolver`. An expired leaf
  (p3-01 M3/F8) or one evicted by p3-04's traffic through the shared 512-entry
  LRU (p3-04 N4, p3-05 half) costs one mint on the next handshake and is never
  served as the fallback while a CA exists — the interval question the plan
  asked to answer no longer exists. Both hazards are pinned by
  `an_evicted_leaf_is_re_minted_on_the_next_handshake_never_the_fallback`.
  The mint (P-256 keygen + sign, 55 µs x86 per p3-01, ≈0.5 ms RB5009 by the
  documented factor) runs on the blocking pool, off the query path, once per
  host per 7 days; it is bounded by the 64-connection cap and the 10 s
  handshake deadline (p3-01's "remote-input CPU amplifier" carry-over stands,
  unchanged in kind from p3-04).
- **`api.tls = false` still loads the API pair when DoT is enabled**
  (`main.rs`): `load_or_generate` runs whenever `api.tls || dot_enabled`, so
  the pair exists on a fresh `/config` and the interrupted-replacement healer
  runs before `api_certified_key()` reads it. This discharges p3-02 LOW-B's
  root for the only reader that exists; `fah-certs` gains no second healer.
  With both off nobody reads the pair.
- **Certificate failure degrades DoT, never the resolver.** No cert store,
  an unloadable API pair, or a `ServerConfig` build error logs `error!` and
  closes the 853 socket (`Server::serve` drops it; `dot_addr()` reads `None`).
  :53 keeps serving — the same posture `main.rs` already takes when
  `CertStore::open` fails. A bind failure on 853 stays a startup error naming
  `[dns.listen] dot_port` / `FAH__DNS__LISTEN__DOT_PORT`. There is no
  plaintext path on 853 at all: a non-TLS client receives a TLS alert or a
  close (`plaintext_dns_on_the_dot_port_gets_no_answer`).
- **DoT `ServerConfig` is assembled inside `fah-dns` (`DotTls::new`)** over
  `MintingResolver` with the API pair as `fallback`, provider named
  explicitly as `fah-http`/`fah-certs` do. The binary therefore adds no
  `rustls` edge; `fah-dns → fah-certs` is the edge ARCHITECTURE.md §Dependency
  Layering already names for this listener. `layering.rs` green. No ALPN is
  offered on 853 (RFC 7858 needs none; clients that offer `dot` proceed
  without a selection).
- **DoH rejections are plain-text statuses** (`415` media type, `400`
  empty/undecodable/padded, `413` over 65 535 bytes), all `Cache-Control:
  no-store`, none `application/dns-message` — a rejection can never be
  mistaken for an answer. `doh_enabled = false` registers no route: a GET
  falls to the SPA shell, a POST is refused by it (plan decision 5, pinned by
  test). The auth exemption did not widen: `PUBLIC_PATHS` is unchanged, and
  `/api/v1/dns-query`, `/api/dns-query` answer `401`.
- **`QueryEvent::new` takes `transport` positionally** (17 call sites), no
  serde default: an observed transport has no honest default (same argument
  `fah_model::Protocol` makes) and no code path deserialises rows.

## Measurements

Dev box, x86_64, **release binary** (`FAH_E2E_BINARY` override in
`tests/common/mod.rs`; the test target itself cannot build `--release` because
`fah-api/test-harness` forbids it), loopback client, 2 000 sequential A
queries for a blocked domain (in-engine, no upstream), single run. Diagnostic
only — the RB5009 conversion is the documented ~9× factor, not a device
reading. Reproduce: `cargo build --release -p fastadhunter` then
`FAH_E2E_BINARY=target/release/fastadhunter.exe cargo test -p fastadhunter
--test encrypted_latency -- --ignored --nocapture`.

| Transport | min | p50 | p90 | p99 | max | Added vs UDP (p50) |
| --- | --- | --- | --- | --- | --- | --- |
| UDP/53 | 25 µs | 28 µs | 39 µs | 79 µs | 247 µs | — |
| DoT (one reused connection) | 42 µs | 45 µs | 53 µs | 99 µs | 279 µs | +17 µs |
| DoH POST, HTTP/1.1 keep-alive | 140 µs | 160 µs | 231 µs | 508 µs | 1 377 µs | +132 µs |

| Figure | Value |
| --- | --- |
| DoT handshake (TLS 1.3, self-signed fallback, excluded from the rows above) | 1.91 ms |
| Inferred RB5009 per-query add (×9) | DoT ≈ +0.15 ms, DoH ≈ +1.2 ms |
| DoT per-connection state | one task + one semaphore permit + the shared 2-byte/`Vec` read path of TCP/53; ≤ 64 connections (`DOT_MAX_CONNECTIONS`) |
| DoH per-query allocation | one `Vec<u8>` for the message (POST body copy or GET decode) + the reply `Vec` the pipeline already returns |
| RSS | not measured here; p3-06 soak re-affirms the 128 MB budget with listeners idle |

The DoH client in this measurement negotiated HTTP/1.1: the workspace
`reqwest` carries no `http2` feature. The API `ServerConfig` offers ALPN
`[h2, http/1.1]` (`fah-certs/src/api.rs:189`) and the auto builder serves
both, so RFC 8484's h2 SHOULD is met by construction — but **h2 on the wire is
unexercised by this task's tests** (see Deferred).

## Tests

| Suite | New / changed | What it pins |
| --- | --- | --- |
| `fah-model` `client_transport::tests` | 3 | lowercase spelling, unknown spelling rejected, encrypted set |
| `fah-model` `query_event::tests` | 4 updated | `transport` round-trips; compat JSON names it |
| `fah-config` `tests` | 3 | defaults (on, 853, on); `FAH__DNS__LISTEN__DOT_PORT`; port-clash and zero-port rules, waived when DoT is off |
| `fah-dns` `dot::tests` | 6 | CA-minted leaf on the first handshake; fallback for no-SNI and no-CA; evicted leaf re-minted never fallback; plaintext gets no answer; silent connection dropped at the deadline; 65th connection queues until a slot frees |
| `fah-dns` `server::tests` | 1 | 853 bind failure names the DoT setting |
| `fah-dns` `tests/server_integration.rs` | 2 updated | existing UDP/TCP suites run with DoT off |
| `fah-api` `doh::tests` | 2 | media-type essence match; padded base64url rejected |
| `fah-api` `tests/api.rs` | 4 | POST + GET answer without credentials and pass the peer IP; every RFC rejection; route absent when disabled, everything else serves; `/api/v1/dns-query` stays `401` |
| `fah-api` `wire::tests` | 1 updated | `transport` is in the documented DNS key set |
| `fah-api` `tests/request_coverage.rs` | passes | `requests/dns-query.http` covers the new route |
| `fastadhunter` `tests/e2e.rs` | extended | verdict parity UDP/DoT/DoH for blocked + allowed; WS `transport` = `udp`/`dot`/`doh`; CA generate → export → a client trusting **only** that CA validates the DoT leaf minted for its SNI |
| `fastadhunter` `tests/encrypted_latency.rs` | 1 (`#[ignore]`) | the diagnostic above |

Gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo test --all-features --workspace` — green on this
Windows box (the e2e passed here; the `WSAEACCES` trap in
`docs/project-state.md` remains environmental).

## Known limitations / deferred

| Item | Owner |
| --- | --- |
| DoH over h2 unexercised on the wire (test client is h1-only); Android/browser DoH clients will negotiate h2 — verify on device | p3-06 |
| Android Private DNS hostname mode against a real device (user CA store consulted?) — the e2e proves the CA-route chain with rustls, not with Android | p3-06 |
| `unwarmed_misses` attribution: it now moves only when the resolver misses **without** a preceding mint — no CA installed (every SNI hello), an invalid SNI, or a mint failure. With a CA installed, DoT leaves it at zero. p3-06 reads it with that meaning | p3-06 |
| DoH draws on the API server's 64-permit semaphore (plan decision 4a) — record peak concurrent DoH sessions in the soak | p3-06 |
| Per-query latency budget rows for DoT/DoH — seed above, device figures needed | p3-06 |
| `ApiServer::bind` uses `TcpListener::bind`, not `fah_common::listen`; a `[api] address = "::"` DoH peer arrives v4-mapped and is canonicalised by `Pipeline::handle`, so policy resolution is unaffected — pre-existing, noted | — |
| Arbitrary SNI on 853 mints into the shared 512-entry LRU (p3-01 carry-over; bounded by 64 connections + 10 s deadline); a hostile LAN client could churn p3-04's leaves, which p3-04 re-mints on its own handshake path | soak watch |

## Doc changes proposed (owner approval required — none applied)

1. **CONFIGURATION.md `[dns.listen]`** — after `port = 53`:

   ```toml
   dot_enabled = true            # boot    — DNS-over-TLS listener (RFC 7858)
   dot_port = 853                # boot    — TCP; binds before the privilege drop
                                 #           like 53. Serves a CA-minted certificate
                                 #           for the hostname the client sends when a
                                 #           CA exists (Android Private DNS hostname
                                 #           mode needs the CA installed), else the
                                 #           API certificate
   doh_enabled = true            # boot    — DNS-over-HTTPS (RFC 8484) at
                                 #           https://<api>/dns-query, unauthenticated;
                                 #           false removes the route entirely
   ```

2. **API.md** — new `### /dns-query` (outside `/api/v1`): GET `?dns=`
   base64url-unpadded and POST `application/dns-message`, both `200`
   `application/dns-message` + `Cache-Control: no-store`; `400`/`413`/`415`
   plain text; no auth; absent when `doh_enabled = false`. WS §`/api/v1/events`:
   `transport` (`udp`|`tcp`|`dot`|`doh`) present on every `kind: dns` item,
   absent on HTTP kinds.
3. **SECURITY.md §API access** — "`GET /health` and `POST /api/v1/auth/login`
   are the two exemptions" → "…the two exemptions inside the admin surface;
   `/dns-query` (DoH, outside `/api/v1/`) is unauthenticated because a DNS
   client can present neither a key nor a cookie, and answers only DNS".
   §Later phases: the DoT/DoH listeners bullet to present tense (shipped;
   CA-minted per SNI, API pair as fallback; never plaintext).
4. **ARCHITECTURE.md §Listeners → DNS** — replace the "later phase" bullet
   with: DoT on `[dns.listen] dot_port` (853, bound before the drop, 64
   connections, 10 s handshake deadline, leaf minted per SNI at the
   handshake); DoH as `/dns-query` on the API listener via the
   `DnsWireSource` port. §Dependency Layering Ports table: add
   `DnsWireSource` — declared by `fah-api`, implemented over `fah-dns`'s
   `Pipeline`.
5. **requests/README.md** — list `dns-query.http` (and its `.bin` body).
6. **CONTEXT.md** — one line: *Client transport* — which listener a query
   arrived on (`udp`, `tcp`, `dot`, `doh`); distinct from the upstream
   *Protocol*.
