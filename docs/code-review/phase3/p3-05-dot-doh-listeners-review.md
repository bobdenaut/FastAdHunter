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
  asked to answer no longer exists. Eviction is pinned by
  `an_evicted_leaf_is_re_minted_on_the_next_handshake_never_the_fallback`
  (`dot.rs`); expiry by `fah-certs`
  `leaf::tests::an_expired_entry_is_neither_served_nor_reused` (`prewarm` on
  an expired entry re-mints), composed with the first-handshake test, which
  pins that the listener calls `prewarm` before `resolve`. `fah-dns` has no
  clock injection and CA validity is day-granular, so the expiry case cannot
  be driven through the listener itself.
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
reading. Reproduce: `cargo build --release -p fastadhunter` then, from the
workspace root, `FAH_E2E_BINARY=../../target/release/fastadhunter.exe cargo
test -p fastadhunter --test encrypted_latency -- --ignored --nocapture`. The
override resolves against the test process's working directory, which cargo
sets to `crates/fastadhunter/`, hence the two `..` (an absolute path also
works); the harness refuses a value that is not a file and prints the path it
uses.

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
| `fah-model` `client_transport::tests` | 2 | lowercase spelling, unknown spelling rejected |
| `fah-model` `query_event::tests` | 4 updated | `transport` round-trips; compat JSON names it |
| `fah-config` `tests` | 3 | defaults (on, 853, on); `FAH__DNS__LISTEN__DOT_PORT`; port-clash and zero-port rules, waived when DoT is off |
| `fah-dns` `dot::tests` | 7 | CA-minted leaf on the first handshake; fallback for no-SNI and no-CA; evicted leaf re-minted never fallback; plaintext gets no answer; silent connection dropped at the deadline; 65th connection queues until a slot frees; server-side close is `close_notify`, not a bare FIN (S1) |
| `fah-dns` `server::tests` | 1 | 853 bind failure names the DoT setting |
| `fah-dns` `tests/server_integration.rs` | 2 updated, 1 new | existing UDP/TCP suites run with DoT off; `dot_enabled = false` leaves the port unbound, `true` holds it (S2) |
| `fah-api` `doh::tests` | 2 | media-type essence match; padded base64url rejected |
| `fah-api` `tests/api.rs` | 4 | POST + GET answer without credentials and pass the peer IP; every RFC rejection; route absent when disabled, everything else serves; `/api/v1/dns-query` stays `401` |
| `fah-api` `wire::tests` | 1 updated | `transport` is in the documented DNS key set |
| `fah-api` `tests/request_coverage.rs` | passes | `requests/dns-query.http` covers the new route |
| `fastadhunter` `tests/e2e.rs` | extended | verdict parity UDP/DoT/DoH for blocked + allowed; WS `transport` = `udp`/`dot`/`doh`; a policy assigned to the client IP is attributed to its DoT and DoH queries in `GET /api/v1/stats` `policies` (S2); CA generate → export → a client trusting **only** that CA validates the DoT leaf minted for its SNI |
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

## Doc changes (proposed in the summary, approved as N10, applied — see §Fixes applied)

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

## Findings

Reviewed commit `6c6e66ae98a8b57e02189335c267bbb73c859675` against
`p3-05-dot-doh-listeners.md` and `p3-05-dot-doh-listeners-plan.md`. Diff
inspected file by file; callers/implementations of every changed symbol
searched. No sub-agents. Severity: **blocker** / **should-fix** / **note**
(plan/CLAUDE.md mapping: blocker = Critical, should-fix = Major/Minor,
note = Minor/Nitpick). Evidence is from the tree unless marked *inference*.

### Blockers

None.

### Should-fix

