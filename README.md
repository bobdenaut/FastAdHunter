# FastAdHunter

> The fastest network-wide ad blocker with the smallest possible memory
> footprint.

API-first • Multi-core • ARM64-first • Rust • Docker-native

------------------------------------------------------------------------

# Executive Summary

FastAdHunter is a high-performance network filtering engine designed
around a single objective: deliver network-wide ad blocking with
predictable latency, minimal CPU usage and a very small memory
footprint.

Rather than optimizing for the largest feature set, FastAdHunter
optimizes for efficient execution. Every architectural decision is
evaluated through the impact it has on throughput, latency and memory
allocations.

The project is built around a modular architecture where the filtering
engine remains independent from the user interface. The core exposes
APIs only, allowing multiple frontends to coexist without increasing
runtime complexity.

------------------------------------------------------------------------

# Why FastAdHunter Exists

Current solutions typically focus on one layer of filtering:

-   DNS
-   Browser extensions
-   Desktop applications

FastAdHunter focuses on the network layer and is designed to evolve
without changing its architecture.
FastAdHunter aims to become a lightweight network engine capable of
protecting every device behind a router while remaining simple to deploy
and maintain.

Goals:

-   Small RAM footprint
-   Low CPU usage
-   Predictable latency
-   Efficient **multicore** execution
-   Clean API-first architecture

------------------------------------------------------------------------

# Project Philosophy

Performance is the primary feature.

The project intentionally favors deterministic behavior over excessive
configurability. Memory allocations are minimized, streaming is
preferred over buffering, and expensive operations are avoided whenever
possible.

Every new capability must justify its runtime cost.

The project intentionally favors:

-   deterministic execution
-   low memory allocations
-   streaming processing
-   simple architecture
-   maintainability

Feature count is always secondary.

------------------------------------------------------------------------

# Design Principles

## API-first

The filtering engine never depends on a graphical interface.

Every capability is exposed through REST APIs and WebSocket endpoints.

Benefits:

-   independent dashboard
-   automated testing
-   third-party integrations
-   future CLI support

Design rules:

-   Performance before features
-   API-first
-   No runtime regex compilation
-   Streaming before buffering
-   Zero-copy where possible
-   ARM64 first-class
-   Simple architecture
-   Deterministic execution

------------------------------------------------------------------------

# Docker-first

Deployment should require only a container.

Persistent data remains outside the container.

------------------------------------------------------------------------

# Multi-core by Design

Work is distributed across Tokio workers.

The architecture avoids centralized processing whenever possible.

------------------------------------------------------------------------

# ARM64 First

The primary deployment target is ARM64 hardware typically used in
routers, home labs and small servers.
FastAdHunter will run on docker deployed on MikroTik RB5009UG+S+IN.

The RB5009UG+S+IN is the perfect home router: 
	Compact, powerful, with multiple powering options and efficient cooling. The RB5009 has it all, and even more!
	The board features 9 wired ports and a full-sized USB 3.0 port. 
	Seven ports are Gigabit Ethernet, another is 2.5 Gigabit Ethernet, and the last one is a 10G SFP+ slot. 
	All ports are connected to a powerful Marvell Amethyst switch chip with a 10 Gbps full-duplex line leading 
	to the Marvell Armada Quad-core ARMv8 1.4 GHz CPU. Both the CPU and the switch chip are located 
	on the bottom of the board—so the case acts as a massive heatsink!
	It is also HW=yes (Hardware Offload), hardware accelerated!
	See docs/images/RB5009UGS.png.

------------------------------------------------------------------------

# Zero-copy

Buffers are referenced instead of copied whenever practical.

This reduces allocations and improves cache locality.

------------------------------------------------------------------------

# Streaming

Large responses should be processed incrementally instead of loading
complete documents into memory.

------------------------------------------------------------------------

# High-Level Architecture

``` text
             Dashboard (optional, implemented later)
                     │
          REST / WebSocket API
                     │
             FastAdHunter Core
                     │
 ┌─────────────────────────────────────┐
 │ DNS Engine                          │
 │ HTTP Engine        (Phase 2)        │
 │ HTTPS Engine       (Phase 3)        │
 │ Rule Engine                         │
 │ Statistics                          │
 │ Metrics                             │
 └─────────────────────────────────────┘
                     │
                 Internet
```

The dashboard is intentionally separated from the filtering engine.

Full diagram: [docs/diagrams/architecture.svg](docs/diagrams/architecture.svg)
(HTML version: [docs/diagrams/architecture.html](docs/diagrams/architecture.html))

------------------------------------------------------------------------

# Request Processing

## DNS Pipeline

``` text
Receive Query
      │
Rule Engine ── blocked? → synthesized reply
      │
Cache Lookup ── hit? → Reply
      │
Upstream Resolver
      │
Cache Store
      │
Reply
```

Each stage performs one responsibility only.

The Rule Engine runs **before** the cache, so rule changes take effect
instantly and the cache never stores verdicts
(see [docs/decisions/0001](docs/decisions/0001-rules-before-cache.md)).

A **Policy** concept (named bundles of rule lists + settings, assignable to
clients/schedules) arrives in Phase 2 — see [ROADMAP.md](ROADMAP.md).

