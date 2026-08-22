# ROADMAP

Phases ship in order. A phase is done when its tasks pass the quality gates in
[CONTRIBUTING.md](CONTRIBUTING.md) and the budgets in [PERFORMANCE.md](PERFORMANCE.md).

Per-task status lives in each phase's `CLAUDE.md` under `plan/` — this file
tracks phases, not tasks. Ticked items here are shipped and verified on the
RB5009, not merely written.

| Phase | Status | Tag |
| ----- | ------ | --- |
| 0 — foundations | ✅ done | `v0.1.0-phase0` |
| 1 — DNS + API + Docker | ✅ done | `v0.2.0-phase1` |
| 1.5 — observability persistence | ✅ done | `v0.3.0-phase1.5` |
| 2 — HTTP | ✅ done | `v0.2.17-phase2` |
| 2.5 — pre-Adaptive hardening | 🚧 in progress | — |
| 3 — HTTPS | ⬜ not started | — |
| 4 — HTML filtering | ⬜ not started | — |

---

## Phase 1 — DNS + REST API + Docker ✅ **DONE** (`v0.2.0-phase1`)

The complete network-wide DNS ad blocker, deployable on the RB5009.
Shipped and verified on-device; see `plan/closed/phase1/`.

### Workspace & foundations

- [x] Cargo workspace, 10 crates, dependency layering per ARCHITECTURE.md
- [x] `fah-model`: Query, Verdict, Client, QueryEvent, shared DTOs
- [x] `fah-logging`: tracing setup, levels, formats
- [x] `fah-common`: error types, small shared utils
- [x] `fah-config`: TOML parse, defaults < file < env (`FAH__*`) < API precedence,
      write-back, runtime-mutable vs boot-only classification, atomic swap reload

### Rule Engine (`fah-rules`)

- [x] Parsers: hosts format, plain domain list, EasyList/uBlock/AdGuard syntax
      (format auto-detection)
- [x] DNS-applicable subset extraction; non-DNS rules parsed, counted, stored inactive
- [x] AdGuard DNS extensions: `$dnstype`, `$dnsrewrite`, `$client` (parsed;
      `$client` inactive until Phase 2)
- [x] Compiled matcher (domain hash/trie, allow > block precedence)
- [x] List lifecycle: download, validate, atomic swap, `/data` caching,
      keep-previous-on-failure, per-list refresh interval (default 24h)
- [x] Inline user rules (personal allow/block) via API
- [x] Default curated list (OISD basic) enabled on first run

### DNS Engine (`fah-dns`)

- [x] UDP/53 + TCP/53 listeners, EDNS(0)
- [x] Pipeline: rules → blocked-response synthesis (0.0.0.0/:: TTL 10s) → cache → upstream
- [x] Cache: bounded (configurable max entries, default 10k), TTL clamps,
      RFC 2308 negative caching, RFC 8767 serve-stale, sharded
- [x] Upstreams: UDP/TCP, DoT, DoH; ordered parallel fallback;
      defaults 1.1.1.1 + 9.9.9.9
- [x] DNSSEC pass-through (DO bit, RRSIGs)
- [x] QueryEvent emission (bounded channel, drop-on-full)

### Stats & metrics

- [x] `fah-stats`: in-RAM aggregates (24h rolling buckets, bounded top-N),
      bounded per-client registry, periodic `/data` snapshots
- [x] `fah-metrics`: QPS, latency histograms, cache hit ratio, memory,
      per-verdict counters — served via `/api/v1/telemetry`; Prometheus export
      removed (`p2-09`)

### API (`fah-api`)

- [x] All endpoints per [API.md](API.md), bearer API key auth
- [x] HTTPS default: rcgen self-signed on first boot, PEM/PFX replaceable
- [x] WebSocket `/api/v1/events` live query stream

### Docker & delivery

- [x] Static musl build, distroless/static image ≤30MB, arm64 + amd64 manifest
- [x] Volumes `/config` + `/data`; first-boot bootstrap (TOML, API key, cert)
- [x] `fastadhunter --healthcheck` self-probe
- [x] Deployment guide for MikroTik RB5009 (RouterOS container)

### Verification

- [x] Integration tests (`tests/`), criterion benches (`benches/`) vs budgets
- [x] Soak test on RB5009: real household traffic, RAM/latency measured

---

## Phase 1.5 — Observability persistence ✅ **DONE** (`v0.3.0-phase1.5`)