**S1 — DoT closes without `close_notify`; every idle close is an unclean TLS
close to the client.**
`crates/fah-dns/src/dot.rs:140` returns
`tcp::handle_connection(stream, …).await` directly; `handle_connection`
(`tcp.rs:90`) takes the stream by value and returns `Ok(())` on the idle
timeout (`tcp.rs:99`, `:110`) and on the pipeline's `None` without calling
`shutdown()`. For TCP/53 a bare FIN is the protocol; for TLS a FIN without
`close_notify` is a truncation the peer's TLS layer reports as an error
(rustls: `UnexpectedEof`, "peer closed connection without sending TLS
close_notify"). Android's DoT client reconnects, so this is not a functional
break, but the primary target client will log every 10 s idle close as a
read error and the plan's "connection reuse; idle timeouts" contract
(decision 2, RFC 7858 §3.4) is met only half-way. *Inference* on the
Android-side logging; the missing `shutdown()` is read from the code.
Remediation: keep `handle_connection` generic, pass `&mut stream`, then
`let _ = stream.shutdown().await;` after it returns (`&mut S` satisfies the
same bounds; no allocation, one extra write on close only). Fix before DONE;
`the_connection_bound_…` test is unaffected because the client side still
drops without `close_notify`.

**S2 — Two plan-listed integration tests are missing.**
Plan §Tests/Integration requires (a) "policy resolution uses the TLS peer
address: a per-client policy assigned to the test client IP applies over DoT
and DoH", and (b) "`dot_enabled = false` ⇒ nothing listens on 853 (connect
refused)". Evidence: `crates/fah-dns/tests/server_integration.rs:84-85` and
`:326-327` boot with `dot_enabled: false` but assert nothing about
`dot_addr()` or the port; `crates/fastadhunter/tests/e2e.rs` has no
per-client policy on the DoT/DoH legs. The DoH peer IP is pinned in
`fah-api/tests/api.rs` (`…names_the_peer`) and the DoT peer is `client.ip()`
at `dot.rs:140` by construction, so the risk is low — but the plan named
these and the summary's test table does not list them as deferred.
Remediation: one `server.dot_addr().is_none()` assertion plus a
connect-refused probe in `server_integration.rs`; one DoT + one DoH query
under a per-client policy in `e2e.rs`. Fix before DONE or record as deferred
explicitly.

### Notes

| # | Where | Evidence | Why it matters | Direction | Fix / defer |
| --- | --- | --- | --- | --- | --- |
| N1 | `dot.rs:122-146` | `prewarm()` runs `spawn_blocking` on **every** SNI hello when a CA exists, hit or miss; `host.to_owned()` (`:123`) + `logged = host.clone()` (`:145`) = two `String`s per handshake | A cache hit is a mutex read; the blocking-pool hop costs a thread wake per reconnect (Android reconnects after each 10 s idle close). Handshake path only, never the query path — *inference* on magnitude, not measured | Check `store.cached_leaf(host).is_some()` synchronously first; only a miss → `spawn_blocking`. Drop the second `String` by returning the host from the closure | defer to p3-06 unless S1 is being touched anyway |
| N2 | `fah-api/src/routes.rs:124`, `doh.rs:85` | Rejections built by axum extractors (413 from `DefaultBodyLimit`, 400 from a missing/invalid `?dns=`) carry no `Cache-Control: no-store`; only `reject()` sets it | The summary's "all `Cache-Control: no-store`" is overstated. Harmless: neither status is heuristically cacheable and none is `application/dns-message` (test asserts that) | A `SetResponseHeaderLayer::overriding(CACHE_CONTROL, no-store)` on the route, or correct the summary sentence | defer |
| N3 | `server.rs:129-134`, `main.rs:715` | A cert failure closes 853 with an `error!` line only; `dot_addr()` becomes `None` after `serve`, but nothing reads it — `/health` and `/api/v1/certificates` do not know DoT is closed | A default-on listener that silently disappears at boot is discoverable only in the container log. Explicit-failure rule (plan Step 5) is met at the log level, not the API level | Surface `dot: closed/listening` in `GET /api/v1/certificates` status or `/health` `checks` — p3-06 call, out of this task's scope | defer to p3-06 |
| N4 | `dot.rs:62-93` vs `tcp.rs:46-72` | Accept + `RetryPolicy` loop copied verbatim except the semaphore | Plan Step 4 authorised the copy ("skeleton copied from `tcp::run`"); principle 4 still applies — two places to fix the next accept-loop bug | A shared `accept_with_backoff(&listener, &mut policy) -> Result<(S, SocketAddr), ListenerDied>` would remove ~20 lines; optional | defer |
| N5 | `fah-model/src/client_transport.rs:22` | `ClientTransport::is_encrypted` has no caller outside its own tests | Principle 14: unjustified API surface | Remove, or keep once a caller exists (WS/dashboard filter) | defer |
| N6 | `server.rs:30`, `:46` | Doc comments still say "Owns the UDP + TCP listener tasks" and "Binds both listeners" | Stale after DoT; hard rule 7 forbids adding comments, not fixing wrong ones | Trim to accurate wording or delete | with S1 |
| N7 | `dot.rs` tests | Summary claims "both hazards [expiry + eviction] are pinned by `an_evicted_leaf_…`"; that test exercises eviction only. Expiry re-mint holds by construction: `LeafCache::prewarm` → `lease` → `take_fresh` (`leaf.rs:262-275`) drops an entry with `not_after <= now` and mints | Claim is stronger than the test. The plan's expiry test (Step 5) was not carried over | Add the expiry case (`fah-certs` unit tests show the clock pattern) or narrow the claim | defer |
| N8 | `dot.rs:122-127`, defaults | With a CA installed, **every** LAN client can drive a mint per handshake for any SNI (default-on 853); p3-04 confined SNI-driven minting to interception-listed clients | Scope of the p3-01 "remote-input CPU amplifier" carry-over widened from opt-in clients to the whole LAN. State is bounded (512-entry LRU); CPU is bounded by 64 connections × one P-256 mint (≈0.5 ms RB5009 by the documented factor) — *inference*, not measured on device | Already in the Deferred table; add the "all clients" wording so p3-06's soak watches mint rate, not only LRU churn | defer to p3-06 soak |
| N9 | `doh.rs:50` | `Vec::from(body)` copies the `Bytes` once per POST so the boxed `'static` future can own it | One ≤ 65 535-byte memcpy per DoH query; the port could take `bytes::Bytes` for zero-copy at the cost of a `bytes` type in `fah-api`'s public port | Not worth the coupling at household scale; keep | none |
| N10 | task file §Acceptance | "CONFIGURATION.md + API.md updated in the same change" — six doc edits are proposed, none applied | Working agreement forbids `.md` edits without a yes; the criterion is open until the owner approves | Owner applies/approves the listed edits | deferred — owner gate |

### Plan compliance — checked

| Unit / criterion | Status | Evidence |
| --- | --- | --- |
| Step 1 config keys, defaults, env, validation | done | `listen.rs`, `env.rs:47-52`, `lib.rs:133-150`; tests |
| Step 2 `ClientTransport`, `QueryEvent.transport`, WS key on `kind: dns` only | done | `client_transport.rs`, `query_event.rs`, `wire.rs:152-153, :176-183` |
| Step 3 `Transport::{Dot,Doh}`, `u16::MAX` budget, single mapping site | done | `pipeline.rs:35-50, :313-315, :503-512` |
| Step 4 own accept loop, permit before accept, 64 / 10 s consts, handshake inside the task, `handle_connection(transport)`, tokio-rustls pinned | done | `dot.rs:22-24, :62-93, :98-141`; `fah-dns/Cargo.toml` |
| Step 5 bind 853 before the drop, `dot_addr()`, `serve(_, Option)`, supervision via `fatal_tx`, `bind_error` naming `DOT_PORT_SETTING` | done | `server.rs:68-78, :103-138`; `a_dot_bind_failure_names_the_dot_port_setting` |
| Step 5 `MintingResolver` + API pair fallback via `api_certified_key()` | done | `dot.rs:33-45`, `main.rs:729` |
| Step 5 pre-warm at boot + 24 h re-warm ticker + LRU-pressure test | **deviation, documented** | replaced by handshake-time mint (`dot.rs:122-127`); eviction test present, expiry test absent (N7). Judged correct: the binary knows no hostname, so the ticker had no input; the invariant "fallback never served while a CA exists for a valid SNI" holds per handshake |
| Step 5 `api.tls = false` + DoT: interrupted-replacement healer for the fallback reader | done, third option | `main.rs:478` runs `load_or_generate` when either is on, so `complete_interrupted_replacement` (`fah-certs/src/api.rs:48`) precedes `api_certified_key()`; no second healer |
| Step 5 "DoT startup must fail explicitly" | **reading chosen: listener fails, resolver survives** | `main.rs:715-761` + `server.rs:129-134`: `error!` + 853 closed, :53 unaffected, never plaintext. Consistent with the existing `CertStore::open` posture in `main.rs`; N3 for the observability gap |
| Step 6 port trait, `AppState.doh`, route outside `/api` and outside auth, body cap, GET/POST rules, `no-store` | done | `ports.rs:117-121`, `state.rs:30`, `routes.rs:119-126` (route added after the auth `.layer`, so unwrapped), `doh.rs` |
| Step 6 binary implements the port over `Arc<Pipeline<F>>` | done | `adapters.rs:183-202` |
| Step 7 no new channel/counters | done | `pipeline.rs` only threads the field |
| Decision 4b "record h2 as verified, not assumed" | **not met** | test client is h1-only; the summary says so and defers to p3-06 — acceptable, but a plan miss, not a verification |
| Decision 3 `ServerConfig` built by the binary | deviation, documented | `DotTls::new` in `fah-dns`; keeps `rustls` out of the binary. Accepted: the edge `fah-dns → fah-certs` is the one ARCHITECTURE.md line 241 names |
| Out of scope (HTTP/3, DoQ, padding, TTL `max-age`, per-transport counters, router config) | respected | none present |
| Acceptance: parity UDP/DoT/DoH | met | `e2e.rs:125-155` blocked + allowed over all three, WS `transport` per leg |
| Acceptance: Android setup documented | text proposed (CONFIGURATION.md caveat); walkthrough is p3-06 | as planned |
| Acceptance: docs in the same change | open (N10) | owner gate |
| Acceptance: gates green | claimed in the summary, not re-run here | — |

### Correctness — checked, acceptable except S1

- Handshake deadline covers the ClientHello read **and** the completion (`timeout_at(deadline, …)` twice, `dot.rs:106, :130`); a slow mint between them cannot extend the total because the second `timeout_at` fires immediately. A timeout drops the acceptor future, which owns the socket — no fd leak.
- Plaintext on 853: `LazyConfigAcceptor` writes the alert then errors; pinned by test.
- Permit lifecycle: acquired before `accept`, dropped on the accept-error `continue`, moved into the task; matches `fah-api/src/server.rs:107-135`.
- Fatal path: `RetryPolicy` → `ListenerDied` → `fatal_tx` — identical to TCP/53 (p2.5-01 template, not the GAR anti-pattern).
- A client dropping without `close_notify` surfaces as `UnexpectedEof` → `Ok(())` in `handle_connection` → slot freed (the bound test depends on this and passes).
- Fallback semantics: no SNI, no CA, invalid SNI, mint error → API pair; never plaintext, never "no certificate".
- `MintingResolver::resolve` (`leaf.rs:331`) is a lock-read only; the mint is on the blocking pool as p3-01 M5 requires.
- DoH: `DefaultBodyLimit::max(65535)` makes the `len() > MAX` check in `answer()` reachable only for GET; both orders yield 413/400 as tested. The `state.doh == None` branch (`doh.rs:55`) is unreachable (route not registered) — harmless.
- v4-mapped DoH peers are canonicalised in `Pipeline::handle` (`pipeline.rs:305`), so policy lookup matches UDP.

### Architecture — checked, acceptable

Layering: `fah-dns → fah-certs` (L3 → L2) is named in ARCHITECTURE.md and enforced by `layering.rs` (`fah-certs` at L2). `fah-api` ↔ `fah-dns` cross only through `DnsWireSource`, implemented in the binary. No sibling import. New public surface: `DotTls`, `DOT_MAX_CONNECTIONS`, `Transport::{Dot,Doh}`, `DnsWireSource`, `WireResolving`, `ClientTransport` — all required by the wiring except `is_encrypted` (N5). Release dependency delta is manifest-only (already-linked crates).

### Performance — checked, acceptable

UDP/TCP query path: one extra `Copy` enum parameter and an `Into` at the event-build site; no allocation, no lock, no branch on the UDP decode path. DoT per query: the TCP/53 loop unchanged. DoT per handshake: N1. DoH per query: one `Vec` (POST copy or GET decode) + the reply `Vec` the pipeline already returns (N9). Figures in the summary are dev-box release, single run, in-engine — diagnostic only, as labelled.

### Memory — checked, acceptable

Per DoT connection: task + permit + the TCP read buffers; ≤ 64. Per handshake: two transient `String`s (N1). Leaf state bounded by the shared 512-entry LRU (N8 for churn). `QueryEvent` grows by one byte-class field (padding-absorbed or +8 B per in-flight event; events are not retained). No timer, no new long-lived allocation, no growth with uptime.

### Rust quality — checked, acceptable

No `unwrap`/`expect`/`panic` outside tests. Cancellation-safe: aborting a connection task drops the TLS stream; an in-flight `spawn_blocking` mint completes on its own and is bounded. `Send`/`'static` bounds satisfied without extra `Arc` clones beyond the two per accept. `match bool` in `server.rs:68` / `main.rs:743` is style only.

### Tests — checked; gaps in S2, N7

Unit + integration suites pin: media-type essence, unpadded base64url, 413/400/415 matrix, route absence when disabled, auth-exemption width, CA-minted leaf on first handshake, fallback on no-SNI/no-CA, eviction re-mint, plaintext rejection, silent-connection timeout, 65th-connection queueing, config defaults/env/collision, WS `transport` key, e2e parity + CA-trusting client. Timing assertions are lower-bound only (`>= 250 ms` against a 300 ms timeout) — not flaky by construction. `encrypted_latency.rs` is `#[ignore]`d and opt-in via `FAH_E2E_BINARY`.

### Regression — checked, acceptable

`server_integration.rs` runs the UDP/TCP suites with `dot_enabled: false`; every binary e2e that boots with a config sets `dot_port` to an ephemeral port (`e2e.rs`, `healthcheck.rs`, `http_e2e.rs`, `encrypted_latency.rs`; `history_e2e.rs` does not boot the binary), so no test depends on binding 853. Existing WS keys byte-stable, `transport` added (`wire.rs` key-set test). `PUBLIC_PATHS` unchanged. p3-04 interaction: shared LRU + `unwarmed_misses` meaning shift are recorded for p3-06.

### Fixes applied (owner-approved: S1, S2)

| Finding | Change | Verification |
| --- | --- | --- |
| S1 | `dot.rs` `serve_connection`: `handle_connection(&mut stream, …)`, then `timeout(tcp::TCP_IDLE_TIMEOUT, stream.shutdown())` — `close_notify` on every server-side close (idle, malformed frame, pipeline `None`), bounded by the same idle clock so a client that stops reading cannot pin the permit; `TCP_IDLE_TIMEOUT` is `pub(crate)`. TCP/53 unchanged (a FIN is its clean close). | new `a_finished_connection_is_closed_with_close_notify_not_a_bare_fin`: framed garbage → pipeline `None` → client `read` returns `Ok(0)`; without the fix rustls reports `UnexpectedEof`. `dot::tests` 7/7 green |
| S2 (a) | `server_integration.rs` `a_disabled_dot_listener_binds_nothing_on_its_port`: `dot_enabled = false` → `dot_addr()` is `None` and a fresh bind of `dot_port` succeeds; `dot_enabled = true, dot_port = 0` → `dot_addr()` is `Some` and a second bind of it fails. A connect-refused probe was tried first and dropped: this Windows box swallows SYNs to closed loopback ports (firewall stealth; >2 s elapsed) — environmental, same class as the WSAEACCES trap | green |
| S2 (b) | `e2e.rs`: `POST /api/v1/policies` `{id: "phone", assignments: [{client: "127.0.0.1"}]}`, then one DoT and one DoH blocked query, then `await_stats` until `policies[phone]` shows `queries ≥ 2, blocked ≥ 2`. Stats attribution is the observable: WS DNS items carry no `policy` key, and a per-policy `blocking_mode` is inert (CONFIGURATION.md), so the wire answer cannot differ by policy today | green (3.1 s) |

Gates after the fixes: `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --all-features --workspace` — green
on the Windows dev box.

### Second round (owner-approved: N6, N7, N10; N3 + N8 carried into p3-06)

| Finding | Change | Verification |
| --- | --- | --- |
| N6 | `server.rs`: the two stale doc blocks (`Server` "Owns the UDP + TCP listener tasks…", `bind` "Binds both listeners…") deleted. Trimming was not an option — `.claude/hooks/no-rust-comments.sh` rejects any edit that writes a comment line, and hard rule 7 makes deletion the compliant direction. The ADR-0004 bind/serve rationale lives in ADR-0004 and ARCHITECTURE.md §Listeners; the "call `shutdown` explicitly" note is `Server::shutdown`'s own contract | fmt, clippy `-D warnings`, `cargo test -p fah-dns` green |
| N7 | Claim narrowed in §Decisions: eviction pinned in `dot.rs`, expiry pinned by `fah-certs` `an_expired_entry_is_neither_served_nor_reused` (`leaf.rs:556`), composition pinned by the first-handshake test. No `fah-dns`-level expiry test is possible: `unix_now()` is not injectable from outside `fah-certs` and `CaParams.validity_days` is day-granular | inspection |
| N10 | Six doc edits applied as listed in §Doc changes: CONFIGURATION.md `[dns.listen]` three keys; API.md §Authentication wording, new §DNS over HTTPS (`/dns-query` contract), §Events `transport` key; SECURITY.md §API access exemption sentence and §Later phases DoT/DoH bullet to present tense; ARCHITECTURE.md §Listeners DNS bullets and the Ports table (`DnsWireSource`, "three of these"); `requests/README.md` row for `dns-query.http` + `.bin`; CONTEXT.md `### Client Transport` | `cargo test -p fah-api --test request_coverage` green (README claims vs routes) |
| N3, N8 | Carried into `plan/wip/phase3/p3-06-phase3-verification-plan.md`: N3 as a "p3-05 carry-over" block in Step 2 (DoT closed-at-boot observability + the closed-posture assertion for suite item 6); N8 as the "Mint-rate watch" beside the third soak watch item in Step 4 item 6, whose `unwarmed_misses` text now states p3-05's shipped semantics (handshake-time mint, no re-warm ticker) | — |

### Third round (owner-approved: N5; N2, N4, N9 closed as won't-fix)

| Finding | Outcome |
| --- | --- |
| N5 | `ClientTransport::is_encrypted` and its test removed (`client_transport.rs`); no caller existed. fmt, clippy `-D warnings`, `cargo test -p fah-model` green |
| N2 | won't-fix: 413/400 are not heuristically cacheable and never `application/dns-message`; a header layer is per-request work for no effect. API.md §DNS over HTTPS states the contract as shipped |
| N4 | won't-fix: ~20 duplicated lines in the p2.5-01 resilience template; a shared helper is a refactor with no measured benefit (principle 16). Revisit if a third identical accept loop appears |
| N9 | won't-fix: one ≤ 64 KiB memcpy per DoH POST vs a `bytes` type in `fah-api`'s public port — the coupling costs more than the copy |

### Status

**PASS WITH DEFERRED FINDINGS** — S1, S2, N5, N6, N7, N10 fixed and
verified; N2, N4, N9 closed won't-fix; N3 and N8 carried into the p3-06 plan;
N1 deferred to p3-06 (profile before touching).

## Second review — commits `6c6e66a` + `dc9bf3d` (2026-09-02)

Both commits reviewed as one changeset against `p3-05-dot-doh-listeners.md`
and `-plan.md`: `git diff --stat` first, diff file by file, callers of every
changed symbol searched, ranges read. No sub-agents. Gates re-run on the
Windows dev box: `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test -p fah-dns -p fah-api -p fah-config
-p fah-model`, `cargo test -p fastadhunter --test e2e --test layering` — all
green; the round-one fixes (S1, S2, N5, N6, N7, N10) are verified in the tree.
Numbering continues from the first round. Evidence is from the tree unless
marked *inference*.

### Blockers

None.

### Should-fix

**S3 — API.md §Certificates contradicts the shipped `api.tls = false` + DoT
behaviour.** `API.md:1386-1388`: "With `api.tls = false` the import is still
accepted and stored, but no restart loads it: the pair waits in `/config`
until TLS is enabled." Since this change `main.rs:478` runs
`load_or_generate_tls` whenever `config.api.tls || config.dns.listen.dot_enabled`,
and `dot_tls` (`main.rs:729`) reads the pair through `api_certified_key()` as
the DoT fallback — with the default `dot_enabled = true` a restart *does* load
an imported pair, onto 853. CLAUDE.md: a change that contradicts a doc updates
it in the same change; the six edits in `dc9bf3d` missed this sentence.
Remediation: one clause — "…until TLS is enabled; with `dot_enabled = true`
the next restart loads it as the DoT fallback certificate". Fix before DONE
(doc edit, owner approval).

**S4 — Two readers of the API pair, two failure postures; the summary states
only one.** §Decisions: "Certificate failure degrades DoT, never the resolver
… an unloadable API pair … logs `error!` and closes the 853 socket. :53 keeps
serving." True of `api_certified_key()` (`main.rs:729-737`), not of the reader
that runs first: `main.rs:478-484` `load_or_generate_tls(...)?` fails the whole
boot on a corrupt or half-present pair (`CertError::Config` /
`IncompletePair`, `fah-certs/src/api.rs:44-49`). Pre-existing for
`api.tls = true`; **new** for `api.tls = false`, where nothing read the pair
before and DoT is default-on — an operator who disabled API TLS around a
broken pair now loses the resolver at boot, and the error names the pair
files, not `[dns.listen] dot_enabled`. The plan permits explicit failure
(rule 11) but says "only … DoT fail startup"; the summary and CONFIGURATION.md
say DoT degrades. The corrupt-pair boot is *inference* from the `?`, not
executed. Remediation, smallest: with `api.tls = false` run
`load_or_generate_tls` inside `dot_tls`'s degrade path (it is already on the
blocking pool) so an error closes 853 and :53 serves — both readers then share
the posture the summary documents. Otherwise keep the boot failure and
correct §Decisions + CONFIGURATION.md `dot_port` to say so. Fix before DONE.

