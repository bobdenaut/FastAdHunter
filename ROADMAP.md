# ROADMAP

Phases ship in order. A phase is done when its tasks pass the quality gates in
[CONTRIBUTING.md](CONTRIBUTING.md) and the budgets in [PERFORMANCE.md](PERFORMANCE.md).

---

## Phase 1 — DNS + REST API + Docker

The complete network-wide DNS ad blocker, deployable on the RB5009.

### Workspace & foundations

- [ ] Cargo workspace, 10 crates, dependency layering per ARCHITECTURE.md
- [ ] `fah-model`: Query, Verdict, Client, QueryEvent, shared DTOs
- [ ] `fah-logging`: tracing setup, levels, formats
- [ ] `fah-common`: error types, small shared utils
- [ ] `fah-config`: TOML parse, defaults < file < env (`FAH__*`) < API precedence,
      write-back, runtime-mutable vs boot-only classification, atomic swap reload

### Rule Engine (`fah-rules`)

- [ ] Parsers: hosts format, plain domain list, EasyList/uBlock/AdGuard syntax
      (format auto-detection)
- [ ] DNS-applicable subset extraction; non-DNS rules parsed, counted, stored inactive
- [ ] AdGuard DNS extensions: `$dnstype`, `$dnsrewrite`, `$client` (parsed;
      `$client` inactive until Phase 2)
- [ ] Compiled matcher (domain hash/trie, allow > block precedence)
- [ ] List lifecycle: download, validate, atomic swap, `/data` caching,
      keep-previous-on-failure, per-list refresh interval (default 24h)
- [ ] Inline user rules (personal allow/block) via API
- [ ] Default curated list (OISD basic) enabled on first run

### DNS Engine (`fah-dns`)

- [ ] UDP/53 + TCP/53 listeners, EDNS(0)
- [ ] Pipeline: rules → blocked-response synthesis (0.0.0.0/:: TTL 10s) → cache → upstream
- [ ] Cache: bounded (configurable max entries, default 10k), TTL clamps,
      RFC 2308 negative caching, RFC 8767 serve-stale, sharded
- [ ] Upstreams: UDP/TCP, DoT, DoH; ordered parallel fallback;
      defaults 1.1.1.1 + 9.9.9.9
- [ ] DNSSEC pass-through (DO bit, RRSIGs)
- [ ] QueryEvent emission (bounded channel, drop-on-full)

### Stats & metrics

- [ ] `fah-stats`: in-RAM aggregates (24h rolling buckets, bounded top-N),
      periodic `/data` snapshots; query log ring buffer + batched segments,
      age/size retention, auto-prune
- [ ] `fah-metrics`: Prometheus export — QPS, latency histograms, cache hit
      ratio, memory, per-verdict counters

### API (`fah-api`)

- [ ] All endpoints per [API.md](API.md), bearer API key auth
- [ ] HTTPS default: rcgen self-signed on first boot, PEM/PFX replaceable
- [ ] WebSocket `/api/v1/events` live query stream

### Docker & delivery

- [ ] Static musl build, distroless/static image ≤30MB, arm64 + amd64 manifest
- [ ] Volumes `/config` + `/data`; first-boot bootstrap (TOML, API key, cert)
- [ ] `fastadhunter --healthcheck` self-probe
- [ ] Deployment guide for MikroTik RB5009 (RouterOS container)

### Verification

- [ ] Integration tests (`tests/`), criterion benches (`benches/`) vs budgets
- [ ] Soak test on RB5009: real household traffic, RAM/latency measured

---

## Phase 2 — HTTP

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

## Backlog (no phase committed)

- Local DNSSEC validation (off by default)
- Upstream load-balancing strategies (latency-based, round-robin)
- Per-client blocked-response modes (NXDOMAIN, REFUSED, custom IP)
- Dashboard (`dashboard/`) — separate deliverable, API-only consumer
- List-file management endpoints (upload/edit local lists via API)
