# p3-04 — TLS Interception — Review

**Task:** `plan/wip/phase3/p3-04-tls-interception.md` · **Plan:**
`p3-04-tls-interception-plan.md` · **Status:** reviewed 2026-09-02 —
**PASS WITH DEFERRED FINDINGS**; the five "Before DONE" items were applied
the same day (§Fixes applied), gates green. Branch `phase3-04`, base
`49c6791`.

## Implementation Summary

A listed client whose SNI is not excluded takes a **terminate** leg instead of
p3-03's splice: the upstream is TLS-verified first (webpki roots, `ServerName`
= SNI, socket = the egress-approved IP), the leaf is pre-warmed, our handshake
is accepted over the replayed ClientHello, and hyper (auto h1/h2) serves the
decrypted stream through the **same** `judge` → `block::response` → `emit`
core the plaintext proxy runs. Allowed requests are forwarded over the one
verified upstream session; bodies stream. Every other client and every
excluded SNI splices exactly as before.

| Crate | Change |
| --- | --- |
| `fah-config` (L1) | `InterceptionConfig { clients, exclude_domains }` as `[https.interception]`, default all-empty; two tests |
| `fah-model` (L1) | `Event::Https` / `EventKind::Https`, wire `https`; additive, round-trip test extended |
| `fah-http` (L3) | **new** `tls.rs` (server/client `rustls` configs, `RewindStream`, `connect_verified_upstream`), **new** `exclusions.rs` (`ExclusionSet` + `BASELINE_EXCLUSIONS`), **new** `intercept.rs` (`Interception`, the terminate leg, `Upstream` bridge); `https.rs` gains the branch, `hello_timeouts`, generic `Activity`; `proxy.rs` verdict core extracted as `pub(crate)` free functions; `absolute_url` takes a scheme |
| `fah-api` / `fah-dns` (L3) | one more match arm each |
| `fastadhunter` (L4) | `interception()` builder, `TlsProxy` built after the `CertStore`, fan-out arms, startup log/warns |

New release deps in `fah-http`: `fah-certs` (L2→ legal), `rustls`,
`tokio-rustls` (pinned as `fah-api`), `webpki-roots = "1"` (already in tree via
`hickory-net`), hyper `http2`, hyper-util `server`/`server-auto`/`http2`.
`crates/fastadhunter/tests/layering.rs` passes with `fah-http → fah-certs`.

**GAR §5.14 — owner decision (2026-09-02): (a).** IP/CIDR is the interception
identity, with the documented precondition that every listed client holds a
static DHCP lease or static address; CONFIGURATION.md states it (proposed
below), p3-06 verifies per device. The startup `info!` names the precondition.

**GAR §5.8 — discharged.** `tls::connect_verified_upstream` connects the socket
to the policy-approved address and verifies the certificate against the SNI
hostname. `LiteralConnector` is untouched and remains plaintext-only.

## Decisions

- **Terminate leg order is fixed and structural:** verify upstream → pre-warm →
  accept → bridge. Any failure before `accept` drops the client TCP with no
  ServerHello sent, so a client can never observe our leaf for an upstream we
  did not verify (`an_unverifiable_upstream_never_yields_our_leaf`).
  Verify-before-mint also keeps unverifiable hosts out of the 512-entry LRU.
- **526 is reserved for verification failures** (M1 as applied).
  `tls::certificate_error` downcasts the `io::Error` tokio-rustls returns to
  `rustls::Error` and answers true only for `InvalidCertificate(_)`; that case
  counts `upstream_cert_failures` and emits `kind: https`, SNI verdict
  (`Pass`, or `Allow` when an exception matched), `status 526`, `bytes 0`.
  Every other failure — TCP refusal, connect deadline, ALPN/protocol/SNI
  alerts, decode errors — counts `upstream_failures` and emits `status 0`, as
  the splice leg does. The same classifier runs on the reconnect path, where
  a certificate failure is answered `526` to the client. The plan said "any
  error"; the split keeps the feed honest for the operator.
- **One verified upstream per client connection, reconnected on demand**
  (M2 as applied). `Upstream` holds `Mutex<Option<Sender>>`; h2 senders are
  cloned out and the lock released, h1 sends serialize under it (hyper's h1
  client refuses a second in-flight request). A sender found closed is
  re-established through the same `connect_verified_upstream` before the
  request is framed. A request that hyper hands back **unsent**
  (`TrySendError::take_message() == Some` — origin `Connection: close`, h2
  GOAWAY, dispatch gone) is retried **once** on a fresh verified session; a
  request that was or may have been written is never retried, whatever its
  method (`502`). Requests are rewritten per upstream ALPN: origin-form +
  `Host` for h1, absolute `https://` URI for h2; hop-by-hop stripping and
  `Via` happen once, before the first attempt, so a retry does not double
  them. A reconnect may **resume** the TLS session the origin ticketed; the
  origin then presents no certificate and the resumption is bound to the
  original verification (standard TLS 1.3 PSK). The reconnect tests disable
  tickets on the origin to exercise the full-handshake path.