### Notes

| # | Where | Evidence | Why it matters | Direction | Fix / defer |
| --- | --- | --- | --- | --- | --- |
| N11 | `dot.rs:127-129` | `prewarm(..).await` sits between the two `timeout_at(deadline, …)` windows, under neither | The handshake *completion* cannot outlive the deadline (the second `timeout_at` fires at once), but the permit and task are held for the blocking-pool wait + mint on top of it — §Correctness "cannot extend the total" is true of the handshake, not of the slot. Bounded: 64 permits, per-host single-flight in `fah-certs`, one P-256 mint (≈ 0.5 ms RB5009 by the documented factor — *inference*) | `timeout_at(deadline, prewarm(..))` frees the slot sooner; the blocking mint runs to completion either way. Or narrow the wording | defer to p3-06 with N1 |
| N12 | `server_integration.rs:90-110` | probe bind → `drop(probe)` (`:95`) → `Server::bind` → re-bind the same port (`:106`) | TOCTOU: a parallel test process can take the freed ephemeral port between `:95` and `:106`; the assertion then fails for the wrong reason | Hold the probe listener and assert `Server::bind` with `dot_enabled = false` succeeds while the port is taken — race-free and strictly stronger | defer |
| N13 | `routes.rs:119` | `/dns-query` registers on `state.doh.is_some()` only; `state.tls` is not consulted | With `api.tls = false` the route answers over plaintext HTTP. Exposure equals :53 (LAN, DNS only, no admin reach), but RFC 8484 is HTTPS-only, no real client uses it, and SECURITY.md §Later phases says "DoH rides the API listener's certificate" — there is none. Nothing logs it | Gate on `state.tls` (route absent, same invariant as `doh_enabled = false`) or one sentence under CONFIGURATION.md `doh_enabled` | owner call |
| N14 | `server.rs:1-2` | module doc: "Binds `[dns.listen]`'s UDP and TCP sockets" | Stale after DoT; N6 deleted the two item-level blocks, the module-level one remains | Delete (rule 7: deletion is the compliant direction) | with S3/S4 |
| N15 | `plan/wip/phase3/CLAUDE.md:24` | row 5 still `WAITING` | Every prior DONE commit in this phase flipped its row in the same commit (`8389503`, `49c6791`, `3a856ef`, `094f167`); neither p3-05 commit did | Owner flips to DONE when this review closes | owner gate |
| N16 | `tests/common/mod.rs:110` | `FAH_E2E_BINARY` overrides the binary for every e2e, silently | A stale value left in the shell makes the whole suite test the wrong build with no trace in the output | `eprintln!` the chosen path when the override is active, or read it only in `encrypted_latency.rs` | defer |
| N17 | `e2e.rs:193-215` | CA generated via the API, then a client trusting only that CA validates the DoT leaf — **no restart** | Live CA activation on DoT is a shipped property (`MintingResolver` reads the CA slot per handshake) that no doc states; CONFIGURATION.md `dot_port` says only "when a CA exists", API.md §Certificates documents restart semantics for the pair alone | One clause on the CA generate/import endpoints: "takes effect on the next DoT handshake" | with S3 |