Unplanned phase, inserted after Phase 1 shipped: the stats surface was
in-RAM-only and lost everything on restart. See `plan/closed/phase1.5/`.

- [x] History rollups — hourly/daily aggregation persisted to `/data`
- [x] Perf sample series (bounded, ~740 B/sample)
- [x] History retention config (`retention_days`)
- [x] History query API + `GET /api/v1/history`
- [x] Cache byte cap (`max_bytes`) alongside `max_entries`, O(1) eviction
- [x] Config mutability contract: `boot` keys persist + `restart_required`,
      `runtime` keys swap live
- [x] `fix(metrics)`: blocked queries no longer counted as cache misses
- [ ] ~~SO_REUSEPORT multi-socket ingest~~ — **measured and deferred.** The
      RB5009's ~15–16k QPS ceiling is FAH-handling-bound, not ingest-bound
      (`fastadhunter` 65.8 % CPU via `/tool profile`); UDP recv and conntrack
      were ruled out. See `docs/code-review/phase1/p1.5-06-review.md`.

## Phase 2 — HTTP ✅ **DONE** (`v0.2.17-phase2`)

Shipped and verified on-device; see `plan/closed/phase2/`.

- [x] **p2-00** parser correctness — sample-based format detection;
      `||domain^*/path` no longer compiles to a whole-domain DNS block
      (`docs/code-review/phase2/p2-00-review.md`). Not HTTP work: a `fah-rules`
      foundation fix Phase 2 turned out to depend on.
- [x] HTTP proxy engine for unencrypted traffic (streaming, pass-through fast
      path); the verdict is taken on the **head**, so a block costs no DNS
      lookup and no upstream connection
- [x] Default-deny egress guard judging the **resolved** address, shared with
      Phase 3
- [x] URL-path rules and HTTP `$options` active in the Rule Engine; a
      literal-run n-gram tier took the unindexed rule count to **0**
- [x] **Policy** — named bundle of rule lists + settings, assignable to clients
      and schedules; `$client` active. All policies share one compiled ruleset
      behind a 16-bit per-rule visibility mask, so N policies cost **+2.03 MiB
      flat** rather than a ruleset each
- [x] Per-client enforcement off one precomputed snapshot, schedules evaluated
      on a tick in the binary
- [x] Operating mode `dns+http`
- [x] Telemetry consolidation — one JSON snapshot at `/api/v1/telemetry`; **no
      Prometheus endpoint and no query-log reader** (`p2-09`, on-device verified)
- [x] Memory breakdown persisted into the perf series, and the compile peak made
      observable via `getrusage`'s high-water mark on `/history/perf` (`p2-13`)
- [x] IPv6 HTTP interception — the last functional gap, a dual-stack origin
      reached over IPv6 no longer bypasses the proxy
- [ ] ~~Reduce the list-refresh memory transient~~ — **accounted for, not
      optimised.** 106.61 of the 125.69 MB device transient explained exactly,
      the dominant term being the *parsed* list form rather than the compiled
      arena. Levers sized and deliberately not taken: the largest, list
      ordering, is worth ±19.92 MB but changes the compiled ruleset, and the
      deployment already sits at the best case. `p2-11` and `p2-12` both closed
      with zero code (`docs/code-review/phase2/p2-12-compile-transient-attribution.md`).

## Phase 2.5 — Pre-Adaptive hardening 🚧 **IN PROGRESS** (`plan/wip/phase2.5-hardening/`)

Unplanned phase, inserted after Phase 2 shipped. Closes the operational risks
and the Adaptive DNS Stage 1 ship-gates a global architecture review raised.
**Adaptive upstream selection itself is not in this phase** — see
`docs/design/adaptive-upstream-selection.md`.

- [x] **p2.5-01** listener resilience — DNS listener loops survive transient
      socket errors instead of dying silently; the healthcheck exercises port 53
- [x] **p2.5-02** list-refresh integrity — a fetched body is validated before it
      can replace the last-good `/data` copy; `parse_errors` reaches the API
- [x] **p2.5-03** encrypted reconnect — a timeout invalidates the pooled DoT/DoH
      connection, so the next exchange reconnects
- [x] **p2.5-04** transport error kinds — `io::ErrorKind` fidelity through the
      encrypted transports; RCODE-is-not-a-failure pinned by test
- [x] **p2.5-05** outcome telemetry — a served SERVFAIL is countable
      (synthesized vs relayed vs refused) and a forwarded query's event names
      the endpoint that answered
