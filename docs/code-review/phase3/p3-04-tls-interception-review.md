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

## Findings

Independent adversarial review, 2026-09-02, working tree on `49c6791`. The
Implementation Summary was **not** trusted: every claim below was re-derived
from `intercept.rs`, `tls.rs`, `exclusions.rs`, `https.rs`, `proxy.rs`,
`main.rs`, `fah-certs/{store,leaf}.rs`, the tests, and the hyper 1.10.1 /
tokio-rustls 0.26.4 sources in the registry. Gates re-run by the reviewer:
fmt clean, clippy `-D warnings` clean, `cargo test --all-features --workspace`
= 45 suites, 1387 passed, 0 failed, 8 ignored. Green gates are **not** the
basis of the verdict; the trace and the findings are.

No code was changed during this review (plan/CLAUDE.md §CODE REVIEW).
Tests named under "missing proof" are proposed, not added.

### Terminate-leg trace — what the code does, step by step

| Step | Code | Verdict |
| --- | --- | --- |
| ClientHello read, deadline | `https.rs:120-136`; EOF/read error/deadline → `hello_timeouts` | correct; m8 as specified |
| No SNI / not TLS / oversize | `https.rs:139-150` return before any interception lookup | **no-SNI can never reach the terminate leg** |
| IP-literal SNI | `https.rs:152-157` refused unless allowed | correct |
| SNI verdict | `https.rs:159-165`; `Block` closes before resolution | correct; a blocked domain is never intercepted |
| Resolution + egress | `https.rs:167-170` → `approved_address` = first policy-approved IP at `origin_port` | correct; open-relay guard shared with splice |
| Client identity | `interception_for(peer.ip(), &host)` `https.rs:90-94`; peer canonicalized at `https.rs:116` | strict IP/CIDR, v4-mapped handled; empty list ⇒ `None` at build (`https.rs:79-82`) |
| Exclusion | same call; `ExclusionSet::contains` exact + label-suffix walk | excluded SNI structurally cannot enter `intercept()` |
| Upstream verify | `tls.rs:65-83`: TCP to `session.address` (approved), `ServerName` = SNI, webpki roots, `hello_timeout` deadline | **name = SNI, socket = approved IP — GAR §5.8 holds** |
| Failure before accept | `intercept.rs:99-115`, `121-130`, `145-154`: every path `return`s with the client TCP dropped, no ServerHello | **our leaf is never presented for an unverified upstream — structural** |
| Prewarm | `spawn_blocking(store.prewarm)` `intercept.rs:119` | blocking mint off the runtime, single-flighted in p3-01 |
| Downstream accept | `TlsAcceptor` over `RewindStream(hello, Activity(tcp))` `intercept.rs:141-143`; resolver = `MintingResolver{fallback: None}` → `cached_leaf` only | `resolve()` never mints; unwarmed miss aborts the handshake (fail-closed) |
| Bridge | hyper auto h1/h2 → `handle_intercepted` → `judge` → `block::response` → `same_host` → `Upstream::send` | verdict core shared with :80; `Host ≠ SNI` → 421 |
| Reconnect | `Upstream::send` `intercept.rs:365-374` → `connect_verified_upstream` again, same approved address | verification repeated; socket target immutable per session |
| Idle / exit | `select!` on hyper vs `idle_watchdog` `intercept.rs:201-210`; permit held by the accept-loop task for the whole future | permit covers the session; see L4 for what outlives it |

Security invariants 1–8 of the review brief: **all hold**. No CRITICAL or
HIGH finding.

### Findings — severity ranked

Severity scale: CRITICAL / HIGH / MEDIUM / LOW / NIT. "Before DONE" = fix
in this task; "Defer" = tracked with an owner. None blocks p3-05.

**M1 — MEDIUM (telemetry integrity) — every rustls error is a "certificate
failure".** `intercept.rs:99` classifies on `io::ErrorKind::InvalidData`,
which tokio-rustls uses for **every** `rustls::Error`
(`tokio-rustls-0.26.4/src/common/mod.rs:115`): `InvalidCertificate(_)`, but
also `AlertReceived(HandshakeFailure | UnrecognizedName | ProtocolVersion)`,
`NoApplicationProtocol`, `PeerIncompatible`, `PeerMisbehaved`, decode errors.
Scenario: an origin that rejects our cipher/ALPN offer, or answers
`unrecognized_name` for the SNI, is counted `upstream_cert_failures += 1`
and emitted `status 526`. The summary's "526 is reserved for verification
failures" is therefore not true as shipped; the operator reads a
misconfigured origin as a MITM warning. Not a security issue — the client is
closed unanswered either way. Fix: downcast `err.get_ref()` to
`rustls::Error` and count 526 only for `InvalidCertificate(_)`; everything
else is `upstream_failures` + `status 0`. Missing proof: origin with an
unsupported ALPN-only config (or a `ServerConfig` that sends
`unrecognized_name`) ⇒ `upstream_cert_failures == 0`, `upstream_failures
== 1`, event `status 0`. **Before DONE** (a ~6-line change).

**M2 — MEDIUM (robustness + telemetry) — reconnect is best-effort and its
failures are misclassified.** (a) `intercept.rs:366` reconnects only when
`is_closed()` is already true, i.e. after hyper's connection task has
dropped the dispatch channel (`hyper/src/client/dispatch.rs:88`). Between an
origin `Connection: close` / h2 GOAWAY and that drop, `ready()` /
`send_request` return `Closed`/`GoAway` and the client gets a **502** with
no retry — the exact browser-idle-then-click case the summary says is
handled. (b) A reconnect whose `connect_verified_upstream` fails with a
certificate error is answered 502 and counted `upstream_failures`
(`intercept.rs:368-371`), not 526/`upstream_cert_failures` — the split M1
defends is silently abandoned on the second connection. The security
invariant is not violated: a 502 refusal is not upstream content. Fix:
(a) on a `Closed`/`GoAway`-class error from `ready()`/`send_request`, drop the
sender and retry **once** through the same verified connect (idempotent
methods only for h1, since the body may have been consumed — a non-idempotent
retry must stay a 502); (b) classify the reconnect error like M1. Missing
proof: origin answering `Connection: close` (h1) and sending GOAWAY after the
first response (h2); assert the second request succeeds and
`origin.connections == 2`. **Before DONE** for (b) (same code as M1);
**defer** (a) to p3-06 with an owner if the retry semantics need the
owner's call — but the test must exist either way, because today nothing
proves reconnect at all (`filtered_end_to_end` asserts `connections == 1`).

