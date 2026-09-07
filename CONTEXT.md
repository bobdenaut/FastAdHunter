# CONTEXT — Ubiquitous Language

Glossary of FastAdHunter domain terms. Code, documentation, API names and
conversations use these terms with exactly these meanings. No implementation
details belong here.

## Terms

### Rule

A single filtering instruction from a rule list (e.g. `||ads.example.com^`,
a hosts entry, an exception `@@||cdn.example.com^`). Every rule is exactly one
of three kinds, decided by what it *addresses*:

- **DNS-applicable** — acts on a domain name. Answered by the Domain Tier.
- **request-applicable** — acts on a URL or on request context (`$script`,
  `$third-party`). Answered by the URL Tier since Phase 2.
- **inactive** — no tier answers it yet: cosmetic (Phase 4), `$client`
  (Policies), and patterns no supported syntax expresses.

"non-DNS" is retired as a category name — it merged the second and third, which
is precisely the distinction that matters now that the URL Tier exists.

### Rule List

A named collection of rules from one source (URL, local file, or user input).
Has a format (hosts, plain domain list, EasyList, uBlock Origin, AdGuard),
a refresh interval, and an enabled/disabled state.

### Rule Engine

The single component that loads, parses, manages and processes rule lists in
all supported formats, compiles them into matchers, and answers verdicts.
There is no separate "Filter Engine" — that term is retired.

### Domain Tier / URL Tier

The two halves of the compiled ruleset, one per request model. The **Domain
Tier** answers a domain name and its parent labels; the **URL Tier** answers a
full request — URL, method, resource type, referer. One compiled matcher holds
both, and one typed entry point serves each; there is no "HTTP matcher" as a
separate component.

A request consults **both** tiers, under one precedence order (see Verdict).
A DNS query consults only the Domain Tier — a URL rule has no answer for a
question that carries no URL.

### HTTP Request

One request as the Rule Engine sees it: URL, host, method, **Resource Type**,
and the document host taken from `Referer`. The HTTP counterpart of a Query,
and equally a pure data type — everything in it is already extracted, so the
engine parses nothing.

### Resource Type

What a request is fetching — `script`, `image`, `stylesheet`, `document`,
`xmlhttprequest`, … — as the adblock `$script` / `$image` options name it.
`unknown` is the honest fallback when the proxy cannot tell. A type-restricted
**block** declines it — guessing `$script` wrong would refuse something nobody
asked to refuse — while a type-restricted **exception** still applies, because
declining one leaves the block it exists to override standing. Both directions
therefore under-block, which is the safe one.

The proxy derives it from `Sec-Fetch-Dest` first (the browser stating its own
intent), then `Accept`, then the path extension, and leaves it `unknown` rather
than guessing when none of them speak.

### Verdict

The Rule Engine's decision for one query or request: **Allow** (explicit
exception match), **Block** (block rule match), or **Pass** (no rule matched —
forward normally). Allow always wins over Block, **across both tiers**: a
domain-tier exception overrides a URL-tier block and vice versa.

### Blocklist / Allowlist

Informal names for rule lists whose rules predominantly block or allow.
Both are just rule lists; the distinction lives in the rules, not the list.

### Query

One DNS question received from a client (domain, record type, client source IP,
timestamp).

### Record Type

The DNS question's type, counted per hour under a fixed set of eleven labels:
`A`, `AAAA`, `HTTPS`, `MX`, `TXT`, `PTR`, `NS`, `SOA`, `SRV`, `CNAME` and
`OTHER`. The set is fixed at compile time because a per-query counter cannot
hold an unbounded set of type strings.

**`OTHER` is a record type, not a leftovers bin** — it counts the types outside
the named ten, and `/history/summary` reports it under that name.

### rest

The dashboard's fold: the query types outside the largest few, summed into one
slice so a donut stays readable. A presentation concern, computed in the
browser, and never a name the API sends.

Deliberately not spelled `other`: the API already sends `OTHER`, and a fold
spelled `other` put two different meanings one case-fold apart in a single
legend. The card names what its `rest` slice holds rather than making the
reader guess.

