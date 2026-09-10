# P3-08 — Client Certificate Rejection — Implementation Plan

**Task:** [p3-08-client-cert-rejection.md](p3-08-client-cert-rejection.md) ·
**ADR:** [ADR-0008](../../../docs/decisions/0008-live-interception-and-client-certificate-rejection.md)
frozen at `fcc7244` · **Depends on:** p3-07 (§11 states the contract) ·
**Status:** plan revised 2026-09-10 after owner review and the final
pre-implementation gate (F6, F7, F8 folded in, §14); decisions frozen (§12);
awaiting implementation approval. Nothing implemented.

## 1. Objective and scope

The accept arm of `intercept()` classifies the `rustls` accept failure. A
client TLS alert that rejects the certificate we presented becomes an `https`
event with status **525** (`ClientCertRejected`) and one tick of a new
`client_cert_rejections` counter. `UnknownCA` and every other failure stay on
`status 0`, by an explicit fallback pinned by test. No new event kind, no
write to the Interception Document, no change to the success path.

## 2. Existing code paths

| Symbol | File | Role |
| --- | --- | --- |
| accept arm `Ok(Err(err)) => { debug!(…); self.emit_session(&host, &session, 0); return; }` | `crates/fah-http/src/intercept.rs:150-153` | the observation thrown away today |
| deadline arm `Err(_) =>` | `intercept.rs:155-158` | our own handshake deadline, stays `0` |
| `UPSTREAM_CERT_FAILURE: u16 = 526`, `upstream_cert_failure_status()` | `intercept.rs:36,43` | the private-code precedent |
| `emit_session(host, session, status)` → `session_event(...)` → `Event::https` | `intercept.rs:227-241`, `https.rs:368-390` | the connection-level `https` item: empty method/path, `bytes 0` |
| `Session { peer, address, verdict, policy, started }` | `https.rs:360-366` | what the event carries |
| `certificate_error(&io::Error) -> bool` | `crates/fah-http/src/tls.rs:85-91` | `InvalidData` + `rustls::Error::InvalidCertificate(_)` — the connect-side shape; **not reused** |
| `ProxyCounters { …, upstream_cert_failures, … }` → `ProxyStats` → `fah_model::ListenerCounters` | `crates/fah-http/src/proxy.rs:73-141`, `crates/fah-model/src/engine.rs:140-155` | counters surfaced in `GET /api/v1/telemetry` (API.md line 143, 186-190) |
| `Event::Https`, `EventKind`, `RequestEvent.status: u16` | `crates/fah-model/src/request_event.rs` | no new kind needed |
| measured contract | `crates/fah-http/tests/client_rejection.rs` | `InvalidData` + `AlertReceived(desc)` on TLS 1.3 and 1.2; `Rejecting` verifier, `rejecting_connector(verdict, versions)`, `untrusting_connector()`, `alert(&io::Error)` helper; wire values 0x2a/0x2e/0x30/0x31; close-without-alert has nothing to classify |
| harness (after p3-07) | `crates/fah-http/tests/interception.rs`: `Setup`, `harness()`, `tls_to_name`, `next_event`, `https_event`, `Harness.counters`, `Harness.state`, `store.minted_total()` | the wire path a rejecting client is driven through |
| raw ClientHello builders | `crates/fah-http/src/sni.rs:299` `tests::hello` is `#[cfg(test)] pub(crate)` — **unreachable from `tests/`**; `crates/fah-http/benches/proxy.rs:279 client_hello(host)` and `crates/fastadhunter/tests/common` `client_hello` are the reachable shapes | the close-after-hello test needs one in `tests/common/mod.rs` (F6) |
| `ListenerCounters` literal | `crates/fah-api/tests/api.rs:373` | struct literal; gains the new field or `..Default::default()` (F7) |
| `Interception { server_config, client_config, store, state }` (after p3-07) | `intercept.rs` | the accept arm must not read `state` |
| dashboard `Detail` cell | `dashboard/frontend/src/pages/live-feed/detail.tsx` | already renders an `https` session row with empty parts; a 525 renders as its status |

