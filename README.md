# FastAdHunter

> Network-wide ad blocking with predictable latency and the smallest memory
> footprint we can defend with a measurement.

**Rust** · **Tokio** · **API-first** · **ARM64-first** · **Docker-native** ·
**Multi-core**

---

## What it is

A network filtering engine that sits between every device in a house and the
internet. One container on the router, no client configuration, no browser
extension, no per-device agent.

It filters **DNS and HTTP today**, HTTPS after that.

**Performance is the primary feature.** Every architectural decision is
evaluated by its effect on throughput, latency and allocations — and the
numbers below are measured on the target hardware, not estimated.

---

## Status

| Phase | Scope | Status | Tag |
| ----- | ----- | ------ | --- |
| 0 | Foundations, workspace, layering | ✅ done | `v0.1.0-phase0` |
| 1 | DNS + REST API + Docker | ✅ done | `v0.2.0-phase1` |
| 1.5 | Observability persistence | ✅ done | `v0.3.0-phase1.5` |
| 2 | HTTP engine + Policies | 🚧 shipped, one task open | — |
| 3 | HTTPS interception | ⬜ not started | — |
| 4 | HTML filtering | ⬜ not started | — |

Running in production on a MikroTik RB5009 as the household's only resolver, in
`dns+http` mode. Phase 2's engine work is deployed — the transparent HTTP proxy,
URL-path rules, per-client Policies and the single JSON telemetry surface. What
remains open is a memory-transient investigation, not a feature.

### Measured, on the RB5009

Quad-core ARMv8 @ 1.4 GHz, 1 GB RAM shared with RouterOS. Every figure comes off
the deployed container, not a dev box. **Measurements are binary (MiB);**
PERFORMANCE.md writes its budgets in decimal MB, which runs ~4.9 % higher for the
same reading.

| | Measured | Budget |
| --- | ---: | ---: |
| Resident memory, 799 k rules + 50 k-entry cache | **53.6 MiB** | ≤ 128 MB |
| Peak RSS at boot | **117.8 MiB** | ≤ 128 MB |
| Peak RSS during a list refresh | **171.7 MiB** | see below |
| Ruleset compile, 1.15 M parsed rules | **2.85 s** | < 3 s |
| Ruleset heap, resident | **25.8 MiB** | ≤ 40 MB |
| Blocked verdict, in-engine | **0.045 ms** mean | < 1 ms p99 |
| Cache hit, fresh + stale-while-refresh | **0.227 ms** mean | < 1 ms p99 |
| HTTP proxy, added latency | **+161 µs** min · **+344 µs** p50 | < 1 ms |
| HTTP proxy, opaque throughput | **271 / 208 MiB/s** | ≥ 100 MiB/s |
| URL verdict, 8 KiB URL, full EasyList+EasyPrivacy | **554 µs** | < 1 ms |
| Sustained DNS throughput, deployed path | **20 k+ QPS** | ≥ 10 k QPS |
| Container image | **13.0 MiB** | ≤ 30 MB |
| Dropped events under real load | **0** | 0 |

Rule lists compile **1 148 024 parsed rules into 798 760** after deduplication —
349 264 duplicates, 30 % of the input across 16 public lists. Deduplication is
paid once at compile time and keeps the matcher smaller for the life of the
process. It also shortens probe chains: a domain carried by two lists occupies
one slot instead of two that hash to the same place, worth **46 % on lookups for
shared domains**.

The 20 k+ QPS figure is `/tool profile` on the live box under a synthetic hammer,
with all four cores sharing evenly — reception is not the bottleneck, so
`SO_REUSEPORT` stays a documented recipe rather than shipped code.

**The refresh transient is the one figure above its budget.** Recompiling the
ruleset briefly holds the outgoing matcher, the freshly fetched list bodies and
the new arena at the same time, peaking around 172 MiB before falling back to
~52 MiB. It is bounded and it does not ratchet — steady RSS returning after every
refresh is the evidence — but it is real, and reducing it is the one Phase 2 task
still open. Structural accounting:
[`p2-12`](plan/wip/phase2/p2-12-compile-transient-structural.md).