### Plan compliance — re-checked

| Item | Status | Evidence |
| --- | --- | --- |
| Round-one fixes S1, S2(a), S2(b), N5, N6, N7, N10 | verified in tree | `dot.rs:141-142` `&mut stream` + bounded `shutdown()`; `a_finished_connection_is_closed_with_close_notify_not_a_bare_fin`; `server_integration.rs:90`; `e2e.rs` policy leg + `await_stats`; `client_transport.rs` has no `is_encrypted`; `server.rs` item docs gone; six doc edits in `dc9bf3d` |
| Steps 1–7, decisions 1–8 | as in the first round | no new deviation in `dc9bf3d` |
| Acceptance "CONFIGURATION.md + API.md updated in the same change" | met with one gap | S3 |
| Acceptance "gates green" | re-run, green | header above |
| Phase table row | not flipped | N15 |

### Correctness — checked, acceptable (S4, N11)

- `ConnectInfo` for DoH is real: `fah-api/src/server.rs` builds
  `into_make_service_with_connect_info::<SocketAddr>()` per connection; the
  peer is pinned by `dns_query_answers_post_and_get_without_credentials_and_names_the_peer`.
- `has_ca()` (`store.rs:311` → `lock_ca().clone()`) is one uncontended mutex
  read + `Arc` clone per SNI hello; `prewarm` re-locks — handshake path only.