Verified against rustls 0.23.42 by the existing test: a fatal alert arrives as
`io::Error { kind: InvalidData, inner: rustls::Error::AlertReceived(AlertDescription) }`;
`InvalidCertificate` never appears on the accept side; a TCP close arrives as
`UnexpectedEof` with no inner `rustls::Error`.

## 3. Design

### 3.1 Predicate and classification

```text
tls.rs       pub(crate) fn client_alert(err: &io::Error) -> Option<rustls::AlertDescription>
             = kind == InvalidData && downcast::<rustls::Error>() matches AlertReceived(d) → Some(d)
intercept.rs const CLIENT_CERT_REJECTED: u16 = 525;
             fn rejection_status(alert: AlertDescription) -> Option<u16>
             = BadCertificate | CertificateUnknown | AccessDenied → Some(525); _ → None
```

`client_alert` sits beside `certificate_error` because both downcast the same
`io::Error`, and it is a different function because it matches a different
variant. `rejection_status` is a closed `match` with a wildcard arm that
**returns `None` on purpose** — the ADR's "intentional fallback, not a missing
arm" — and a test table documents it (§7).

Classified set, on the ADR's evidence and definition:

| Alert | Wire | Status | Counter | Basis |
| --- | --- | --- | --- | --- |
| `BadCertificate` | 0x2a | 525 | +1 | measured: `NotValidForName` verdict |
| `AccessDenied` | 0x31 | 525 | +1 | measured: `ApplicationVerificationFailure` verdict |
| `CertificateUnknown` | 0x2e | 525 | +1 | RFC definition; wire value pinned |
| `UnknownCA` | 0x30 | 0 | none | owner decision 2026-09-10: trust diagnostic, not an exclusion signal, not counted |
| every other alert, transport error, EOF, deadline | — | 0 | none | intentional fallback |

### 3.2 The accept arm after the change

```text
Ok(Err(err)) => {
    let status = client_alert(&err).and_then(rejection_status).unwrap_or(0);
    if status == CLIENT_CERT_REJECTED {
        self.counters.client_cert_rejections.fetch_add(1, Relaxed);
        debug!(%peer, %host, alert = ?…, "the client rejected our certificate");
    } else {
        debug!(%peer, %host, error = %err, "the client did not complete our handshake");
    }
    self.emit_session(&host, &session, status);
    return;
}
```

Both lines stay at `debug` (frozen): a pinned application retries every few
seconds, and an `info` line per attempt would flood the log the ADR calls the
audit surface. The event is the same `https` item the arm emits today with one
field changed. The deadline arm is untouched. The success path is untouched.
The arm never touches `interception.state`.

### 3.3 What is deliberately not done

No new `Event` kind or `EventKind` variant; no field on `RequestEvent`; no
`UnknownCA` status or counter; no write, mark, or staging toward the
Interception Document (`fah-http` cannot reach `fah-api`; `layering.rs`); no
change to the `https-sni` leg (a pinned application's SNI verdict is `pass`
and the failure happens after it).

## 4. File-by-file changes

**1 · `crates/fah-http/src/tls.rs`** — add `client_alert` (§3.1). Why: the
detector needs its own predicate; `certificate_error` would classify nothing.
Invariant: `certificate_error` unchanged. Tests: unit, §7.1.

**2 · `crates/fah-http/src/intercept.rs`** — `CLIENT_CERT_REJECTED`,
`rejection_status`, the accept arm (§3.2). Why: the observation becomes the
event. Invariants: every other arm emits what it emits today; the arm still
returns without a ServerHello being completed; `intercept()` never reads
`Interception::state`. Tests: §7.1 table, §7.2 wire.

**3 · `crates/fah-http/src/proxy.rs`** — `ProxyCounters.client_cert_rejections:
AtomicU64`, `ProxyStats.client_cert_rejections: u64`, the two conversions
(frozen: the counter ships). Why: same convention as 526
(`upstream_cert_failures`), and the only place the count exists once the event
has left the bounded channel. Invariant: counters are `Relaxed` atomics, no
lock.

