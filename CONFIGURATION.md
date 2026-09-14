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

The whole `[https]` section is **boot** on the same terms: the listener and its
semaphore are built once at bind, and the two timeouts become per-connection
deadlines held by the proxy handle. `[https.sni] no_sni` is read per connection
but from the boot-time handle, so changing it needs a restart like the rest.

Two things on the interception path are **not** boot, and neither is in this
file: who is intercepted and what always splices (the Interception Document,
§Interception Document, applied on the next connection through
`PUT /api/v1/interception`), and the CA the leaves are minted from
(`/api/v1/certificates` can generate or replace it without a restart).

`[runtime]` is **boot** too: `http_runtimes` is read once when the HTTP
listener starts its allocation domains (CONTEXT.md), so `POST /api/v1/config`
persists it and answers `restart_required: true`.

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

# ─── Runtime ───────────────────────────────────────────────────────────
[runtime]
http_runtimes = 2             # boot    — HTTP allocation domains (CONTEXT.md):
                              #           N single-thread runtimes, one OS thread
                              #           each, behind one acceptor; 0 = serve on
                              #           the shared runtime. Default max(1, cores/2)
                              #           — 2 on the RB5009 — computed by the
                              #           machine that writes the file, so a first
                              #           boot pins it. Max 64.
                              #           Env: FAH__RUNTIME__HTTP_RUNTIMES

# ─── DNS listener ──────────────────────────────────────────────────────
[dns]
tcp_max_connections = 1024    # boot    — ceiling on concurrent DNS-over-TCP
                              #           connections; the accept loop takes its
                              #           permit before accepting, so a burst
                              #           queues in the kernel backlog rather
                              #           than in process memory. Initial safety
                              #           bound, not a tuned value: retune from
                              #           /telemetry's dns_tcp_connections.peak
                              #           after a 7-day soak. Must be ≥ 1.
                              #           Env: FAH__DNS__TCP_MAX_CONNECTIONS
udp_max_inflight = 0          # boot    — ceiling on concurrent in-flight UDP DNS
                              #           queries: a count, not a rate. 0 = no cap,
                              #           today's behaviour: no admission accounting
                              #           runs, so /telemetry's dns_udp_inflight
                              #           active and peak stay 0. Past the ceiling a
                              #           datagram is dropped unanswered, counted in
                              #           /telemetry's dns_udp_inflight.shed; the client
                              #           retries as for any lost packet. Sizing:
                              #           in-flight peaks at arrival rate × the
                              #           full-outage walk (3.2 s at four UDP
                              #           upstreams and timeout_ms = 800) at ~8 KiB
                              #           heap each — 1000 qps into a dead upstream
                              #           set holds ~3200 queries, ~26 MiB. Off for
                              #           the household deployment by measurement; an
                              #           operational memory guardrail for office or
                              #           high-volume deployments. Evidence:
                              #           docs/code-review/phase2.6/f2-udp-inflight.md
                              #           Env: FAH__DNS__UDP_MAX_INFLIGHT

[dns.listen]
address = "::"                # boot    — bind address; "::" = one dual-stack
                              #           socket serving IPv4 + IPv6 (IPV6_V6ONLY
                              #           off explicitly; v4 clients are reported
                              #           canonically, never as ::ffff:… mapped)
port = 53                     # boot    — UDP + TCP
dot_enabled = true            # boot    — DNS-over-TLS listener (RFC 7858)
dot_port = 853                # boot    — TCP; binds before the privilege drop
                              #           like 53; must differ from every other
                              #           listener port. Serves a CA-minted
                              #           certificate for the hostname the client
                              #           sends when a CA exists (Android Private
                              #           DNS hostname mode needs the CA
                              #           installed), else the API certificate
doh_enabled = true            # boot    — DNS-over-HTTPS (RFC 8484) at
                              #           https://<api>/dns-query, unauthenticated;
                              #           false removes the route entirely.
                              #           Needs [api] tls = true: with TLS off the
                              #           route is absent (DoH is HTTPS-only) and
                              #           the boot log warns once

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
strategy = "adaptive"         # boot    — "adaptive" is the only value: an endpoint that
                              #   keeps failing is penalized and skipped until its
                              #   penalty expires, then one query probes it on the way
                              #   past. Every endpoint penalized still sends a query.
                              #   "fallback" (the ordered walk, the default until 0.3.3)
                              #   is rejected at load; drop the key or say "adaptive".
penalty_failures = 2          # boot    — consecutive transport failures that penalize a
                              #   healthy endpoint, 1..=255.
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

# ─── HTTPS SNI filtering (Phase 3) ─────────────────────────────────────
# Inert unless [engine] mode is "dns+http+https". Nothing on this path
# decrypts: the ClientHello is read, the SNI is judged, and the connection is
# then closed or spliced byte-for-byte (SECURITY.md).
[https.listen]
address = "::"                # boot    — as [http.listen]: "::" is one
                              #           dual-stack socket, IPV6_V6ONLY off