------------------------------------------------------------------------

# HTTP Pipeline

``` text
TCP
 │
HTTP Parser
 │
Header Processing
 │
Rule Engine
 │
HTML Processing
 │
Compression
 │
Client
```

HTML processing is intended only when required.

Other traffic should pass through with minimal overhead.

------------------------------------------------------------------------

# Operating Modes

Configurable at docker start phase:

	- DNS

Network-wide DNS filtering.

	- DNS + HTTP

HTTP filtering for unencrypted traffic.

	- DNS + HTTP + HTTPS

HTTPS interception for managed environments.

------------------------------------------------------------------------

# Runtime Architecture

```text
               Tokio Runtime
                     │
      ┌──────────────┼──────────────┐
      │              │              │
 Worker 1       Worker 2       Worker N
      │              │              │
      ├── DNS        ├── DNS        ├── DNS
      ├── HTTP       ├── HTTP       ├── HTTP
      ├── HTTPS      ├── HTTPS      ├── HTTPS
      └── Rules      └── Rules      └── Rules
```
------------------------------------------------------------------------
# API Layer

``` text
GET  /health
GET  /metrics
GET  /api/v1/stats
GET  /api/v1/clients
POST /api/v1/config
WS   /api/v1/events
etc
etc
```
Dashboard communicates exclusively through the API.
------------------------------------------------------------------------

# Certificate Management

```text
/api/v1/certificates
etc
```

Operations:
- Import PEM
- Import PFX
- Generate CA
- Export CA
- Status

No manual TLS implementation.

Libraries:
- rustls
- rcgen
- x509-parser

> **Rule:** Do not reinvent cryptography.

------------------------------------------------------------------------

# Repository Layout

``` text
FastAdHunter/
├── Cargo.toml            # workspace root
├── crates/
│   ├── fah-common/       # shared errors, small utils
│   ├── fah-logging/      # tracing init, formats, levels
│   ├── fah-config/       # TOML, precedence, hot-reload
│   ├── fah-model/        # domain model + shared DTOs (pure data types)
│   ├── fah-rules/        # Rule Engine: parsers + compiled matchers
│   ├── fah-dns/          # listeners, pipeline, cache, upstreams
│   ├── fah-api/          # Axum REST + WebSocket
│   ├── fah-metrics/      # ops telemetry: Prometheus
│   ├── fah-stats/        # product data: query log, aggregates
│   └── fastadhunter/     # thin binary — wires everything
├── tests/                # workspace integration tests
├── benches/              # criterion benches vs PERFORMANCE.md budgets
├── docs/                 # images/, diagrams/, decisions/
└── dashboard/            # independent UI (later phase)
```

Dependency layering (see [ARCHITECTURE.md](ARCHITECTURE.md)): dependencies
point downward only, siblings never import each other, `fah-model` stays pure.

------------------------------------------------------------------------

# Technology Stack

```text
  Area       Choice
  ---------- --------------
  Language   Rust
  Runtime    Tokio
  HTTP       Hyper / Axum
  DNS        Hickory
  TLS        rustls
  HTML       lol_html
```
------------------------------------------------------------------------

# Performance Goals

-   low latency
-   low RAM usage
-   efficient CPU utilization
-   streaming pipelines
-   multicore scalability

------------------------------------------------------------------------

# Deployment

A single container is expected to provide the complete filtering engine.

Configuration, blocklists and certificates are mounted as persistent
volumes.

------------------------------------------------------------------------

# Project Goals

FastAdHunter IS:

-   lightweight
-   modular
-   predictable
-   API-first

FastAdHunter IS NOT:

-   a browser
-   an IDS
-   an antivirus
-   a general-purpose firewall

------------------------------------------------------------------------

# Roadmap

Phase 1

-   DNS
-   REST API
-   Docker

Phase 2

-   HTTP

Phase 3

-   HTTPS

Phase 4

-   HTML filtering

------------------------------------------------------------------------

# Documents

All design documents are written and approved before any code:

```text
FastAdHunter/
│
├── README.md                 ✅ this file
├── CONTEXT.md                ✅ glossary — the project's ubiquitous language
├── ARCHITECTURE.md           ✅ components, crates, layering, runtime model
├── ROADMAP.md                ✅ phases and their tasks
├── API.md                    ✅ all endpoints with request/response
├── CONFIGURATION.md          ✅ all config options, precedence, mutability
├── RULE_ENGINE.md            ✅ formats, verdicts, matcher, list lifecycle
├── PERFORMANCE.md            ✅ golden rules + numeric budgets
├── SECURITY.md               ✅ API key, TLS, certificates, docker hardening
├── CONTRIBUTING.md           ✅ conventions and local quality gates
├── LICENSE                   ✅ ❌ None (private repository)
│
└── docs/
    ├── images/               ✅ RB5009UGS.png
    ├── diagrams/
    └── decisions/            ✅ ADRs 0001–0003
```

No CI service is used (deliberate) — quality gates run locally,
see [CONTRIBUTING.md](CONTRIBUTING.md).

# Repository

- Visibility: Private
- License: None (proprietary until public release, if ever!)