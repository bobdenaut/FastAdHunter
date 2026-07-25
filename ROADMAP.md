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
| 2 — HTTP | 🚧 in progress | — |
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
      periodic `/data` snapshots; query log ring buffer + batched segments,
      age/size retention, auto-prune
- [x] `fah-metrics`: Prometheus export — QPS, latency histograms, cache hit
      ratio, memory, per-verdict counters

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
- [x] History retention config (`retention_days`, `retention_max_mb`)
- [x] History query API + `GET /api/v1/history`
- [x] Cache byte cap (`max_bytes`) alongside `max_entries`, O(1) eviction
- [x] Config mutability contract: `boot` keys persist + `restart_required`,
      `runtime` keys swap live
- [x] `fix(metrics)`: blocked queries no longer counted as cache misses
- [ ] ~~SO_REUSEPORT multi-socket ingest~~ — **measured and deferred.** The
      RB5009's ~15–16k QPS ceiling is FAH-handling-bound, not ingest-bound
      (`fastadhunter` 65.8 % CPU via `/tool profile`); UDP recv and conntrack
      were ruled out. See `docs/code-review/p1.5-06-review.md`.

## Phase 2 — HTTP 🚧 **IN PROGRESS** (`plan/wip/phase2/`)

- [x] **p2-00** parser correctness — sample-based format detection;
      `||domain^*/path` no longer compiles to a whole-domain DNS block
      (`docs/code-review/p2-00-review.md`). Not HTTP work: a `fah-rules`
      foundation fix Phase 2 turned out to depend on.
- HTTP proxy engine for unencrypted traffic (streaming, pass-through fast path)
- URL-path rules and HTTP `$options` activate in the Rule Engine
- **Policy** concept lands: named bundle of rule lists + settings, assignable
  to clients and schedules (parental-control style); `$client` rules activate
- Per-client statistics grow into per-client policy reporting
- Operating mode `dns+http`

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

## Open question — a FastAdHunter browser extension

**Undecided. Revisit before Phase 4 hardens, and again after it ships.**
Nothing on disk until then.

A router cannot see the DOM after JavaScript has run, so a proxy — however
complete — will never reach first-party ads (YouTube, Facebook), dynamically
injected elements, or anti-adblock scripts. That limit is architectural, not a
matter of effort, and it is the honest ceiling on what Phases 1–4 can deliver.

The obvious workaround is to inject an agent script into rewritten HTML.
**That option is rejected on security grounds unless someone makes a strong new
case:** it means the router holds arbitrary code execution in the origin of
every site every device visits — read any password field, read `localStorage`,
issue authenticated same-origin requests. Compromising the ad blocker would
mean compromising every account in the house, and enabling it means weakening
`Content-Security-Policy` on sites that set one.

The alternative is a real extension, and the split is clean:

| Layer | Component | Covers |
| ----- | --------- | ------ |
| Network | FastAdHunter | every device — TVs, phones, IoT, guests; no rule ceiling |
| DOM | extension | browsers only — cosmetic rules and scriptlets |

This sidesteps Manifest V3's central problem. MV3's rule cap applies to
`declarativeNetRequest` (network blocking); cosmetic filtering runs from a
content script and is **not** capped — which is why uBlock Origin Lite still
hides elements well while its network blocking is crippled. Because
FastAdHunter already blocks at the network layer for every device, the
extension needs no network rules at all: only cosmetic ones (~24,400 across
EasyList + EasyPrivacy). That makes it far smaller than uBlock, with minimal
permissions.

If lists ever drive script execution, copy uBlock's constraint exactly: a
filter list may **reference** pre-written, audited scriptlets with parameters,
never supply JavaScript. Retrofitting that boundary once list authors expect
flexibility is close to impossible.

**One decision this forces early.** If cosmetic rules are eventually *served to
a client* rather than only applied in-process by `lol_html`, the compiled
cosmetic representation must be serializable. Cheap to design in during Phase
4, expensive to retrofit afterwards — see `plan/open/phase4/`.

Scope, so the size is not underestimated: own codebase, own release channel,
store review, cross-browser testing, plus a router↔extension protocol and its
authentication. Phase 5 at the earliest, and only if Phase 4 proves cosmetic
rules compile and apply well in the first place.

## Backlog (no phase committed)

- Local DNSSEC validation (off by default)
- Upstream load-balancing strategies (latency-based, round-robin)
- Per-client blocked-response modes (NXDOMAIN, REFUSED, custom IP)
- Dashboard (`dashboard/`) — separate deliverable, API-only consumer
- List-file management endpoints (upload/edit local lists via API)
