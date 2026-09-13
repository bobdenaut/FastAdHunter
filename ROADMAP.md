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
| 2.5 — pre-Adaptive hardening | ✅ done | `v0.2.19-phase2.5` |
| 2.6 — Adaptive DNS Stage 1 | ✅ done — closed 2026-09-07; `adaptive` is the only strategy since p2.6-12, cherry-picked to `main` 2026-09-11 after the A/B measurement | `soak-p2.6-11` · `v0.3.2` |
| 5 — web dashboard | ✅ done — closed 2026-09-01, four verification rows deferred | `0.3.0` |
| 3 — HTTPS | 🚧 p3-01…p3-05 done, p3-06 `AWAITING SOAK` | — |
| 4 — HTML filtering | ⬜ not started | — |

Production runs **0.3.3** — HTTP served on allocation domains
([ADR-0006](docs/decisions/0006-http-allocation-domains.md)), N=2, plus the
dashboard fixes on top of 0.3.2 — on a **7-day soak from 2026-09-07 to
2026-09-14**. It replaced the 0.3.1 soak that was to carry `p2.6-11`'s
acceptance, stopped on day 6 for the swap by owner decision; the 0.3.0 re-soak
before that was terminated at T0+59 h to change the measurement method, not on a
gate, and carries no verdict. Phase 5's Stage B needed no deploy of its own —
0.3.1 already was the phase-5 build — so it was collected read-only against that
container.

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

