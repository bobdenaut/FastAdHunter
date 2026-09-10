# P3-08 — Client Certificate Rejection — Review

**Task:** [p3-08-client-cert-rejection.md](../../../plan/wip/phase3/p3-08-client-cert-rejection.md) ·
**Plan:** [p3-08-client-cert-rejection-plan.md](../../../plan/wip/phase3/p3-08-client-cert-rejection-plan.md) ·
**ADR:** [ADR-0008](../../decisions/0008-live-interception-and-client-certificate-rejection.md)
step 2 · **Depends on:** p3-07 · **Date:** 2026-09-10

## Implementation Summary

The accept arm of `intercept()` classifies the `rustls` accept failure. A client
TLS alert that rejects the leaf we presented becomes an `https` event with
status **525** plus one tick of `client_cert_rejections`. `UnknownCA` and every
other failure stay on `status 0` by an explicit, test-pinned fallback. No new
event kind, no write to the Interception Document, no change to the success
path.

Implemented exactly as the plan's §4 file-by-file list; no scope added, none
dropped. The plan's §8 doc consequences (API.md, CONTEXT.md, SECURITY.md) are
written in the same change, after the owner's explicit approval.

### Predicate and classification

| Symbol | File | Shape |
| --- | --- | --- |
| `client_alert(&io::Error) -> Option<AlertDescription>` | `crates/fah-http/src/tls.rs:93` | `InvalidData` + downcast to `rustls::Error::AlertReceived(d)` |
| `CLIENT_CERT_REJECTED: u16 = 525` | `crates/fah-http/src/intercept.rs:37` | beside `UPSTREAM_CERT_FAILURE = 526` |
| `rejection_status(AlertDescription) -> Option<u16>` | `crates/fah-http/src/intercept.rs:49` | closed match, wildcard `None` |

| Alert | Wire | Status | Counter |
| --- | --- | --- | --- |
| `BadCertificate` | 0x2a | 525 | +1 |
| `CertificateUnknown` | 0x2e | 525 | +1 |
| `AccessDenied` | 0x31 | 525 | +1 |
| `UnknownCA` | 0x30 | 0 | none |
| any other alert / transport error / EOF / deadline | — | 0 | none |

`client_alert` sits beside `certificate_error` and does not reuse it: the two
match different `rustls::Error` variants (accept side never produces
`InvalidCertificate`). `certificate_error` is unchanged.

### Files changed

| # | File | Change |
| --- | --- | --- |
| 1 | `crates/fah-http/src/tls.rs` | `client_alert` + unit test |
| 2 | `crates/fah-http/src/intercept.rs` | constant, `rejection_status`, accept arm, 2 unit tests |
| 3 | `crates/fah-http/src/proxy.rs` | `ProxyCounters.client_cert_rejections` (`AtomicU64`), `ProxyStats.client_cert_rejections`, both conversions |
| 4 | `crates/fah-model/src/engine.rs` | `ListenerCounters.client_cert_rejections: u64`, `#[serde(default)]` |
| 5 | `crates/fah-api/tests/api.rs` | the one `ListenerCounters` struct literal in the tree gains the field (plan F7) |
| 6 | `crates/fah-http/tests/common/mod.rs` (new) | `provider`, `Rejecting`, `rejecting_connector`, `untrusting_connector`, `client_hello` (a real ClientHello from `rustls::ClientConnection::write_tls`, after F-01) |
| 7 | `crates/fah-http/tests/client_rejection.rs` | helpers moved to `common`; its four tests unchanged |
| 8 | `crates/fah-http/tests/interception.rs` | local `provider()` deleted (now `common::provider`); 9 wire tests + `refused_leaf` helper |
| 9 | `API.md` | `525` added to the synthesized `https` statuses; the `status 0` sentence no longer claims a client rejection is `0`; telemetry prose + both JSON examples gain `client_cert_rejections` |
| 10 | `CONTEXT.md` | new term **Client Certificate Rejection**, placed before §Destination Claim |
| 11 | `SECURITY.md` | new interception bullet: a refused leaf is closed, surfaced as 525, and excludes nothing |

Code diff: 7 modified files + 1 new, 352 insertions / 90 deletions. Docs edited
in the same change on the owner's explicit approval (plan §8).

