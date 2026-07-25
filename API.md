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
    "code": "validation_failed",
    "message": "line 14: invalid rule syntax: \"||^\""
  }
}
```

`code` is a stable machine-readable slug; `message` is human-readable.
The full code set: `unauthorized` (401), `bad_request` (400), `not_found`
(404), `conflict` (409), `validation_failed` (422), `internal` (500).

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

**Serves the in-RAM ring only.** This endpoint reads `[query_log] ring_entries`
(default 10 000) most-recent events; it does **not** read the `/data` segments
that `retention_days` / `retention_max_mb` govern. The window it can answer for
is therefore `ring_entries ÷ current QPS` — about 2.8 h for a household at
~1 QPS, but only ~2 minutes at 85 QPS. `from`/`to` filter *within* that window:
a range older than the ring returns an **empty** `items` array, not an error, so
"no results" here means "outside the retained ring", not "no queries happened".

The on-disk segments are written for a future reader (per-query drill-down in
the dashboard phase) and are not reachable through any endpoint today. Long-term
aggregates come from `/api/v1/history/*` instead, which reads `/data/history`.

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

## History (persisted series)

Long-term observability read back from `/data/history` — where
`GET /api/v1/stats` is a live rolling-24h view, these serve the 30/60/90 days
`history.retention_days` keeps (CONFIGURATION.md `[history]`). Auth required,
like everything under `/api/v1/`.

Shared query parameters:

- `from`, `to` — RFC 3339 bounds of the half-open window `[from, to)`. `to`
  defaults to now; `from` to `to − 24h` (`− 7d` for `/top`, whose data is
  stored per completed day).
- `max_points` — response point budget. `summary`: default 5000, max 10000.
  `perf`: default 1000, max 5000.

`from ≥ to` is a `400` — a window that *cannot* hold data is a request bug. A
window that simply *has* no data is a `200` with an empty `items`, never a
`404`. Ranges are bounded on both ends: only the day-files inside the window
are opened, each is streamed, and the result is capped at `max_points` as it is
built — so a 90-day request costs the same memory as a one-hour one.

When a series has more points than the budget, only every `stride`-th is
returned and `stride` says so. Decimation keeps whole rows — it never averages,
so every point served is a real reading rather than a smoothed one.

### `GET /api/v1/history/summary`

Aggregate series from the hourly rollups. `resolution` is `hour` (default) or
`day`; a day point is that UTC day's hourly rows summed.

```json
{
  "resolution": "hour",
  "from": "2026-07-16T00:00:00Z",
  "to": "2026-07-17T00:00:00Z",
  "stride": 1,
  "items": [
    {
      "ts": "2026-07-16T10:00:00Z",
      "queries": 5120,
      "blocked": 610,
      "blocked_percent": 11.91,
      "cache_hits": 3143,
      "per_type": { "A": 3900, "AAAA": 1100, "HTTPS": 120 }
    }
  ]
}
```

`ts` is the **start** of the bucket. `per_type` uses the fixed label set the
rollups record (`A`, `AAAA`, `HTTPS`, `MX`, `TXT`, `PTR`, `NS`, `SOA`, `SRV`,
`CNAME`, `OTHER`); zero buckets are omitted.

### `GET /api/v1/history/perf`

The persisted `PerfSample` series — RSS, QPS, per-interval verdict deltas,
cache stats, latency percentiles and upstream health, one row per
`history.sample_interval_seconds` (default 60 s). At that cadence a single day
is 1440 samples, so this is the endpoint `stride` usually applies to.

`fields` takes a comma-separated subset of the response keys —
`rss_bytes`, `qps`, `queries_delta`, `blocked_delta`, `allowed_delta`, `cache`,
`latency`, `upstreams` — and drops the rest (**absent**, not null). `ts` is
always present. An unknown name is a `400` rather than being ignored, so a typo
cannot silently remove the series a chart wanted. `fields` trims the response,
not the read.

```json
{
  "from": "2026-07-17T09:00:00Z",
  "to": "2026-07-17T10:00:00Z",
  "stride": 1,
  "items": [
    {
      "ts": "2026-07-17T09:01:00Z",
      "rss_bytes": 55000000,
      "qps": 12.5,
      "queries_delta": 750,
      "blocked_delta": 210,
      "allowed_delta": 5,
      "cache": {
        "entries": 10000, "capacity": 16384,
        "fresh": 9000, "stale": 800, "expired": 200,
        "hits": 500000, "misses": 120000, "evictions": 3400,
        "bytes": 21000000, "max_bytes": 67108864
      },
      "latency": {
        "block_p50": 0.0001, "block_p99": 0.0005,
        "cache_hit_p50": 0.0001, "cache_hit_p99": 0.00025,
        "forward_p50": 0.005, "forward_p99": 0.05
      },
      "upstreams": [
        { "address": "1.1.1.1", "protocol": "dot",
          "attempts": 12000, "failures": 3,
          "consecutive_failures": 0, "tls_handshakes": 4 }
      ]
    }
  ]
}
```

Latency percentiles are **in seconds** and are bucket-granularity estimates
over the sampling interval, saturating at the top finite bucket — good for a
trend line, not exact quantiles. `qps` and the `*_delta` counters are
per-interval; the `cache` counters `hits`/`misses`/`evictions` are
process-lifetime totals, the rest of `cache` — `bytes` against `max_bytes`
included — is point-in-time. Rows written before the byte cap existed carry
neither field and read back as `0`.

### `GET /api/v1/history/top`

Top-N over the range, merged from the daily top-N files. `kind` is `blocked`
(default), `queried` or `clients`; `n` defaults to 10, max 100.

```json
{
  "kind": "blocked",
  "from": "2026-07-10T00:00:00Z",
  "to": "2026-07-17T00:00:00Z",
  "items": [ { "domain": "ads.example.com", "count": 12890 } ]
}
```

`kind=clients` returns `{ "ip": "192.168.10.15", "name": "liviu-phone",
"count": 30122 }` items instead.

This ranking is an **approximation**. Each day-file already holds only that
day's top-N (a space-saving estimate), so a domain that missed the daily cut-off
contributes nothing for that day — a steadily-just-below-the-line domain can end
up ranked under one that spiked into a single day's top-N. It answers "what
dominated this week", not "the exact order".

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

## Cache

### `GET /api/v1/cache`

DNS-cache usage for the dashboard. Entries are counted by lifetime stage:
**fresh** (still within TTL, answers directly), **stale** (past TTL but
within the RFC 8767 serve-stale window — answers only after a failed
forward), **expired** (past the stale window — dead weight awaiting eviction
or a clean). `hits`/`misses`/`evictions` are process-lifetime counters.

```json
{
  "entries": 7261,
  "capacity": 10000,
  "fresh": 7026,
  "stale": 52,
  "expired": 183,
  "hits": 18639283,
  "misses": 1543921,
  "evictions": 21483,
  "bytes": 21000000,
  "max_bytes": 67108864,
  "load_percent": 72.61,
  "byte_load_percent": 31.29
}
```

`capacity` is the cache's real bound (per-shard capacity × shard count),
which can round slightly below `dns.cache.max_entries`. `max_bytes` is the
same story for the byte bound (`dns.cache.max_bytes`), and `bytes` is what the
resident answers hold against it. The cache is bounded by **both**: eviction
runs oldest-first until entries and bytes are each back inside their bound, so
the higher of `load_percent` / `byte_load_percent` is the one about to evict.
`bytes` is a coarse per-entry estimate, not an allocator audit, and excludes
the hash-table slabs that `/debug/memory` counts.

### `POST /api/v1/cache/clean`

Removes expired entries now instead of waiting for capacity eviction.
Stale-window entries are **kept by default** — they are the serve-stale
insurance an upstream outage is survived on; pass `?stale=true` to purge
them too (an explicit admin choice).

```json
{
  "removed_expired": 1834,
  "removed_stale": 0,
  "entries_before": 9095,
  "entries_after": 7261,
  "freed_bytes": 2846720,
  "duration_ms": 4.7
}
```

`cache_estimated_bytes` (under `/debug/memory`) counts the hash-table slabs
— every bucket, occupied or not, at hashbrown's 8/7-of-capacity sizing —
plus each entry's own heap (key string, refcounted answer block, record
buffers, a flat per-record allowance for what hickory owns internally), each
block rounded to 16-byte allocator granularity. `freed_bytes` counts only
the removed entries' own heap: a clean never shrinks the table slab, which
is also why RSS does not drop by `freed_bytes` after one. Built so the gap
to RSS is explainable — not an allocator audit.

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
  ],
  "compiled_rules": 512883,
  "duplicates_removed": 87422
}
```

`compiled_rules` and `duplicates_removed` describe the **merged** ruleset, not
any single list, which is why they sit on the envelope. The compiled matcher
holds distinct rules only (RULE_ENGINE.md §Deduplication): loading two
near-identical corpora (say AdGuard's `filter_48` and HaGeZi's `pro`) stores
the overlap once and reports how much was collapsed. The per-list `rules_*`
counts stay parse-based — each list really does contain those rules — so
`compiled_rules` is smaller than their sum by roughly `duplicates_removed`.

The identity is not exact, because **inline user rules
(`PUT /api/v1/rules/user`) take part in the merge but are not one of the
`items`**: they contribute to `compiled_rules` and can be collapsed into
`duplicates_removed` like any other rule. The exact relation is

```text
sum(items[].rules_active_dns) + user_rules_active - compiled_rules
    = duplicates_removed
```

A single user rule that duplicates a list rule is enough to make the
`items`-only arithmetic look off by one.

`last_status` (`ok` | `degraded` | `failed` | `never`) reports the last
*refresh attempt*;
the `rules_*` counts report what the list contributes to the ruleset that is
**currently serving**. They are deliberately independent: a failed refresh
leaves the previous ruleset in place (RULE_ENGINE.md §Failure policy), so
`"last_status": "failed"` with a non-zero `rules_total` is the normal and
correct report for a list whose download broke but whose rules keep blocking.
The counts only reach zero when the list genuinely contributes nothing —
disabled, or never yet fetched on a first-ever boot.

`degraded` means the fetch succeeded but most of the list failed to parse
(more errors than rules) — the signature of a **format misdetection**, not of a
few malformed lines. The list is almost certainly contributing far fewer rules
than it should; check its syntax against RULE_ENGINE.md §Supported formats. It
is reported separately from `ok` because it used to be indistinguishable from
it: a misdetected EasyList yielding 83 rules and 69,514 errors still read as
`"last_status": "ok"`.

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

### `POST /api/v1/lists/refresh`

Force-refresh **every** enabled list in one pass, then recompile the ruleset a
single time (not once per list). Unlike the per-list route this is
**synchronous** — it returns once the whole batch is done, with the per-list
outcome — and **best-effort**: a list whose fetch fails is reported and skipped
(its last-good cached copy keeps serving), the rest still refresh. `200 OK`:

```json
{
  "refreshed": 14,
  "failed": 1,
  "results": [
    { "id": "oisd-basic", "status": "ok", "rules_active_dns": 51234 },
    { "id": "hagezi-pro", "status": "failed", "error": "fetch https://… failed: …" }
  ]
}
```

Each list also emits a `list_refreshed` event, the same as a single refresh.

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

Most options are boot-only: `[dns.cache]`, `[dns.upstreams]`, `[dns.blocking]`,
`[query_log]`, `[stats]`, `log.level` and `history.sample_interval_seconds` are
each read once during startup, so they persist and ask for a restart rather
than reporting an apply that no code performs. The runtime set is
`history.enabled`, `history.retention_days`, `api.metrics_public` and
`rules.refresh_hours_default`. See [CONFIGURATION.md](CONFIGURATION.md) for
every option and its mutability class.

**`rules.lists` is not accepted here — 422.** Rule lists are managed
exclusively through the [`/lists`](#rule-lists--rules) endpoints, which apply a change
live (fetch, recompile, atomic swap) *and* write `[[rules.lists]]` back to the
TOML themselves. Allowing the array through this endpoint too would give one
piece of state two writers: this handler does not reload the engine, so the
next `/lists` call would persist the engine's set over the patch and the edit
would vanish. The TOML is the boot source and the durable record; `/lists` is
the runtime API. Editing `[[rules.lists]]` in the file by hand and restarting
also works.

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

## Debug

### `GET /api/v1/debug/memory`

Where the RAM goes — for checking the PERFORMANCE.md memory budget against a
live box. Every **bounded** structure reports its own heap; `residual_bytes` is
what RSS holds beyond all of them.

```json
{
  "ruleset_bytes": 23002595,
  "cache_entries": 1109,
  "cache_estimated_bytes": 1053072,
  "stats_aggregates_bytes": 41984,
  "stats_clients_bytes": 9216,
  "query_log_ring_bytes": 1179648,
  "query_log_pending_bytes": 24576,
  "accounted_bytes": 25311091,
  "residual_bytes": 18389133,
  "process_rss": 43700224
}
```

**The residual is the number to watch.** It legitimately covers binary text and
data pages, thread stacks, the tokio runtime, and allocator memory musl has not
returned to the OS — so it is never zero. What matters is its *trend*: growth
in `residual_bytes` while the components stay flat is the leak signal, because
the growth you legitimately expect has already been subtracted out. Growth in a
*component* is not a leak — it is that structure filling toward its cap.

`accounted_bytes` is the sum of the component fields.
`residual_bytes` = `process_rss − accounted_bytes`, floored at zero: components
can never really exceed RSS, so a negative value would be an accounting bug
rather than a real state, and the server logs that case at `warn` instead of
reporting a wrapped number.

`process_rss` is read from `/proc/self/status` and is `null` on platforms
without procfs (a non-Linux dev machine); `residual_bytes` is then `null` too,
since it cannot be computed.

The same figures are exported as `fastadhunter_memory_component_bytes`
(labelled by `component`) and `fastadhunter_memory_residual_bytes` on
`/metrics`. Those are sampled together on the 10 s telemetry poll, so they can
lag this endpoint — which reads live — by up to one interval.

---

## Certificates *(Phase 3 — reserved)*

`/api/v1/certificates` — import PEM, import PFX, generate CA, export CA,
status. Endpoints specified when Phase 3 begins; namespace reserved now.
