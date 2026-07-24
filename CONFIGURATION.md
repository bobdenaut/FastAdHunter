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

## Reference

Values below are the built-in defaults.

```toml
# ─── Engine ────────────────────────────────────────────────────────────
[engine]
mode = "dns"                  # boot    — "dns" | "dns+http" | "dns+http+https"

# ─── DNS listener ──────────────────────────────────────────────────────
[dns.listen]
address = "0.0.0.0"           # boot    — bind address; "::" = one dual-stack
                              #           socket serving IPv4 + IPv6 (IPV6_V6ONLY
                              #           off explicitly; v4 clients are reported
                              #           canonically, never as ::ffff:… mapped)
port = 53                     # boot    — UDP + TCP

# ─── Blocking behavior ─────────────────────────────────────────────────
[dns.blocking]
mode = "null_ip"              # runtime — "null_ip" (0.0.0.0 / ::) ; later: "nxdomain", "refused", "custom"
ttl_seconds = 10              # runtime — TTL of synthesized blocked answers

# ─── Cache ─────────────────────────────────────────────────────────────
[dns.cache]
max_entries = 10000           # runtime — bounded cache size (raise to 100k+ if RAM allows)
max_bytes = 67108864          # runtime — 64 MiB ceiling on what cached answers hold;
                              #           evicts oldest-first like max_entries, whichever
                              #           bound binds first (min 1 MiB)
min_ttl_seconds = 0           # runtime — clamp: honor upstream by default
max_ttl_seconds = 86400       # runtime — clamp: 24h cap
negative_ttl_max_seconds = 60 # runtime — RFC 2308 negative-cache cap
serve_stale = true            # runtime — RFC 8767: serve expired (≤24h) when upstreams down

# ─── Upstreams ─────────────────────────────────────────────────────────
[dns.upstreams]
strategy = "fallback"         # runtime — ordered parallel fallback (more modes later)
timeout_ms = 2000             # runtime — per-upstream attempt timeout

[[dns.upstreams.servers]]
address = "1.1.1.1"           # runtime
protocol = "udp"              # runtime — "udp" | "dot" | "doh"

[[dns.upstreams.servers]]
address = "9.9.9.9"
protocol = "udp"

# DoT example:  address = "1.1.1.1", protocol = "dot", hostname = "cloudflare-dns.com"
# DoH example:  address = "https://cloudflare-dns.com/dns-query", protocol = "doh"
#   (doh cert name comes from the URL host; hostname optionally overrides it,
#    e.g. for IP-literal URLs)

# ─── Rule lists ────────────────────────────────────────────────────────
[rules]
refresh_hours_default = 24    # runtime — per-list override via API

[[rules.lists]]
id = "oisd-basic"             # runtime — shipped default list
url = "https://small.oisd.nl" # runtime
enabled = true                # runtime
# refresh_hours = 6           # runtime — optional per-list override of
#                             #           refresh_hours_default (omit to follow it)
# format auto-detected: hosts | domains | easylist-family
# a mounted file is a list too: url = "/data/lists/local.txt"

# ─── Query log ─────────────────────────────────────────────────────────
[query_log]
enabled = true                # runtime
ring_entries = 10000          # runtime — in-RAM ring buffer
retention_days = 7            # runtime — SSD segment retention (age cap)
retention_max_mb = 500        # runtime — SSD segment retention (size cap)
flush_interval_seconds = 5    # runtime — batched writes (SSD-friendly)

# ─── Statistics ────────────────────────────────────────────────────────
[stats]
snapshot_interval_seconds = 300  # runtime — periodic snapshot to /data

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
level = "info"                # runtime — "error" | "warn" | "info" | "debug" | "trace"
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

The `[history]` defaults suit the RB5009's 1 TB SSD: hourly/daily rollups are
kilobytes/day and the 60 s perf series is tens of MB over 90 days, so keeping
`retention_days` at 30 (or raising it to 60/90) costs almost nothing. Both are
pruned by age like the query log — memory and disk stay bounded (hard rule 4).
