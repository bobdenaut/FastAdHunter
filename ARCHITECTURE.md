# ARCHITECTURE

How FastAdHunter is put together. Terms are defined in [CONTEXT.md](CONTEXT.md);
decisions with real trade-offs are recorded in [docs/decisions/](docs/decisions/).

---

## System Overview

![FastAdHunter architecture](docs/diagrams/architecture.svg)

Full-page version: [docs/diagrams/architecture.html](docs/diagrams/architecture.html)

```text
             Dashboard (optional, later phase)
                     │
          REST / WebSocket API  (fah-api)
                     │
             FastAdHunter Core
                     │
 ┌─────────────────────────────────────┐
 │ DNS Engine        (fah-dns)         │
 │ HTTP Engine       (Phase 2)         │
 │ HTTPS Engine      (Phase 3)         │
 │ Rule Engine       (fah-rules)       │
 │ Statistics        (fah-stats)       │
 │ Metrics           (fah-metrics)     │
 └─────────────────────────────────────┘
                     │
                 Internet
```

The dashboard is a separate deliverable and communicates exclusively through
the API. The core never depends on any UI.

---

## DNS Pipeline

Order matters: the Rule Engine runs **before** the cache, so rule changes take
effect instantly and the cache never stores verdicts
(see [ADR-0001](docs/decisions/0001-rules-before-cache.md)).

```text
Receive Query (UDP/53, TCP/53)
      │
Rule Engine ── verdict
      │
      ├─ Block  → synthesize 0.0.0.0 / :: (TTL 10s) ──► Reply
      │
Cache Lookup ── hit ────────────────────────────────► Reply
      │ miss
Upstream Resolver (UDP/TCP → DoT/DoH per config)
      │
Cache Store
      │
Reply
```

Pipeline properties:

- **Verdict first.** Every query gets a fresh verdict; unblocking/blocking a
  domain needs no cache flush.
- **Cache stores upstream answers only.** Bounded size (configurable),
  TTL-respecting with clamps, RFC 2308 negative caching, RFC 8767 serve-stale
  when upstreams are unreachable.
- **Blocked queries never touch the network.**

## Listeners (Phase 1)

- UDP/53 with EDNS(0); TCP/53 for truncation fallback (mandatory).
- DoT/DoH **listeners** arrive in a later phase (client cert distribution
  depends on Phase 3 certificate machinery).
- DNSSEC: pass-through (DO bit and RRSIGs forwarded untouched). Local
  validation is a roadmap item, off by default when it lands.

## Upstreams

- Protocols: plain UDP/53 with TCP fallback, DoT, DoH (Hickory + rustls).
- Strategy: ordered parallel fallback — primary first, next on timeout/failure.
- Defaults: `1.1.1.1`, `9.9.9.9`, plain DNS; encrypted upstreams are opt-in.

---

## Workspace Layout

```text
FastAdHunter/
├── Cargo.toml            # workspace root
├── crates/
│   ├── fah-common/       # shared errors, small utils
│   ├── fah-logging/      # tracing init, formats, levels
│   ├── fah-config/       # TOML load/merge, precedence, hot-reload
│   ├── fah-model/        # domain model + shared DTOs (Query, Verdict, Client…)
│   ├── fah-rules/        # Rule Engine: parsers + compiled matchers
│   ├── fah-dns/          # listeners, pipeline, cache, upstreams
│   ├── fah-api/          # Axum REST + WebSocket
│   ├── fah-metrics/      # ops telemetry: Prometheus counters/histograms
│   ├── fah-stats/        # product data: query log, aggregates, snapshots
│   └── fastadhunter/     # thin binary — wires everything
├── tests/                # workspace integration tests
├── benches/              # criterion benches vs PERFORMANCE.md budgets
├── docs/                 # images/, diagrams/, decisions/
└── dashboard/            # empty until the dashboard phase
```

DNS wire format comes from `hickory-proto`; `fah-model` holds our own domain
types only.

## Dependency Layering

```text
L4:  fastadhunter (binary — wires everything)
L3:  fah-dns   fah-api   fah-stats   fah-metrics
L2:  fah-rules
L1:  fah-model   fah-config   fah-common   fah-logging
```