## Phase 2.5 — Pre-Adaptive hardening ✅ **DONE** (`plan/closed/phase2.5-hardening/`)

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
- [x] **p2.5-06** per-endpoint failure run-length distribution on `/telemetry`
      (the data source for Stage 1's gate S1-G4) — live since the 0.2.18
      deploy, 2026-08-23
- [x] **p2.5-07** SWR refresh-claim lease provably exceeds the worst-case
      upstream walk
- [x] **p2.5-08** hygiene — tracked bearer token gone and rotated, layering
      guard covers the whole workspace, stale docs reconciled
- [x] **p2.5-09** phase verification — gates green, 0.2.18 deployed,
      listener-death drill passed, S1-G4 collection running
- [x] **p2.5-10** WS endpoint attribution — `endpoint` reaches the WS `query`
      event
- [x] **p2.5-11** refusal log hygiene — egress refusals log at `debug` and are
      counted on `/telemetry`

0.2.18 went to the device after `p2.5-08`: the failure counters and the
run-length distribution want **deployment time**, since every day they run
before Stage 1 lands is measurement data for judging it.

## Phase 2.6 — Adaptive DNS Stage 1 ✅ **DONE** — closed 2026-09-07 (`plan/closed/phase2.6-adaptive-stage1/`)

Failure-aware upstream selection, behind `strategy = "adaptive"`, opt-in.
Specification frozen: [docs/design/adaptive-upstream-selection.md](docs/design/adaptive-upstream-selection.md)
with its benchmark protocol
[adaptive-upstream-selection-benchmarks.md](docs/design/adaptive-upstream-selection-benchmarks.md).
Thirteen tasks, each with an implementation plan; validated and frozen for
implementation on 2026-08-23.

What Stage 1 does: per-endpoint health (64-byte packed word), penalty after
`penalty_failures` consecutive transport failures, penalized endpoints skipped
during selection, on-path recovery probing, `resolve_host` isolated from
health, telemetry (`state`, `penalty_round`, `penalties`, `probes`, …).
DNS RCODEs never affect health. Nothing from Stage 2 (RTT ordering) or
Stage 3 (hedging) — both stay candidate designs behind their own benchmark
gates and are **not scheduled**.

Two tiers of acceptance:

- **Merge tier** (dev box) — `adaptive` ships opt-in: correctness gates
  S1-G1 #1–#17, the pinned microbench (S1-G2 tier 1) and the injected-failure
  bench (S1-G3). Fully mechanical. **All passed**; `adaptive` has been opt-in
  in production since 2026-08-25.
- **Deployment tier** (RB5009) — `adaptive` becomes the default and `fallback`
  is deleted. **S1-G2 tiers 2 and 3 passed** (`attempts/miss = 1.000000`;
  +0.036 % against the frozen 5.00 % band). **S1-G4 and S1-G5 route 2 closed
  unvalidated**, by owner decision: the `fallback` run-length window ended with
  three closed runs, all of length 1, over 42 639 primary attempts — far too
  few to calibrate `penalty_failures`, which stays at its compiled default of
  **2, provisional and empirically uncalibrated**. Do not later describe
  Stage 1's constants as calibrated on this deployment. The window cannot be
  reopened without reverting the strategy.

All thirteen tasks are closed. **p2.6-11's first 7-day soak was terminated on
day 5** (owner decision, 2026-08-29): its RSS excursions were traced to list
refreshes re-downloading unchanged bodies — not to the adaptive path — and two
more observation days added no information. That defect is fixed in 0.3.0
(conditional GET, below). The re-soak of 0.3.0 was **terminated at T0+59 h to
change the measurement method, not on a gate**, and the third soak, **0.3.1 from
2026-09-01T07:27 Z, was stopped on day 6 (2026-09-07)** for the allocation-domain
production swap; `p2.6-11` closed on its RSS-drift evidence
(`docs/code-review/phase2.6/resoak-0.3.1-memory-diagnosis.md`). Sequencing of
the three: [plan/resoak-orchestration.md](plan/resoak-orchestration.md).

**p2.6-12 — the default flip — done on `phase3-06` (2026-09-07) and
cherry-picked to `main` on 2026-09-11.** `adaptive` is the compiled-in default,
the `fallback` variant and walk are deleted, and a config naming `fallback`
fails at load with a removal message. The gates it waited on: two
deployment-tier gates closed unvalidated — three length-1 runs, too few to
calibrate `penalty_failures`, which stays at 2 — so the flip was an owner
decision on that record. The `main` cherry-pick rests on a controlled A/B
instead: `fallback` vs `adaptive` at production timings across 0/4, 1/4, 2/4
and 4/4 dead upstreams plus recovery
([strategy-ab-fallback-vs-adaptive.md](docs/code-review/phase2.6/strategy-ab-fallback-vs-adaptive.md)).
Review of the change itself:
[p2.6-12-default-flip-review.md](docs/code-review/phase2.6/p2.6-12-default-flip-review.md).

### Shipped alongside — list refresh, conditional GET (0.3.0)

Not a Stage 1 feature; the repair the terminated soak paid for.

- [x] `If-None-Match` / `If-Modified-Since` with a 304 short-circuit —
      validators persisted beside the cached `.raw` files; a 304 refresh
      allocates O(1), touches no list buffer and does not recompile
- [x] `counters.lists` on `/telemetry` — `bodies`, `not_modified`,
      `bytes_fetched` — plus the allocator-commit figure in the perf sample, so
      a future RSS step is attributable from `/history/perf` alone. The
      2026-08-29 hunt only closed because the owner opened the router's
      bandwidth graph; that dependency is now removed.

### Shipped alongside — HTTP allocation domains (0.3.2)

Not a Stage 1 feature either; what the 0.3.1 soak's memory diagnosis led to.

- [x] Each HTTP connection served end to end on one of `[runtime]
      http_runtimes` single-thread runtimes on their own OS threads, behind one
      acceptor — a connection's allocations are freed by the thread that made
      them ([ADR-0006](docs/decisions/0006-http-allocation-domains.md)). N=2 on
      the RB5009 from the N sweep: a third less CPU per request, DNS p50 under
      HTTP load 0.96 ms against 3.35, +19 MiB held after a 900 MiB burst against
      +56..+60
- [ ] The 7-day soak at N=2 (0.3.3, to 2026-09-14) is the verdict on the memory
      plateau; the predeclared floor criterion was not met in the A/B and the
      adoption is recorded as an owner decision in the ADR

## Phase 5 — Web Dashboard ✅ **DONE** — closed 2026-09-01 (`plan/closed/phase5/`)

**Numbered 5, scheduled ahead of 3 and 4.** Execution order is
2.5 → 2.6 → **5** → 3 → 4: the number follows the capability roadmap, the
position follows what the household needs (owner decision, 2026-08-25).
Implementation ran **in parallel** with Phase 2.6, on the `phase5-NN` cumulative
branch chain (owner decision, 2026-08-26), and merged to `main` as **0.3.0**.

A static, API-only web interface served by `fah-api` itself on the existing TLS
listener. No second container, no Node in the runtime image, no new port. Pi-hole
supplies the visual and interaction language; the FAH API decides what exists.
Design record: [docs/dashboard/](docs/dashboard/) — the capability matrix is the
gate for what gets built, and a screen with no endpoint behind it is cut, never
faked.

- [x] **p5-01** static serving — `fah-api` serves the baked `/web`; route
      ordering, cache split, pre-compressed assets, multi-stage image
- [x] **p5-02** certificate and browser spike — real desktop/phone evidence, the
      SAN decision, the regeneration migration
- [x] **p5-03** API contract additions — `/events` subscription protocol,
      `GET /clients` policy fields, reserved API.md sections
- [x] **p5-04** authentication — Argon2id, session cookie, cookie on REST and the
      WebSocket upgrade
- [x] **p5-05** frontend foundation — Vite/TS/Preact shell, typed client, socket
      manager, bundle-size gate
- [x] **p5-06 … p5-09** thirteen screens — Dashboard · Lists · Custom Rules ·
      Policies · Clients · Rule Tester · Cache · Performance · Upstreams ·
      Settings · Health · Memory · Live Feed
- [ ] **p5-10** verification — **Stage A passed** on the dev box: e2e, route
      ordering, bundle, image, per-figure API trace, route-scoped fetching,
      socket load, emulated mobile pass. **Stage B ran read-only against the
      deployed 0.3.1** and needed no deploy of its own — that build already is
      the phase-5 build, so the p2.6 dependency the plan assumed never applied.
      Four rows are **deferred, not passed**: concurrent Argon2id peak RSS, RSS
      deltas above the drift floor, `/cache` at a second occupancy, and the
      real-phone leg

Three things decided whether it succeeds, and all three are measured:
**128,730 B gzip against the 150 KB budget** (83.8 %), an inactive page performs
approximately zero API work, and the **arm64 rootfs is 14.07 MiB against 30 MB**
(the dashboard's own share is ~4.24 MB). Performance is the product, and a
dashboard that contradicts it is worse than no dashboard.

Evidence: [docs/code-review/phase5/](docs/code-review/phase5/).

**Phase 3 and Phase 4 each trigger a dashboard re-review** when they land — Phase
3 adds certificate UI and per-client HTTPS-interception controls, Phase 4 moves
cosmetic rules out of `rules_inactive` and changes the Lists partition. Neither
is designed for in advance.

## Phase 3 — HTTPS 🚧 **IN PROGRESS** (`plan/wip/phase3/`)

Conditional on §5.7–14 of the reconciled architecture review (certificate
machinery home, connector redesign, DoH/DoT listener placement, telemetry
taxonomy, memory caps per new state owner, 443 steering v4+v6, on-device TLS
measurements, opt-in bound to a stable identity). Items 7–11 and 14 are closed;
**12 (443 steering) and 13 (on-device TLS measurements) stay open, owner-side.**

`p3-01`…`p3-05` are `DONE`; **`p3-06` is `AWAITING SOAK`.** Its dev-box suite
and the on-device probe campaign have run — SNI, P6 and P7-store pass, P5 is a
recorded budget miss that does not block closure, and P4's PERFORMANCE.md rows
returned to `TBD` when its declared statistic proved unstable across sessions.
Four arms (P1-LAN, P1-control, P2, P3) are **parked on hardware**: they need a
second wired LAN endpoint. The row flips on the 24 h full-mode soak, which
cannot start before the 0.3.3 allocation-domain soak ends 2026-09-14. Evidence:
[docs/code-review/phase3/](docs/code-review/phase3/).

**2026-09-07: `main` (857865d) merged into `phase3-06`.** The HTTPS listener now
feeds the same allocation domains as HTTP
([ADR-0006](docs/decisions/0006-http-allocation-domains.md)): one acceptor per
listener, and the ClientHello peek, SNI verdict, splice or MITM handshake all run
on the domain thread. ADR-0006's revisit trigger — remeasure N with TLS on the
RB5009 — is open, owner-side.

- SNI-level HTTPS filtering for every client, no setup and no decryption
  (`p3-03`) — blocked domains die at the ClientHello; ECH/no-SNI is closed, not
  forwarded (measured transport limit, SECURITY.md)
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
- ~~Upstream health and load-balancing~~ — **promoted out of the backlog**
  as Phase 2.6, Adaptive DNS Stage 1 (above). Stages 2 and 3 stay candidate
  designs behind explicit benchmark gates and are not scheduled. Still the
  thing that makes a second-family (IPv6) upstream safe to add.
- Per-client blocked-response modes (NXDOMAIN, REFUSED, custom IP)
- ~~Dashboard (`dashboard/`) — separate deliverable, API-only consumer~~ —
  **promoted out of the backlog** as Phase 5, Web Dashboard (above), and
  scheduled ahead of Phases 3 and 4.
- List-file management endpoints (upload/edit local lists via API)
