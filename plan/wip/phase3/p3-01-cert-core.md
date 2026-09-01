# P3-01 — Certificate Core

**Phase:** 3 · **Depends on:** phase2 · **Model:** Fable

## Goal

The certificate machinery: generate a CA, mint per-host leaf certificates,
import user certs, all inside the SECURITY.md constraints.

## Context

SECURITY.md is binding: rustls + rcgen + x509-parser only; the CA private key
never leaves `/config`; export is public-certificate-only; interception is
never default. This crate-level work lives where TLS already lives (extend
`fah-http`/`fah-common` placement per ARCHITECTURE — decide and document; a
new `fah-certs` L2/L1 crate is acceptable if layering stays clean, with
ARCHITECTURE.md updated in the same change).

**There is now a precedent for that placement decision — do not re-litigate it
from scratch (p2-01 audit).** The situation is structurally identical to one
already solved: `fah-api` (L3) owns `rcgen` and `load_or_generate_tls` for the
self-signed API certificate, and this task needs CA + leaf minting inside
`fah-http` (L3). They are siblings and may not import each other, so the
choices are duplicate `rcgen` usage in both, or push the shared machinery
down.

p2-01 hit the same shape with socket binding — `fah-dns` and `fah-http` both
needed the dual-stack bind — and resolved it by moving the behaviour to
`fah_common::listen` (L1), deleting the original from `fah-dns`. See
`docs/code-review/phase2/p2-01-review.md` §2 for the admission test used: *does
divergence between the two siblings produce a silent bug?* For certificates it
plainly does — two crates disagreeing about validity, SAN construction or key
permissions is a security defect, not a style difference.

Weigh against that: `fah-common` gained `tokio` + `socket2` in p2-01 and
ARCHITECTURE.md warns it must not become a dumping ground. Adding `rcgen` +
`x509-parser` there too may be one admission past the line — which is the
argument **for** a dedicated `fah-certs` at L1/L2 rather than a third
extension of `fah-common`. Decide explicitly and record it.

## Scope

- CA generation (rcgen): configurable CN/validity, key stored in `/config`
  with restrictive permissions; regeneration = explicit destructive operation
  (old CA archived, warning logged).
- Leaf minting: per-host certs signed by the CA, SAN-correct (host + SNI),
  short validity, in-memory bounded cache (LRU) — minting off the connection
  hot path where possible (pre-warm on SNI first-sight). The rustls
  `ResolvesServerCert` type over the cache ships here (pure, shared by
  p3-04 and p3-05); its wiring does not.
- Import: user PEM (cert+key) for the **API server cert** (replaces the
  self-signed from Phase 1); validation via x509-parser with precise errors
  (expired, key mismatch, not CA where CA expected). **PFX/PKCS#12 descoped**
  (plan §Decisions 8, Option B — owner decision): it would need several new
  crypto crates outside SECURITY.md's fixed set, and `openssl pkcs12` converts.
  Recorded in ADR-0006 and API.md.
- Export: CA public certificate as PEM and DER (Android wants DER).
- Status introspection: CA fingerprint, validity window, leaf-cache stats.
- Tests: mint → rustls client with CA trusted verifies successfully; import
  rejects garbage/expired/mismatched pairs with named errors; export never
  contains private material (assert on PEM structure).

## Acceptance criteria

- A rustls client trusting only the exported CA connects clean to a server
  using a minted leaf (round-trip test).
- Grep-proof: no private key bytes in any export path.
- SECURITY.md + ARCHITECTURE.md updated for crate placement decision.
- Gates green.

## Out of scope

API endpoints (p3-02), interception itself (p3-04).

## Suggested prompt

> Read SECURITY.md fully, ARCHITECTURE.md layering, and
> plan/wip/phase3/p3-01-cert-core.md. Implement CA generation, leaf minting
> with bounded cache, PEM/PFX import and public-only export, with the
> round-trip and rejection tests. Decide crate placement and update docs.