- **HTTP buffering is bounded by explicit limits, not hyper defaults** (M3).
  h2: 64 KiB stream window, 256 KiB connection window, 64 KiB send buffer,
  64 concurrent streams (server side); h1: 128 KiB read/write buffer. Set on
  both the downstream `auto::Builder` and the upstream `http1`/`http2`
  client builders; ≈ 1.3 MiB of HTTP buffering per intercepted session worst
  case, stated in CONFIGURATION.md `[https] max_connections`.
- **`Host` must name the verified SNI, else `421`.** The session is verified
  for one name; forwarding another host's request over it would answer a
  request no certificate check covered. Judged first (a block still costs
  nothing), then refused with `refused_claim += 1`.
- **`Activity<S, L: Deref<Target = AtomicU64>>`.** hyper-util's auto server
  needs `I: 'static`, so the p3-03 idle watchdog is reused with `Arc` atomics
  on the terminate leg while the splice leg keeps `&AtomicU64` — no second
  watchdog, no allocation added to the everyone-path.

**m8 carry-over applied:** `non_tls` now counts `HelloScan::NotTls` only; EOF
before a hello, any read error and the `hello_timeout` deadline count
`hello_timeouts`. Both fields sit on `ProxyCounters`/`ProxyStats` next to
`upstream_cert_failures`. `main.rs`'s telemetry poll sums only refusals into
`requests_refused`, and neither new counter is a refusal, so the aggregation
is unchanged; nothing in `/telemetry` surfaces per-listener `ProxyStats`
today (finding for the reviewer, see TODOs).

## Measurements

Dev box only (x86_64, debug tests). No on-device figure; p3-06 owns the
budget rows.

| Property | Evidence |
| --- | --- |
| Upstream verify precedes downstream accept | structural (`intercept.rs` order); test asserts `minted_total == 0`, `size == 0`, one 526 event, client handshake error |
| Empty `clients` ⇒ nobody intercepted | `with_interception` stores `None` for an empty list; 64 random configs × 512 random v4/v6 IPs + fixed addresses all `false` |
| Non-listed client / excluded SNI splice | client trusting only our CA fails; client trusting the origin CA loads the page; `minted_total == 0`; events are `https-sni` |
| Filtering inside TLS, h1 and h2, both cross pairings | `/ads/pixel.gif` → 200 empty `image/gif`, origin requests 0; `/page` byte-identical 200 KiB, `Via` present, origin saw `Host: origin.test`; one upstream connection |
| Streaming | bodies are `Incoming` relayed through `Either::Left`, same as p2-02; `bytes` from `size_hint().exact()` |
| Idle watchdog covers the terminate leg | h2 session with `idle 300 ms` is closed after `3 × idle` |
| Added connect latency | **inherent, not measured here**: one upstream TLS handshake precedes our accept on every intercepted connection (plan §4). p3-06 measures on the RB5009 |

| Memory | Note |
| --- | --- |
| Per intercepted connection | 1 client TLS session + 1 upstream TLS session + hyper h1/h2 buffers + `RewindStream` buffer (released once rustls has read the hello) + 2 `Arc<AtomicU64>` |
| Bound | `https.max_connections` permits (shared with splice) + the p3-01 leaf LRU |
| DNS hot path | untouched |

## Tests

| Suite | Added | Notes |
| --- | --- | --- |
| `fah-http/tests/interception.rs` | 12 | real CA (`fah_certs`), real TLS origin (h1 / h2, auto server), `tokio-rustls` clients trusting our CA or the origin's |
| `fah-http` unit | 11 | `RewindStream` order + oversize read, ALPN order, `ExclusionSet` exact/suffix/lookalike/baseline/normalize/empty, `same_host` |
| `fah-http/tests/sni.rs` | 1 (+1 assertion) | EOF-before-hello is `hello_timeouts`, garbage stays `non_tls` |
| `fah-http/src/tls_server.rs` | +2 assertions | deadline path counts `hello_timeouts`, not `non_tls` |
| `fah-config` | 2 | default-off, section parses, unknown key rejected |
| `fah-model` | round-trip extended | `kind: https` |