port = 8444                   # boot    — NOT 443 (privileged, ADR-0004) and
                              #           NOT 8443, which [api] port already
                              #           uses — two listeners on one default
                              #           port would make dns+http+https fail
                              #           to boot on an untouched config. The
                              #           router dst-nats 443 here. A value equal
                              #           to [api], [http.listen] or [dns.listen]
                              #           port is rejected at load, by name

[https]
max_connections = 1024        # boot    — ceiling on concurrent HTTPS sessions,
                              #           spliced and intercepted alike; the
                              #           accept loop is the same one [http]
                              #           uses, permit before accept. Splice
                              #           memory is 2 x 16 KiB per session
                              #           (~32 MB at the default). An
                              #           INTERCEPTED session (a client listed
                              #           in the Interception Document) is
                              #           bounded by
                              #           fixed hyper limits set in code, not
                              #           by hyper's defaults: 64 concurrent
                              #           streams, h2 receive window 64 KiB per
                              #           stream and 64 x 64 KiB = 4 MiB per
                              #           connection (the connection window is
                              #           derived from the other two so a few
                              #           stalled streams cannot starve the
                              #           rest), 64 KiB send buffer PER STREAM,
                              #           h1 buffers 128 KiB — on BOTH the
                              #           client and the origin side. These are
                              #           flow-control CEILINGS, not a measured
                              #           footprint: per leg they bound what a
                              #           fully stalled session may hold at
                              #           ~4 MiB of receive window plus the
                              #           64 x 64 KiB send buffers; typical is
                              #           far below, since a buffer fills only
                              #           when the client stops reading. The
                              #           observed per-session memory is
                              #           measured on the device by p3-06 P3
                              #           (docs/code-review/phase3/), which is
                              #           the only authority for that figure.
                              #           Plus two TLS sessions; bodies stream
                              #           and are never held. A silent
                              #           preconnect holds a permit for up to
                              #           hello_timeout_ms
hello_timeout_ms = 10000      # boot    — deadline for a client to finish
                              #           sending its ClientHello, and the
                              #           deadline on the upstream connect. A
                              #           blackholed destination must not hold
                              #           a max_connections permit for the
                              #           kernel's SYN-retry window. On an
                              #           intercepted session it also bounds
                              #           the upstream TLS verification (each
                              #           reconnect too) and our own handshake;
                              #           the wait between requests is
                              #           idle_timeout_ms, h1 and h2 alike. 0
                              #           is rejected at load
idle_timeout_ms = 60000       # boot    — a spliced or intercepted session with
                              #           no activity in BOTH directions for
                              #           this long is closed (one session-wide
                              #           deadline, not one per direction).
                              #           Long-lived idle connections
                              #           (WebSocket over TLS) are cut and must
                              #           reconnect; raise it if that matters.
                              #           0 is rejected at load

[https.sni]
no_sni = "pass"               # boot    — "pass" | "block". A ClientHello with
                              #           no plaintext SNI (or an ECH-encrypted
                              #           one) is CLOSED EITHER WAY: the
                              #           container cannot recover the
                              #           pre-dst-nat destination
                              #           (docs/routeros-traps.md — measured
                              #           SO_ORIGINAL_DST = ENOENT), so there is
                              #           nothing to splice to. This key decides
                              #           only how that closed connection is
                              #           classified in events and metrics

# [https.interception] NO LONGER EXISTS. `clients` and `exclude_domains` moved
# out of this file in release N — see §Interception Document below.

# ─── Egress (where the proxies may connect) ────────────────────────────
# NOT under [http] on purpose: Phase 3's HTTPS path derives its destination
# from SNI — an equally client-controlled claim — and judges it with these same
# rules (CONTEXT.md §Egress Guard). Since p3-03 the SNI path uses them at port
# 443 — the SNI hostname is the destination claim, judged after resolution
# exactly as a Host header is.
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
                              #           against the resolved address. It
                              #           governs the HTTP proxy path only. On
                              #           the HTTPS path an IP-literal SNI is
                              #           never served: refused here while the
                              #           switch is off, and failing at
                              #           hostname resolution once it is on.
                              #           The option does not offer symmetric
                              #           IP-literal support across HTTP and
                              #           HTTPS — decided 2026-09-14, audit F1

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
# address, when set to a literal IP rather than 0.0.0.0 or ::, is included in the
# SANs of the generated API certificate. It does not need to be set: the box's own
# LAN address is discovered at generation time. Pin it only when the reachable
# address is not the one the default route selects.
# /health and POST /api/v1/auth/login are unauthenticated and not configurable;
# every other route needs the API key or a session cookie.
# api key: stored in /config, never in this file's plaintext sections;
# rotate via POST /api/v1/config/apikey/rotate
# dashboard password: Argon2id hash in /config/auth-hash, session secret in
# /data/session-secret — neither is a config key. tls = false removes session
# login (the __Host- cookie needs Secure); bearer auth is unaffected.

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
4. a dashboard password — **printed once to the container log**, with only its
   Argon2id hash stored in `/config/auth-hash`. It is never printed again and
   no API route ever returns it. Lost it? Delete `/config/auth-hash` and
   restart; see [SECURITY.md](SECURITY.md) §Password recovery.