Memory is otherwise **bounded, not merely small**, and the instrument that proves
it runs continuously: `RSS − Σ(components) = residual` is exported on every
sample, so a leak shows as the residual growing while the named components stay
flat — the growth you legitimately expect has already been subtracted out. Across
two independent on-device windows of 61.3 h and 36.2 h (4 657 samples), the
residual slopes are **+0.082 and +0.027 MiB/h**, with the sign disagreeing between
the windows' final thirds: drift indistinguishable from noise.

> RouterOS reports the container larger than the process is. `memory-current` sits
> around 78 MiB against a ~54 MiB resident set; the difference is cgroup **page
> cache** — reclaimable file-backed pages from the image layers and the cached
> rule lists — not consumption. The figure that matters is the process's own
> resident set.

---

## Why it exists

Existing solutions each cover one layer:

- **DNS filters** — blind to paths; blocking `example.com` to kill one tracker
  takes the whole domain with it.
- **Browser extensions** — excellent, but per-browser and per-device. Nothing
  protects the TV, the thermostat or a guest's phone.
- **Desktop applications** — per-machine, per-OS.

FastAdHunter filters at the network layer and is designed to grow from DNS to
HTTP to HTTPS **without changing its architecture**.

It does not try to replace uBlock Origin. A router cannot see a page's DOM;
cosmetic filtering belongs where the DOM is. What a router can do is protect
every device in the house at once, including the ones that will never run an
extension.

---

## Philosophy

Deterministic behaviour over configurability. Allocations minimised. Streaming
preferred over buffering. Expensive operations avoided rather than optimised.

**Every new capability must justify its runtime cost.** Feature count is always
secondary.

### Design rules

- Performance before features
- API-first — the engine never depends on a UI
- No locks, no allocations, no regex on the hot path
- Ruleset and config changes via atomic swap
- **Bounded everything** — memory must not grow with traffic or uptime
- Streaming before buffering; zero-copy where practical
- ARM64 first-class
- No hand-rolled cryptography

The Rule Engine runs **before** the cache, so rule changes take effect instantly
and the cache never stores verdicts
([ADR-0001](docs/decisions/0001-rules-before-cache.md)).

---

## Architecture

![FastAdHunter architecture](docs/diagrams/architecture.svg)

```text
             Dashboard (optional, later phase)
                     │
          REST / WebSocket API  (fah-api)
                     │
             FastAdHunter Core
                     │
 ┌─────────────────────────────────────┐
 │ DNS Engine        (fah-dns)         │
 │ HTTP Engine       (fah-http)        │
 │ HTTPS Engine      (Phase 3)         │
 │ Rule Engine       (fah-rules)       │
 │ Statistics        (fah-stats)       │
 │ Metrics           (fah-metrics)     │
 └─────────────────────────────────────┘
                     │
                 Internet
```

The dashboard is a separate deliverable and talks only to the API. The core
never depends on any UI.

Full diagram: [SVG](docs/diagrams/architecture.svg) ·
[HTML](docs/diagrams/architecture.html) ·
[detailed](docs/diagrams/architecture-full.svg)

### Dependency layering

```text
L4:  fastadhunter (binary — wires everything)
L3:  fah-dns   fah-http   fah-api   fah-stats   fah-metrics
L2:  fah-rules
L1:  fah-model   fah-config   fah-common   fah-logging
```

Dependencies point **downward only**; **siblings never import each other** — the
binary wires them via channels and ports. `fah-model` stays pure data.

This is enforced, not just documented: `crates/fastadhunter/tests/layering.rs`
parses every manifest and fails the build on any edge that points sideways or up.

---

## One Rule Engine, two pipelines

This is the central idea of the project.

