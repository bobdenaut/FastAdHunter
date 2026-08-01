# CONFIGURATION

Single source of truth: `/config/fastadhunter.toml`.

## Precedence

```text
built-in defaults  <  config file  <  environment variables  <  API changes
```

- **Environment variables** override file values for Docker-compose users.
  Naming: `FAH__` prefix, `__` as section separator, upper-case.
  Example: `FAH__DNS__CACHE__MAX_ENTRIES=100000` → `[dns.cache] max_entries`.
- **API changes** (`POST /api/v1/config`) are validated and **written back to
  the file** — no hidden state; the file always reflects the running intent.
- `GET /api/v1/config` returns the merged effective config, secrets redacted.

## Mutability classes

| Class | Behavior |
|-------|----------|
| **runtime** | applied live via atomic swap, no restart |
| **boot** | API accepts + persists, responds `restart_required: true` |

A key is **runtime** only when something re-reads it after the patch. Four do:
`history.enabled` and `history.retention_days` (pushed into the history
writers' shared retention atomic), `api.metrics_public` and
`rules.refresh_hours_default` (read per request), plus the list set, which the
`/lists` endpoints keep in step with the file. Everything else is read once
during startup — the cache and upstream pool are built there, the query-log and
stats intervals become timers, the log level becomes a tracing filter — so it is
**boot**, and the API says so rather than reporting an apply that never
happened. Tuning the cache or the upstreams means restarting the container;
`POST /api/v1/cache/clean` is the way to release cache memory without one.

The whole `[http]` section is **boot** for the same reason, including
`max_connections`, which looks runtime-shaped but is not: the semaphore is
sized once when the listener binds. Promoting a key means giving it a live
consumer first — never relabelling it and hoping.

## Reference

Values below are the built-in defaults.

```toml
# ─── Engine ────────────────────────────────────────────────────────────
[engine]
mode = "dns"                  # boot    — "dns" | "dns+http" | "dns+http+https"

# ─── DNS listener ──────────────────────────────────────────────────────
[dns.listen]
address = "::"                # boot    — bind address; "::" = one dual-stack
                              #           socket serving IPv4 + IPv6 (IPV6_V6ONLY
                              #           off explicitly; v4 clients are reported
                              #           canonically, never as ::ffff:… mapped)
port = 53                     # boot    — UDP + TCP

# ─── Blocking behavior ─────────────────────────────────────────────────
[dns.blocking]
mode = "null_ip"              # boot    — "null_ip" (0.0.0.0 / ::) ; later: "nxdomain", "refused", "custom"
ttl_seconds = 10              # boot    — TTL of synthesized blocked answers

# ─── Cache ─────────────────────────────────────────────────────────────
[dns.cache]
max_entries = 10000           # boot    — bounded cache size (raise to 100k+ if RAM allows)
max_bytes = 67108864          # boot    — 64 MiB ceiling on what cached answers hold;
                              #           evicts oldest-first like max_entries, whichever
                              #           bound binds first (min 1 MiB)
min_ttl_seconds = 0           # boot    — clamp: honor upstream by default
max_ttl_seconds = 86400       # boot    — clamp: 24h cap
negative_ttl_max_seconds = 60 # boot    — RFC 2308 negative-cache cap
serve_stale = true            # boot    — serve expired entries (≤24h) rather than failing
swr_workers = 3               # boot    — background refreshers for stale entries (ADR-0005);
                              #           a stale hit answers from cache at once and the
                              #           refresh happens off the query path. 0 disables,
                              #           reverting to "stale only after a failed forward"
cleanup_interval_seconds = 360 # boot   — background sweep of entries past the stale
                              #           window. 0 disables. Does NOT bound the cache —
                              #           max_entries/max_bytes do; this returns memory
                              #           underneath them. Expect it to reclaim little
                              #           while serve_stale is on (see below)

# ─── Upstreams ─────────────────────────────────────────────────────────
[dns.upstreams]
strategy = "fallback"         # boot    — ordered parallel fallback (more modes later)
timeout_ms = 800              # boot    — per-upstream attempt timeout (fails over
                              #   inside a client's own timeout; above a slow lookup)

[[dns.upstreams.servers]]
address = "1.1.1.1"           # boot
protocol = "udp"              # boot    — "udp" | "dot" | "doh"

[[dns.upstreams.servers]]
address = "9.9.9.9"
protocol = "udp"

# DoT example:  address = "1.1.1.1", protocol = "dot", hostname = "cloudflare-dns.com"
# DoH example:  address = "https://cloudflare-dns.com/dns-query", protocol = "doh"
#   (doh cert name comes from the URL host; hostname optionally overrides it,
#    e.g. for IP-literal URLs)

# ─── HTTP engine (Phase 2) ─────────────────────────────────────────────
# Inert unless [engine] mode includes "http". There is deliberately no
# `enabled` key here — mode is the only switch (CONTEXT.md §Operating Mode).
[http.listen]
address = "::"                # boot    — as [dns.listen]: "::" is one
                              #           dual-stack socket, IPV6_V6ONLY off
port = 8080                   # boot    — NOT 80: the container is unprivileged
                              #           after the ADR-0004 drop, and the router
                              #           dst-nats 80 here instead

[http]
max_connections = 1024        # boot    — ceiling on concurrent connections; the
                              #           accept loop takes its permit before
                              #           accepting, so a burst queues in the
                              #           kernel rather than in process memory
idle_timeout_ms = 60000       # boot    — how long an idle *upstream* connection
                              #           is kept in the pool. Client-side idle
                              #           is bounded by header_timeout_ms below,
                              #           which hyper arms while waiting for the
                              #           next request head
header_timeout_ms = 10000     # boot    — slowloris bound: deadline for a client
                              #           to finish sending its request head.
                              #           Also caps how long a keep-alive
                              #           connection may sit between requests

# ─── Egress (where the proxies may connect) ────────────────────────────
# NOT under [http] on purpose: Phase 3's HTTPS path derives its destination
# from SNI — an equally client-controlled claim — and judges it with these same
# rules (CONTEXT.md §Egress Guard).
#
# The router dst-nats port 80 into the container and RouterOS exposes no
# SO_ORIGINAL_DST, so the only statement of where a client meant to go is a
# header it wrote. Default-deny is therefore the security property, not a
# preference: without it any LAN device could use the proxy to reach the router
# or FastAdHunter's own API.
[egress]
allow_destinations = []       # boot    — private/local destinations the proxy
                              #           may nonetheless reach, as IPs or CIDR
                              #           blocks ("192.168.10.50",
                              #           "192.168.10.0/24"). Empty = every
                              #           private, loopback, link-local, CGNAT
                              #           and unique-local address is refused.
                              #           Judged on the RESOLVED address, so a
                              #           public name pointing at 192.168.x.x
                              #           (a DNS rebind) is refused too
allow_ip_literal_hosts = false # boot   — whether a client may name a bare IP as
                              #           its destination. A browser resolving a
                              #           name never produces one, so this is
                              #           the shape of a probe; prefer
                              #           allow_destinations, which is checked
                              #           against the resolved address

# ─── Rule lists ────────────────────────────────────────────────────────
[rules]
refresh_hours_default = 24    # runtime — per-list override via API

[[rules.lists]]               # /lists — see "Who owns the list set" below
id = "oisd-basic"             #          shipped default list
url = "https://small.oisd.nl"
enabled = true
# refresh_hours = 6           #          optional per-list override of
#                             #          refresh_hours_default (omit to follow it)
# format auto-detected: hosts | domains | easylist-family
# a mounted file is a list too: url = "/data/lists/local.txt"

# ─── Query log ─────────────────────────────────────────────────────────
[query_log]
enabled = true                # boot
ring_entries = 10000          # boot    — in-RAM ring; the ONLY thing
                              #           GET /api/v1/queries can read
retention_days = 7            # boot    — SSD segment retention (age cap)
retention_max_mb = 500        # boot    — SSD segment retention (size cap);
                              #           whichever cap binds first prunes
flush_interval_seconds = 5    # boot    — batched writes (SSD-friendly)

# ─── Statistics ────────────────────────────────────────────────────────
[stats]
snapshot_interval_seconds = 300  # boot    — periodic snapshot to /data

# ─── History (long-term observability on /data/history) ────────────────
[history]
enabled = true                # runtime — master switch; false stops both
                              #           history writers + the perf sampler
sample_interval_seconds = 60  # boot    — perf/system/cache sampling cadence
                              #           (ticker built at startup)
retention_days = 30           # runtime — age cap on /data/history day-files;
                              #           30/60/90 typical (applied live to the
                              #           next prune, no restart)

# ─── API ───────────────────────────────────────────────────────────────
[api]
address = "0.0.0.0"           # boot    — bind LAN-side only; never expose to WAN
port = 8443                   # boot
tls = true                    # boot    — self-signed generated on first boot; opt-out is UNSAFE
metrics_public = true         # runtime — /health + /metrics without API key
# api key: stored in /config, never in this file's plaintext sections;
# rotate via POST /api/v1/config/apikey/rotate

# ─── Logging ───────────────────────────────────────────────────────────
[log]
level = "info"                # boot    — "error" | "warn" | "info" | "debug" | "trace"
format = "text"               # boot    — "text" | "json"
```

## First boot

Empty `/config` volume → FastAdHunter generates:

1. `fastadhunter.toml` with the defaults above
2. an API key (printed once to the container log, stored in `/config`)
3. a self-signed TLS certificate (rcgen) for the API

The container is functional with zero configuration.

## Volumes

| Mount | Class | Contents |
|-------|-------|----------|
| `/config` | small, back this up | TOML, API key, TLS certs |
| `/data`   | bulky, regenerable  | cached rule lists, query-log segments, stats snapshots, history rollups + perf series |

## Who owns the list set

`[[rules.lists]]` has a third mutability class of its own: **owned by
`/api/v1/lists`**. Adding, removing, enabling, disabling or re-scheduling a
list through those endpoints applies **live** — fetch, recompile, atomic swap,
no restart — and they write the result back to this file themselves, so the
TOML always describes the running engine.

`POST /api/v1/config` therefore **rejects** a patch carrying `rules.lists`
(422). It would be a second writer over the same state with no reconciliation:
that path does not reload the engine, so the next `/lists` call would persist
the engine's set over the patch and the edit would disappear. One owner
instead — the file is the boot source and the durable record, `/lists` is the
runtime API.

Editing `[[rules.lists]]` here by hand still works; it takes effect at the next
start, like any boot value. Arrays replace wholesale rather than merging, which
is another reason not to hand-edit a set the API is also maintaining.

## What reads the query log

`[query_log]` has two storage tiers and only one of them is readable today:

- **`ring_entries`** — the in-RAM ring. This is what `GET /api/v1/queries`
  serves, and the only thing it serves. The history it can answer for is
  `ring_entries ÷ current QPS`: ~2.8 h for a household at ~1 QPS, ~2 minutes at
  85 QPS. A `from`/`to` range older than the ring comes back **empty**, not as an
  error.
- **`retention_days` / `retention_max_mb`** — the `/data` segments. Written and
  pruned correctly (whichever cap binds first), but **no endpoint reads them
  yet**; they exist for per-query drill-down in the dashboard phase. Long-term
  aggregates come from `/api/v1/history/*`, which reads `/data/history` instead.

`retention_max_mb` counts **MiB** (`retention_max_mb × 1024 × 1024`), and the
total may sit slightly *above* the cap: prune never deletes the segment
currently being written, and segments roll at 1 MiB, so the real bound is
`cap + (active segment < 1 MiB)`. Measured on the RB5009 at the 500 MiB default:
524,534,431 B against a 524,288,000 B cap — 240 KB over, the active segment's
fill. Bounded, and not a defect.

Sizing consequence: the segments cost real disk and real write volume for data
nothing can currently return. At household rates that is ~25 MB/day and
irrelevant. Under a synthetic load generator it is not — 85 QPS produces roughly
1.8 GB/day of segment writes, recycling a 500 MiB cap about every 6.7 hours, so
`retention_max_mb` binds long before `retention_days` and the retained span is
hours rather than the configured week. Consider `enabled = false` for synthetic
soak runs.

## The two cache bounds

`[dns.cache]` bounds the DNS cache twice, and both bounds evict through the
same oldest-first (FIFO) order:

- `max_entries` caps **how many** answers are resident. Divided across 16
  shards, so the real bound (`GET /api/v1/cache`'s `capacity`) can round
  slightly below it.
- `max_bytes` caps **how large** those answers are allowed to be in total.
  Same per-shard split, same rounding.

Whichever is reached first triggers eviction. Entry count alone cannot bound
memory: a ~91h soak under an adversarial generator (7 query types × 1M unique
domains, so every answer a large TXT/SOA/NXDOMAIN) filled an entry-bounded
cache to a ~230 MiB plateau — 80% past PERFORMANCE.md's 128 MB budget — while
real household traffic sat at ~55 MiB. Raising `max_entries` "to 100k+ if RAM
allows" is safe precisely because `max_bytes` still holds the ceiling.

The figure `max_bytes` governs is the one `GET /api/v1/cache` reports as
`bytes`: what the cached answers themselves hold. The hash-table and eviction
queue slabs sit outside it — they scale with `max_entries`, not with answer
size, and `/debug/memory`'s `cache_estimated_bytes` is the number that
includes them.

`max_bytes` is a soft ceiling, not a hard wall: each shard keeps at least one
answer even when that single answer is larger than the shard's byte share
(`max_bytes / 16`), so it degrades to "one entry per shard" instead of evicting
what it just stored. Resident bytes are therefore bounded by
`16 × max(per-shard share, one largest answer)`. At the default 64 MiB the share
is ~4 MiB/shard — far above any DNS answer — so `bytes` stays at or under
`max_bytes`. It only matters near the 1 MiB floor: there the ~64 KiB/shard share
is below a maximum-size TCP answer, and a record-dense 64 KiB answer costs on the
order of a megabyte of tracked bytes on its own, so `bytes` can settle around
**10× a 1 MiB cap** rather than a little over it. Still bounded, still not a
leak — but a real reason not to run near the floor, and the arithmetic to use is
the formula above, not "a bit over".

Two related things the cap does **not** cover, both measured on the RB5009
(`docs/code-review/p1.5-05-final-review.md`):

- The figure it enforces is the *answers'* heap. The hash-table slab and the
  eviction queue sit outside it and measured **+55 %** on top at 50 000 entries —
  so a 64 MiB `max_bytes` corresponds to roughly 99 MiB of real cache memory
  whenever the cap actually binds. `/debug/memory`'s `cache_estimated_bytes` is
  the number that includes them, and `GET /api/v1/cache` reports the enforced
  `bytes`.
- The eviction queue's own ceiling scales with `max_entries`, not with
  `max_bytes` — worth knowing before raising `max_entries` by an order of
  magnitude.

## The cleanup sweep is not a third bound

`cleanup_interval_seconds` (default 360) runs a background sweep that removes
entries past the serve-stale window. Read it as a **memory-return** knob, not
as a bound — the two bounds above are what keep the cache from growing, and
they hold whether the sweep runs or not.

**Expect it to find nothing most of the time, and do not read that as a fault.**
With `serve_stale = true` (the default) an entry is only sweepable 24 h after
its TTL lapsed, and under any real query rate FIFO eviction has reached such
entries long before. The case the sweep actually serves is the cache that goes
*idle below both caps* — a household resolver overnight — where nothing else
would ever reclaim those entries.

What it returns is the entries' own heap plus their eviction-queue nodes. The
hash-table slab is **not** returned: shrinking it is a reallocation and a full
rehash whose cost on a 1.4 GHz ARM core has not been measured, so it is left
allocated deliberately rather than paid for on spec.
`fastadhunter_cache_cleanup_duration_seconds` at real occupancy is the figure
that decision is waiting on.

Setting it to `0` disables the sweep and touches nothing else. The admin
`POST /api/v1/cache/clean` remains available either way, and both count into
the same `fastadhunter_cache_cleanup_*` metrics because they are the same
operation.

The `[history]` defaults suit the RB5009's 1 TB SSD: hourly/daily rollups are
kilobytes/day and the 60 s perf series is tens of MB over 90 days, so keeping
`retention_days` at 30 (or raising it to 60/90) costs almost nothing. Both are
pruned by age like the query log — memory and disk stay bounded (hard rule 4).