- Permit dropped on the accept-error `continue` (`dot.rs:71-88`); `RetryPolicy`
  use identical to TCP/53.
- Close path: `handle_connection` `Ok` and `Err` both reach `shutdown()`; a
  dead peer bounds it at `TCP_IDLE_TIMEOUT`.
- Live CA activation on DoT (N17) is correct by construction:
  `MintingResolver::resolve` → `cached_leaf`, and `prewarm`, read the CA slot
  per call.

### Architecture — checked, acceptable

Layering unchanged from round one; `layering.rs` green here
(`fah-certs` at L2, `fah-dns → fah-certs` is the ARCHITECTURE.md-named edge).
No new abstraction, dependency or public symbol in `dc9bf3d`.

### Performance — checked, acceptable

UDP/TCP query path unchanged since round one (`Copy` enum + `Into` at the
event-build site). DoT: one `close_notify` write per connection end (S1), none
per query. DoH unchanged.

### Memory — checked, acceptable

No new retained state in `dc9bf3d`. Per DoT connection: task + permit + TLS
session, ≤ 64. rustls' session cache is its bounded default (256), the same
posture as `fah-certs/src/api.rs`.

### Rust quality — checked, acceptable

`serve_connection` moves `tls.config` after borrowing `tls.store` — a valid
partial move (`DotTls` has no `Drop`). No `unwrap`/`expect`/`panic` outside
tests. Aborting the listener task drops the socket; connection tasks are
detached exactly as TCP/53's are (pre-existing pattern, not widened).