**4 · `crates/fah-model/src/engine.rs`** — `ListenerCounters.client_cert_rejections: u64`
with `#[serde(default)]`. Additive. The one struct-literal construction in
the tree, `crates/fah-api/tests/api.rs:373`, gains the field (F7). The
dashboard has no `ListenerCounters` type — `Telemetry` in
`dashboard/frontend/src/api/types.ts:166` carries only `counters: Counters`
— so the counter is API telemetry only and is not consumed by the dashboard
in this phase (F8).

**5 · `crates/fah-http/tests/common/mod.rs` (new)** — `provider()`, the
`Rejecting` verifier, `rejecting_connector(verdict, versions)` and
`untrusting_connector()` moved out of `client_rejection.rs`, plus a
`client_hello(host: &str) -> Vec<u8>` builder (F6) — a minimal TLS 1.2-style
ClientHello record carrying one `server_name` extension, the shape
`benches/proxy.rs:279` already builds; the library's `sni::tests::hello` is
`#[cfg(test)] pub(crate)` and cannot be named from an integration test. Both
integration files declare `mod common;`. Why: the harness tests need the same
rejecting client and one raw hello; principle 4. `client_rejection.rs` keeps
its four tests unchanged.

**6 · `crates/fah-http/tests/interception.rs`** — the wire tests of §7.2,
built on p3-07's `Setup`/`Active::compile` harness.

**7 · docs (not edited here, §8).**

## 5. Data and control flow

Before: rejecting client → `acceptor.accept()` fails → `debug!` → `https`
event `status 0` → counters untouched.

After: the same path, with `client_alert` and `rejection_status` between the
failure and `emit_session`; 525 on the classified set, `0` otherwise; the
counter ticks on 525 only. The event travels the existing bounded channel to
`fah-stats`/`fah-api` and reaches the dashboard as a `query` item with
`kind: "https"`, `status: 525`, `method: ""`, `path: ""`, `bytes: 0`.

## 6. Runtime, concurrency, performance, memory, security

- **Lifecycle:** nothing new — no task, state or timer. The classification is a
  pure function on an error already in hand. Shutdown and cancellation are
  those of the connection task, unchanged.
- **Concurrency:** the arm runs inside the connection's task; the counter is a
  `Relaxed` atomic as its siblings are; no shared state is read or written.
- **Hot path:** the success path is untouched; the predicate runs only in the
  failure arm (one downcast, one match, no allocation). No bench owed.
- **Memory:** none; the event has the same shape and size.
- **Error propagation / atomicity:** the arm cannot fail — an unrecognised
  error is `0`; nothing is persisted, so nothing can half-apply.
- **Security/correctness:** the client observed our leaf and refused it — the
  handshake ended without application data; nothing is decrypted, nothing is
  forwarded, the connection closes as today. Detection observes and cannot
  mutate policy (structural). The feature is best-effort: a client that closes
  without an alert is `0` and does not appear anywhere — API.md must say so.

## 7. Test strategy

### 7.1 Unit (in-crate)

- `tls.rs`: `client_alert_matches_only_an_alert_received` — constructed
  `io::Error`s: `InvalidData` + `AlertReceived(AccessDenied)` → `Some`;
  `InvalidData` + `InvalidCertificate(…)` → `None`; `UnexpectedEof` → `None`;
  `InvalidData` with a non-rustls inner → `None`.
- `intercept.rs`: `rejection_status_classifies_exactly_three_alerts` — a table
  over every named `AlertDescription` variant plus `Unknown(0xff)`: only
  `BadCertificate`, `CertificateUnknown`, `AccessDenied` map to 525; the
  table lists `UnknownCA → None` and `CertificateExpired → None` explicitly,
  so the fallback is asserted by name, not by omission.
- `the_private_codes_are_distinct` — 525 ≠ 526.

### 7.2 Wire (`tests/interception.rs`, listed client via `Active::compile`, origin verified)

