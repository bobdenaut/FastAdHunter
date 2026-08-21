# P3-02 — Certificates API

**Phase:** 3 · **Depends on:** p3-01 · **Model:** Opus

## Goal

`/api/v1/certificates` — the reserved namespace becomes a specified,
implemented surface.

## Context

API.md reserved the namespace in Phase 1 with operations: import PEM, import
PFX, generate CA, export CA, status. This task writes the full endpoint spec
into API.md (request/response shapes, error codes) and implements it in
fah-api on top of p3-01 — same change, doc and code together.

## Scope

- Spec + implement:
  - `GET  /api/v1/certificates` — status: CA present?, fingerprint, validity,
    API-server cert source (self-signed | imported), leaf-cache stats.
  - `POST /api/v1/certificates/ca/generate` — explicit `{"confirm": true}`
    guard; returns fingerprint; archives previous CA.
  - `GET  /api/v1/certificates/ca/export?format=pem|der` — public cert only.
  - `POST /api/v1/certificates/import` — API server cert, PEM (cert+key) or
    PFX+passphrase; applied via listener rebind or documented
    `restart_required: true` (pick one, document in API.md).
- Auth: everything requires the bearer key (no exemptions here).
- Secrets hygiene: passphrases never logged; request bodies with key material
  excluded from any debug logging.
- Integration tests: full lifecycle over HTTPS — generate, export, verify
  fingerprint matches, import replacing the API cert, status reflects it.

## Acceptance criteria

- API.md §certificates fully specified and matching implementation
  (golden-file tests).
- Export responses never contain private keys (asserted).
- Gates green.

## Out of scope

Interception toggles (p3-04 config), client CA-install docs (p3-06).

## Suggested prompt

> Read API.md (reserved certificates section), SECURITY.md, and
> plan/wip/phase3/p3-02-certificates-api.md. Write the endpoint spec into
> API.md and implement it in fah-api over the p3-01 core, with lifecycle
> integration tests.
