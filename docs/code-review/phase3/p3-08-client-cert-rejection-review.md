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
| 6 | `crates/fah-http/tests/common/mod.rs` (new) | `provider`, `Rejecting`, `rejecting_connector`, `untrusting_connector`, `client_hello` |
| 7 | `crates/fah-http/tests/client_rejection.rs` | helpers moved to `common`; its four tests unchanged |
| 8 | `crates/fah-http/tests/interception.rs` | local `provider()` deleted (now `common::provider`); 8 wire tests + `refused_leaf` helper |
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

All eight go through `refused_leaf`, which asserts on every case: exactly one
event, `EventKind::Https`, host `origin.test`, client `127.0.0.1`, empty method
and path, `bytes 0`, no second event on the channel (no `https-sni` item),
`minted_total == 1`, origin connected once and served zero requests.

| Test | Client | Status | Counter |
| --- | --- | --- | --- |
| `a_client_refusing_our_leaf_emits_one_https_event_with_status_525` | `ApplicationVerificationFailure`, all versions | 525 | 1 |
| `a_name_rejection_is_also_525` | `NotValidForName` | 525 | 1 |
| `a_tls12_rejection_is_525` | `ApplicationVerificationFailure`, TLS 1.2 only | 525 | 1 |
| `an_untrusting_client_stays_on_status_0_and_is_not_counted` | empty root store → `UnknownCA` | 0 | 0 |
| `an_expired_verdict_is_an_intentional_fallback_to_0` | `CertificateError::Expired` → `CertificateExpired` | 0 | 0 |
| `a_rejection_never_touches_the_interception_state` | pinning | 525 | `Arc::ptr_eq` on `state.current()` before/after |
| `a_client_that_closes_after_the_hello_stays_on_status_0` | raw `common::client_hello` then `shutdown()` | 0 | 0 |
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
| `fah-http` `interception` + `client_rejection` | 44 + 4 passed |
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

Not started. Awaiting `start code review`.