### Accept arm

```rust
Ok(Err(err)) => {
    match client_alert(&err).and_then(|alert| Some((alert, rejection_status(alert)?))) {
        Some((alert, status)) => {
            self.counters.client_cert_rejections.fetch_add(1, Ordering::Relaxed);
            debug!(%peer, %host, ?alert, "the client rejected our certificate");
            self.emit_session(&host, &session, status);
        }
        None => {
            debug!(%peer, %host, error = %err, "the client did not complete our handshake");
            self.emit_session(&host, &session, 0);
        }
    }
    return;
}
```

Deviation from the plan's §3.2 sketch: the sketch computed
`…unwrap_or(0)` and logged `alert = ?…` from an `Option`. The `match` binds the
`AlertDescription` itself, so the `debug!` line carries `alert=AccessDenied`
rather than `alert=Some(AccessDenied)`. Same statuses, same counter, same
single `emit_session` per branch, no extra work on any path.

## Decisions

- Both branches stay at `debug` (plan 6b): a pinned application retries every
  few seconds; `info` would flood the audit surface.
- Counter is a `Relaxed` atomic beside `upstream_cert_failures` (plan 6a); it
  is the only place the count survives once the event leaves the bounded
  channel.
- `ListenerCounters` gains `#[serde(default)]` — additive on the wire, so an
  older telemetry payload still deserializes. The dashboard has no
  `ListenerCounters` type (plan F8), so this is API telemetry only.
- `tests/common/mod.rs` carries `#![allow(dead_code)]`, matching
  `crates/fastadhunter/tests/common/mod.rs` and `fah-dns/tests/support`: each
  test binary uses a subset.
- `interception.rs`'s local `provider()` was deleted rather than kept beside
  `common::provider` (principle 4).

## Tests

### Unit

| Test | File | Pins |
| --- | --- | --- |
| `client_alert_matches_only_an_alert_received` | `src/tls.rs` | `AlertReceived` → `Some`; `InvalidCertificate`, non-rustls inner, `UnexpectedEof`, non-`InvalidData` kind → `None` |
| `rejection_status_classifies_exactly_three_alerts` | `src/intercept.rs` | all 35 named `AlertDescription` variants + `Unknown(0xff)`; `UnknownCA` and `CertificateExpired` asserted **by name**, not by omission |
| `the_private_codes_are_distinct` | `src/intercept.rs` | 525 ≠ 526, both literal |

### Wire (`tests/interception.rs`, listed client, origin verified)

Seven of the nine go through `refused_leaf`, which asserts on every case: exactly one
event, `EventKind::Https`, host `origin.test`, client `127.0.0.1`, empty method
and path, `bytes 0`, no second event on the channel (no `https-sni` item),
`minted_total == 1`, origin connected once and served zero requests.

| Test | Client | Status | Counter |
| --- | --- | --- | --- |
| `a_client_refusing_our_leaf_emits_one_https_event_with_status_525` | `ApplicationVerificationFailure`, all versions | 525 | 1 |
| `a_name_rejection_is_also_525` | `NotValidForName` | 525 | 1 |
| `a_tls12_rejection_is_525` | `ApplicationVerificationFailure`, TLS 1.2 only | 525 | 1 |
| `an_unspecified_rejection_is_also_525` (F-02) | `CertificateError::Other` → `CertificateUnknown` (0x2e) | 525 | 1 |
| `an_untrusting_client_stays_on_status_0_and_is_not_counted` | empty root store → `UnknownCA` | 0 | 0 |
| `an_expired_verdict_is_an_intentional_fallback_to_0` | `CertificateError::Expired` → `CertificateExpired` | 0 | 0 |
| `a_rejection_never_touches_the_interception_state` | pinning | 525 | `Arc::ptr_eq` on `state.current()` before/after |
| `a_client_that_closes_after_the_hello_stays_on_status_0` | a real ClientHello (`common::client_hello`) then `shutdown()`: the server completes its flight and reads EOF (`UnexpectedEof`, tokio-rustls 0.26.4 `common/mod.rs:175`); origin connected once, leaf minted once | 0 | 0 |
| `a_rejection_returns_its_permit_as_a_normal_close_does` | pinning, `max_connections = 1` | — | gauge to zero, next session served |