```text
                        ┌─────────────────────────┐
                        │      Rule Engine        │
                        │       (fah-rules)       │
                        │                         │
                        │  ONE compiled index     │
                        │  ONE atomic swap        │
                        │  contiguous arena       │
                        └────────────┬────────────┘
                                     │
                   one typed entry point per request model
                                     │
                 ┌───────────────────┴───────────────────┐
                 │                                       │
         lookup_dns(domain, qtype)          lookup_http(host, path,
                 │                            method, type, client)
                 │                                       │
                 ▼                                       ▼
        ┌─────────────────┐                     ┌─────────────────┐
        │  DNS pipeline   │                     │  HTTP pipeline  │
        │    (fah-dns)    │                     │   (fah-http)    │
        └─────────────────┘                     └────────▲────────┘
                                                         │
                                          HTTPS, after TLS termination
                                          reuses the SAME request model
                                                    (Phase 3)
```

**One list of rules protects every protocol.** Add a blocklist once and DNS,
HTTP and later HTTPS all start enforcing it — there is no second ruleset to
configure, no second copy in memory, and no way for the two to disagree about
whether a domain is blocked.

The matcher is **protocol-model aware, not transport aware**. It never sees a
socket, a datagram, TCP or TLS — only a DNS question or an HTTP request. That is
what lets one index serve both:

- **A DNS question** is `(domain, qtype)`. It cannot express a path, so a rule
  like `||example.com^*/ads/banner.gif` is stored and classified but stays
  inactive on this path — blocking the whole domain to kill one tracker would be
  wrong.
- **An HTTP request** carries host **plus** path, method, resource type and
  third-party flag, so the same rule matches exactly what it was written for.
- **HTTPS** reuses the HTTP request model after TLS termination. No third
  matcher, ever — a `lookup_https()` would mean transport had leaked into the
  engine.

Two typed entry points, not a trait object: virtual dispatch on the hot path is
forbidden by [PERFORMANCE.md](PERFORMANCE.md). The rules themselves stay
model-specific — `$dnstype` is meaningless for HTTP, path anchoring is
meaningless for DNS — while the arena, the deduplication and the atomic swap are
paid for once.

### Policies — the same ruleset, seen differently per client

A **Policy** is a named bundle of lists and settings, assignable per client and
per schedule.

Every policy shares **one** compiled ruleset and carries a 16-bit per-rule
visibility mask, so N policies cost **+2.03 MiB flat** at deployed scale — 6.839
MiB shared, against 12.099 MiB if each policy compiled its own, measured over four
lists. A deployment with no policies allocates nothing extra, and resolving a
client's policy costs **+8.5 ns** per query, documented rather than claimed as
free.

Schedules are POSIX TZ strings, DST-correct, with no tzdb dependency.

---

## Request processing

### DNS pipeline

```text
Receive query (UDP/53, TCP/53)
      │
Rule Engine ── verdict
      │
      ├─ Block  → synthesize 0.0.0.0 / ::  (TTL 10s) ──► Reply
      │           never touches cache or network
Cache lookup ── hit ────────────────────────────────► Reply
      │ miss
Upstream resolver (UDP/TCP → DoT / DoH per config)
      │
Cache store
      │
Reply
```

Every query gets a fresh verdict, so unblocking a domain needs no cache flush.
The cache stores upstream answers only — bounded by both entry count **and**
bytes, TTL-respecting with clamps, and RFC 2308 negative caching.

#### An expiring entry never costs a client a round trip

A cache entry passes through three states: **fresh** → **stale** → **expired**.
The interesting one is stale.

```text
past TTL, still within the stale window
      │
      ├─► answer the client NOW from cache        (microseconds, not 20–50 ms)
      │   with a short retry-soon TTL
      │
      └─► hand the refresh to a detached worker pool
          the query path never awaits it
```

Naively, every client asking during the gap between expiry and the next refresh
pays a full upstream round trip — **and they all pay it in parallel**.
FastAdHunter answers from cache immediately and refreshes in the background on a
fixed pool of `swr_workers` (default 3), so the tail disappears without the
client ever knowing there was one.

Many simultaneous requests for the same expiring name produce **exactly one**
refresh. The deduplication claim lives inside the cache entry and is taken under
the shard lock the lookup already holds — no global lock, no second data
structure ([ADR-0005](docs/decisions/0005-serve-stale-while-refresh.md)).

The pool **never back-pressures**: enqueue is a `try_send`, and a full queue
drops the refresh rather than delaying anybody. Over a 15 h window on the
reference deployment: 5 579 refreshes queued, 5 579 completed, 0 failed,
0 dropped.

