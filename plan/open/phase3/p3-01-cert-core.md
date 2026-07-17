# P3-01 — Certificate Core

**Phase:** 3 · **Depends on:** phase2 · **Model:** Opus

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

## Scope

- CA generation (rcgen): configurable CN/validity, key stored in `/config`
  with restrictive permissions; regeneration = explicit destructive operation
  (old CA archived, warning logged).
- Leaf minting: per-host certs signed by the CA, SAN-correct (host + SNI),
  short validity, in-memory bounded cache (LRU) — minting off the connection
  hot path where possible (pre-warm on SNI first-sight).
- Import: user PEM (cert+key) and PFX/PKCS#12 for the **API server cert**
  (replaces the self-signed from Phase 1); validation via x509-parser with
  precise errors (expired, key mismatch, not CA where CA expected).
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
