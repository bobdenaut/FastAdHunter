# P3-08 — Client Certificate Rejection

**Phase:** 3 · **Depends on:** p3-04, p3-07 (ADR-0008 step 2) · **Model:** Fable

## Goal

The moment a client refuses our minted leaf becomes a first-class `https`
event with status **525** (`ClientCertRejected`). Nothing else changes: no new
event kind, no write to the Interception Document, and `UnknownCA` and every
unclassified failure stay on `status 0`.

## Context

[ADR-0008](../../../docs/decisions/0008-live-interception-and-client-certificate-rejection.md)
step 2 of §Phasing — read §Detect instead of predict, §detect ≠ auto-exclude,
§What counts as a rejection and §The missing-CA case. The accept-side contract
is already measured and pinned by `crates/fah-http/tests/client_rejection.rs`:
a client's fatal alert reaches `acceptor.accept()` as an `io::Error` of kind
`InvalidData` carrying `rustls::Error::AlertReceived(..)`, on TLS 1.3 and 1.2
alike; a client that closes without alerting carries nothing to classify. The
accept arm of `intercept()` currently drops all of it on `status 0`, shared
with six other failure paths.

## Scope

- **The predicate.** Own matcher in the accept arm of `intercept()`:
  `InvalidData` + `AlertReceived(desc)`. `certificate_error` in `tls.rs` is
  precedent only — it matches `InvalidCertificate`, which the accept side never
  produces — and is not reused.
- **The classified set.** `BadCertificate` (0x2a), `AccessDenied` (0x31),
  `CertificateUnknown` (0x2e) ⇒ status 525, a constant beside
  `UPSTREAM_CERT_FAILURE = 526`. `UnknownCA` (0x30) ⇒ `0`, by name, by owner
  decision (2026-09-10: a trust diagnostic, not an exclusion event). Every
  other alert, every transport error and the RST/FIN close ⇒ `0`, as an
  intentional fallback pinned by test, not a missing arm. The set is open: an
  alert joins it only with a test case.
- **The event.** An `https` event (`Event::Https`) with status 525 and the
  session's host, client and time — never `https-sni`, never a new
  `EventKind`. `ClientCertRejected` names the constant and, in p3-09, the
  dashboard filter. The existing `debug!` line stays.
- **Tests.** Through the wire path in the `fah-http` harness: listed client,
  rejecting connector per classified alert ⇒ exactly one `https` event with
  status 525; `UnknownCA` ⇒ `0`; transport close ⇒ `0`; the `https-sni` event
  of the same connection unchanged. `client_rejection.rs` keeps the wire-value
  test.
- **Docs, same change.** API.md (`status 525` on `https` events beside 526,
  the classified set and the fallback rule), CONTEXT.md (new term **Client
  Certificate Rejection**, `ClientCertRejected`), SECURITY.md interception
  bullets where they describe the accept-side failure.

## Acceptance criteria

The three ADR-0008 §Acceptance lines this step owns, plus the structural one:

- A classified alert produces exactly one `https` event with status 525; an
  accept failure carrying no classified alert stays on `status 0` — a test pins
  the fallback by name, `UnknownCA` included.
- No new `Event` kind; the `kind` set stays `dns`, `http`, `https-sni`,
  `https`.
- The detector has no path to the document writer: `layering.rs` green,
  `fah-http` still depends on L1 and L2 only.
- The success path is untouched — the predicate runs only on the failure arm;
  a bench is owed only if the plan touches the handshake path.
- Docs updated in the same change. Gates green.

## Out of scope

The view and the action (p3-09); a client-level missing-CA diagnosis and any
contract for `UnknownCA` beyond "stays on `0`" (ADR-0008 §Revisit); rejections
that close without an alert; a "pinned" label anywhere.

## Suggested prompt

> Read ADR-0008 §What counts as a rejection and §detect ≠ auto-exclude,
> plan/wip/phase3/p3-08-client-cert-rejection.md,
> `crates/fah-http/tests/client_rejection.rs` and the accept arm of
> `intercept()` in `fah-http/src/intercept.rs`. Plan the predicate, the
> constant and the harness tests; wait for approval; then implement with the
> fallback pinned by name.