### Client

A device on the network, identified by the source IP of its queries. May carry
an optional user-assigned name. In Phase 1 clients were observed and reported
(per-client statistics) while filtering stayed global; since `p2-05` a client
is also what a Policy is assigned to, and what a `$client` rule names.

### Client Transport

The listener a DNS query arrived on: `udp`, `tcp`, `dot` (TLS on
`[dns.listen] dot_port`) or `doh` (`/dns-query` on the API listener). Carried
on every DNS query event as `transport`. Distinct from an Endpoint's protocol,
which is the transport FastAdHunter speaks **upstream**.

### Policy

A named bundle of rule lists and settings assignable to clients or schedules
(parental-control style). Modelled in `p2-05`; **enforced in both pipelines
since `p2-06`**, which is also when the API gained `/api/v1/policies`.

Every deployment has a **default policy** — every enabled list, no overrides —
and a client with no Assignment is judged under it. That is what filtering was
before Policies existed, so a configuration that defines none behaves exactly
as it did.

A policy is a *subset* of the configured rule lists, never a source of new
ones: which lists exist stays a property of `[[rules.lists]]`. All policies
share one compiled ruleset, each rule carrying the set of policies that can see
it — a second policy costs a mask, not a second copy of the corpus.

### Active Policies

The client → policy mapping in force at one instant: every Schedule already
evaluated, every Assignment naming a Client by name already resolved to
addresses. Rebuilt on a coarse tick by the binary and swapped atomically, so a
query resolves its policy by walking a short array — no clock, no timezone, no
name lookup on the hot path (`p2-06`).

### Schedule

A recurring weekly window — days of the week plus a start and end time — during
which an Assignment applies. Deliberately not a date range: "school nights
21:00–07:00" is the shape parental control needs.

Times are **local wall-clock**, read in the timezone from `[schedule]
timezone`. A window whose end is not after its start wraps midnight, and
belongs to the day it opened: `mon-fri 21:00–07:00` covers Saturday morning
because Friday opened it, and does not cover Monday morning.

### Assignment

Binds a Client to a Policy, optionally only while a Schedule is active. The
most specific Assignment matching a client wins — a name, then an address, then
the longest prefix. One whose schedule is not currently active does not apply,
and the client falls to the next Assignment that covers it, reaching the
default policy only when none does.

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
  name produce **one** refresh rather than one each. The **lease** is
  `max(5 s, 2 × worst-case walk)`, derived by the binary from `[dns.upstreams]`
  (ADR-0005): a **walk** is one ordered pass over the configured upstreams,
  an **attempt** is one server's share of it, and every attempt is wall-clock
  bounded at `ATTEMPT_LEGS (3) × timeout_ms`, so the worst-case walk is
  `3 × servers × timeout_ms` (`fah_dns::worst_case_walk`).
- **Cooldown** — the longer suppression a *failed* refresh leaves behind, so a
  dead upstream cannot turn every stale hit into a forward.

`swr_workers = 0` disables the pool, and a stale entry then answers only after a
forward has actually failed — the pre-ADR-0005 behaviour, and the one RFC 8767
§4 describes. With the pool on, the behaviour is closer to HTTP's
`stale-while-revalidate` (RFC 5861): do not call it "RFC 8767 serve-stale".

### Upstream

An external DNS resolver FastAdHunter forwards unblocked, uncached queries to.
Speaks plain DNS, DoT, or DoH.

### Endpoint

One `[[dns.upstreams.servers]]` entry — an Upstream address plus its protocol —
and the unit that health, penalties and probing are tracked per. Identified by
its index in configured order, the same index Answering Endpoint records. Two
entries pointing at the same resolver are two Endpoints: "Upstream" names the
resolver, "Endpoint" names the configured row FastAdHunter talks to.

### Endpoint Health

Which of three states an Endpoint is in under `[dns.upstreams] strategy =
"adaptive"`:

- **Healthy** — selected in configured order like any other.
- **Penalized** — skipped until its penalty deadline passes. It is still
  configured, so when every Endpoint is Penalized the query still goes out.
- **Probing** — claimed by exactly one query as the recovery attempt for the
  current deadline.

Under `strategy = "fallback"` no Endpoint has health; every one reads Healthy.

### Probe

The single on-path attempt that tests whether a Penalized Endpoint has
recovered. It is a real client query, never synthetic traffic: the first query
to reach a Penalized Endpoint past its deadline claims the Probe, moves it to
Probing and forwards to it. One claim per Endpoint per deadline, at most one
Probe per query, and never from an internal hostname lookup.

### Penalty

The interval a failing Endpoint is skipped for. Derived from `timeout_ms`,
never configured: base `10 x 3 x timeout_ms`, doubled per Penalty Round, capped
at 300 s. The cap bounds that *nominal* value; the deadline then carries ±25 %
jitter on top, so a capped round skips the Endpoint for 225–375 s and Endpoints
penalized together do not return in lockstep. `penalty_failures` decides how
many consecutive transport failures apply one; an unreachable path or a failed
handshake applies one immediately.

### Penalty Round

The doubling exponent of the last Penalty applied to an Endpoint. It increments
on each Penalty and saturates at 15. Recovery does not clear it — a Healthy
Endpoint still carries the round it reached. What the 300 s decides is the round
the *next* Penalty starts at: 1 only when the Endpoint has been Healthy
continuously for that long, so a flapping Endpoint resumes its escalated backoff
instead of restarting cheap.

### Penalty Policy

The constants the Penalty arithmetic reads — `penalty_failures`, the derived
base and the 300 s cap — built once from config and shared immutably by every
Endpoint. Distinct from Policy, the parental-control bundle: a Penalty Policy
never reaches a client or a rule list. It is also not the state machine, which
is Endpoint Health.

### Answer Outcome

What the client actually received: an answer, a **synthesized** SERVFAIL
FastAdHunter minted because every Upstream failed and no stale entry could
cover it, or a SERVFAIL/REFUSED **relayed** from an Upstream that answered.
Distinct from Verdict, which is what the Rule Engine decided before anything
went to the network — a Pass query can still end in a failure outcome.

A stale serve that masked an Upstream failure is an *answer*, not a failure:
the client got records. That case is counted by Stale-while-refresh's own
figures, so counting it here too would double-count one query as both served
and failed.

### Answering Endpoint

Which Upstream produced the answer, as its index in configured order. Recorded
per query only when an Upstream actually answered — a block, a Cache hit, a
stale serve and a synthesized failure all name none. An index rather than an
address: the address is an allocation the query path does not make, and
`/api/v1/telemetry`'s `upstreams` array is published in the same configured
order for the join.

### Upstream RTT

Time-to-answer of an Upstream's **answered** attempts, measured per Endpoint
around the whole attempt: UDP retransmit legs and cold TCP/TLS connection
setup are inside it. A Probe is a real round trip and is included; a timed-out
or otherwise failed attempt is never observed — so a dead Endpoint cannot pin
the percentiles at `timeout_ms`. This is network time, the part of the
`forward` stage that is not FastAdHunter; served as `upstreams[].rtt` on
`/api/v1/telemetry` (lifetime percentiles) and `/api/v1/history/perf`
(per-interval percentiles).

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

Since p3-04 the word also names the per-client **TLS termination** on the
HTTPS port (SECURITY.md §Later phases): a client listed in
`[https.interception] clients` has its TLS terminated with a minted leaf and
its requests judged by the URL Tier. Which meaning is intended is clear from
the section: dst-nat is how traffic *arrives*, termination is what happens to
a listed client's traffic once it has.

### Splice Leg / Terminate Leg

The two outcomes of an HTTPS connection after the SNI verdict is not a Block.
The **splice leg** (p3-03) relays the bytes untouched in both directions — no
key, no decryption, the client validates the origin's own certificate. The
**terminate leg** (p3-04) verifies the origin first, then terminates TLS with
a minted leaf, runs the HTTP pipeline over the decrypted requests and
re-encrypts to the verified origin. Every connection takes the splice leg
unless the client is listed **and** the SNI is not an Exclusion.

