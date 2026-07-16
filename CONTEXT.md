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

### Upstream

An external DNS resolver FastAdHunter forwards unblocked, uncached queries to.
Speaks plain DNS, DoT, or DoH.

### Query Log

The bounded, persisted record of individual queries (timestamp, client, domain,
type, verdict, duration). Product data, distinct from Statistics.

### Statistics

Aggregated counters derived from queries: totals, blocked percentage,
top domains, top clients, rolling time buckets. Never per-query rows.

### Metrics

Operational telemetry (Prometheus counters/histograms: QPS, latencies, cache
hit ratio, memory). For operators; distinct from Statistics (for users).

### Operating Mode

The engine's filtering scope, fixed at container start: `dns`, `dns+http`,
or `dns+http+https`.

### Atomic Swap

The reload mechanism for compiled rulesets and config: build the new value
completely, then replace the old one in a single pointer swap. Queries in
flight never observe a partial state; the hot path takes no lock.