`CertificateError::Expired → CertificateExpired` was verified against rustls
0.23.42 `error.rs:633` and holds at runtime; the test asserts status 0 either
way, so a different mapping inside the unclassified set does not break it.

### Gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | 53 suites ok, 0 failed |
| `fah-http` `interception` + `client_rejection` | 45 + 4 passed (after the review fixes) |
| `layering.rs` | green — `fah-http` still L1/L2 only, manifest unchanged |

No bench. The success path is untouched; the predicate runs only in the failure
arm (one downcast, one match, no allocation), so the plan's §6 "no bench owed"
holds and the task's acceptance line is satisfied.

## Known limitations / deferred

- Best-effort by construction: a client that closes without alerting is `0` and
  appears nowhere as a rejection. API.md and CONTEXT.md both say so.
- `UnknownCA` has no surface, counter or client-level diagnosis (ADR §Revisit).
- The rejection view and the exclude action are p3-09.

## Findings

Reviewer: Fable 5.1, 2026-09-10. Scope: `1a869d4` against `43fb31b`, checked
against the plan (`p3-08-client-cert-rejection-plan.md`), the task file and
ADR-0008 §What counts as a rejection / §Acceptance. Every line below was
verified in the tree or in the vendored rustls 0.23.42 / tokio-rustls 0.26.4
sources, not against the summary above.

