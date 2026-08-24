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

A key is **runtime** only when something re-reads it after the patch. Three do:
`history.enabled` and `history.retention_days` (pushed into the history
writers' shared retention atomic) and
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

`[schedule]` and `[[policies]]` became **runtime** in p2-06, through their own
endpoints (API.md §Policies) rather than `POST /api/v1/config`, which rejects
`policies` with 422 for the same one-owner reason `[[rules.lists]]` is rejected.
Editing the file by hand and restarting still works.

Which policies exist, and which lists each enables, decide the per-rule masks
baked into the compiled ruleset — so those two edits recompile it (seconds of
CPU on the RB5009). Assignments, schedules and labels change no mask and apply
in milliseconds. A schedule *window* opening at 21:00 never needed a restart and
still does not: the effective client → policy map is re-evaluated on a 20 s
tick, which is the precision of a boundary.

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
strategy = "fallback"         # boot    — "fallback" | "adaptive". "fallback" walks the
                              #   servers in configured order on every query. "adaptive"
                              #   is opt-in and the default is unchanged: an endpoint
                              #   that keeps failing is penalized and skipped until its
                              #   penalty expires, then one query probes it on the way
                              #   past. Every endpoint penalized still sends a query.
penalty_failures = 2          # boot    — consecutive transport failures that penalize a
                              #   healthy endpoint, 1..=255. Read only under "adaptive".
                              #   A refused connection, an unreachable network or host
                              #   and a failed TLS/DoH handshake penalize on the first
                              #   failure whatever this is set to; a timeout counts
                              #   toward it. An RCODE (SERVFAIL, NXDOMAIN, REFUSED) is a
                              #   transport success and clears the streak.
timeout_ms = 800              # boot    — per-upstream attempt timeout, 1..=10000. It
                              #   bounds one leg (connect, send, read); one attempt
                              #   against one endpoint is bounded at 3 x timeout_ms.
                              #   Fails over inside a client's own timeout; above a
                              #   slow lookup.
                              #   Under strategy = "adaptive" it also fixes the
                              #   penalty backoff, which is derived and never a
                              #   key: round n is 10 x 3 x timeout_ms doubled
                              #   n-1 times, capped at 300000 ms. The cap bounds
                              #   that nominal value; the deadline then gets
                              #   +/-25 % jitter on top, so a capped round is
                              #   skipped for 225000..375000 ms and endpoints
                              #   penalized together do not return in lockstep.
                              #   At the default the nominal ladder is 24 s,
                              #   48 s, 96 s, 192 s, then 300 s a round. From
                              #   timeout_ms = 10000 (the validated maximum) the
                              #   first penalty already sits at the cap, so
                              #   every round is a flat 5 minutes and the
                              #   backoff stops escalating.

[[dns.upstreams.servers]]
address = "1.1.1.1"           # boot
protocol = "udp"              # boot    — "udp" | "dot" | "doh"

[[dns.upstreams.servers]]
address = "9.9.9.9"
protocol = "udp"

# At least one server is required, and at most 8: an answering upstream is
# attributed per query by its index in this list (CONTEXT.md §Answering
# Endpoint), and under strategy = "adaptive" every entry owns a cache-line of
# health state walked in one pass per query. A longer list is rejected at load,
# not truncated.

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

# ─── Policies (Phase 2) ────────────────────────────────────────────────
# Optional. With no [[policies]] every client is judged under the implicit
# default policy — every enabled list, no overrides — which is exactly what
# filtering did before Policies existed (CONTEXT.md §Policy).
[schedule]
timezone = "UTC"              # runtime — POSIX TZ string, not an IANA name:
                              #           the distroless image ships no
                              #           timezone database. Bucharest is
                              #           "EET-2EEST,M3.5.0/3,M10.5.0/4".
                              #           Note POSIX signs offsets WEST-positive
                              #           — "EET-2" is UTC+2.

# [[policies]]                # runtime — at most 15, plus the default;
#                             #           /api/v1/policies, not /config
# id = "kids"                 #           referenced by nothing else; stable
# name = "Kids"               #           optional label, defaults to id
# lists = ["oisd-basic"]      #           subset of [[rules.lists]] ids; omit
#                             #           to inherit every enabled list
# blocking_mode = "null_ip"   #           per-policy override of
#                             #           [dns.blocking] mode — INERT, and so
#                             #           is the global it overrides: null_ip
#                             #           is the only implemented mode, and
#                             #           `blocked()` synthesizes exactly it.
#                             #           Becomes observable when a second mode
#                             #           (nxdomain/refused) lands, not before.
#
#   [[policies.assignments]]  #           which clients this policy applies to
#   client = "192.168.1.50"   #           an address, a CIDR block, or a
#                             #           client name (CONTEXT.md §Client)
#   days = "mon-fri"          #           mon-fri | sat,sun | daily | fri-mon
#   start = "21:00"           #           local wall clock, in [schedule]
#   end = "07:00"             #           timezone; end <= start wraps midnight
#
# days alone (no start/end) means whole days. Omit all three for an assignment
# that is always in force. The most specific assignment wins: a name, then an
# address, then the longest prefix.

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
# /health is unauthenticated and not configurable; every other route needs the key.
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
is another reason not to hand-edit a set the API is also maintaining. The
cached copy under `/data/lists/` and the content-gate baseline it restores
belong to the `id`, not the `url` — point a new source at a new id, or
`DELETE` + re-add (API.md `GET /api/v1/lists`).

## Per-query data

FastAdHunter keeps no per-query record on disk. The live feed is
`WS /api/v1/events`; long-term aggregates come from `GET /api/v1/history/*`,
which reads `/data/history`. A client wanting searchable per-query history
subscribes to the websocket and stores the events itself.

`[history] enabled` is the only switch over what is persisted about traffic, and
`false` is the privacy-maximal setting (SECURITY.md).

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
(`docs/code-review/phase1/p1.5-05-final-review.md`):

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
rehash, so it is left allocated deliberately rather than paid for on spec.

The sweep's own cost is now measured on-device — `330 + 64.4 × entries_removed`
microseconds at ~1,000 entries (0.2.9 soak). That does **not** settle the
shrink: the tables never grew past 2.2 % of `max_entries`, so nothing could have
been reclaimed by shrinking them. Deciding it needs a cache filled and then
drained, not a soak.

Setting it to `0` disables the sweep and touches nothing else. The admin
`POST /api/v1/cache/clean` remains available either way, and both count into
the same `counters.cache_cleanup` figures on `/api/v1/telemetry` because they
are the same operation.

The `[history]` defaults suit the RB5009's 1 TB SSD: hourly/daily rollups are
kilobytes/day and the 60 s perf series is tens of MB over 90 days, so keeping
`retention_days` at 30 (or raising it to 60/90) costs almost nothing. Both are
pruned by age, so memory and disk stay bounded (hard rule 4).

Each perf row also carries the memory breakdown (`memory` + `minor_page_faults`,
p2-07), which is what lets a soak attribute growth to a component instead of
only reporting RSS. It adds roughly 7 MB per 30 days at the default 60 s
cadence, pruned by `retention_days` like every other row.