### Exclusion

An SNI hostname that always takes the splice leg, even for a listed client:
the compiled-in baseline of certificate-pinned families plus
`[https.interception] exclude_domains`. Matched by exact host or parent suffix
(`api.bank.example` is excluded by `bank.example`) on the ClientHello, before
any decryption. An Exclusion is not a verdict: the connection is still judged
at the SNI and still egress-guarded; it only decides which leg carries it.

### Destination Claim

Where a client *says* it was going. After Interception there is no
`SO_ORIGINAL_DST` on RouterOS, so the claim is all we have: the `Host` header
for HTTP, and SNI for HTTPS in Phase 3. It is written by the client, so it is
**never** trusted — it is parsed, then judged by the Egress Guard. Parsing is
protocol-specific and lives in the engine; judging is not.

### Egress Guard

The default-deny policy deciding where a proxy may connect. It judges the
**resolved address**, never the Destination Claim's name — checking the name
would let a public hostname whose record points at `192.168.10.1` walk straight
through (a DNS rebind). Refuses loopback, link-local, unique-local, RFC 1918,
CGNAT, multicast and broadcast, plus any port other than the intercepted one;
`[egress] allow_destinations` opts specific ranges back in.

Shared by HTTP and HTTPS because both face the same problem, so it lives at L1
(`fah-common`) rather than in either engine. What stops the proxy being an
**open relay** into the LAN.

### Port

A trait a lower layer declares to describe what it needs from a higher one, so
the binary can supply the implementation without the dependency arrow pointing
upward. `HostResolver` (at L1, implemented over the Upstreams; used by the Rule
Engine for list downloads and by the HTTP Engine for proxy upstreams) and
`StatsSource`/`TelemetrySource`/`CacheSource` (declared by the API) are the
existing ones. See ARCHITECTURE.md §Dependency Layering.

### Request Event

The completed-HTTP-request record the proxy publishes: the **HTTP Request** as
served, plus verdict, duration, response status and relayed bytes. The
counterpart of a Query Event, and it travels the same channel — see Event.

### Event

What the one bounded observability channel carries: a Query Event or a Request
Event, tagged `dns` / `http`. **One channel, not one per pipeline**, so
"we shed N events" stays a single number; two independent drop counters would
answer different questions about different queues and could not be added.

### Query Log

The bounded, persisted record of individual queries **and requests** (timestamp,
client, name, verdict, duration, plus each kind's own fields). Product data,
distinct from Statistics. A DNS question's domain and an HTTP request's host are
both "the name it was about", and the log's `domain` filter searches either.

### Statistics

Aggregated counters derived from queries: totals, blocked percentage,
top domains, top clients, rolling time buckets. Never per-query rows.

### Metrics

Operational telemetry (counters and histograms: QPS, latencies, cache hit
ratio, memory), served as JSON on `/api/v1/telemetry`. For operators; distinct
from Statistics (for users).

### Accounted / Residual

The two-way split of resident memory, reported by `/api/v1/telemetry`,
`/api/v1/debug/memory` and — as a series — `/api/v1/history/perf` (p2-07,
`crates/fastadhunter/src/allocator.rs`). Use these words for these things and no
others.

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
`docs/code-review/phase2/0.2.7-router-memory-and-throughput.md` §5.2, §6.1.

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

### Allocation Domain

One `current_thread` Tokio runtime on its own OS thread that serves whole HTTP
connections end to end — accept hand-off, request, upstream fetch, response,
close — so that everything a connection allocates is freed by the thread that
allocated it (ADR-0006). `[runtime] http_runtimes` sets how many the HTTP
Engine runs behind its one acceptor; `0` serves on the shared runtime instead.

Not a domain *name*. In `fah-http` the word with an index (`fah-http-0`,
`http_domain = 0`) always means this; the DNS sense is never shortened to it.