- [ ] **p2.5-06** per-endpoint failure run-length distribution on `/telemetry`
      (the data source for Stage 1's gate S1-G4)
- [ ] **p2.5-07** SWR refresh-claim lease provably exceeds the worst-case
      upstream walk
- [ ] **p2.5-08** hygiene — tracked bearer token gone and rotated, layering
      guard covers the whole workspace, stale docs reconciled
- [ ] **p2.5-09** phase verification — gates green, deployed, listener-death
      drill passed, S1-G4 collection running

A mid-phase deploy after `p2.5-06` is recommended: the failure counters and the
run-length distribution want **deployment time**, since every day they run
before Stage 1 lands is measurement data for judging it.

## Phase 3 — HTTPS

- HTTPS interception for managed environments (opt-in, per-client)
- Certificate management: generate CA, import PEM/PFX, export CA, status —
  `/api/v1/certificates` (rustls + rcgen + x509-parser; no hand-rolled crypto)
- DoT/DoH **listeners** (Android Private DNS support)
- Operating mode `dns+http+https`

## Phase 4 — HTML Filtering

- Streaming HTML rewriting powered by lol_html — element/cosmetic rules
  activate; the differentiator AdGuard Home lacks
- Applied only where required; all other traffic passes through untouched

## Future — Browser Integration (exploratory)

**Goal:** let FastAdHunter **policies** influence browser-level cosmetic
filtering, so a per-client policy shapes what a page looks like and not only
which domains resolve.

**Not intended to replace uBlock Origin.** Potentially a thin companion
extension. Nothing is promised here — this records a direction, not a plan, and
nothing goes on disk until the criteria below are met.

**Decision criteria — all three, or it does not get built:**

1. Phases 2/3/4 ship and demand is real.
2. The Policy model provides value nothing else does.
3. Integration value exceeds the maintenance cost of a second codebase,
   release channel, store review, and a router↔extension protocol with its
   own auth.

### Why it stays exploratory

The product is the network firewall for the whole house. That is the piece with
no substitute: TVs, phones, apps, IoT, guests. A browser integration would be
the **last 10 %** that ties the experience together — not the foundation, and
worth building only once the foundation is proven.

It would also not be a capability win. Manifest V3's rule cap applies to
`declarativeNetRequest` — **network** blocking — while cosmetic filtering runs
from a content script and is uncapped. That is why uBlock Origin Lite still
hides elements competently even as its network blocking is crippled. So
FastAdHunter + uBO Lite already covers both layers today. Any FAH extension
would add *integration* (central rule management, per-client policy reaching
into the browser), not a filtering ability users cannot otherwise get — while
competing with a free, excellent, deeply trusted incumbent.

### Two constraints worth recording now

**HTML script injection is rejected on security grounds.** The tempting
shortcut — injecting an agent into rewritten HTML instead of shipping an
extension — would give the router arbitrary code execution in the origin of
every site every device visits: read any password field, read `localStorage`,
issue authenticated same-origin requests. It converts a compromise of the ad
blocker into a compromise of every account in the house, and it requires
weakening `Content-Security-Policy` on sites that set one. Reopening this needs
a strong new case and an ADR, not an implementation.

**Serializability is the only thing this costs today.** If cosmetic rules are
ever served to a client rather than only applied in-process by `lol_html`, the
compiled cosmetic form is what would travel. Designing it to be writable out is
free during Phase 4 and expensive afterwards — see `plan/open/phase4/`. Build
no endpoint and no protocol; keep the shape open, and decide later.

And should filter lists ever drive script execution, copy uBlock's constraint
exactly: a list may **reference** pre-written, audited scriptlets with
parameters, never supply JavaScript.

## Backlog (no phase committed)

- Local DNSSEC validation (off by default)
- ~~Upstream health and load-balancing~~ — **promoted out of the backlog.**
  Using `consecutive_failures` for *selection* rather than only reporting it is
  now Adaptive DNS Stage 1, accepted as a specification:
  `docs/design/adaptive-upstream-selection.md`. Phase 2.5 is its prerequisite
  hardening; Stages 2 and 3 stay candidate designs behind explicit benchmark
  gates. Still the thing that makes a second-family (IPv6) upstream safe to add.
- Per-client blocked-response modes (NXDOMAIN, REFUSED, custom IP)
- Dashboard (`dashboard/`) — separate deliverable, API-only consumer
- List-file management endpoints (upload/edit local lists via API)