Gates (2026-09-02): `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets -D warnings`, `cargo test --all-features --workspace` — all
green, 0 failed. p3-03 splice tests pass unchanged.

## Files changed

| File | Change |
| --- | --- |
| `crates/fah-config/src/schema/https.rs`, `schema/mod.rs`, `lib.rs` | `InterceptionConfig`, export |
| `crates/fah-model/src/request_event.rs` | `Event::Https`, `EventKind::Https` |
| `crates/fah-api/src/ports.rs`, `crates/fah-dns/src/pipeline.rs`, `crates/fastadhunter/tests/outcome_telemetry.rs` | new match arm |
| `crates/fah-http/Cargo.toml` | deps/features above; `tempfile` dev-dep; `rustls`/`tokio-rustls` moved from dev to release deps |
| `crates/fah-http/src/tls.rs` | **new** |
| `crates/fah-http/src/exclusions.rs` | **new** |
| `crates/fah-http/src/intercept.rs` | **new** |
| `crates/fah-http/src/https.rs` | branch, `Session`, `session_event`, counters split, generic `Activity`, `pub(crate)` fields |
| `crates/fah-http/src/proxy.rs` | `judge`/`emit`/`publish` free fns, `Judged` + helpers `pub(crate)`, two counters |
| `crates/fah-http/src/request.rs` | `absolute_url(scheme, …)` + test |
| `crates/fah-http/src/claim.rs` | `authority_of` `pub(crate)` |
| `crates/fah-http/src/lib.rs`, `tls_server.rs`, `tests/sni.rs`, `tests/interception.rs` | exports, tests |
| `crates/fastadhunter/src/main.rs` | `interception()`, proxy build moved after the store, fan-out arms |
| `Cargo.lock` | `webpki-roots` direct |

Pre-existing, unrelated working-tree changes not part of this task:
`docs/code-review/phase2.6/resoak-0.3.1-predeclaration.md`,
`docs/code-review/phase2.6/resoak-0.3.0/origin-log.tsv`, four untracked
`pull-adhoc-20260901T2220Z-*.json`.

## Known limitations

- **Interception requires SNI and a `Pass`/`Allow` SNI verdict**; no-SNI falls
  to p3-03's classification (plan §1).
- **Per-policy interception flag, QUIC, HTML rewriting, mTLS origins, upgrades
  (WebSocket) inside TLS** — not implemented; `Upgrade` is a hop-by-hop header
  and is stripped, as on :80.
- **A listed client that does not trust our CA** pays one upstream TLS session
  per attempt before its own handshake fails; logged at `debug!`, emitted as
  `kind: https`, `status 0`. Not counted separately.
- **No CA installed** with clients listed: connections from listed clients close
  (status-0 events) until a CA exists; one `warn!` at boot. Not fatal, DNS
  unaffected.
- The 526 verdict is the SNI verdict, so an `Allow` exception on the SNI is
  reported as `Allow`, not `Pass`, on that event.

## Remaining TODOs

- p3-06: RB5009 handshake + first-byte latency (intercepted vs spliced), leaf
  hit/miss, real phone with the CA installed, banking app through the baseline
  exclusions; confirm §5.14 closed per device (static leases).
- Reviewer: `ProxyStats` (incl. `hello_timeouts`, `upstream_cert_failures`) is
  not surfaced by `/telemetry`; only the refused sum is. Decide whether API.md
  §telemetry should grow a per-listener block, or the counters stay internal.
- `BASELINE_EXCLUSIONS` is a first cut (Apple/Google/Microsoft update + push +
  store hosts, WhatsApp/Signal, PayPal/Revolut/Wise/N26, six Romanian banks);
  owner to trim/extend before p3-06's walkthrough.

### Proposed doc edits — **applied 2026-09-02** with owner approval (§Fixes applied, item 5)

- **SECURITY.md** §"Later phases" — extend the "HTTPS interception (MITM)"
  bullet: opt-in via `[https.interception] clients`, per-client, never default;
  the upstream is verified (webpki roots, name = SNI, socket = egress-approved
  IP) **before** our leaf is presented and a failure closes the TCP connection
  unanswered; HSTS is transparent because the leaf chains to the client-trusted
  CA; a compiled-in exclusion baseline (pinned families) plus
  `exclude_domains` always splice; the CA key stays in `/config` and this path
  only reads minted leaves from the p3-01 cache; `Host` ≠ SNI is refused 421.