### Gates re-run by the reviewer

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --message-format=short -- -D warnings` | clean |
| `cargo test -p fah-http --all-features --lib --test interception --test client_rejection` | 84 + 44 + 4 passed |
| `cargo test -p fastadhunter --test layering` | green |
| `cargo test -p fah-api --test api` | 130 passed |

### Severity-ranked

Blockers: **none**.

**F-01 · should-fix · test coverage** —
[interception.rs:2438-2472](../../../crates/fah-http/tests/interception.rs#L2438-L2472),
[common/mod.rs:89-122](../../../crates/fah-http/tests/common/mod.rs#L89-L122)

- Evidence: `client_hello` carries one extension, `server_name`, and no
  `signature_algorithms`. rustls 0.23.42 `src/server/hs.rs:747-755` rejects
  such a hello with a fatal `handshake_failure` /
  `PeerIncompatible::SignatureAlgorithmsExtensionRequired` while processing
  the ClientHello itself, before reading any further record; tokio-rustls
  0.26.4 `src/common/mod.rs:109-116` wraps that as
  `io::Error { InvalidData, PeerIncompatible(..) }`. The `shutdown()` at line
  2456 is never observed by the acceptor.
- Why: the arm sees an unacceptable hello, not the EOF that the test name, plan
  §7.2 ("`accept()` fails reading the client's next record without an alert
  (`UnexpectedEof`)"), task §Tests ("transport close ⇒ `0`") and the Tests table
  above all describe. Status 0 holds for both, so the test is green, but the
  RST/FIN case ADR-0008 §What counts as a rejection names is not on the wire in
  this suite, and plan §9 row 9 claims it is. The plan's lead that
  `benches/proxy.rs:279` is "the shape" was false for this leg: that builder
  only ever feeds the SNI parser and the splice leg, never a rustls server.
- Fix: build the hello with a real client —
  `rustls::ClientConnection::new(config, ServerName)` then
  `write_tls(&mut Vec::new())` — send it, `shutdown()`. The server then
  completes its flight and fails on the client's next record with
  `UnexpectedEof`. Keep the assertions; add `origin.connections == 1`. The
  hand-rolled builder (a byte-for-byte twin of `benches/proxy.rs:279-311`,
  principle 4) then leaves `common`.

**F-02 · note · test coverage** —
[interception.rs:2364-2398](../../../crates/fah-http/tests/interception.rs#L2364-L2398)

- Evidence: wire cases drive `BadCertificate` (`NotValidForName`) and
  `AccessDenied` (`ApplicationVerificationFailure`); none drives
  `CertificateUnknown`. rustls 0.23.42 `src/error.rs:655` maps
  `CertificateError::Other(..)` → `CertificateUnknown`, so
  `rejecting_connector(CertificateError::Other(OtherError(Arc::new(..))), ALL_VERSIONS)`
  reaches the third classified alert through `refused_leaf` unchanged.
- Why: task §Tests says "rejecting connector per classified alert"; plan §7.2
  left the third to the unit table and the `client_rejection.rs` wire value.
  The 525 event for 0x2e is pinned by composition, not by a test.
- Fix: one test, one `Refusal`.

**F-03 · note · Rust quality** —
[intercept.rs:151](../../../crates/fah-http/src/intercept.rs#L151)

- Evidence: `client_alert(&err).and_then(|alert| Some((alert, rejection_status(alert)?)))`.
- Why: `?` inside `Some(..)` is a control-flow trick where
  `rejection_status(alert).map(|status| (alert, status))` says the same thing
  in the plain idiom. `rejection_status` only ever yields
  `Some(CLIENT_CERT_REJECTED)`, so `status` is always 525 — fine while the
  plan's `Option<u16>` shape stays.
- Fix: the `map` form; no behaviour change.

**F-04 · note · docs precision** —
[SECURITY.md:299-300](../../../SECURITY.md#L299-L300)

- Evidence: "A client that answers our ServerHello with a certificate alert".
- Why: the alert answers the Certificate / CertificateVerify messages (TLS 1.3:
  same flight as ServerHello; TLS 1.2: after ServerHelloDone), not the
  ServerHello. Harmless to the operator; imprecise in the governing document.
- Fix: "answers our certificate with a TLS alert". `.md` — owner yes.

**F-05 · note · docs precision** —
[CONTEXT.md:380-383](../../../CONTEXT.md#L380-L383)

- Evidence: "the leaf we minted for it" and "status **525** (`ClientCertRejected`)".
  The leaf is minted per host (`store.prewarm(&name)`,
  [intercept.rs:129](../../../crates/fah-http/src/intercept.rs#L129)) and
  presented to the client; `ClientCertRejected` matches no symbol in the tree —
  the constant is `CLIENT_CERT_REJECTED`
  ([intercept.rs:37](../../../crates/fah-http/src/intercept.rs#L37)), Rust
  casing beside `UPSTREAM_CERT_FAILURE`, while ADR-0008 and the task say
  `ClientCertRejected` "names the constant and the dashboard filter".
- Why: CONTEXT.md is binding vocabulary; an agent grepping the backticked name
  finds nothing until p3-09 adds the filter. Naming the constant in Rust casing
  is the right call and the deviation from the ADR's literal wording is
  harmless, but the doc should say which is which.
- Fix: "the leaf we minted for the host"; "(constant `CLIENT_CERT_REJECTED`;
  `ClientCertRejected` is the p3-09 filter label)". `.md` — owner yes.

### Docs checked (API.md, CONTEXT.md, SECURITY.md)

| Check | Result |
| --- | --- |
| Plan §8 coverage | API.md `https` item (525, classified set, `unknown_ca` and fallback rule, best-effort), telemetry prose and both JSON blocks; CONTEXT.md term with "observed, not why", no "pinned"; SECURITY.md bullet — all present, wording matches the code. |
| Claims vs code | API.md 1186-1200 and CONTEXT.md 380-390 name exactly `bad_certificate`, `certificate_unknown`, `access_denied` → 525 and `unknown_ca` + everything else → 0, matching `rejection_status` and the unit table. The old status-0 sentence (API.md 1203) no longer lists "the client rejected our certificate". `client_cert_rejections` sits beside `upstream_cert_failures` in both JSON blocks with the `http` block at 0. |
| Neighbours left stale | none: no other `.md` outside `plan/`, `docs/code-review/` and ADR-0008 mentions 526, the sibling counter or a client rejection; CONTEXT.md §Terms has no index to extend; SECURITY.md 294-295 (upstream `status 0`) still true. |
| Style | present tense throughout; no paragraph over five lines; links resolve (`docs/decisions/0008-…` from root, `#Client Certificate Rejection` heading exists). F-04, F-05 are the only precision defects. |

### Categories checked

