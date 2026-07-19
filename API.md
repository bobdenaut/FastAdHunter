# API

REST + WebSocket surface of FastAdHunter Core. The dashboard and every other
client communicate exclusively through this API.

- Base URL: `https://<host>:8443` (HTTPS by default — see [SECURITY.md](SECURITY.md))
- All bodies are JSON, UTF-8.
- Versioned under `/api/v1/`.

## Authentication

Single API key (bearer token), generated on first boot, rotatable.

```http
Authorization: Bearer <api-key>
```

Required for everything under `/api/v1/`. Exempt by default: `GET /health`,
`GET /metrics` (exemption configurable). Missing/invalid key → `401`.

## Error format

Every non-2xx response:

```json
{
  "error": {
    "code": "invalid_rule_syntax",
    "message": "line 14: unknown option '$foo'"
  }
}
```

`code` is a stable machine-readable slug; `message` is human-readable.
Common codes: `unauthorized`, `not_found`, `validation_failed`,
`restart_required`, `internal`.

---

## Health & telemetry

### `GET /health`

Liveness/readiness. No auth (default). Used by the Docker healthcheck
(`fastadhunter --healthcheck`).

```json
{ "status": "ok", "version": "0.1.0", "uptime_seconds": 86400 }
```

`status`: `ok` | `degraded` (e.g. all upstreams failing — serve-stale active).

### `GET /metrics`

Prometheus text exposition format (ops telemetry: QPS, latency histograms,
cache hit ratio, memory, per-verdict counters). No auth by default.

---

## Statistics & query log

### `GET /api/v1/stats`

Aggregated statistics (product data, for users/dashboard).

```json
{
  "window": "24h",
  "queries_total": 184233,
  "blocked_total": 23411,
  "blocked_percent": 12.7,
  "cache_hit_percent": 61.4,
  "top_blocked_domains": [ { "domain": "ads.example.com", "count": 1289 } ],
  "top_queried_domains": [ { "domain": "api.example.org", "count": 4021 } ],
  "top_clients":         [ { "ip": "192.168.10.15", "name": "liviu-phone", "count": 30122 } ],
  "buckets": [ { "start": "2026-07-17T10:00:00Z", "queries": 5120, "blocked": 610 } ]
}
```

### `GET /api/v1/queries`

Query log, newest first. Pagination + filters via query string:
`limit` (default 100, max 1000), `cursor`, `client`, `domain` (substring),
`verdict` (`allow|block|pass`), `from`, `to` (RFC 3339).

```json
{
  "items": [
    {
      "ts": "2026-07-17T10:41:03.412Z",
      "client": "192.168.10.15",
      "client_name": "liviu-phone",
      "domain": "ads.example.com",
      "qtype": "A",
      "verdict": "block",
      "rule": "||ads.example.com^",
      "list": "oisd-basic",
      "duration_ms": 0.3,
      "upstream": null,
      "cached": false
    }
  ],
  "next_cursor": "opaque-token-or-null"
}
```

---

## Clients

### `GET /api/v1/clients`

Observed clients (by source IP) with stats and optional names.

```json
{
  "items": [
    {
      "ip": "192.168.10.15",
      "name": "liviu-phone",
      "first_seen": "2026-07-01T08:00:00Z",
      "last_seen": "2026-07-17T10:41:03Z",
      "queries_24h": 30122,
      "blocked_24h": 3020
    }
  ]
}
```

### `PUT /api/v1/clients/{ip}`

Assign/change a client name. Body: `{ "name": "liviu-phone" }`.
Returns the updated client object. `DELETE` of the name: send `{ "name": null }`.

---

## Rule lists & rules

### `GET /api/v1/lists`

```json
{
  "items": [
    {
      "id": "oisd-basic",
      "url": "https://example.org/oisd-basic.txt",
      "format": "auto",
      "enabled": true,
      "refresh_hours": 24,
      "last_refresh": "2026-07-17T04:00:00Z",
      "last_status": "ok",
      "rules_total": 214001,
      "rules_active_dns": 198500,
      "rules_inactive": 15501
    }
  ]
}
```