- `a_client_refusing_our_leaf_emits_one_https_event_with_status_525`:
  `rejecting_connector(ApplicationVerificationFailure, ALL_VERSIONS)` against
  the harness with SNI `origin.test` → exactly one event, `EventKind::Https`,
  `status 525`, `domain origin.test`, client `127.0.0.1`, empty method and
  path, `bytes 0`; `counters.client_cert_rejections == 1`;
  `store.minted_total() == 1` (the leaf was minted before the accept); no
  `https-sni` item for the session.
- `a_name_rejection_is_also_525` (`NotValidForName` → `BadCertificate`).
- `a_tls12_rejection_is_525` (`ApplicationVerificationFailure`, TLS 1.2 only).
- `an_untrusting_client_stays_on_status_0_and_is_not_counted`:
  `untrusting_connector()` → `status 0`, counter unchanged.
- `an_expired_verdict_is_an_intentional_fallback_to_0`:
  `rejecting_connector(CertificateError::Expired, …)` → the client sends
  `CertificateExpired` → `status 0`. If rustls maps the verdict differently,
  the test asserts whatever alert arrives is unclassified — the point is a
  real alert outside the set.
- `a_client_that_closes_after_the_hello_stays_on_status_0`: send
  `common::client_hello("origin.test")` (F6) over a plain `TcpStream`, then
  `shutdown()` → the proxy connects and verifies the origin, mints the leaf,
  and `accept()` fails reading the client's next record without an alert
  (`UnexpectedEof`) → `status 0`, counter unchanged, `minted_total == 1`.
- `a_rejection_never_touches_the_interception_state`: `Arc::ptr_eq` on
  `harness.state.current()` before and after; the document is a harness
  concern only when one exists — assert the state, which is what the arm could
  reach.
- `a_rejection_returns_its_permit_as_a_normal_close_does` (reuse the pattern
  of `an_idle_intercepted_session_is_closed_and_its_permit_returned`).

### 7.3 Regression

`client_rejection.rs` unchanged and green — it is the measured contract the
design rests on. `layering.rs` green (this task touches `fah-http`; its
manifest gains nothing). The full `interception.rs` suite green.

### 7.4 Gates

`cargo fmt`, `clippy -D warnings`, `cargo test --all-features --workspace`.

## 8. Documentation consequences (not edited here)

- API.md: on the `https` item — "**`525`** — the client rejected the
  certificate we presented (a TLS alert of `bad_certificate`,
  `certificate_unknown` or `access_denied`); `unknown_ca` and any failure that
  is not one of those alerts stay `0`. Best-effort: a client that closes
  without alerting is `0`"; telemetry `client_cert_rejections` beside
  `upstream_cert_failures`.
- CONTEXT.md: **Client Certificate Rejection** (`ClientCertRejected`, 525):
  what was observed, not why — never "pinned".
- SECURITY.md interception bullets: one sentence that a client refusing our
  leaf is closed unanswered and surfaced as 525; nothing is excluded.

## 9. Acceptance mapping (ADR-0008 §Acceptance)

| ADR line | Proof |
| --- | --- |
| 9 · An accept failure carrying no classified alert stays on `status 0` | §7.1 table, §7.2 expired-verdict and close-after-hello tests |
| 10 · `UnknownCA` is never a 525 | §7.1 table row, §7.2 untrusting-client test (status and counter) |
| 11 · No new `Event` kind | `EventKind` untouched; §7.2 asserts `EventKind::Https`; the dashboard `KINDS` list is unchanged |
| 8 · `fah-http` has no path to `fah-api` (cross-task, p3-08 half) | `layering.rs` green with this task's `fah-http` edits; manifest unchanged |
| 12 · Nothing writes the document from observation (cross-task, p3-08 half) | `layering.rs`; §7.2 pointer-equality test |

## 10. Out of scope and follow-ups

The view and the action (p3-09); any `UnknownCA` surface, counter or
client-level diagnosis (ADR §Revisit); rejections that close without an alert;
a "pinned" label anywhere; rate-limiting the `debug!` line (the level is the
throttle).

