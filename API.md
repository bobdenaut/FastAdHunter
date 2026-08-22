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

Required for everything under `/api/v1/`. `GET /health` is the one exemption.
Missing/invalid key → `401`, and auth answers before routing does — an unknown
path under `/api/v1/` is `401` without a key, `404` with one.

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

### `GET /api/v1/telemetry`

The whole engine state as JSON, so a dashboard makes one call instead of
polling `/api/v1/cache` and `/api/v1/debug/memory` separately.

Every endpoint serves data no other endpoint serves. There is no Prometheus
surface: it published nothing that is not here or on `/api/v1/debug/memory`,
apart from raw histogram buckets, whose percentiles `/api/v1/history/perf`
already reports as JSON.

`counters.events_dropped` covers **both** pipelines: they share one bounded
channel, so the shed figure stays one number. `counters.http` is kept separate
from `counters.dns` because "queries" has meant "DNS questions answered" since
p1-08 and widening it would silently redefine every figure built on it.

**Compatibility contract.** New fields may be added; existing fields must not
change meaning or units. Figures that may change with the implementation live
under `/api/v1/debug/*` instead, which promises nothing.

The boundary is **who produces a figure**, not how useful it looks:

| Producer | Home |
| -------- | ---- |
| FastAdHunter's own counters, ruleset, latency, upstreams, cache | `/api/v1/telemetry` |
| Kernel (`process_rss`, `process_peak_rss`, major/minor page faults) | `/api/v1/telemetry` |
| The linked allocator (`allocator_committed_*`) | `/api/v1/debug/memory` |

Top-level blocks: `process`, `ruleset`, `counters`, `latency`, `upstreams`,
`cache` (identical to `GET /api/v1/cache`), `memory` (identical to
`GET /api/v1/debug/memory` minus the two `allocator_committed_*` fields).

```json
{
  "process": { "version": "0.2.10", "uptime_seconds": 184920 },
  "ruleset": { "rules": 1043886, "duplicates_removed": 41207,
               "compile_duration_seconds": 7.412 },
  "counters": {
    "dns":  { "pass": 812044, "allow": 1201, "block": 96318,
              "cache_hits": 640119, "cache_misses": 269446, "cache_stale": 3187 },
    "http": { "pass": 4412, "allow": 0, "block": 918, "response_bytes": 148223904 },
    "events_dropped": 0,
    "swr": { "enqueued": 12044, "deduplicated": 3311, "dropped": 0,
             "completed": 8702, "failed": 31 },
    "cache_cleanup": { "runs": 308, "entries_removed": 44120,
                       "bytes_freed": 9871232, "last_duration_micros": 1842 }
  },
  "latency": {
    "dns":  { "block":     { "count": 96318,  "sum_seconds": 2.114 },
              "cache_hit": { "count": 640119, "sum_seconds": 18.907 },
              "forward":   { "count": 269446, "sum_seconds": 6021.338 } },
    "http": { "block":   { "count": 918,  "sum_seconds": 0.031 },
              "forward": { "count": 4412, "sum_seconds": 12.884 } }
  },
  "upstreams": [ { "address": "1.1.1.1:853", "protocol": "dot", "attempts": 201883,
                   "failures": 12, "consecutive_failures": 0, "tls_handshakes": 41 } ]
}
```

Reading it correctly:

- **Every counter is process-lifetime cumulative**, so charting a rate means
  deltaing two reads. Check `process.uptime_seconds` first: a restart returns
  all of them to zero, and a delta taken across that boundary is meaningless,
  not merely small.
- **JSON carries no equivalent of Prometheus's `# TYPE`.** The one field that is
  a **last-value gauge** rather than a total is
  `counters.cache_cleanup.last_duration_micros` — the most recent sweep only.
  Deltaing it produces nonsense.
- **`latency` gives `count` and `sum_seconds`, never an average.** A lifetime
  mean flattens within hours of uptime; delta both figures and divide for the
  interval mean. Percentiles need buckets and are served, windowed, by
  `GET /api/v1/history/perf`.