**M3 — MEDIUM (bounded memory, hard rule 4) — h2 flow-control windows are
hyper defaults, ~200× the splice bound per session, undocumented.**
Downstream `auto::Builder` h2: 1 MiB connection window, 1 MiB stream window,
200 streams, 400 KiB send buffer (`hyper-1.10.1/src/proto/h2/server.rs:36-75`).
Upstream `http2::handshake`: **5 MiB** connection window, 2 MiB stream window
(`proto/h2/client.rs:48-49`). A fast origin and a slow listed client buffer up
to ~6.4 MiB per intercepted session inside hyper; CONFIGURATION.md documents
`https.max_connections` as "2 × 16 KiB per session, ~32 MB at the default".
Bounded (by the same permit), but the bound the operator is told is wrong by
two orders of magnitude on a 1 GB device. Fix: set explicit windows on both
builders — e.g. server `initial_connection_window_size(256 KiB)`,
`initial_stream_window_size(64 KiB)`, `max_concurrent_streams(64)`,
`max_send_buf_size(64 KiB)`; client `initial_connection_window_size(256
KiB)`, `initial_stream_window_size(64 KiB)` — and one CONFIGURATION.md line
under `[https] max_connections` for the intercepted-session bound. p3-06
measures the throughput cost on-device. **Before DONE** (builder calls;
the doc line rides on the pending CONFIGURATION.md edit).

**M4 — MEDIUM (test quality) — untested unhappy paths.** Each row is a
path the code has and no test exercises; the invariant it guards cannot
currently fail visibly.

