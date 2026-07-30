# CONTEXT — Ubiquitous Language

Glossary of FastAdHunter domain terms. Code, documentation, API names and
conversations use these terms with exactly these meanings. No implementation
details belong here.

## Terms

### Rule

A single filtering instruction from a rule list (e.g. `||ads.example.com^`,
a hosts entry, an exception `@@||cdn.example.com^`). A rule is either
**DNS-applicable** (acts on a domain name) or **non-DNS** (cosmetic, URL-path,
HTTP-option based — inactive until the HTTP/HTML phases).

### Rule List

A named collection of rules from one source (URL, local file, or user input).
Has a format (hosts, plain domain list, EasyList, uBlock Origin, AdGuard),
a refresh interval, and an enabled/disabled state.

### Rule Engine

The single component that loads, parses, manages and processes rule lists in
all supported formats, compiles them into matchers, and answers verdicts.
There is no separate "Filter Engine" — that term is retired.

### Verdict

The Rule Engine's decision for one query: **Allow** (explicit exception match),
**Block** (block rule match), or **Pass** (no rule matched — forward normally).
Allow always wins over Block.

### Blocklist / Allowlist

Informal names for rule lists whose rules predominantly block or allow.
Both are just rule lists; the distinction lives in the rules, not the list.

### Query

One DNS question received from a client (domain, record type, client source IP,
timestamp).

### Client

A device on the network, identified by the source IP of its queries. May carry
an optional user-assigned name. In Phase 1 clients are observed and reported
(per-client statistics); filtering policy is global.

### Policy *(reserved — not implemented in Phase 1)*

A named bundle of rule lists and settings assignable to clients or schedules
(parental-control style). Planned for Phase 2+.

### Blocked Response

The synthesized DNS answer returned for a blocked query: `0.0.0.0` for A,
`::` for AAAA, with a short fixed TTL.

### Cache

The bounded in-memory store of **upstream answers only**. The cache never
stores verdicts; the Rule Engine runs before the cache on every query.
An entry passes through three lifetime stages: **fresh** (within TTL —
answers queries directly), **stale** (past TTL but within the serve-stale
window — see below), **expired** (past the stale window — dead weight until
something removes it).

### Cache clean

The sweep that removes expired entries. It runs on a schedule
(`[dns.cache] cleanup_interval_seconds`) and on demand from the admin API; both
are the same operation, so both are counted the same way. It removes **only**
expired entries — never fresh ones, and never stale ones, which are the
serve-stale insurance an outage is survived on. Purging the stale window too is
a separate, explicit admin choice.

A clean is not what *bounds* the cache — `max_entries` and `max_bytes` do that,
by eviction. A clean returns memory the cache has stopped needing while it sits
below those bounds, which is otherwise never reclaimed.

### Stale-while-refresh

What a **stale** entry does. It answers the query **immediately**, at cache-hit
latency, and a refresh job goes to a small pool of detached background workers
(`[dns.cache] swr_workers`) that the query path never waits on — ADR-0005. The
reply carries a deliberately short TTL so the asking resolver comes back soon.

Two terms belong to that pool and mean nothing outside it:

- **Claim** — the right to refresh one stale entry, held for a short lease and
  taken under the cache shard lock the lookup already holds. Exactly one query
  per lease gets it, which is what makes many simultaneous hits on one expiring
  name produce **one** refresh rather than one each.
- **Cooldown** — the longer suppression a *failed* refresh leaves behind, so a
  dead upstream cannot turn every stale hit into a forward.

`swr_workers = 0` disables the pool, and a stale entry then answers only after a
forward has actually failed — the pre-ADR-0005 behaviour, and the one RFC 8767
§4 describes. With the pool on, the behaviour is closer to HTTP's
`stale-while-revalidate` (RFC 5861): do not call it "RFC 8767 serve-stale".

### Upstream

An external DNS resolver FastAdHunter forwards unblocked, uncached queries to.
Speaks plain DNS, DoT, or DoH.

### HTTP Engine

The component that filters unencrypted HTTP by **URL**, not just by hostname —
`fah-http`. It sees the request line, so a rule can target one path on a host
the rest of the site still needs, which the DNS Engine structurally cannot do.
Phase 2.

### Pass-through

A request the HTTP Engine relays without inspecting or buffering its body:
bytes are streamed between client and origin in both directions. The common
case and the fast path. Distinct from a **Block**, which is answered locally
and never reaches the origin. Response bodies are always pass-through in
Phase 2; Phase 4 adds opt-in HTML rewriting for that content type alone.

### Interception

Getting client traffic to FastAdHunter without configuring the clients: the
router redirects the port (dst-nat) to the container. **Transparent** —
browsers hold no proxy setting and nothing on the client changes. The same
mechanism already carries DNS; extending it to HTTP is one more rule, and
rollback is removing it.

### Port

A trait a lower layer declares to describe what it needs from a higher one, so
the binary can supply the implementation without the dependency arrow pointing
upward. `HostResolver` (declared by the Rule Engine, implemented over the
Upstreams) and `StatsSource`/`TelemetrySource`/`CacheSource` (declared by
the API) are the existing ones. See ARCHITECTURE.md §Dependency Layering.

### Query Log

The bounded, persisted record of individual queries (timestamp, client, domain,
type, verdict, duration). Product data, distinct from Statistics.

### Statistics

Aggregated counters derived from queries: totals, blocked percentage,
top domains, top clients, rolling time buckets. Never per-query rows.

### Metrics

Operational telemetry (Prometheus counters/histograms: QPS, latencies, cache
hit ratio, memory). For operators; distinct from Statistics (for users).

### Accounted / Residual

The two-way split of resident memory, reported by `/api/v1/debug/memory` and
`/metrics` (p2-07, `crates/fastadhunter/src/allocator.rs`). Use these words for these things and no others.

- **Accounted** — the sum of every *bounded* component that reports its own
  heap: compiled ruleset, DNS cache, and the Statistics structures. Growth here
  is not a leak; it is a structure filling toward its cap.
- **Residual** — `RSS − accounted`. Binary text and data pages, thread stacks,
  the runtime, and memory the allocator holds without having returned it. Never
  zero, and it is the *trend* that carries meaning: **residual growing while the
  components stay flat is the leak signal**, because expected growth has been
  subtracted out.

**"Allocator retained" is a retired term.** It named `committed − accounted` and
was served as `allocator_retained_bytes` until 0.2.8. Measured on the RB5009 it
reported 260 MiB of "retention" in a process with 70 MiB resident — impossible
for anything resident — because mimalloc v3 never decrements its commit counter
when a purge returns pages, so the minuend only ever rises. The residual is the
only split; nothing subdivides it. See
`docs/code-review/0.2.7-router-memory-and-throughput.md` §5.2, §6.1.

**Committed** is what the allocator has committed by its own accounting — not a
kernel reading, not the same as resident, and **not a live figure**: it
accumulates and routinely exceeds RSS. It is reported for correlation, never as
a footprint.

### Operating Mode

The engine's filtering scope, fixed at container start: `dns`, `dns+http`,
or `dns+http+https`.

It is the **only** switch for whether an engine runs — there is no second
`enabled` flag per section. A mode that does not name an engine means that
engine's listener is never bound, not bound and idle: a held port that
completes a `connect()` and then does nothing is indistinguishable, from the
client's side, from a hung proxy.

### Atomic Swap

The reload mechanism for compiled rulesets and config: build the new value
completely, then replace the old one in a single pointer swap. Queries in
flight never observe a partial state; the hot path takes no lock.
