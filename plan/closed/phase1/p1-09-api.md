# P1-09 — REST + WebSocket API

**Phase:** 1 · **Depends on:** p1-03, p1-07, p1-08 · **Model:** Opus

## Goal

`fah-api` serves the complete API.md surface: bearer-key auth, HTTPS by
default, every endpoint, live WS events.

## Context

API.md is the contract — request/response shapes verbatim. SECURITY.md fixes
auth and TLS: key generated on first boot (printed once, stored in `/config`),
self-signed cert via rcgen, plain HTTP only via explicit opt-out. Heavy task:
many endpoints, but each is thin — all logic lives behind handles from
fah-rules / fah-stats / fah-metrics / fah-config.

## Scope

- Axum over rustls: `[api]` bind/port/tls from config; rcgen self-signed cert
  generation on first boot into `/config`; PEM replacement honored.
- Auth middleware: `Authorization: Bearer` on `/api/v1/*`; `/health` +
  `/metrics` exempt per `metrics_public`; key rotation endpoint (returns new
  key once).
- Endpoints per API.md: health, metrics (from p1-08 registry), stats, queries
  (pagination/filters), clients (list, PUT name), lists CRUD + refresh,
  rules/user GET/PUT (validate line-by-line → 422 with per-line messages),
  rules/test dry-run, config GET (redacted) / POST (deep-merge, write-back,
  `restart_required` for boot-only keys), apikey rotate.
- `WS /api/v1/events`: query stream + periodic stats delta + config/list
  events; token via header or `?token=`; slow consumers disconnected.
- Error format: `{ "error": { "code", "message" } }` everywhere (typed).
- Integration tests (`tests/`): full round-trips over HTTPS with the
  self-signed cert, auth failures, config write-back reflected in the TOML
  file, WS receives a blocked-query event end-to-end.

## Acceptance criteria

- Every endpoint in API.md exists and matches its documented shape
  (golden-file tests for response JSON).
- Wrong/missing key → 401 with the error format; no route bypasses auth
  except the two documented ones.
- Boot with `api.tls = true` (default): curl requires `-k`; cert persists
  across restarts.
- Gates green.

## Out of scope

Certificates management API (`/api/v1/certificates` — Phase 3), dashboard.

## Suggested prompt

> Read API.md fully, SECURITY.md §API access + §TLS, CONFIGURATION.md §[api],
> and plan/wip/phase1/p1-09-api.md. Implement fah-api on axum+rustls exactly
> to the documented contract, wiring the existing crate handles, with
> integration tests over real HTTPS.