**Rules:**

1. Dependencies point **downward** only. Lower layers never import higher ones.
2. **Siblings never import each other.** The binary wires them together via
   channels and handles. Example: `fah-dns` emits a `QueryEvent` (a `fah-model`
   type) into a channel; `fah-stats` consumes it; the channel is created and
   connected in `fastadhunter`.
3. **`fah-model` purity rule:** only data types and trivial traits. No business
   logic, no network access, no parsers, no cache.
4. `fah-common` is for genuinely shared small utilities — not a dumping ground.

**Ports.** When a lower layer needs something a higher one owns, it declares a
trait describing what it needs and the binary supplies the implementation — the
dependency arrow stays pointing down. Two of these exist:

| Port                             | Declared by      | Implemented in `fastadhunter` over       |
| -------------------------------- | ---------------- | ---------------------------------------- |
| `StatsSource`, `TelemetrySource` | `fah-api` (L3)   | `fah-stats`, `fah-metrics` (L3 siblings) |
| `HostResolver`                   | `fah-rules` (L2) | `fah-dns`'s `UpstreamPool` (L3)          |

`HostResolver` is what lets the Rule Engine's list downloader resolve its
sources through FastAdHunter's own configured upstreams rather than the
system's `/etc/resolv.conf` — the container has no working one. It deliberately
bypasses the query pipeline: resolution for list downloads must not be
filterable, or a blocklist could block the host serving its own next copy and
permanently prevent its own replacement.

Cargo enforces acyclicity natively; the layering above is enforced by review
against this document.

---

## Runtime Model

```text
               Tokio Runtime (multi-threaded)
                     │
      ┌──────────────┼──────────────┐
      │              │              │
 Worker 1       Worker 2       Worker N
      │              │              │
   full pipeline  full pipeline  full pipeline
```

- Every worker runs the complete pipeline; no stage is pinned to a thread and
  there is no central dispatcher.
- **Ingest socket topology:** today one `recv_from` loop pulls UDP datagrams off
  a single socket and spawns a task per datagram, so the expensive stages (match,
  cache, forward, reply) already spread across all workers — only *reception* is
  serial, and it is the cheapest step. The documented scaling path, *if and when*
  reception itself becomes the ceiling, is `SO_REUSEPORT`: N sockets, N recv
  loops, the kernel load-balancing datagrams across cores. Measured on-device it
  is not the ceiling (all cores share evenly with ~80% idle under a synthetic
  hammer), so it stays a ready recipe rather than shipped code — see
  `plan/wip/phase1.5/p1.5-06-reuseport-multisocket-ingest.md`.
- Shared state (compiled ruleset, config, cache shards) is reached through
  lock-free reads: **atomic swap** for ruleset/config, sharding for the cache.
  No global lock on the hot path.
- Cross-component communication (e.g. query events to `fah-stats`) uses bounded
  channels; a slow consumer drops events rather than back-pressuring the
  pipeline.

## Data & Persistence

No database. RAM plus files on the persistent volumes:

| Volume    | Contents                                                        |
|-----------|------------------------------------------------------------------|
| `/config` | `fastadhunter.toml`, API key, TLS certificates                   |
| `/data`   | cached raw rule lists, query-log segments, statistics snapshots  |

- Rule lists: downloaded to temp, parsed, validated, atomically swapped in;
  raw copies cached in `/data` so boot never waits on the network.
- Query log: in-RAM ring buffer + batched append-only segments on `/data`,
  pruned by age and size caps.
- Statistics: fixed-size in-RAM aggregates, snapshotted to `/data` periodically.

See [ADR-0002](docs/decisions/0002-no-embedded-database.md).

## Principles

- **Zero-copy where possible** — buffers referenced, not copied.
- **Streaming before buffering** — later phases process HTTP/HTML incrementally
  (lol_html); nothing loads whole documents.
- **No runtime regex compilation** — rules compile to matchers at load time.
- **Deterministic execution** — bounded structures everywhere; memory does not
  grow with traffic.

Numeric budgets live in [PERFORMANCE.md](PERFORMANCE.md).