## 11. Dependencies — what p3-08 consumes from p3-07, and why the order holds

p3-08 is not merely "after" p3-07. It builds on three things p3-07 defines:

1. **The runtime contract of `Interception`.** p3-07 replaces
   `Interception { clients, exclusions }` with
   `Interception { …, state: Arc<InterceptionState> }` and makes
   `interception_for` the single reader of the scope. The accept arm p3-08
   edits sits inside `intercept()`, which p3-07 specifies as never reading
   `state`. p3-08's change and its test
   `a_rejection_never_touches_the_interception_state` are written against
   that invariant and that type; written against p3-04's shape they would be
   rewritten by p3-07 anyway.
2. **The harness contract.** After p3-07, a listed client in the wire tests is
   expressed as `Active::compile(InterceptionDocument { clients: ["127.0.0.1"], .. })`
   published into `Harness.state`. Every p3-08 wire test needs a listed client
   to reach the terminate leg; p3-08 therefore consumes p3-07's `Setup`,
   `Active::compile` and `Harness.state`.
3. **The API-side contract p3-09 needs from both.** p3-09's rejection view
   consumes p3-08's 525 events and p3-07's `GET`/`PUT` with `details`. If
   p3-08 landed first, 525 rows would exist with no live exclusion to act on —
   the ADR's "legible failure the operator still cannot fix without a
   restart". The release is one build, so the order is about which task's
   contract the next is written against, and about never having a build state
   where detection exists without the document.

What p3-08 does **not** depend on: p3-07's migration, endpoint, runtime status
or `details` contract — the detector reads none of them. `layering.rs`
guarantees it cannot.

Order: p3-07 → p3-08 → p3-09.

## 12. Decisions — frozen by owner review, 2026-09-10

| # | Decision | Frozen as |
| --- | --- | --- |
| 6a | Telemetry counter | `client_cert_rejections` added beside `upstream_cert_failures`, ticks on 525 only |
| 6b | Log level | `debug` for both branches |
| 6c | `UnknownCA` | status 0, not counted, never a 525 |
| — | `CertificateExpired` mapping | asserted by test; expectation `CertificateError::Expired → CertificateExpired`; unclassified either way |
| — | Shared test module | `tests/common/mod.rs` (new) rather than duplicating the verifier; carries `client_hello` too (F6) |
| — | `ListenerCounters` wire addition | `#[serde(default)]`; API telemetry only — the dashboard has no such type and does not read it in this phase (F8) |

## 14. Final gate corrections (2026-09-10)

| # | Finding | Where fixed |
| --- | --- | --- |
| F6 | close-after-hello test cited `sni.rs` builders that are `#[cfg(test)] pub(crate)` | §2 row, §4 step 5, §7.2 |
| F7 | `crates/fah-api/tests/api.rs:373` `ListenerCounters` literal not listed | §2 row, §4 step 4 |
| F8 | plan named a dashboard `ListenerCounters` type that does not exist | §4 step 4, §12 |
| F9 (p3-07) | `compile` → `Active::compile` | §11 |

## 13. Final verification pass (owner's checklist, p3-08 half)

| Check | Where it holds |
| --- | --- |
| Acceptance lines 9, 10, 11 covered once; 8 and 12 as the p3-08 half of cross-task lines | §9 |
| No scope expansion | one predicate, one constant, one arm, one counter (frozen), one shared test module; nothing on the success path, nothing on the document |
| Concurrency / cancellation / shutdown / atomicity concrete | §6: pure function in the connection task; no shared state |
| No hot-path blocking or allocation | §6: failure arm only; downcast + match |
| `fah-api` has no dependency on `fah-http` | untouched; `layering.rs` |
| Detection cannot mutate live policy; no auto-exclusion | §3.3, §7.2 pointer-equality; `fah-http` cannot name `fah-api` |
| Bootstrap with `clients=[]` | unaffected by this task; p3-07's invariant preserved (the arm reads no scope) |
| Migration cannot overwrite `interception.json` | not touched by this task |