A background sweep removes entries past the stale window every
`cleanup_interval_seconds` (default 360), on the blocking pool and one shard at a
time. It is **not** a bound — `max_entries`/`max_bytes` are, and they hold with
the sweep disabled — it returns memory a cache stops needing while idling *below*
both caps, which nothing else reclaims.

### HTTP pipeline

```text
Accept (TCP :8080, router dst-nats :80 here)
      │
Read request line + headers   ── timeout bounds a slowloris
      │
Rule Engine ── URL verdict (host + path + method + resource type + client)
      │
      ├─ Block → synthesized response, origin never contacted ──► Client
      │
Egress guard ── default-deny, post-resolution (SSRF / DNS rebinding)
      │
Pass-through: stream origin ⇄ client, byte for byte
```

**The body is never parsed and never buffered.** Images, archives, PDFs and video
stream through untouched — buffering a response to inspect it would make memory
grow with traffic, which the bounded-everything rule forbids outright. HTML
rewriting arrives in Phase 4 and is opt-in, for that content type alone.

The verdict is taken on the **head**, before the origin is resolved, so a blocked
request costs no DNS lookup and no upstream connection — measured **48–55 %
cheaper than a forwarded one** on the device.

Every rule is indexed — the unindexed count is **zero**, because a literal-run
n-gram tier covers the patterns a token index cannot. That holds a worst-case
8 KiB URL against the full EasyList + EasyPrivacy corpus to **554 µs** on-device.
The unanchored scan uses SIMD `memchr` — a single-byte search, which is the one
primitive the "no regex on the hot path" rule leaves open.

---

## Operating modes

Fixed at container start via `engine.mode`:

| Mode | Filters |
| ---- | ------- |
| `dns` | Network-wide DNS filtering |
| `dns+http` | …plus URL-level filtering of unencrypted HTTP — **deployed today** |
| `dns+http+https` | …plus HTTPS interception, for managed environments |

A mode that does not name an engine means that engine's listener is **never
bound** — not bound and idle.

---

## Runtime model

```text
               Tokio Runtime
                     │
      ┌──────────────┼──────────────┐
      │              │              │
 Worker 1       Worker 2       Worker N
      │              │              │
      ├── DNS        ├── DNS        ├── DNS
      ├── HTTP       ├── HTTP       ├── HTTP
      └── Rules      └── Rules      └── Rules
```

Work is distributed across Tokio workers; the architecture avoids centralised
processing. One task per datagram, one shared compiled ruleset behind an atomic
swap, and no lock on the path that answers a query.

---

## API

Everything the engine can do is reachable over REST + WebSocket. HTTPS by
default, bearer-key auth. `/health` is the only unauthenticated route.

```text
GET   /health
GET   /api/v1/telemetry             whole engine state as JSON: counters,
                                    latency stages, upstreams, cache, memory
GET   /api/v1/stats                 aggregates, top domains/clients
GET   /api/v1/clients               per-client view; PUT to name one
PUT   /api/v1/clients/{ip}/policy   assign a policy to a client
GET   /api/v1/policies              policy CRUD, schedules, assignments
GET   /api/v1/lists                 rule lists; POST /lists/refresh
GET   /api/v1/rules/user            user rules; PUT to replace
POST  /api/v1/rules/test            verdict for a domain, with the rule
GET   /api/v1/cache                 cache stats; POST /cache/clean
GET   /api/v1/history/{summary,perf,top}
GET   /api/v1/config                POST to patch, validated + written back
POST  /api/v1/config/apikey/rotate  rotate the bearer key
GET   /api/v1/debug/memory          per-component heap + residual
WS    /api/v1/events                live query stream
```

**The read surface is JSON only — there is no Prometheus endpoint.**
`/api/v1/telemetry` is the single snapshot: one document carrying counters,
latency stages, upstreams, cache and memory, so a dashboard reads the rule count
without parsing a text exposition format. Percentiles come from `/history/perf`,
windowed; `/telemetry` serves `count` + `sum_seconds`, which are means.