### Tests — checked; N12, N16

Round-one suites plus the fix tests all green here: `fah-dns` 115 (7 in
`dot::tests`), `fah-api` 208, `fah-config` 71, `fah-model` 57, e2e 3.1 s,
`layering` 1. Timing assertions remain lower-bound only.
`encrypted_latency.rs` stays `#[ignore]`d.

### Regression — checked, acceptable (S4)

The one behavioural change outside the task's stated scope is the
`api.tls = false` boot now depending on the API pair (S4). `PUBLIC_PATHS`
unchanged (`auth.rs:11`); WS key set pinned; UDP/TCP suites run with DoT off.

### Status

**PASS WITH DEFERRED FINDINGS** — S3 and S4 to fix before DONE (one doc
clause; one posture choice with a small code option); N11–N17 recorded,
N13 is an owner call, N15 is the owner's row flip.

### Fixes applied — second review (owner-approved: S3, S4)

| Finding | Change | Verification |
| --- | --- | --- |
| S3 | `API.md` §Certificates import paragraph: with `api.tls = false` the API listener never loads the pair, but `[dns.listen] dot_enabled = true` (default) loads it at the next restart as the DoT fallback; only with both off does the pair wait in `/config` | inspection against `main.rs` |
| S4 | `main.rs` `Engine::start`: `load_or_generate_tls` still runs when either `api.tls` or `dot_enabled` is on, but its error is fatal only when `api.tls = true` (the API server needs the pair — pre-existing posture). With `api.tls = false` the error is logged once, naming `[dns.listen] dot_enabled` as the reason the pair was needed, `dot_pair_loaded` is `false`, `dot_tls` is skipped and `Server::serve` closes 853; :53 serves. Both readers of the pair now share the posture §Decisions documents. **Not test-pinned:** the branch lives in `main.rs`, and the e2e harness readies on `https://` so an `api.tls = false` boot cannot be driven through it — verified by inspection and the gates | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` (binary crate re-checked after `touch`), `cargo test -p fastadhunter` (e2e, healthcheck, http_e2e, layering, mode_sync) — green |

### Status after the fixes

**PASS WITH DEFERRED FINDINGS** — S3, S4 fixed and verified; N11–N17
recorded (N13 owner call, N15 row flip, the rest deferred).

### Third round — second review (owner-approved: N13, N14, N15, N17)

| Finding | Change | Verification |
| --- | --- | --- |
| N13 | Runtime gate, not validation (a validation error would fail every existing `api.tls = false` config on upgrade, `doh_enabled` being default-on). `routes.rs`: `/dns-query` registers only when `state.tls && state.doh.is_some()` — route absent, the same invariant `doh_enabled = false` gives. `main.rs`: one boot `warn!` when `doh_enabled = true` and `api.tls = false`, naming both keys. CONFIGURATION.md `doh_enabled` and API.md §DNS over HTTPS state it | new `dns_query_is_absent_without_tls_because_doh_is_https_only` (`fah-api/tests/api.rs`): plain-HTTP harness with the port wired — GET and POST never answer `application/dns-message`, the fake pipeline sees no query, `/health` and `/api/v1/stats` still serve |
| N14 | `server.rs` module doc deleted | `cargo test -p fah-dns` green |
| N15 | `plan/wip/phase3/CLAUDE.md` row 5 → `DONE` | after the gates below |
| N17 | API.md `POST /api/v1/certificates/ca/generate`: DoT picks the new authority up on its next handshake, no restart (unlike an API-pair import). No CA *import* endpoint exists — `POST /api/v1/certificates/import` replaces the API pair only, whose restart semantics S3 covers | inspection; the e2e CA-generate → DoT leg already pins the behaviour |

Gates after this round: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets -- -D warnings`, `cargo test -p fah-api -p fah-dns
-p fastadhunter` (incl. e2e, healthcheck, http_e2e, layering,
request_coverage) — green on the Windows dev box.