- **CONFIGURATION.md** — new `[https.interception]` block after `[https.sni]`:
  `clients = []` (IP/CIDR, **default empty = nobody intercepted**; precondition:
  every listed client holds a static DHCP lease or static address — a
  reassigned lease silently moves interception to whichever device inherits
  the address), `exclude_domains = []` (merged with the compiled-in baseline,
  listed), boot-class like the rest of `[https]`. One line: a silent preconnect
  holds a permit for up to `hello_timeout_ms` (m8 corollary).
- **API.md** §events — `kind: https` (full `RequestEvent`, like `http`, scheme
  `https`); synthetic `status 526` = upstream certificate not verified,
  `bytes 0`; `421` = `Host` not the verified SNI. §telemetry — `hello_timeouts`
  vs `non_tls` meaning, `upstream_cert_failures` (pending the TODO above).
- **CONTEXT.md** — add **Terminate leg / Splice leg** (the two outcomes of an
  HTTPS connection after the SNI verdict) and **Exclusion** (an SNI that always
  splices); note that **Interception** now also names the per-client TLS
  termination, not only dst-nat.
- **ROADMAP.md** — interception delivered when the phase closes (plan).

## Findings — consolidated 2026-09-03; full history: `git show e35203e:docs/code-review/phase3/p3-04-tls-interception-review.md`

Two reviews and three fix rounds; M4 rows 4–9 and N10 were closed by the
p3-06 carry-over tests, L5 by p3-06 post-review work A. Fixed and withdrawn
items are omitted — git has them. Still open:

| id(s) | Issue | Status | Where |
| --- | --- | --- | --- |
| S1 | h2 limits (`H2_*`, 64 streams × 64 KiB send buffer, 256 KiB / 64 KiB windows) were set, not measured; worst case ≈ 5.5 MiB per stalled session, no per-leg cap | deferred | p3-06 review §Pre-declaration P3 (sole authority for the ceiling) |
| L4 | an h2 session cut by the idle watchdog leaves its in-flight stream tasks and the upstream task alive until the origin answers or `hello_timeout`, outside `max_connections` | deferred | p3-06 review §Runbook 6 watch item (e): RSS must not trend with idle cuts |
| L7 | a listed client loses ECH: the outer-SNI hello is terminated under the public name and the browser retries without ECH (documented, SECURITY.md) | deferred | p3-06 review §Runbook 2 step 6 (device check) |
| N4 | prewarm-then-evict: the pre-warmed leaf can be evicted before `resolve` when > 512 first-sight hosts land inside one handshake; the handshake then aborts | deferred | p3-06 review §Runbook 6 watch item (c) (`unwarmed_misses` with a CA installed) |
| `BASELINE_EXCLUSIONS` | the shipped 34-entry list is a first cut; the owner's banking app must be covered before the pinned-app check means anything | deferred | p3-06 review §Post-review work B, §Runbook 4 |
| GAR §5.14 | interception identity is the source IP; the static-lease precondition is verified for the two test devices only and must be re-checked per added client | deferred | p3-06 review §Runbook 4 |
| ROADMAP | delivered wording for interception not yet in ROADMAP.md | deferred | p3-06 §Step 5 doc sweep |
| on-device | handshake cost, leaf hit/miss, phone with the CA, banking app — none measured on the RB5009 | deferred | p3-06 review P2, §Runbook 2 and 4 |
| L2 | a listed client with no CA installed is closed after the upstream check, not spliced | won't-fix | owner decision, fail-closed; Runbook 2 sequencing (install the CA before listing the device) |
| N7 | an h2 GOAWAY landing in the window before hyper's channel closes answers 502 with no retry | won't-fix | hyper-inherent; browsers retry |
| N8 | `spawn_blocking(prewarm)` on every intercepted connection, cache hit or not | won't-fix | measured: 4 µs hop vs a ≥ 1 ms handshake (p3-06 D10); re-checked by P5 |
| N9 | no deadline on the upstream response head; an origin that completes TLS then stalls holds the permit until `idle_timeout` | won't-fix | parity with `:80`; bounded by `idle_timeout` |
| N1 | `an_empty_client_list_intercepts_nobody` unit test asserts only that an empty `Vec` has no member | won't-fix | property proven at binary level by p3-06 Step 2 |
| N5 | wall-clock assertion (`< 1 s` for 10 000 lookups) inside a correctness test | won't-fix | test hygiene; bench territory |

**PASS WITH DEFERRED FINDINGS** — 14 open rows (8 deferred, 6 won't-fix). `AWAITING SOAK` until p3-06.