Metrics are placed by **producer**: application and kernel figures live on
`/telemetry`, a stable contract; allocator internals live on `/debug/*`, which is
free to change.

Full request/response shapes: [API.md](API.md).

---

## Terminal monitor

`fah-tui-monitor` is a live console dashboard for a running instance. It ships in
this workspace and is the proof that the API-first rule holds: it reads
**nothing** but REST and WebSocket, has no privileged access to the engine, and
would work unchanged against a remote appliance.

![FastAdHunter TUI monitor](docs/images/tui-monitor.png)

It polls `/api/v1/telemetry` on an interval and streams `WS /api/v1/events` for
the live query feed, so the expensive figures refresh slowly and the feed stays
instant.

- **Header** — RSS, peak, ruleset and residual; cache hit ratio over *lookups*;
  cache load; and a persisted RSS history graph coloured by threshold.
- **Live feed** — every query as it is decided: client, domain, type, verdict,
  cache outcome and latency.
- **Windows** — 24 h and 7 d rollups with hourly and daily sparklines, top
  blocked domains and top clients.
- **Right column** — upstream health, the memory breakdown, cache state, and the
  engine's three latency stages (`block`, `cache hit`, `forward`), which together
  partition every resolved query.

Two figures on that screen are easy to misread, so it names them precisely:
`Stale ent` is the count of entries *currently* in the stale window, not the
number of stale serves; and every byte figure is binary, labelled `MiB`, because
the API serves raw bytes and a decimal-MB reading of the same number runs 4.9 %
higher.

A frame the build cannot decode is counted and shown rather than silently
dropped — otherwise a renamed server field would empty the feed in silence.

```sh
cargo run --release -p fah-tui-monitor
```

Host, port and bearer token come from `tui-monitor/config.toml`.

---

## Deployment

One container. Configuration, blocklists, certificates and history live on
mounted volumes, outside the image.

```text
/config    small, back this up      TOML, API key, TLS certificates
/data      bulky, regenerable      cached lists, snapshots, history
```

The image is distroless/static with a statically linked musl binary — no shell,
no package manager, non-root after binding.

Step-by-step for the reference deployment, including the RouterOS container
setup, port redirects and the soak procedure:
**[docs/deploy-rb5009.md](docs/deploy-rb5009.md)**.

---

## ARM64 first

The primary target is ARM64 hardware in routers, home labs and small servers. The
reference deployment is a **MikroTik RB5009UG+S+IN** running the container
natively under RouterOS.

![MikroTik RB5009](docs/images/RB5009UGS.png)

> Compact, powerful, with multiple powering options and efficient cooling. Nine
> wired ports and a full-sized USB 3.0 port — seven Gigabit Ethernet, one
> 2.5 Gigabit Ethernet, and a 10G SFP+ slot. All connected to a Marvell Amethyst
> switch chip with a 10 Gbps full-duplex line to the Marvell Armada quad-core
> ARMv8 1.4 GHz CPU. Both the CPU and the switch chip sit on the bottom of the
> board, so the case acts as a massive heatsink. Hardware offload supported.

Budgets assume this box: four ARMv8 cores and **1 GB of RAM shared with RouterOS
itself**. That constraint is why the memory numbers matter.

Dynamic frequency scaling is enabled: during CPU-bound benchmarks the governor
boosts between idle (350 MHz) and 1400 MHz, and control measurements show
throughput unchanged between runs reporting those frequencies. Clock readings
therefore cannot calibrate performance here — the reference is the measured **~9×
x86 → RB5009 factor**, which **does not convert HTTP work**, where the observed
spread is 4.55–10.09×.

---

## Repository layout