- `counters.dns.cache_hits + cache_misses` equals `pass + allow`, never
  `+ block` — a blocked query never reaches the cache (ADR-0001). A hit ratio
  divides by resolved queries, not by every query.
- No `ruleset.heap_bytes`: that is `memory.ruleset_bytes`, so the number has one
  home.
- `ruleset`, `upstreams`, `counters.swr` and `counters.cache_cleanup` are pushed
  into the registry on a 10 s poll, so they can be up to one interval old.
  `cache` and `memory` are read at request time.

---

## Statistics

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
  "buckets": [ { "start": "2026-07-17T10:00:00Z", "queries": 5120, "blocked": 610 } ],
  "policies": [ { "policy": "kids", "queries": 812, "blocked": 244 } ]
}
```

`policies` counts both pipelines, unlike the domain tables which stay DNS-only.
Clients under no assignment are counted under `default`. Rows with no traffic in
the window are omitted.

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
cache stats, latency percentiles, the memory breakdown and upstream health, one
row per `history.sample_interval_seconds` (default 60 s). At that cadence a
single day is 1440 samples, so this is the endpoint `stride` usually applies to.

`fields` takes a comma-separated subset of the response keys —
`rss_bytes`, `peak_rss`, `qps`, `queries_delta`, `blocked_delta`,
`allowed_delta`, `cache`, `latency`, `upstreams`, `memory`,
`minor_page_faults` — and drops the rest
(**absent**, not null). `ts` is always present. An unknown name is a `400`
rather than being ignored, so a typo cannot silently remove the series a chart
wanted. `fields` trims the response, not the read.

```json
{
  "from": "2026-07-17T09:00:00Z",
  "to": "2026-07-17T10:00:00Z",
  "stride": 1,
  "items": [
    {
      "ts": "2026-07-17T09:01:00Z",
      "rss_bytes": 55000000,
      "peak_rss": 189071360,
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
      "memory": {
        "ruleset_bytes": 23000000, "cache_estimated_bytes": 5000000,
        "stats_aggregates_bytes": 1000000, "stats_clients_bytes": 500000,
        "accounted_bytes": 29500000, "residual_bytes": 25500000
      },
      "minor_page_faults": 4211337,
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

`peak_rss` is the process high-water RSS (`getrusage`'s `ru_maxrss`), **monotone
within one container lifetime** — a drop in the series is a restart, never a
reclaim. It is here because it is the only way this endpoint sees the ruleset
compile: the compile is seconds long against a sampling interval of minutes, and
a high-water mark does not have to be read while it is being set. Decimation
keeps every n-th row verbatim, so a wide range can delay when a step appears but
cannot erase it. `0` means the row predates the field or `getrusage` was
unavailable — not that the peak was zero.

`memory` is the same breakdown `/api/v1/debug/memory` serves, minus the live-only
figures: **no RSS** (`rss_bytes` above is it) and **no allocator counters**, of
which only `minor_page_faults` is worth a series — the rest are monotone over
process lifetime, so a chart of them is a ramp. `residual_bytes` is derived on
read from the row's own `rss_bytes` rather than stored, so it cannot disagree
with the components beside it. Rows written before this shipped carry neither
key and read back as zeros. `minor_page_faults` is cumulative since process
start: chart its **derivative** — a rising fault rate at flat RSS is the
allocator purging and re-faulting pages it is about to reuse.

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
      "rules_total": 223182,
      "rules_active_dns": 198500,
      "rules_active_url": 9181,
      "rules_inactive": 15501,
      "parse_errors": 0
    }
  ],
  "compiled_rules": 512883,
  "duplicates_removed": 87422
}
```

The three `rules_*` counts partition `rules_total` — a rule is in exactly one:

| Field | Rules that… |
| ----- | ----------- |
| `rules_active_dns` | answer a domain question (the Domain Tier) |
| `rules_active_url` | answer an HTTP request (the URL Tier) |
| `rules_inactive` | no tier answers yet: cosmetic, `$client`, unsupported |

`rules_active_url` **was split out of `rules_inactive`** in `p2-03`, when the
URL Tier went live. Before that it did not exist and those rules were counted
inactive, which for an EasyList-family list understated what the list actually
does by tens of thousands of rules. Expect `rules_inactive` to drop sharply for
adblock-format lists on upgrade; the sum is unchanged.

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

`last_status` (`ok` | `degraded` | `failed` | `rejected` | `never`) reports the
last *refresh attempt*;
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

`rejected` means the fetch (or local file read) succeeded but the body failed
the content gate (RULE_ENGINE.md §List lifecycle), so the last-good `/data`
copy and the serving ruleset are untouched — exactly as for a failed fetch. A
body is refused when it is an HTML document (its first non-blank text is
`<!doctype` or `<html`, any case), when it looks misparsed (100 or more parse
errors and more errors than rules), and — only while the serving copy has DNS
rules — when it has zero DNS rules, or when its DNS + URL rule count is under
a tenth of a serving copy of 1000 or more rules. A source that legitimately
restructures below those lines stays `rejected` on every attempt, across
restarts. **The recovery contract is `DELETE /api/v1/lists/{id}` followed by
re-adding the list**: a list with no cached copy has no baseline, so its first
fetch is guarded by the document and misparse rules alone. `DELETE` removes
the cached copy before it drops the list and answers `500` with the list kept
if it cannot, so a `204` means the baseline is gone — a refresh still in
flight for that id cannot write it back. Disabling and re-enabling does not clear it — the
cached copy is the baseline. The copy and the baseline belong to the `id`, not
the `url`: pointing an existing id at a new source keeps the old source's copy
as the baseline, so use a new id or `DELETE` + re-add.

`last_error` is present only when `last_status` is `failed` or `rejected`, and
absent otherwise. It carries the fetch error chain, or one of
`rejected: html document`,
`rejected: misparse: <N> errors, <M> rules`,
`rejected: collapse: 0 dns rules vs baseline <B>`,
`rejected: collapse: <M> rules vs baseline <B>`.

`parse_errors`, like the `rules_*` counts, describes the copy that is
**currently serving** — the unparseable lines the last compile skipped. It
never describes a refused body: after a rejection it still reports the
last-good copy, and the refused body's counts exist only inside `last_error`.
It is `0` when the list contributes nothing — read it beside `enabled` and
`rules_total`, as for the `rules_*` counts, to tell a clean copy from an absent
one.

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
is keyed by list id and only read for lists the config declares. `DELETE`
removes the id's `.raw` file, and a refresh in flight for a deleted id is
discarded rather than committed, so re-declaring the id starts from no copy.

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
  "refreshed": 13,
  "failed": 2,
  "results": [
    { "id": "oisd-basic", "status": "ok", "rules_active_dns": 51234 },
    { "id": "hagezi-pro", "status": "failed", "error": "fetch https://… failed: …" },
    { "id": "adaway", "status": "rejected", "error": "rejected: collapse: 0 dns rules vs baseline 6710" }
  ]
}
```

`status` is `ok` | `failed` | `rejected` — the words `GET /api/v1/lists` uses
for `last_status`, and `error` is the same text it reports as `last_error`.
`degraded` does not occur on a refresh result, because the content gate refuses
a misparsed body before it can commit; `never` describes a list, not an
attempt. `failed` counts every list that did not refresh, rejected ones
included, so `refreshed + failed` is the number of `results`.

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
→ `{ "verdict": "block", "rule": "||ads.example.com^", "list": "oisd-basic", "policy": "kids" }`.

| field | meaning |
| --- | --- |
| `client` | address *or* client name. Selects the policy in force for it and satisfies `$client` rules. Live since p2-06. |
| `policy` (request) | test under a named policy directly, ignoring assignments — "what would kids see?". Unknown id → `422`. |
| `policy` (response) | which policy decided; `default` when nothing was assigned. |

---

## Policies

Named bundles of rule lists assignable to clients, optionally on a schedule
(CONTEXT.md §Policy). All policies share one compiled ruleset; see
[CONFIGURATION.md](CONFIGURATION.md#policies) for the TOML shape and the
16-policy ceiling.

### `GET /api/v1/policies`

```json
{
  "timezone": "EET-2EEST,M3.5.0/3,M10.5.0/4",
  "items": [
    {
      "id": "kids",
      "name": "Kids",
      "lists": ["oisd-basic"],
      "blocking_mode": null,
      "assignments": [
        { "client": "192.168.1.50", "days": "mon-fri", "start": "21:00", "end": "07:00" }
      ]
    }
  ],
  "active_assignments": 1
}
```

`lists: null` means every enabled list. `active_assignments` is how many
assignments are in force *right now* — a closed schedule window is excluded, so
this reports a boundary having passed without waiting for a query.

### `POST /api/v1/policies`

Body: the item shape above minus `active_assignments`. `id` is required and
locked to lowercase alphanumerics plus `.`/`_`/`-`; `default` is reserved.
`409` on a duplicate id. **Recompiles the ruleset** — seconds of CPU on the
RB5009, because per-rule policy masks are built at compile time.

### `PATCH /api/v1/policies/{id}` / `DELETE /api/v1/policies/{id}`

`PATCH` leaves absent fields alone; `"lists": null` clears the subset back to
every enabled list. Only a `lists` change recompiles — renaming or reassigning
does not. `DELETE` → `204`, and recompiles.

### `GET|PUT|DELETE /api/v1/clients/{ip}/policy`

Assigns one address to a policy. `PUT` body:
`{ "policy": "kids", "days": "mon-fri", "start": "21:00", "end": "07:00" }` —
schedule fields optional, `start`/`end` set together or not at all.

- **No recompile**: assignments change no mask, so this is live in
  milliseconds.
- One assignment per address: `PUT` replaces any existing one, in any policy.
- `404` if the policy does not exist, or on `DELETE` with nothing assigned.
- `GET` returns the policy in force **now**, so a client whose window is shut
  reports whatever else covers it — the subnet assignment, else `default`.

```json
{
  "ip": "192.168.1.50",
  "policy": "kids",
  "assignment": { "client": "192.168.1.50", "days": "mon-fri", "start": "21:00", "end": "07:00" }
}
```

`assignment` is absent when the client is covered by a subnet or name
assignment rather than one naming its address.

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
`[stats]`, `log.level` and `history.sample_interval_seconds` are
each read once during startup, so they persist and ask for a restart rather
than reporting an apply that no code performs. The runtime set is
`history.enabled`, `history.retention_days`,
`rules.refresh_hours_default` and `schedule.timezone`. See
[CONFIGURATION.md](CONFIGURATION.md) for every option and its mutability class.

**`rules.lists` is not accepted here — 422.** Rule lists are managed
exclusively through the [`/lists`](#rule-lists--rules) endpoints, which apply a change
live (fetch, recompile, atomic swap) *and* write `[[rules.lists]]` back to the
TOML themselves. Allowing the array through this endpoint too would give one
piece of state two writers: this handler does not reload the engine, so the
next `/lists` call would persist the engine's set over the patch and the edit
would vanish. The TOML is the boot source and the durable record; `/lists` is
the runtime API. Editing `[[rules.lists]]` in the file by hand and restarting
also works.

**`policies` is not accepted here either — 422**, for the same reason plus one
more: only the [`/policies`](#policies) endpoints know when an edit needs the
ruleset recompiled. `schedule.timezone` *is* accepted here and applies live.

### `POST /api/v1/config/apikey/rotate`

Generates a new API key, returns it **once**, invalidates the old one.

---

## Events (WebSocket)

### `WS /api/v1/events`

Live event stream (dashboard "tail" view). Auth via
`Authorization` header or `?token=` query param on the upgrade request.

**This is the only per-query feed** — a client that wants individual rows
subscribes here rather than polling.

Server → client messages:

```json
{ "type": "query", "data": { /* one event, shape below */ } }
{ "type": "stats", "data": { /* the GET /api/v1/stats payload, every ~2s */ } }
{ "type": "config_changed", "data": { "restart_required": false } }
{ "type": "list_refreshed", "data": { "id": "oisd-basic", "status": "ok" } }
```

`list_refreshed.status` is `ok` | `failed` | `rejected`, the vocabulary of
`POST /api/v1/lists/refresh`. The event carries no reason — it is a nudge to
re-read `GET /api/v1/lists`, where `last_error` has it.

A `query` event carries both pipelines (p2-04), tagged by `kind`:

```json
{
  "kind": "http",
  "ts": "2026-07-17T10:41:03.610Z",
  "client": "192.168.10.15",
  "client_name": "liviu-phone",
  "domain": "ads.example.com",
  "qtype": null,
  "verdict": "block",
  "rule": "||ads.example.com^",
  "list": "easylist",
  "duration_ms": 0.1,
  "upstream": null,
  "cached": false,
  "method": "GET",
  "path": "/pixel.gif?id=7",
  "resource_type": "image",
  "status": 200,
  "bytes": 0
}
```

Every key is always **present**, so a client never has to tell "absent" from
"not applicable": a DNS event leaves the HTTP-only fields `null` (`method`,
`path`, `resource_type`, `status`, `bytes`), and an HTTP event leaves `qtype`
`null` and `cached` `false`.

`status` is what the client actually received — a synthesized block's status as
much as an origin's — and `bytes` is the body relayed downstream, so a block
reads `0`. `resource_type` is the `$option` vocabulary (`script`, `image`,
`xmlhttprequest`, …) or `unknown` when the proxy could not tell
(RULE_ENGINE.md §HTTP matching).

The `stats` push is byte-for-byte the `GET /api/v1/stats` payload. That endpoint
is still worth calling once on connect: the first push is up to ~2 s away.

Slow consumers are disconnected rather than back-pressuring the engine.

---

## Debug

### `GET /api/v1/debug/memory`

Where the RAM goes — for checking the PERFORMANCE.md memory budget against a
live box. Every **bounded** structure reports its own heap; `residual_bytes` is
what RSS holds beyond all of them.

**This is `/api/v1/telemetry`'s `memory` block plus exactly the two
`allocator_committed_*` fields**, and both are gathered in the same pass, so
the two endpoints can never report a different RSS or residual for one instant.
The split is the producer boundary: the allocator fields describe whichever
allocator is linked in and carry **no** compatibility promise, where everything
else is FastAdHunter's own accounting or a kernel reading. A dashboard should
read `/api/v1/telemetry`; this endpoint is for diagnosing the allocator.

The two come from independent sources — `getrusage` for the kernel figures,
the allocator for the commit counters — so an allocator that reports nothing
nulls `allocator_committed_*` and **only** those. `process_peak_rss` and both
fault counters keep working, which is what makes the stable contract true
rather than merely intended.

```json
{
  "ruleset_bytes": 23002595,
  "cache_entries": 1109,
  "cache_estimated_bytes": 1053072,
  "stats_aggregates_bytes": 41984,
  "stats_clients_bytes": 9216,
  "accounted_bytes": 24106867,
  "residual_bytes": 19593357,
  "process_rss": 43700224,
  "allocator_committed_bytes": 318046208,
  "allocator_committed_peak_bytes": 318046208,
  "process_peak_rss": 71303168,
  "major_page_faults": 0,
  "minor_page_faults": 4211337
}
```

**The residual is the number to watch.** It legitimately covers binary text and
data pages, thread stacks, the tokio runtime, and memory the allocator holds but
has not returned to the OS — so it is never zero. What matters is its *trend*:
growth in `residual_bytes` while the components stay flat is the leak signal,
because the growth you legitimately expect has already been subtracted out.
Growth in a *component* is not a leak — it is that structure filling toward its
cap. The allocator fields below cannot refine it further; pair it with
`minor_page_faults` instead, since a rising fault rate at flat RSS is purge
thrash rather than a leak.

`accounted_bytes` is the sum of the component fields.
`residual_bytes` = `process_rss − accounted_bytes`, floored at zero: components
can never really exceed RSS, so a negative value would be an accounting bug
rather than a real state, and the server logs that case at `warn` instead of
reporting a wrapped number.

`process_rss` is read from `/proc/self/status` and is `null` on platforms
without procfs (a non-Linux dev machine); `residual_bytes` is then `null` too,
since it cannot be computed.

#### Allocator fields

These come from the process allocator (see `crates/fastadhunter/src/allocator.rs`) and are `null` where it cannot
report — absent rather than `0`, so "unavailable" never charts as a measurement.

`allocator_committed_bytes` is the bytes the allocator has committed, by its own
accounting rather than any kernel reading.

**Read it as a high-water mark, and expect it to exceed `process_rss` — often
several times over.** mimalloc v3 does not decrement the counter when a purge
returns pages to the OS, so it only ever rises. Measured on an RB5009: 318 MB
committed against 70 MB `process_rss`. That gap is memory that was committed,
touched, and has since been reclaimed by the kernel — it is *not* memory being
held. `process_rss` is the authority on footprint.

`allocator_committed_peak_bytes` is its high-water mark, and is currently equal
to it at every reading for the reason above. **That equality is the
diagnostic:** should the two ever diverge, the allocator has begun accounting
purges and `allocator_committed_bytes` has become a live figure worth reading as
one.

> **Removed in 0.2.8: `allocator_retained_bytes`.** It served
> `allocator_committed_bytes − accounted_bytes` as "fragmentation, free lists and
> size-class rounding". Because the minuend only rises, the difference grows
> without bound: on the device it reported 260 MiB of retention in a process
> with 70 MiB resident. Nothing resident can exceed RSS, so the field was not
> imprecise but impossible. Do not reintroduce this derivation. Judge retention
> from `residual_bytes` against its own history.

`process_peak_rss`, `major_page_faults` and `minor_page_faults` come from
`getrusage` and are **process-lifetime monotonic**: they never decrease, so read
the faults as rates and `process_peak_rss` as a budget check, not a trend line.
`process_peak_rss` earns its place next to `process_rss` because it cannot be
missed — a spike between two polls still shows, which a sampled current value
cannot promise. That has already paid off: a 150.7 MiB startup-compile peak fell
entirely between two 2-minute samples and was visible only here.

`major_page_faults` is structurally near-zero — nothing FAH touches is
demand-paged from disk — so a non-zero value means real host memory pressure.
`minor_page_faults` is the one that moves, and is **the purge-thrash detector**:
returning pages with `MADV_DONTNEED` and then reallocating costs one minor fault
per page faulted back in, which is exactly the trade-off `MIMALLOC_PURGE_DELAY`
tunes. A rising fault rate at flat RSS means the purge delay is too short. It is
`0` off Unix, where `getrusage` does not exist.

This endpoint is `/api/v1/telemetry`'s `memory` block plus exactly
`allocator_committed_bytes` and `allocator_committed_peak_bytes` — the two
figures that read near zero under a different allocator, which is why they sit
under `/debug/*` and carry no compatibility promise.

The components and `minor_page_faults` are also persisted per sample into
`GET /api/v1/history/perf`, which is where a *trend* in the residual is read —
this endpoint serves one instant.

---

## Certificates *(Phase 3 — reserved)*

`/api/v1/certificates` — import PEM, import PFX, generate CA, export CA,
status. Endpoints specified when Phase 3 begins; namespace reserved now.