`last_status` (`ok` | `failed` | `never`) reports the last *refresh attempt*;
the `rules_*` counts report what the list contributes to the ruleset that is
**currently serving**. They are deliberately independent: a failed refresh
leaves the previous ruleset in place (RULE_ENGINE.md §Failure policy), so
`"last_status": "failed"` with a non-zero `rules_total` is the normal and
correct report for a list whose download broke but whose rules keep blocking.
The counts only reach zero when the list genuinely contributes nothing —
disabled, or never yet fetched on a first-ever boot.

`last_refresh` is `null` until the first successful refresh *in this process*;
a boot-from-cache is a load, not a refresh.

### `POST /api/v1/lists`

Add a list. Body: `{ "url": "...", "enabled": true, "refresh_hours": 24 }`
(or `{ "path": "/data/lists/local.txt" }` for a mounted file).
Format auto-detected. Returns the created list object.

`id` is optional and derived from the URL's file stem or host when omitted
(`https://small.oisd.nl` → `small.oisd.nl`, `.../Xtra/hosts.txt` → `hosts`).
Derivation collides across sources that share a filename — two different
repositories' `hosts.txt` both derive to `hosts`, and the second returns
`409 conflict` saying so. Pass `id` explicitly to disambiguate.

A source may only be configured once: adding a `url`/`path` that another list
already holds is `409 conflict` naming that list, even under a different `id`.
Two ids over one source would fetch, cache and compile it twice.

### `PATCH /api/v1/lists/{id}` / `DELETE /api/v1/lists/{id}`

Enable/disable, change refresh interval, remove.

### Persistence

`POST`, `PATCH` and `DELETE` rewrite `[[rules.lists]]` in
`/config/fastadhunter.toml` before the change reaches the engine, so a list
added through the API survives a restart. A failed write is a `500` and the
mutation does not happen — the file and the running engine never disagree.

Downloaded list *content* is cached separately under `/data/lists/`. That cache
is keyed by list id and only read for lists the config declares, so a `.raw`
file whose entry has been deleted is inert.

### `POST /api/v1/lists/{id}/refresh`

Force refresh now. `202 Accepted`; result visible in `last_status`.

### `GET /api/v1/rules/user` / `PUT /api/v1/rules/user`

Inline personal rules (one rule per line, any supported syntax).

```json
{ "rules": ["||tracker.example.com^", "@@||goodsite.example.com^"] }
```

`PUT` validates and atomically swaps; invalid lines → `422 validation_failed`
with per-line messages.

### `POST /api/v1/rules/test`

Dry-run a verdict: `{ "domain": "ads.example.com", "qtype": "A", "client": "192.168.10.15" }`
→ `{ "verdict": "block", "rule": "||ads.example.com^", "list": "oisd-basic" }`.

---

## Configuration

### `GET /api/v1/config`

Effective configuration (all sources merged), secrets redacted.

### `POST /api/v1/config`

Partial update (deep-merge of provided keys). Changes are validated, written
back to `/config/fastadhunter.toml`, and applied:

- runtime-mutable options → applied live (atomic swap), response `"applied": true`
- boot-only options → persisted only, response `"restart_required": true`

```json
{ "applied": true, "restart_required": false }
```

See [CONFIGURATION.md](CONFIGURATION.md) for every option and its mutability class.

### `POST /api/v1/config/apikey/rotate`

Generates a new API key, returns it **once**, invalidates the old one.

---

## Events (WebSocket)

### `WS /api/v1/events`

Live event stream (dashboard "tail" view). Auth via
`Authorization` header or `?token=` query param on the upgrade request.

Server → client messages:

```json
{ "type": "query", "data": { /* same shape as a query-log item */ } }
{ "type": "stats", "data": { /* periodic stats delta, every ~2s */ } }
{ "type": "config_changed", "data": { "restart_required": false } }
{ "type": "list_refreshed", "data": { "id": "oisd-basic", "status": "ok" } }
```

Slow consumers are disconnected rather than back-pressuring the engine.

---

## Certificates *(Phase 3 — reserved)*

`/api/v1/certificates` — import PEM, import PFX, generate CA, export CA,
status. Endpoints specified when Phase 3 begins; namespace reserved now.