5. a session secret in `/data/session-secret`, which every session cookie is
   signed with

The container is functional with zero configuration.

**There is no `[auth]` section**, and the dashboard password has no mutability
class — it is not a config key. The hash and the session secret are two
standalone files, changed through `POST /api/v1/auth/password` or by deleting
them. `POST /api/v1/config` rejects a top-level `auth` key with `422`, and
`GET /api/v1/config` omits auth material entirely. Keeping them out of the TOML
is also what keeps a rollback to an older binary clean.

## Volumes

| Mount | Class | Contents |
|-------|-------|----------|
| `/config` | small, back this up | TOML, `interception.json`, API key, TLS certs, `auth-hash` |
| `/data`   | bulky, regenerable  | `session-secret`, cached rule lists, query-log segments, stats snapshots, history rollups + perf series |

`/data/session-secret` is regenerable in the sense that a fresh one is written
when it is missing — but regenerating it **ends every signed-in session**, and
the password is unchanged. An ephemeral `/data` therefore signs everyone out on
each restart, logged at `warn!`.

## Interception Document

`clients` and `exclude_domains` are **not** config keys. They live in
`/config/interception.json` — the Interception Document (CONTEXT.md) — and are
read and replaced whole through `GET`/`PUT /api/v1/interception` (API.md).

A `PUT` applies on the **next accepted connection**: no restart, and the
response carries no `restart_required`. This is the third mutability class
again, like `[[rules.lists]]` above, and for the same reason — one owner. The
machinery exists whenever the HTTPS listener runs and the certificate store
opened, so listing the first client is a swap, not a rebuild.

```json
{
  "clients": ["192.168.88.10", "192.168.88.0/24"],
  "exclude_domains": ["unicredit.ro"]
}
```

| Key | Meaning |
|-----|---------|
| `clients` | IP addresses or CIDR blocks whose HTTPS is TERMINATED with a leaf minted by the installed CA and filtered at URL level (SECURITY.md §Later phases, CONTEXT.md §Terminate Leg). Empty = nobody is intercepted; every other client splices. Cap 256 |
| `exclude_domains` | SNI hostnames that always splice, even for a listed client. A name excludes itself and every subdomain (`bank.example` covers `api.bank.example`, not `notbank.example`). Cap 512 |

**There is no compiled-in baseline.** An empty `exclude_domains` excludes
nothing. Earlier releases shipped a hard-coded list of certificate-pinned
families (Apple, Google/Android, Microsoft, WhatsApp, Signal, PayPal and
several banks) that could not be removed from config; it is gone. Anything that
must splice has to be listed here — decide per household, and see SECURITY.md.

**Precondition, unchanged:** every listed client holds a static DHCP lease or a
static address on the router. The source IP is the only identity the container
sees, and a reassigned lease silently moves interception to whichever device
inherits the address. A listed client must also trust the CA
(`/api/v1/certificates` export); one that does not is closed after a wasted
upstream handshake.

Two conditions the engine cannot supply (SECURITY.md §Later phases): the
router must refuse UDP 443 for listed clients, or HTTP/3 bypasses the steer
(deploy-rb5009.md §5c); and on Android most apps ignore the user trust store,
so a listed phone is intercepted in its browsers and refused by its apps —
exclude per host or do not list it. `GET /api/v1/clients` `intercepted` shows
what each client did.

Listed clients with no CA installed, or with a certificate store that did not
open, are spliced (store) or closed (no CA) — never fatal, DNS keeps resolving;
one `warn!` at boot names which. A `PUT` listing a client while the store is
closed answers **503**, and nothing is written.

Rejections are **422 with a structured `details` object** (API.md) and change
nothing — neither the file nor the running scope. The stored document keeps the
spelling you sent; only the matcher is normalised.

`POST /api/v1/config` **rejects** a patch carrying `https.interception` (422,
naming this endpoint), and `GET /api/v1/config` does not contain it. `FAH__`
cannot set either list — `FAH__HTTPS__INTERCEPTION__CLIENTS` fails boot as an
unknown key.

### Migration from 0.3.x

Release N migrates once, at the first boot that finds the old keys:

| Boot state | Result |
|------------|--------|
| `[https.interception]` present, no document | document written from the TOML values; TOML re-saved without the keys; one `info!` |
| Fresh install | empty document written; TOML untouched |
| Document present | document wins; nothing rewritten |
| Keys re-added by hand after migrating | document still wins; `warn!` naming both files; the keys are removed from the TOML again |
| Document unreadable, malformed, over cap, or carrying an invalid entry | **boot fails naming the file**; it is never overwritten |
| `[https.interception]` carrying an invalid entry or over a cap, no document | **boot fails naming the list**, in every `engine.mode` — 0.3.x checked the keys only when the HTTPS listener ran, so a DNS-only install could carry a bad entry unnoticed; document not written; TOML untouched |

The document is written on exactly one branch — when it does not exist — so an
existing file is never clobbered. The TOML is rewritten from the file layer, so
a value that came from a `FAH__` variable at that boot is not baked into the
file. Hand-written comments in the TOML do not survive that rewrite, as with
any API write.

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