| Category | Result |
| --- | --- |
| Plan compliance | §4 steps 1–6 present as specified; docs §8 in the same commit, approval recorded above. One deviation, the `match` shape, is documented above and behaviour-neutral. `rejection_status_classifies_exactly_three_alerts` lists all 35 named variants of rustls 0.23.42 `src/enums.rs` plus `Unknown(0xff)` — verified by count. Acceptance lines 9, 10, 11 covered; 8 and 12 by `layering.rs` and the `ptr_eq` test. Out-of-scope boundaries (§10) respected: no `UnknownCA` surface, no document write, no "pinned". |
| Correctness | Accept arm is a pure classification: one `emit_session`, one `return`, no `state` read; deadline arm and success path byte-identical in the diff. `client_alert` kind check + downcast matches the measured contract. A `close_notify` or warning-level alert during the handshake ends as `AlertReceived(CloseNotify)` (TLS 1.3, `common_state.rs:527-531`) or EOF — both fall to 0, consistent with "closes without alerting". Counter ticks before `publish`, so a full channel drops the event but keeps the count, same as 526. Acceptable. |
| Architecture | No new dependency; `fah-http` manifest unchanged; `layering.rs` green. `CLIENT_CERT_REJECTED` and `rejection_status` private, `client_alert` `pub(crate)`. No new `EventKind`; `fah-http` still cannot name `fah-api` or the document. Acceptable. |
| Performance | Failure arm only. `client_alert` is one `kind()` compare and one `downcast_ref`, no allocation; `?alert` formats only under `debug`. Success path untouched, so no bench owed (plan §6). Acceptable. |
| Memory | +8 B in each of `ProxyCounters`, `ProxyStats`, `ListenerCounters`; no retained state, buffer, task or timer. Acceptable. |
| Rust quality | No `unwrap`/`expect`/panic on the path; `AlertDescription` is `Copy`, no new clone; `Relaxed` matches the siblings. F-03 only. |
| Tests | F-01, F-02. The other seven wire cases assert what plan §7.2 lists; `next_event` and `gauge_settles_at_zero` are bounded at 5 s, no sleep used as synchronisation. `client_rejection.rs`'s four tests are byte-identical (diff is imports and helper removal only). |
| Regression | `benches/intercept.rs:222` summary line omits the new counter (cosmetic). No dashboard or `fah-metrics` reader of `ListenerCounters` fields (grep on `hello_timeouts` / `upstream_failures` outside `fah-http` is empty), so F8 holds. `#[serde(default)]` keeps older payloads deserialisable. `ProxyCounters` is `Default`-built, no literal to update. Acceptable. |

### Outcomes

| # | Outcome | Where |
| --- | --- | --- |
| F-01 | **fixed** — `common::client_hello` now emits a real ClientHello (`rustls::ClientConnection::new` + `write_tls` on the untrusting config, shared with `untrusting_connector` through `untrusting_config`); the server accepts it, sends its flight and fails on the client's FIN with `UnexpectedEof`. Hand-rolled builder deleted. Test also asserts `origin.connections == 1` | `crates/fah-http/tests/common/mod.rs`, `tests/interception.rs` |
| F-02 | **fixed** — `an_unspecified_rejection_is_also_525`: `CertificateError::Other(OtherError(io::Error))` → `CertificateUnknown` → 525, counter 1 | `crates/fah-http/tests/interception.rs` |
| F-03 | **fixed** — `rejection_status(alert).map(\|status\| (alert, status))`; same value, same branches | `crates/fah-http/src/intercept.rs` accept arm |
| F-04 | **fixed** (owner yes) — "answers our certificate with a TLS alert" | `SECURITY.md:299-300` |
| F-05 | **fixed** (owner yes) — "minted for the host"; "(constant `CLIENT_CERT_REJECTED`; `ClientCertRejected` is the p3-09 filter label)" | `CONTEXT.md:380-384` |

Gates after the fixes: `cargo fmt --all -- --check` clean; `cargo clippy
--workspace --all-targets -- -D warnings` clean; `fah-http` lib 84,
`interception` 45, `client_rejection` 4 — all green.

### Verdict

**PASS** — no blocker. All five findings fixed in the working tree: F-01
(the one should-fix) and F-02/F-03 in code with gates green, F-04/F-05 in
SECURITY.md and CONTEXT.md on the owner's yes.