```text
FastAdHunter/
├── Cargo.toml            # workspace root
├── crates/
│   ├── fah-common/       # shared errors, listener binding, small utils
│   ├── fah-logging/      # tracing init, formats, levels
│   ├── fah-config/       # TOML, precedence, validation
│   ├── fah-model/        # domain model + shared DTOs (pure data types)
│   ├── fah-rules/        # Rule Engine: parsers + compiled matchers
│   ├── fah-dns/          # listeners, pipeline, cache, upstreams
│   ├── fah-http/         # HTTP engine: proxy, pass-through, URL filtering
│   ├── fah-api/          # Axum REST + WebSocket
│   ├── fah-metrics/      # ops telemetry: counters + stage histograms
│   ├── fah-stats/        # product data: aggregates, clients, history
│   └── fastadhunter/     # thin binary — wires everything
├── tui-monitor/          # fah-tui-monitor — live console dashboard
├── tests/                # workspace integration tests
├── benches/              # criterion benches vs PERFORMANCE.md budgets
├── plan/                 # task orchestration: open / wip / closed phases
├── docs/                 # images/, diagrams/, decisions/, code-review/
└── dashboard/            # empty until the dashboard phase
```

---

## Technology

| Area | Choice | Why |
| ---- | ------ | --- |
| Language | Rust | no GC pauses, no runtime, predictable memory |
| Runtime | Tokio | multi-threaded work stealing |
| HTTP | Hyper / Axum | streaming-first |
| DNS | Hickory | pure-Rust wire format and upstream clients |
| TLS | rustls | no OpenSSL, no C dependency |
| Certificates | rcgen · x509-parser | generation and parsing only |
| Allocator | mimalloc | +27 % throughput, −17 % CPU/query vs mallocng |
| Scanning | memchr | SIMD single-byte search — not regex |
| Terminal UI | ratatui | the monitor only; the engine has no UI dependency |
| HTML | lol_html *(Phase 4)* | streaming rewriter, never buffers a document |

> **Rule:** do not reinvent cryptography. rustls, rcgen and x509-parser are the
> complete crypto surface.

---

## Scope

**FastAdHunter is:** lightweight · modular · predictable · API-first ·
network-wide.

**FastAdHunter is not:** a browser · an IDS · an antivirus · a general-purpose
firewall · a replacement for a good browser extension.

---

## Roadmap

| Phase | Delivers |
| ----- | -------- |
| **1** ✅ | DNS filtering, REST API, Docker image, on-device soak |
| **1.5** ✅ | Persisted history, perf series, byte-bounded cache |
| **2** 🚧 | HTTP proxy ✅, URL-path rules ✅, Policies ✅, telemetry consolidation ✅ — refresh-transient memory open |
| **3** | HTTPS interception, certificate management, DoT/DoH listeners |
| **4** | HTML filtering with `lol_html`, cosmetic rules |

Detail and per-phase task status: [ROADMAP.md](ROADMAP.md) and `plan/`.

---

## Documentation

Design documents are written and approved **before** the code they describe. A
change that contradicts a doc updates the doc in the same commit — or adds an ADR
if the decision is being reversed.

```text
├── CONTEXT.md            glossary — the project's ubiquitous language
├── ARCHITECTURE.md       components, crates, layering, runtime model
├── ROADMAP.md            phases and their tasks
├── API.md                every endpoint with request/response
├── CONFIGURATION.md      every option, precedence, boot vs runtime
├── RULE_ENGINE.md        formats, verdicts, matcher, list lifecycle
├── PERFORMANCE.md        golden rules + numeric budgets
├── SECURITY.md           API key, TLS, certificates, container hardening
├── CONTRIBUTING.md       conventions and local quality gates
│
└── docs/
    ├── decisions/            ADRs 0001–0005
    ├── diagrams/             architecture SVG + HTML
    ├── code-review/          per-task review notes with measured results
    ├── deploy-rb5009.md      end-to-end deployment + soak procedure
    ├── routeros-traps.md     what bites you on RouterOS, and why
    ├── measurement-traps.md  how to read a bench, soak or memory figure
    └── project-state.md      where the work is right now
```

### Quality gates

No CI service — deliberately. Gates run locally before every commit:

```sh
sh scripts/gates.sh   # fmt + clippy -D warnings + test --workspace
cargo bench           # when a hot path is touched
```

One line per gate instead of the ~1 000 the raw commands print; full output
always lands in `target/gates.log`. A >10 % regression on a hot-path bench needs
an explicit justification. See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## Repository

- **Visibility:** private
- **License:** none — proprietary until a public release, if ever
