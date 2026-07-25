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

It filters DNS today, HTTP next, HTTPS after that.

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
| 2 | HTTP engine + Policies | 🚧 in progress | — |
| 3 | HTTPS interception | ⬜ not started | — |
| 4 | HTML filtering | ⬜ not started | — |

Running in production on a MikroTik RB5009 as the household's only resolver.

### Measured, on the RB5009

Quad-core ARMv8 @ 1.4 GHz, 1 GB RAM shared with RouterOS.

| | Measured | Budget |
| --- | ---: | ---: |
| Resident memory, 684 k rules loaded | **42.1 MiB** | ≤ 128 MB |
| Compiled ruleset | **21.9 MiB** | ≤ 40 MB |
| Startup to serving, 1.21 M rules | **2 440 ms** | 1–3 s |
| Blocked verdict, in-engine | **0.013–0.148 ms** | < 1 ms p99 |
| Sustained throughput, deployed path | **~15–16 k QPS** | ≥ 10 k QPS |
| Container image | **~12 MB** | ≤ 30 MB |
| Dropped events under real load | **0** | 0 |

Rule lists compile **1 023 132 parsed rules into 684 087** after
deduplication — a third of the input across 15 public lists is redundant, and
paying for it once at compile time keeps the matcher smaller for the life of
the process.

Memory is **bounded, not merely small**: a 91-hour soak plateaued and held.
`RSS − Σ(components) = residual` is exported continuously, so a leak shows as
the residual growing while the named components stay flat — the growth you
legitimately expect has already been subtracted out.

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

The Rule Engine runs **before** the cache, so rule changes take effect
instantly and the cache never stores verdicts
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

Dependencies point **downward only**; **siblings never import each other** —
the binary wires them via channels and ports. `fah-model` stays pure data.

This is enforced, not just documented: `crates/fastadhunter/tests/layering.rs`
parses every manifest and fails the build on any edge that points sideways or
up.

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
socket, a datagram, TCP or TLS — only a DNS question or an HTTP request. That
is what lets one index serve both:

- **A DNS question** is `(domain, qtype)`. It cannot express a path, so a rule
  like `||example.com^*/ads/banner.gif` is stored and classified but stays
  inactive on this path — blocking the whole domain to kill one tracker would
  be wrong.
- **An HTTP request** carries host **plus** path, method, resource type and
  third-party flag, so the same rule matches exactly what it was written for.
- **HTTPS** reuses the HTTP request model after TLS termination. No third
  matcher, ever — a `lookup_https()` would mean transport had leaked into the
  engine.

Two typed entry points, not a trait object: virtual dispatch on the hot path
is forbidden by [PERFORMANCE.md](PERFORMANCE.md). The rules themselves stay
model-specific — `$dnstype` is meaningless for HTTP, path anchoring is
meaningless for DNS — while the arena, the deduplication and the atomic swap
are paid for once.

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
The cache stores upstream answers only — bounded by both entry count and bytes,
TTL-respecting with clamps, RFC 2308 negative caching and RFC 8767 serve-stale
when upstreams are unreachable.

### HTTP pipeline *(Phase 2)*

```text
Accept (TCP, router dst-nats :80 here)
      │
Read request line + headers   ── timeout bounds a slowloris
      │
Rule Engine ── URL verdict (host + path + method + resource type)
      │
      ├─ Block → synthesized response, origin never contacted ──► Client
      │
Egress guard ── default-deny, post-resolution (SSRF / DNS rebinding)
      │
Pass-through: stream origin ⇄ client, byte for byte
```

**The body is never parsed and never buffered.** Images, archives, PDFs and
video stream through untouched — buffering a response to inspect it would make
memory grow with traffic, which the bounded-everything rule forbids outright.
HTML rewriting arrives in Phase 4 and is opt-in, for that content type alone.

---

## Operating modes

Fixed at container start via `engine.mode`:

| Mode | Filters |
| ---- | ------- |
| `dns` | Network-wide DNS filtering |
| `dns+http` | …plus URL-level filtering of unencrypted HTTP |
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
default, bearer-key auth.

```text
GET   /health
GET   /metrics                      Prometheus text exposition
GET   /api/v1/stats                 aggregates, top domains/clients
GET   /api/v1/queries               query log, filtered + paginated
GET   /api/v1/clients               per-client view; PUT to name one
GET   /api/v1/lists                 rule lists; POST /lists/refresh
POST  /api/v1/rules/test            verdict for a domain, with the rule
GET   /api/v1/cache                 cache stats; POST /cache/clean
GET   /api/v1/history/{summary,perf,top}
GET   /api/v1/config                POST to patch, validated + written back
GET   /api/v1/debug/memory          per-component heap + residual
WS    /api/v1/events                live query stream
```

Full request/response shapes: [API.md](API.md).

---

## Deployment

One container. Configuration, blocklists, certificates and history live on
mounted volumes, outside the image.

```text
/config    small, back this up      TOML, API key, TLS certificates
/data      bulky, regenerable      cached lists, query log, history
```

The image is distroless/static with a statically linked musl binary — no shell,
no package manager, non-root after binding.

Step-by-step for the reference deployment, including the RouterOS container
setup, port redirects and the soak procedure:
**[docs/deploy-rb5009.md](docs/deploy-rb5009.md)**.

---

## ARM64 first

The primary target is ARM64 hardware in routers, home labs and small servers.
The reference deployment is a **MikroTik RB5009UG+S+IN** running the container
natively under RouterOS.

![MikroTik RB5009](docs/images/RB5009UGS.png)

> Compact, powerful, with multiple powering options and efficient cooling. Nine
> wired ports and a full-sized USB 3.0 port — seven Gigabit Ethernet, one
> 2.5 Gigabit Ethernet, and a 10G SFP+ slot. All connected to a Marvell
> Amethyst switch chip with a 10 Gbps full-duplex line to the Marvell Armada
> quad-core ARMv8 1.4 GHz CPU. Both the CPU and the switch chip sit on the
> bottom of the board, so the case acts as a massive heatsink. Hardware
> offload supported.

Budgets assume this box: four cores at 1.4 GHz and **1 GB of RAM shared with
RouterOS itself**. That constraint is why the memory numbers matter.

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
│   ├── fah-metrics/      # ops telemetry: Prometheus
│   ├── fah-stats/        # product data: query log, aggregates, history
│   └── fastadhunter/     # thin binary — wires everything
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
| **2** 🚧 | HTTP proxy, URL-path rules, **Policies** — named rule bundles assignable to clients and schedules |
| **3** | HTTPS interception, certificate management, DoT/DoH listeners |
| **4** | HTML filtering with `lol_html`, cosmetic rules |

Detail and per-phase task status: [ROADMAP.md](ROADMAP.md) and `plan/`.

---

## Documentation

Design documents are written and approved **before** the code they describe.
A change that contradicts a doc updates the doc in the same commit — or adds an
ADR if the decision is being reversed.

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
    ├── decisions/        ADRs 0001–0004
    ├── diagrams/         architecture SVG + HTML
    ├── code-review/      per-task review notes with measured results
    └── deploy-rb5009.md  end-to-end deployment + soak procedure
```

### Quality gates

No CI service — deliberately. Gates run locally before every commit:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench            # when a hot path is touched
```

A >10 % regression on a hot-path bench needs an explicit justification.
See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## Repository

- **Visibility:** private
- **License:** none — proprietary until a public release, if ever