| Path | Code | Missing proof |
| --- | --- | --- |
| upstream TCP refusal / timeout on the terminate leg | `intercept.rs:107-114` | listed client, origin port closed ⇒ `upstream_failures == 1`, event `status 0`, `upstream_cert_failures == 0`, client handshake error |
| no CA installed, client listed | `intercept.rs:121-124` | store without CA ⇒ close, `minted_total == 0`, one upstream connection (see L2) |
| `Host`/`:authority` ≠ SNI over **h2** | `intercept.rs:279-284` via `destination_of` | h2 client with `:authority: other.test` ⇒ 421 (h1 is tested; h2's `:authority` path is a different parser branch, `claim.rs:118-124`) |
| reconnect after origin close / GOAWAY | `intercept.rs:365-374` | see M2 |
| concurrent h2 requests from one listed client | `Upstream::send` h2 clone-out | 8 parallel `/page` on one h2 session ⇒ all 200, one upstream connection |
| streaming request body | `to_upstream` moves `Incoming` | POST 4 MiB ⇒ origin sees 4 MiB, proxy RSS delta bounded (M3) |
| client disconnect mid-response / upstream disconnect mid-response | hyper error paths | session ends, permit released (`max_connections = 1` harness, second connect succeeds) |
| shutdown with live sessions | `TlsServer::shutdown` | see L4 — documents pre-existing semantics |
| IPv6 listed client end-to-end | only `intercepts()` unit-level | `[::1]` listed, connect over v6 ⇒ intercepted |

**Before DONE:** the first three rows (cheap in the existing harness). Defer
the rest to p3-06 with this table as its checklist.

**L1 — LOW (correctness of the judged URL) — `same_host` ignores the port.**
`intercept.rs:305-309` compares names only. `Host: origin.test:8443` passes,
`judge` builds `https://origin.test:8443/…` (`proxy.rs:528-532`), and
`to_upstream` forwards that authority — but the bytes go to the verified
`origin_port` socket. On :80 the port is real (`proxy.rs:449` connects to
`claim.port`); here the event and the rule matcher describe a port the
connection does not use. A dst-nat'd browser cannot produce this; a
misbehaving app on a listed device can dodge a `||host/path` rule with it.
Fix: require `claim.port == self.origin_port` next to `same_host`, else 421.
**Defer** (managed clients only) — or fold into M1's edit, it is one line.

**L2 — LOW (efficiency + observability) — missing CA is discovered after
the upstream handshake, counted nowhere, and handled unlike the `None`
store.** `interception()` (`main.rs:721-727`) warns and still installs the
branch; each listed-client connection then pays a full upstream TLS
verification before `prewarm` returns `NoCa` (`intercept.rs:119-124`), closes
with `status 0` and increments **no** counter — indistinguishable from a
connect failure. The plan's rule for a store that did not open is "splice,
never MITM"; a store with no CA closes instead. Fix: check `store.has_ca()`
per connection **before** `connect_verified_upstream` (it can flip at runtime
via `/api/v1/certificates`), and either splice (parity with `None`, my
recommendation) or close with its own counter — owner's call. **Defer** with
the owner decision recorded here.

**L3 — LOW (behaviour, perf) — h1 intercepted sessions idle out at
`hello_timeout`, not `idle_timeout`.** `builder.http1().header_read_timeout
(self.hello_timeout)` `intercept.rs:194`; hyper arms it every time it waits
for the next head (`hyper-1.10.1/src/proto/h1/conn.rs:219-233`), so an h1
keep-alive connection is cut after 10 s of think time and the next click
costs two TLS handshakes (upstream verify + ours) plus a possible mint.
h2 clients are unaffected. Fix: `header_read_timeout(self.idle_timeout)`;
the `Activity` watchdog already bounds silent sockets. **Defer**; note in
CONFIGURATION.md's `hello_timeout_ms` text if kept.

**L4 — LOW (lifecycle) — after the idle watchdog closes an h2 session,
in-flight stream tasks and the upstream connection task outlive the permit.**
hyper's h2 server spawns each request on `TokioExecutor`; dropping `serving`
(`intercept.rs:207`) does not cancel a task parked in `Upstream::send`
awaiting a slow origin, nor the detached upstream connection task
(`intercept.rs:323-336`) they keep alive through the `Upstream` `Arc`. They
end when the origin answers or the connect deadline fires — bounded by
`hello_timeout` and origin response time, but outside `max_connections`.
h1 downstream is inline and unaffected. `Engine::shutdown` aborts only the
accept loop (`tls_server.rs:59-63`), as :80 and the splice leg already do;
live sessions die with the runtime drop at process exit — **pre-existing
semantics, not a p3-04 regression**. **Defer**; p3-05/p3-06 may want a
`CancellationToken` for all listeners at once.

**L5 — LOW (telemetry semantics) — `TlsProxy` counters mix per-connection
and per-request meanings.** `requests` is per connection (`https.rs:117`)
and is **not** incremented in `handle_intercepted`; `blocked` and
`refused_claim` are incremented per intercepted request (`intercept.rs:253,
272, 280`). `blocked > requests` is now possible on one listener. Nothing
surfaces `ProxyStats` beyond the refused sum today (`main.rs:839-845`), so
no consumer is wrong yet — but the API.md §telemetry decision in the TODOs
must settle this before the counters are exposed. **Defer** to that
decision.

**L6 — LOW (config validation) — `exclude_domains` entries are not
validated.** `ExclusionSet::new` (`exclusions.rs:47-55`) keeps anything
non-empty after trim/lowercase; `*.bank.example`, `https://bank.example`,
`bank.example/` are stored verbatim and never match. `clients` fails startup
on a bad entry; this list silently degrades. Fix: reject entries that fail
the hostname syntax `sni.rs::normalize` already enforces (or reuse it).
**Defer**.

**L7 — LOW (documented behaviour gap) — an ECH ClientHello enters the
terminate leg under its outer (public) SNI.** `scan_client_hello` does not
classify extension `0xfe0d`; the outer name (e.g. `cloudflare-ech.com`) is
judged, verified, minted and served. The client then sees a public-name
certificate it trusts and no retry configs; browsers retry without ECH and
the retry is intercepted under the real name, so filtering still applies —
at the cost of one extra upstream handshake, one wasted leaf and one
`status 0` event per ECH-enabled origin. Effectively an ECH downgrade for
listed clients, inherent to MITM. **Defer**; one sentence in the SECURITY.md
edit ("listed clients lose ECH") and p3-06 observes it on a real browser.

**L8 — LOW (availability) — an SNI our parser accepts but rustls rejects
closes the connection instead of splicing.** `sni.rs::normalize` accepts
e.g. an all-numeric last label; `ServerName::try_from` (`tls.rs:71`) refuses
it → `InvalidInput` → `upstream_failures += 1`, close. The plan's rule for
"cannot intercept" is "fall through to p3-03's handling"; splice would serve
it. **Defer**; rare.

**N1 — NIT (test proves nothing).** `intercept.rs:437-444`
`an_empty_client_list_intercepts_nobody` asserts that an empty `Vec` has no
member; it never constructs an `Interception`. The real proof is the
integration fuzz. Delete it.

**N2 — NIT (allocation).** `intercept.rs:138` allocates a `written`
`Arc<AtomicU64>` that nothing reads. Harmless; either drop it from
`Activity`'s terminate-leg use or use it for session bytes.

**N3 — NIT (h2 hygiene).** `to_upstream` keeps a downstream `Host` header
alongside `:authority` on an h2 upstream (`intercept.rs:407-413`). RFC 9113
§8.3.1 permits it when they agree (they do — `authority_of` checked); some
origins log it as odd. Remove `Host` in the `Alpn::H2` arm.

**N4 — NIT (theoretical race).** The prewarmed leaf is discarded
(`intercept.rs:120`) and the resolver re-reads the LRU. With more than 512
distinct hosts prewarmed between one connection's `prewarm` and its
`resolve`, the entry can be evicted → handshake abort + `unwarmed_misses`.
Needs >512 concurrent first-sight hosts inside one handshake window; p3-06's
`unwarmed_misses == 0` assertion is the detector. No action.

**N5 — NIT (test hygiene).** `the_baseline_exclusions_ship_without_any_configuration`
asserts 10 000 lookups `< 1 s` — a wall-clock assertion in a correctness
test. Move the timing to a bench or drop it.

### Looks suspicious, is correct — do not "fix"

- `Sender::handshake` (h1/h2 preface) runs **after** the downstream accept
  (`intercept.rs:157`): the upstream certificate was verified at
  `tls.rs:76-78`; this is only HTTP framing. A failure here closes with
  `status 0` and leaks nothing.
- `tokio::sync::Mutex` held across `.await` in the h1 arm of `Upstream::send`:
  intentional — hyper's h1 client refuses a second in-flight request.
- `parts.version = HTTP_11` only in the h1 arm: hyper's h2 client ignores the
  version; hyper's h1 server coerces an `HTTP_2`-versioned relayed response
  to `HTTP/1.1` (`conn.rs::enforce_version`).
- `bytes = size_hint().exact().unwrap_or(0)`: `Incoming` reports
  `Content-Length` for both h1 and h2 bodies (hyper sets `content_length`
  from the header on h2), else `None` → 0; `HEAD` → 0; a truncated relay
  overstates. Identical to p2-02's documented semantics for `kind: http`;
  API.md's `kind: https` note must say "same as http".
- `duration` on request events is `Instant::now()` at request entry →
  response head (`intercept.rs:248, 290`): request latency, not session
  lifetime. `emit_session` uses the session clock for 526/0 events, as the
  splice leg does for its connection event.
- 421 event names the **claimed** host (`other.test`), which we never
  contacted — deliberate, the test asserts it; the operator sees what the
  device asked for.
- `Host ≠ SNI` is judged **before** it is refused: a block still costs
  nothing and is attributed; only allowed requests pay the 421.
- `hello_timeouts` counts a read error too (reset before a hello): m8's
  "EOF or deadline" spirit; a reset is EOF-shaped silence, not garbage.
- Egress policy not re-checked on reconnect: the address is per-session
  immutable and `[egress]` is boot-class (`config_store.rs:35-52`).
- Request smuggling: hyper re-frames both hops (`transfer-encoding` and
  `connection`-named headers stripped, body length from the body itself), as
  on :80. `CONNECT` and `Upgrade` cannot tunnel — no `on_upgrade`, no 101.
- `RewindStream` drops its buffer once drained (`tls.rs:112-115`);
  `MAX_HELLO_BYTES` bounds it before that.
- rustls resumption caches (server 256 sessions default, client 256 per
  `ClientConfig`) are bounded and keyed by `ServerName`; sharing one
  `ClientConfig` across sessions is safe.
- `webpki-roots = "1"` is **not** a second bundle: `webpki-roots 0.26.11`
  (via `tokio-tungstenite`, dev/tui only) is a shim over 1.0.8, which
  `hickory-net` already pulls. Summary claim verified.
- Layering: `fah-http → fah-certs` is L3→L2; `layering.rs` passes. No
  second TLS or cert implementation exists outside `fah-certs`; `tls.rs` only
  builds rustls configs and a stream adapter.
- Boot class: `"https"` is a whole-section `BOOT_KEYS` entry
  (`config_store.rs:35-46`), so `[https.interception]` already answers
  `restart_required: true`. `deny_unknown_fields` tested; a bad `clients`
  entry fails startup (`main.rs:699-706`).

### Plan compliance

| Plan item | Status | Class |
| --- | --- | --- |
| §1 branch inside p3-03 handler, order no-SNI → Block → list ∧ ¬excluded | as specified | — |
| §2 `AllowedNet` list, default empty, fail startup on bad entry, fuzz + branch test | as specified | — |
| §2 GAR §5.14 owner decision | (a) recorded in summary; CONFIGURATION.md text pending | owner-approved |
| §2 per-policy flag | deferred | plan-approved |
| §3 `ExclusionSet` baseline ∪ user, suffix match, before terminate | as specified; user entries unvalidated (L6) | — |
| §4 verify-before-present, `ServerName` = SNI, socket = approved IP, close on failure | as specified; **structural** | — |
| §4 "upstream failure ⇒ 526 event" | split into 526 (rustls error) vs 0 (TCP/deadline); the split itself is over-broad (M1) and not applied on reconnect (M2b) | undocumented deviation, justified in summary; **defect in the split** |
| §5 verdict core reused, dedicated verified upstream, ALPN both sides independent | as specified (`judge`/`emit`/`publish` free fns) | — |
| §6 `MintingResolver{fallback: None}`, `prewarm` via `spawn_blocking`, configs built once, `RewindStream` | as specified | — |
| §6 no Debug/Display over key material | verified: `CertifiedKey` never logged; `Interception` has no `Debug` | — |
| §7 `Event::Https`, fan-out, 526 with SNI verdict (`Allow` possible) | as specified; verdict nuance documented | documented deviation |
| Step 4 m8 split (`hello_timeouts`) | as specified | — |
| Step 6 `None` store ⇒ splice + warn; startup log | as specified; `has_ca() == false` ⇒ close (L2) | undocumented choice |
| Tests: unit (`ExclusionSet`, `RewindStream`, `intercepts`) | present; `intercepts` CIDR test is integration-level, fine | — |
| Tests: 4 security properties, h1/h2 both pairings | present and **falsifiable** (each was checked for what breaks it) | — |
| Tests: unhappy paths | absent (M4) | gap |
| Bench / on-device | deferred to p3-06 | plan-approved |
| Task AC "SECURITY.md + CONFIGURATION.md updated in the same change" | **not done** — proposed, awaiting owner approval per working agreement | process-approved deferral; AC literally unmet |
| Task AC "non-listed never intercepted", "bad upstream never yields local success", gates | met | — |

### Blockers

None of CRITICAL/HIGH severity. The security core is sound and structural.

**Before `DONE`** (small, all in `intercept.rs` / one test file, no design
change):

1. M1 — 526 only for `rustls::Error::InvalidCertificate`.
2. M2(b) — same classification on the reconnect path; M2 test for
   reconnect after origin close (h1) and GOAWAY (h2), with M2(a)'s single
   retry or an owner-recorded decision to keep 502.
3. M3 — explicit h2 window/stream limits on both builders.
4. M4 rows 1–3 — upstream refusal, no-CA, h2 `:authority ≠ SNI`.
5. Owner approval + application of the SECURITY.md / CONFIGURATION.md /
   API.md / CONTEXT.md edits listed above (task acceptance criterion).
   Add to them: M3's bound line, L7's ECH sentence, `bytes` = "as `http`".

### Deferred findings

M2(a) if the owner prefers 502 semantics · M4 rows 4–9 → p3-06 checklist ·
L1 · L2 (owner decision: splice vs close without CA) · L3 · L4 · L5 (with
the `/telemetry` decision) · L6 · L7 · L8 · N1–N5.

### Safe-to-defer rationale

- Every deferred item is reachable only by a **listed** client (opt-in,
  managed device) or affects observability, never the everyone-path: splice
  behaviour, the DNS hot path and non-listed clients are untouched
  (`spliced_not_intercepted`, p3-03 suites unchanged).
- No deferred item can present our leaf for an unverified upstream, widen
  the client set, bypass an exclusion, or move the upstream socket off the
  policy-approved address — those are the four properties the task exists
  for, and each is enforced by control flow, not by a check that can be
  skipped.
- L4's detached tasks are bounded by `hello_timeout` and origin latency;
  L3 costs handshakes, not correctness; L1/L6/L8 need a misbehaving or
  misconfigured managed device to matter.
- p3-05 (DoT/DoH listeners) consumes `MintingResolver` and the same
  `server_config` shape; none of the above changes that seam. **No finding
  blocks p3-05.**

### Verdict

**PASS WITH DEFERRED FINDINGS.** Not `DONE` until the five "Before DONE"
items land; after that, `AWAITING SOAK` is the honest table cell until p3-06
produces the RB5009 handshake figures and the real-device CA walkthrough.

## Fixes applied — "Before DONE" items, 2026-09-02

Owner instruction: fix the five items, leave L1–L8 / N1–N5 untouched.
Nothing in this round touches the RB5009.

| # | Finding | Status | Where |
| --- | --- | --- | --- |
| 1 | M1 — 526 only for `rustls::Error::InvalidCertificate` | **fixed** | `tls.rs::certificate_error` (downcast through `io::Error::get_ref`), used at the initial connect and on the reconnect path |
| 2 | M2 — reconnect hardening + classification | **fixed** (retry criterion differs, see deviations) | `intercept.rs` `Upstream::send`/`attempt`, `AttemptError { unsent, error }` |
| 3 | M3 — explicit h2 windows/buffers, both sides | **fixed** | `intercept.rs` `H2_*`/`H1_MAX_BUF` constants on `auto::Builder` and the `http1`/`http2` client builders; CONFIGURATION.md `[https] max_connections` |
| 4 | M4 rows 1–3 — refusal/timeout, no CA, h2 `:authority` ≠ SNI | **fixed** | `tests/interception.rs`, counters and events asserted |
| 5 | Doc edits — SECURITY, CONFIGURATION, API, CONTEXT | **applied** | see below; ROADMAP.md untouched (not in the approved list) |

### What changed

- **M1.** `certificate_error(&io::Error) -> bool`: `InvalidData` **and** the
  inner error downcasts to `rustls::Error::InvalidCertificate(_)`. Unit test
  covers `UnknownIssuer`/`NotValidForName` (true) vs `NoApplicationProtocol`,
  `AlertReceived(HandshakeFailure | UnrecognisedName)`, `ConnectionRefused`,
  `TimedOut`, a non-rustls `InvalidData` (all false). Integration:
  `an_origin_that_rejects_our_alpn_is_an_upstream_failure_not_a_certificate_failure`
  (origin ALPN `nothing-we-speak` ⇒ `upstream_failures == 1`,
  `upstream_cert_failures == 0`, event `status 0`, no leaf minted).
- **M2(b).** `handle_intercepted` classifies `Upstream::send` errors with the
  same function: certificate ⇒ `upstream_cert_failures += 1`, event and
  response `526`; else `upstream_failures += 1`, `502`. Test
  `a_reconnect_to_an_upstream_whose_certificate_changed_is_refused_as_526`:
  origin serves a good leaf on connection 1 and a self-signed one on
  connection 2 (`Rotating` resolver, `Connection: close` in between) ⇒
  request 2 answered `526`, `origin.connections == 2`, `origin.requests == 1`
  (nothing rides the unverified reconnect), counters 1/0.
- **M2(a).** `Upstream::send` strips hop-by-hop and appends `Via` once, then
  `attempt` (reconnect-if-closed → frame for the negotiated ALPN → `ready()` →
  `try_send_request`). An error that returns the request (`take_message() ==
  Some`) or a failed `ready()` is `unsent`; `send` drops the sender and runs
  `attempt` once more; a second failure is final. Tests
  `an_origin_that_closes_after_each_response_is_reconnected_with_verification`
  (h1, three rounds ⇒ `connections == 3`, all 200) and
  `an_h2_origin_that_goes_away_is_reconnected_with_verification` (origin
  `graceful_shutdown` after the first response ⇒ GOAWAY; second request 200,
  `connections == 2`).
- **M3.** Downstream: `http1().max_buf_size(128 KiB)`;
  `http2()` stream window 64 KiB, connection window 256 KiB, 64 concurrent
  streams, 64 KiB send buffer. Upstream: `http2::Builder` with the same
  windows and send buffer; `http1::Builder::max_buf_size(128 KiB)`. Adaptive
  windows stay off. The 200 KiB `/page` relays in every pairing (> one stream
  window), so flow control across window updates is exercised by the
  existing suite.
- **M4.** `an_upstream_that_refuses_the_connection_is_an_upstream_failure`
  (closed port ⇒ `upstream_failures == 1`, cert 0, event `status 0`,
  `minted_total == 0`);
  `an_upstream_that_never_answers_is_cut_off_at_the_hello_deadline`
  (TEST-NET-1, `hello 300 ms`, returns within `10 × hello`, same counters);
  `a_listed_client_without_a_ca_is_closed_after_the_upstream_check` (store
  without CA ⇒ handshake error, `origin.connections == 1`, `requests == 0`,
  `minted_total == 0`, event `status 0`, both counters 0 — L2's observation
  confirmed, still deferred);
  `an_h2_authority_that_is_not_the_verified_sni_is_refused_as_misdirected`
  (`:authority: other.test` ⇒ 421, event host `other.test`,
  `refused_claim == 1`, `origin.requests == 0`; the same h2 session then
  serves `/page`).
- **Docs.** SECURITY.md §Later phases: the interception bullet rewritten —
  opt-in per listed client, static-lease precondition, verify-before-present
  with the 526-only-for-`InvalidCertificate` rule and resumption note, one
  name per session (421), HSTS transparency, exclusion baseline, **listed
  clients lose ECH** (L7), per-session bound. CONFIGURATION.md: boot-class
  note for `[https.interception]`; `max_connections` states the intercepted
  per-session bound (M3); `hello_timeout_ms` states it also bounds the
  upstream verification, our handshake and the h1 next-head wait (L3, kept);
  `idle_timeout_ms` covers intercepted sessions; new `[https.interception]`
  block (`clients`, `exclude_domains`, the baseline listed, default-off, the
  static-lease precondition, no-CA / no-store behaviour). API.md: `kind:
  https` item — shaped as `http`, **`bytes` same as `http`**, 526 and 421
  semantics, the session-level `status 0` item, "no `https-sni` item for an
  intercepted session"; §telemetry names `non_tls` vs `hello_timeouts` and
  `upstream_cert_failures` as unpublished per-listener counters. CONTEXT.md:
  **Interception** gains the termination meaning; new **Splice Leg /
  Terminate Leg** and **Exclusion** terms.

### Deviations from the instruction

- **M2 retry criterion.** The instruction said "retry once … for safely
  retryable/idempotent requests; never retry a non-idempotent request after
  its body may have been consumed". Applied criterion: retry **only** a
  request hyper hands back unsent. Stricter on one side (an idempotent
  request that was written is not retried — hyper no longer holds it, and a
  buffered copy would break p2-02's streaming rule) and wider on the other
  (an unsent POST is retried — its body was never touched, so the retry is
  exactly as safe as for a GET). No method-based rule exists in the code;
  the safety property is structural.
- **M1 scope.** On a reconnect certificate failure the client gets a
  synthesized `526` response — the session is already terminated, so
  "close unanswered" is not available; the event carries `526`, `bytes 0`.
- **M3 scope.** The h1 buffer cap (`max_buf_size`, both sides) was added
  beyond the h2 ask so that no hyper default remains the effective bound on
  the terminate leg.
- **Resumption.** The reconnect tests needed `send_tls13_tickets = 0` and
  `NoServerSessionStorage` on the test origin; with tickets on, rustls
  resumed and the origin never re-presented a certificate (the first run of
  the cert-changed test returned 200). Production keeps resumption: it is
  bound to the original verification and saves a full handshake on the
  RB5009. Documented in SECURITY.md.

### Verification

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean (one `large_enum_variant` hit during the round, resolved by the `AttemptError` struct) |
| `cargo test --all-features --workspace` | 1405 passed, 0 failed, 8 ignored |
| `fah-http` alone | 131 passed (`tests/interception.rs` 12 → 20; `tls.rs` +1 unit) |

### Updated finding status

M1 fixed · M2(a) fixed (unsent-retry criterion) · M2(b) fixed · M3 fixed ·
M4 rows 1–3 fixed (rows 4–9 remain p3-06's checklist) · item 5 applied.
L1–L8, N1–N5 unchanged, still deferred as recorded above. Verdict stands:
**PASS WITH DEFERRED FINDINGS**; nothing now gates `DONE` except the owner's
table edit, and `AWAITING SOAK` remains the honest cell until p3-06.

## Second review — post-fix state, 2026-09-02

Independent pass over the working tree after §Fixes applied (base `49c6791`,
plan = `p3-04-tls-interception-plan.md` as source of truth). Read: full diff
of the 17 changed files, the three new modules, `tests/interception.rs`, the
`fah-certs` seams (`store.rs::prewarm/cached_leaf`, `leaf.rs::cached/resolve`),
and hyper 1.10.1 (`server/conn/http2.rs`, `client/dispatch.rs`,
`proto/h2/client.rs`, `proto/h1/conn.rs`) where a claim depended on it.
Gates re-run: fmt clean, clippy `-D warnings` clean, `fah-http` 131 passed
(20 in `tests/interception.rs`), 0 failed. Severity scale this pass:
**blocker / should-fix / note**. No code changed.

### Blockers

None. No path presents our leaf for an unverified upstream, widens the client
set, bypasses an exclusion, or moves the upstream socket off the approved
address; each is enforced by control flow (`https.rs:169-181` →
`intercept.rs:99-124` return-before-accept).

### Should-fix

**S1 — memory bound is misstated by 4–8× (CONFIGURATION.md `[https]
max_connections`, SECURITY.md "Bounded per session").** Evidence: hyper's
`max_send_buf_size` is *per stream* ("maximum write buffer size for each
HTTP/2 stream", `hyper-1.10.1/src/server/conn/http2.rs:242`);
`intercept.rs:37-41` sets 64 streams × 64 KiB on the downstream server
(`intercept.rs:206-212`) and the same per-stream cap on the upstream client
(`intercept.rs:346-349`). hyper reserves ≥ 1 byte of capacity then hands h2
the whole chunk, so each stream buffers up to 64 KiB + one ≤ 16 KiB frame
before it stops pulling. Worst case per intercepted session (64 streams, slow
client, fast origin): ≈ 5 MiB of response send buffers + 256 KiB receive
window per side, ≈ 10 MiB if uploads stall symmetrically — not "~1.3 MiB".
Still bounded (hard rule 4 holds); the operator-facing number is wrong on a
1 GB device. Fix: either state the real bound (≈ 5.5 MiB, `H2_MAX_STREAMS ×
H2_SEND_BUF` dominates) or lower the constants (32 streams × 32 KiB ≈ 1 MiB
+ windows) and keep the sentence — owner's call; p3-06 measures either way.
Inference from hyper source, not measured.

**S2 — a test depends on the real network (`tests/interception.rs:953-982`).**
`an_upstream_that_never_answers_is_cut_off_at_the_hello_deadline` connects
to `192.0.2.1:443`. It passes only while nothing on the path answers that SYN:
a captive portal, a transparent proxy, or — the concrete case — p3-06's own
RB5009 dst-nat of :443, behind which this dev box sits, will answer with a
certificate that fails verification → `upstream_cert_failures == 1`, test
fails. A box with no default route passes instantly (`ENETUNREACH`) without
exercising the deadline at all. Fix: a local origin that accepts TCP and never
reads or writes (hold the socket) → the TLS connect hits `TimedOut` at
`hello_timeout`; no network, deadline path actually taken.

**S3 — h1 keep-alive is cut at `hello_timeout` (`intercept.rs:203`).** Prior
L3, kept and documented. Re-rated from the perf side: hyper arms
`header_read_timeout` every time it waits for a request head
(`proto/h1/conn.rs:219-231`), so a browser's idle h1 connection dies after
10 s and the next click pays our handshake + the upstream verify (or
resumption) on the RB5009. `idle_timeout` exists for this and the `Activity`
watchdog already bounds silence at 60 s; the only thing `hello_timeout` buys
is a shorter permit hold for a *working* keep-alive. One-token fix
(`self.idle_timeout`) plus reverting the CONFIGURATION.md sentence that
explains the workaround. Owner already chose to keep it; recorded as
disagreement, not re-litigation.

### Notes

**N6 — `Upstream::send` `take()` discards a sender a concurrent stream just
re-established (`intercept.rs:441`).** Every `unsent` path also leaves
`is_closed()` true (hyper drains the request channel on receiver drop,
`client/dispatch.rs:215-226`), so `attempt`'s reconnect arm
(`intercept.rs:453-465`) already covers it; the unconditional `take()` only
matters when N h2 streams come back unsent together after a GOAWAY — then up
to N verified upstream handshakes instead of one. Bounded by
`H2_MAX_STREAMS`; guard the `take()` with `is_closed()` or delete it.

**N7 — h2 GOAWAY window with no retry.** A request hyper's `ClientTask` has
already dequeued when `h2_tx.send_request` fails comes back `message: None`
(`proto/h2/client.rs:738-745`) → 502, no retry. Inherent to hyper.
`an_h2_origin_that_goes_away…` sleeps 500 ms (`tests/interception.rs:817`) to
stay out of that window — timing-dependent on the safe side. No action.

**N8 — `spawn_blocking(prewarm)` on every intercepted connection
(`intercept.rs:128`), cache hit or not.** One cross-thread hop (and a thread
spawn on a cold blocking pool) per connection. `cached_leaf` cannot serve as
the pre-check: it counts `unwarmed_misses` (`leaf.rs:149`), which p3-06
asserts zero. Would need a non-counting peek in `fah-certs`. Measure on-device
first; note only.

**N9 — no deadline on the upstream response head (`intercept.rs:302`,
`attempt`).** An origin that completes TLS then stalls holds the permit, both
TLS sessions and — h1 upstream — the `Upstream` mutex (every other stream of
an h2 client queues) until the downstream idle watchdog fires (60 s). Parity
with :80 (hyper-util's legacy client has no response timeout either).
Bounded; note.

**N10 — test name overclaims (`tests/interception.rs:734-765`).**
`…_and_its_permit_returned` asserts the session closed, nothing about the
permit (M4 row 7 is still open). Rename, or add the `max_connections = 1`
second-connect assertion.

**N11 — `Interception::client_count` (`intercept.rs:76-78`) has no caller.**
Dead public API; delete (principle 14).

**N12 — `frame_for` re-parses the authority (`intercept.rs:498`) that
`destination_of` already parsed and `same_host` already checked** — a second
`Authority` parse (allocates) per forwarded request. Pass the claim's
authority into `send`.

Prior N1–N5 verified still present and stand as written.

### Checked and found acceptable

| Category | Evidence |
| --- | --- |
| Plan compliance | Every plan §1–§7 item and step 1–6 implemented; the deviations (526 split, single unsent retry, 421, no-CA ⇒ close, h1 head timeout, h2 limits) are each in the summary **and** in the applied SECURITY/CONFIGURATION/API/CONTEXT text; task AC "docs in the same change" now met in the working tree |
| Correctness | Terminate order structural (`intercept.rs:99-164`); every pre-accept failure returns with no ServerHello; hop-by-hop + `Via` once before the first attempt (`intercept.rs:422-425`); retry only when hyper hands the request back; downstream RST_STREAM drops the service future → h1 callback canceled → hyper closes that upstream → next request reconnects (clean, bounded); no `unwrap`/`expect`/panic in new non-test code (`unwrap_or` at `intercept.rs:44` cannot hit, 526 is a valid code) |
| Architecture | `fah-http → fah-certs` L3→L2, `layering.rs` green; no second pipeline (`judge`/`emit`/`publish` shared); `Interception` groups the plan's five `TlsProxy` fields; `Event::Https` additive, every match arm compiler-enforced; DNS hot path untouched (`pipeline.rs` diff is test-only) |
| Performance (everyone-path) | One `Option::as_ref().filter()` + `AllowedNet` any-match per :443 connection; empty list stores `None` (`https.rs:79-82`) ⇒ zero work; exclusion walk only after a listed-client match; `debug!` on failure paths only, nothing logged on success; per-request allocations equal to the :80 path |
| Memory | Per session bounded (S1 corrects the number); `RewindStream` frees its buffer once drained (`tls.rs:120-123`); `hello` starts at 2 KiB; rustls client/server session caches 256 entries; leaf LRU 512; upstream connection tasks end when their `SendRequest` drops (self-terminating, no handle needed); no new unbounded collection |
| Rust quality | `Activity<S, L: Deref<Target = AtomicU64>>` keeps `&AtomicU64` on the splice leg — no allocation added there; `Send`/`Sync` fine; `tokio::sync::Mutex` across `.await` deliberate (h1 has no pipelining); SNI case: scanner lowercases (`sni.rs:274`), store normalizes (`store.rs:569`), so rustls' `server_name()` and our prewarm key agree |
| Tests | 20 integration + 11 unit; falsifiability spot-checked: the self-signed-upstream test asserts `size == 0` and a 526 event, the splice tests assert a client trusting only our CA fails; S2/N7/N10 are the test-quality findings |
| Regression | p3-03 splice untouched except the m8 counter split (specified); `absolute_url` internal; telemetry `requests_refused` sum unchanged (neither new counter is a refusal); `fah-config` default-off + `deny_unknown_fields` tested |

### Verdict

**PASS WITH DEFERRED FINDINGS** — unchanged. S1–S3 are doc-accuracy, test
robustness and a perf trade; none touches the four security properties. S2
should land before p3-06 deploys the dst-nat, or that test starts failing on
the owner's LAN. `AWAITING SOAK` remains the honest cell until p3-06.

## Fixes applied — second review S1–S3, 2026-09-02

Owner instruction: fix S1, S2, S3; N6–N12 untouched.

| # | Finding | Status | Where |
| --- | --- | --- | --- |
| S1 | per-session bound misstated | **fixed (doc)** | CONFIGURATION.md `[https] max_connections`: send buffer named PER STREAM, worst case ≈ 5.5 MiB (64 × (64 + 16) KiB + two 256 KiB windows), ≈ 11 MiB only with 64 stalled uploads, typical far below. Constants unchanged — lowering them before p3-06 measures throughput on the RB5009 would be tuning without measurement (principle 8) |
| S2 | network-dependent deadline test | **fixed** | `tests/interception.rs`: `silent_origin()` — a local listener that accepts and holds every socket, never reads or writes; the TLS connect now hits `TimedOut` at `hello_timeout`. Asserts `elapsed ≥ HELLO` (deadline path, not a refusal), `< 10 × HELLO`, exactly one accepted upstream connection, `upstream_failures == 1`, cert failures 0, event `status 0`. No address leaves the box |
| S3 | h1 keep-alive cut at `hello_timeout` | **fixed** | `intercept.rs` `header_read_timeout(self.idle_timeout)`; CONFIGURATION.md `hello_timeout_ms` now says it bounds the upstream verification (each reconnect too) and our handshake, and that the wait between requests is `idle_timeout_ms` for h1 and h2 alike |

Verification: `cargo fmt --all -- --check` clean · `cargo clippy --workspace
--all-targets -- -D warnings` clean · `cargo test --all-features --workspace`
1405 passed, 0 failed · the rewritten S2 test run 3× in isolation, 3/3 pass.
Verdict unchanged: **PASS WITH DEFERRED FINDINGS**; `AWAITING SOAK` until
p3-06.

## Fixes applied — cleanup L1 · L6 · L8 · N6 · N11 · N12 · N2 · N3, 2026-09-02

Owner instruction: these eight only; L2/L5/baseline/ROADMAP decisions and
L4/N4/N7/N8/N9/L7 untouched. Each item re-derived from the code first.

| # | Status | What changed |
| --- | --- | --- |
| L1 | **fixed** | `handle_intercepted`: 421 when `claim.port != origin_port`, next to `same_host`. Judged first, as before |
| L6 | **fixed** | `ExclusionSet::new` → `Result<_, InvalidExclusion>`; each entry is trimmed, a trailing dot stripped, then validated by the p3-03 SNI grammar (`sni::normalize`, now `pub(crate)`), so an exclusion accepts exactly the names an SNI can carry. `main.rs` fails startup by name (`[https.interception] exclude_domains: not a hostname: "…"`) **before** the empty-`clients` early return — a malformed boot-class key is rejected whether or not interception is on |
| L8 | **no change — already closes** | `ServerName::try_from` fails before the TCP connect (`tls.rs`), the `Err` arm in `intercept()` counts `upstream_failures` and returns; no splice path exists. Proven by the new unit test |
| N6 | **fixed** | The unconditional `take()` deleted. Every `unsent` path leaves `is_closed()` true (hyper drains the request channel on receiver drop), and `attempt` already replaces a closed sender under the lock — so deleting the line *is* the `is_closed()`-guarded form; a sender a concurrent stream rebuilt is now reused, never discarded |
| N11 | **fixed** | `Interception::client_count` removed (no caller) |
| N12 | **fixed** | `Upstream` carries `authority: Authority` + `host_header: HeaderValue`, built once per session from the verified SNI (`forward_authority`, before the upstream connect; a failure — unreachable after the SNI grammar — closes with `status 0`). `frame_for` takes them; `authority_of` is private again |
| N2 | **fixed** | `Activity::written` is `Option<L>`; the splice leg passes `Some(&counter)`, the terminate leg `None`. Cost: one `Option` check per `poll_write` on the splice leg; one fewer `Arc` per intercepted session |
| N3 | **fixed** | `frame_for` H2 arm removes `Host`; `:authority` alone reaches an h2 origin |

Tests: `a_host_naming_another_port_is_refused_as_misdirected` (h1, `Host:
origin.test:<other>` ⇒ 421, origin requests 0, session survives) ·
`a_malformed_entry_is_rejected_by_name` (`""`, `"."`, `*.`, `https://`,
trailing `/`, `:443`, leading hyphen, empty label) ·
`every_baseline_entry_is_a_valid_hostname` · `user_entries_are_normalized`
(replaces the blank-dropped test) · `a_name_rustls_rejects_never_reaches_a_socket`
(`origin.123` ⇒ `InvalidInput`, not a certificate error, listener never
accepts) · the test origin now reports `x-host-header` present/absent and
`filtered_end_to_end` asserts h1 origin = present, h2 origin = absent in all
four pairings. N6 has no new test: the race is not reproducible on demand;
both reconnect suites still pass. A label-final hyphen (`bad-.example`) is
accepted by the p3-03 grammar and therefore by the exclusion list — not
re-litigated.

Gates: `cargo fmt --all -- --check` clean · `cargo clippy --workspace
--all-targets --all-features -- -D warnings` clean · `cargo test
--all-features --workspace` 1409 passed, 0 failed.

Doc edits **applied** with owner approval: CONFIGURATION.md
`exclude_domains` — a non-hostname entry (wildcard, scheme, path, port) is
rejected at load, by name; API.md `421` — also when the `Host`/`:authority`
names a port other than the origin port. Verdict unchanged:
**PASS WITH DEFERRED FINDINGS**; `AWAITING SOAK` until p3-06.

## Ownership trace of every remaining item, 2026-09-02

Each open item traced against `p3-05-dot-doh-listeners-plan.md` and
`p3-06-phase3-verification-plan.md`; the plan text was updated where a phase
owns it, and the decision recorded here where none does. No code changed. No
finding was downgraded: "neither" means no existing plan supports the
change, not that the item is closed.

| Finding | Owner | Required before that task's DONE? | Plan section updated | Reason |
| --- | --- | --- | --- | --- |
| M4 rows 5–9 + N10 (concurrent h2, 4 MiB POST, disconnect mid-body + permit, shutdown with live sessions, IPv6 e2e) | p3-06 | yes — Step 2 evidence | p3-06 §Step 2 "p3-04 carry-over" table | integration tests the accepted p3-04 review deferred; lifecycle/memory proofs |
| L2 no-CA ⇒ close | neither (owner decision stands) · p3-06 sequencing only | p3-06: walkthrough order | p3-06 §Step 4.2 "Sequencing" | decision recorded; the device walkthrough must install the CA before listing the client or every connection closes |
| L4 detached tasks / shutdown semantics | p3-06 | soak evidence (RSS) + recorded semantics | p3-06 §Step 4.6 fourth watch item; §Step 2 table row | lifecycle/soak; pre-existing, shared with :80; no plan owns a cross-listener cancellation redesign |
| L5 counter mix + `/telemetry` TODO | p3-06 | **yes — prerequisite for the existing soak watch item** | p3-06 §Step 4.6 "Prerequisite" | `non_tls`/`hello_timeouts`/`upstream_cert_failures` are counted but published nowhere; Step 4.6 already consumes them |
| L7 listed clients lose ECH | p3-06 | walkthrough check | p3-06 §Step 4.2 "ECH" | device validation; documented in SECURITY.md |
| N4 prewarm-then-evict (shared 512 LRU) | **p3-05** (re-warm interval + test) · p3-06 (detector) | p3-05: **yes**; p3-06: soak watch | p3-05 §TASK START 4, §Step 5 re-warm bullet; p3-06 §Step 4.6 third watch item | p3-04's terminate leg now shares the LRU with the DoT leaf: eviction between re-warms serves the fallback, which hostname mode rejects — a p3-05 correctness dependency inside the plan's own "interval is free to be short" |
| N7 h2 GOAWAY window ⇒ 502, no retry | neither | no | — | hyper-inherent; browsers retry; no plan supports a change |
| N8 `spawn_blocking` on cache hit | p3-06 | measure first (diagnostic) | p3-06 §Step 1 "p3-04 carry-over" | perf; principle 8 |
| N9 no upstream response-head deadline | neither | no | — | parity with :80; a deadline is a new design across both proxies; bounded by `idle_timeout` |
| N1 empty-list unit test, N5 wall-clock assert | neither | no | — | test hygiene; N1's property is proven at full-binary level by p3-06 Step 2.2 |
| S1 h2 limits unmeasured | p3-06 | measured trade, owner decision | p3-06 §Step 1 "p3-04 carry-over" | memory/perf measurement before tuning |
| `BASELINE_EXCLUSIONS` first cut | p3-06 | yes — before the pinned-app check | p3-06 §Step 4.4 | owner decision; the spot check is meaningless otherwise |
| ROADMAP.md delivered wording | p3-06 | doc sweep at phase close | p3-06 §Step 5 | existing phase documentation contract (p3-04 plan §Doc changes) |
| GAR §5.14 static leases per device | p3-06 | already required | p3-06 §Step 4.4 (unchanged) | already owned |
| RB5009 handshake/first-byte, leaf hit/miss, phone with CA, banking app | p3-06 | already required | p3-06 §Step 1 rows, §Step 4.2/4.4/4.5 (unchanged) | already owned |

Nothing was placed in p3-05 beyond N4's p3-05 half; p3-05's API,
architecture and acceptance criteria are otherwise independent of p3-04.