### Fourth round — second review (owner-approved: N12, N16)

| Finding | Change | Verification |
| --- | --- | --- |
| N12 | Oracle checked first: `fah_common::listen::bind_tcp` sets no `SO_REUSEPORT`; `SO_REUSEADDR` only on Unix (tokio's own default), which still refuses a second *listening* socket on one addr:port — so "bind fails ⇔ port held" holds on Linux, macOS and Windows. Test rewritten: the probe socket is **held** through the assertions. (1) `dot_enabled = true` on the held port ⇒ `Server::bind` fails and the error names `DoT` — the oracle is proven on the platform running the test, not assumed; (2) `dot_enabled = false` on the same held port ⇒ `Server::bind` succeeds, `dot_addr()` is `None`; (3) `dot_enabled = true, dot_port = 0` ⇒ `dot_addr()` is `Some` and a second bind of it fails. No drop window, no sleeps, no retries | `cargo test -p fah-dns --test server_integration` 11/11 green |
| N16 | `tests/common/mod.rs` `binary_under_test()`: unset ⇒ `CARGO_BIN_EXE_fastadhunter` as before. Set ⇒ `std::path::absolute`, `is_file()` required, otherwise a panic naming `FAH_E2E_BINARY`, the resolved path and the working directory it resolved against; the path is printed once per test process (`Once`). No fallback when the override is set. §Measurements reproduce line corrected: cargo runs the test process in `crates/fastadhunter/`, so the relative value is `../../target/release/fastadhunter.exe` | default: `healthcheck` 5/5 green, nothing printed. `FAH_E2E_BINARY=../../target/release/fastadhunter.exe` ⇒ "override active: E:\FastAdHunter\target\release\fastadhunter.exe", 5/5 green. `nope.exe` ⇒ panic "resolves to …\crates\fastadhunter\nope.exe, which is not a file; … working directory, E:\FastAdHunter\crates\fastadhunter". Empty ⇒ panic "cannot make an empty path absolute" |

### Final status

**PASS WITH DEFERRED FINDINGS** — S1–S4, N5, N6, N7, N10, N12, N13, N14, N15,
N16, N17 fixed and verified; N2, N4, N9 won't-fix; N3, N8 in the p3-06 plan;
N1, N11 deferred to p3-06 (profile before touching).
