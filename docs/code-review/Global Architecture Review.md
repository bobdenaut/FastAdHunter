# Global Architecture Review — main @ ad9343c (2026-08-21)

Commissioned by [GAR-request.md](GAR-request.md). Read-only audit; seven
parallel auditor passes (layering, DNS/upstreams, HTTP/TLS/security,
policy/state, perf/memory, observability/failure/deploy, precedent).
Baseline note: GAR-request names `37e5a8f`; HEAD is `ad9343c` — the two
commits on top are a test fix and a docs reorg, architecturally identical.

Classification per GAR method: **VALIDATED** (code + tests + deployed
evidence) / **SUPPORTED** (coherent, deployment evidence incomplete) /
**ASSUMPTION** (unvalidated) / **RISK** (real problem indicated) / **UNKNOWN**.

## 1. Architecture verdict

**PASS WITH REQUIRED CHANGES.**

The load-bearing core is genuinely validated: layering enforced and clean,
one authoritative policy path shared by DNS and HTTP, bounded state
everywhere, drop-never-backpressure telemetry, rules-before-cache — all with
soak-grade on-device evidence (0.2.5 → 0.2.16-72h lineage). Nothing requires
a redesign. The required changes are: two spec-breaking upstream-transport
defects that Adaptive DNS would inherit, one deployed availability hole
(silent listener death), one ruleset-poisoning hole, and a Phase 3
measurement debt that lands exactly on the workloads where the project's own
findings say dev benches don't transfer.

## 2. Current architecture map (summary)

| Component | Owner | Hot-path access | Lifecycle | Class |
| --- | --- | --- | --- | --- |
| Compiled ruleset (`Matcher`) | `fah_rules::ListManager` | ArcSwap load | live recompile + atomic swap | VALIDATED |
| Policy snapshot (`ActivePolicies`) | binary (20 s tick + API write-through) | ArcSwap load | live | VALIDATED |
| DNS cache | `fah_dns::Pipeline` | 16 Mutex shards, dual entry+byte bound | boot | VALIDATED |
| Upstream pool | `UpstreamPool`, per-server relaxed atomics | ordered walk, no liveness state | boot | VALIDATED (defect — Adaptive's target) |
| SWR pool | Pipeline + binary-spawned workers | bounded mpsc, try_send-drop | boot | VALIDATED |
| Events | binary: one bounded mpsc 4096 → one fan-out → stats/metrics/WS | try_send, counted drops | boot | VALIDATED |
| Stats/history | `fah_stats::Stats`, warn-and-continue persistence | off hot path (fan-out only writer) | mostly boot | VALIDATED |
| Config | `fah_api::ConfigStore` ArcSwap, `BOOT_KEYS` test-enforced | — | split, test-pinned | VALIDATED |
| TLS state | API cert pair only; no CA/interception state exists | — | boot | VALIDATED (absence) |
| HTTP conns | hyper pool + permit-before-accept semaphore | streaming, no buffering | boot | VALIDATED (count-bounded; bytes unmeasured) |

Flow: client → RouterOS (dst-nat 53/80) → verdict → cache/egress-guard →
upstream; both pipelines share one `Matcher`, one `PolicyState`, one
`context_for` identity point (source IP, canonicalized once per entry), one
event channel. Libraries hold state; the binary holds clocks, ticks, and the
single spawn/abort list. Ports (`StatsSource`, `HostResolver`) keep arrows
pointing down. Layering test passes; every manifest edge is downward
(layering dossier §1).

## 3. Top architectural risks (ranked by impact)

1. **Silent DNS listener death — RISK, deployed today.** UDP/TCP receive
   loops `warn` + `return` on socket error (`fah-dns/src/udp.rs:24-31`,
   `tcp.rs:31-38`); no task supervision anywhere (`Engine.tasks` is
   abort-only, `main.rs:391-454`); healthcheck only re-parses config;
   RouterOS has no auto-restart; netwatch failover masks death as
   *unfiltered* resolution. The API accept loop already shows the
   sleep-and-continue pattern (`fah-api/src/server.rs:115-124`). Phase 3's
   new listeners would clone the fatal template.
2. **List refresh trusts any HTTP-200 body — RISK.** A wrong-but-200 body
   (CDN error page, empty file) overwrites the last-good `/data` copy and
   swaps in a near-empty ruleset; `looks_misparsed` only warns and
   `parse_errors` reaches no API (`fah-rules/src/lifecycle/mod.rs:637-654,
   1127-1135`). Network-triggered, daily, silent protection loss.
3. **Phase 3 measurement debt — RISK (budget-breaker class).** Zero bench or
   on-device number for: splice throughput (syscall-bound — the ~9× factor
   is proven not to convert it, PERFORMANCE.md:118-120), TLS interception
   CPU+memory (p3-04 has no bench in acceptance), cert mint/cache, DoT/DoH
   handshake, per-connection memory under concurrency (every HTTP figure is
   single-connection), all-cores combined load. At `max_connections=1024`,
   even 64 KiB/conn of TLS state ≈ 64 MiB — more than today's entire RSS.
4. **Encrypted upstream transport breaks the Adaptive spec — RISK.**
   (a) Pooled DoT/DoH connections never reconnect on timeout
   (`upstream/encrypted.rs:84-96`); an IPv6 prefix rotation blackholes the
   stale source, whose failure shape *is* timeout → permanent per-query
   timeout; contradicts spec S1.15 ("reconnects") and breaks S1.7 recovery
   probing (probe reuses the dead exchange → healthy endpoint pinned at
   PENALTY_MAX). (b) `map_err(io::Error::other)` at `encrypted.rs:143,154`
   erases `io::ErrorKind`, making S1.4's path-failure tier unimplementable.
5. **Policy path fail-open edges — RISK ×3.** (a) `/policies` edit window:
   new-mask matcher paired with stale `PolicyId` snapshot → affected clients
   get Pass-on-everything for the recompile window (`routes.rs:1031-1035`,
   `matcher.rs:881-886`; untested interleaving). (b) Name-based assignments
   resolve through fah-stats' evictable 4096-LRU registry — enforcement
   state in an observer crate; eviction/data loss silently drops
   restrictions. (c) `with_rules`/`with_policies` wiring is convention only
   (`proxy.rs:194,404-405`); a forgotten builder call on a future Phase 3
   listener ships an unfiltered path with no failure.
6. **Phase 3 seams are narrower than the module docs claim — RISK.**
   Retarget-to-literal-IP (the rebind closure) conflicts with upstream TLS
   verification — rustls needs the hostname as ServerName, the connector
   only sees an IP (`claim.rs:147-158`, `proxy.rs:126-160`); "pipeline
   reused unchanged" is oversold (hardcoded `LiteralConnector`, `http://`
   scheme, http1-only builder, single origin_port 80); p3-05 puts
   unauthenticated DoH on the 64-conn admin listener (shared trust boundary
   and semaphore); cert machinery (rcgen) lives in fah-api while its future
   consumers are siblings fah-http/fah-dns — must be pushed down (fah-certs
   preferred over further widening fah-common).
7. **IP-keyed interception opt-in vs rotating identity — RISK.** The most
   security-sensitive toggle (p3-04 per-client opt-in) rests on address
   stability nobody enforces; IPv6 privacy addresses rotate, DHCP leases
   reassign — silent mis-scoping in both directions.
8. **Compile transient — RISK (known, p2-12 open).** Peak ratchets to
   181–230 MiB — up to 90 % of the notional 256 MB ceiling — bounded
   empirically by allocator behavior, not architecturally; grows with list
   size, which the operator controls.
9. **Hygiene, standing:** live bearer token tracked in
   `tui-monitor/config.toml` (project-state.md:177); doc drift (`/metrics`
   and "Prometheus" references to a removed endpoint; stale root `benches/`
   layout note; stale project-state.md, last rewritten 2026-08-09,
   pre-reset).

## 4. Unvalidated assumptions (the "dual-stack discovery" class)

| # | Assumption | Exposure |
| --- | --- | --- |
| 1 | IPv6 upstream forwarding works (defaults are v4 literals; only parsing is tested — no test or deployment ever forwarded over a v6 upstream socket) | first v6 upstream configured |
| 2 | SWR/`resolve_host` queries carry no EDNS → any >512 B refresh answer pays a UDP+TCP retry on every refresh, for 69 % of upstream traffic, invisible to telemetry (`swr.rs:226-233`, `plain.rs:41`) | already live, unobservable |
| 3 | DoH bootstrap resolves via the OS resolver in a distroless container (`encrypted.rs:167-180`) — `/etc/resolv.conf` contents unverified; possible self-loop | first hostname DoH upstream |
| 4 | Every upstream serves TCP/53 on the same address (TC retries, mock-tested only); ICMP errors are visible to connected UDP in the container (S1.4 path tier rests on it) | Adaptive Stage 1 |
| 5 | `MIMALLOC_PURGE_DELAY=0` returns memory promptly above ~0.5 qps (validated only at household idle) | any load growth |
| 6 | Cache byte-cap eviction path behaves under pressure (never fired on-device; entry cap always binds first) | larger answers/config change |
| 7 | HTTP path bounds hold under concurrency (all figures single-connection; 1024-permit ceiling never exercised) | Phase 3 moves all 443 traffic here |
| 8 | The ~9× x86→RB5009 factor applies to TLS/crypto (derived on non-crypto workloads; already proven wrong for syscall-bound HTTP) | Phase 3 budgets |
| 9 | The layering guard covers the workspace — it walks only `crates/` (tui-monitor unguarded; renamed-dep and `target.*`/`build-dependencies` tables unscanned; no CI, so the gate script running is itself a process assumption) | any future edit |
| 10 | Single-writer `/data`, uid 65532 volume ownership, LAN-only perimeter (no client ACL, no rate limiting — public exposure would make port 53 an open resolver and 8443 a reachable admin surface) | deployment change |

## 5. Required changes before Adaptive DNS Stage 1

1. Fix encrypted-transport reconnect semantics (drop pooled exchange after
   N consecutive timeouts or on penalty entry) **or** amend spec S1.15/S1.7
   — as written, code and spec contradict (risk 4a).
2. Restore `io::ErrorKind` fidelity through the encrypted transport (risk
   4b) — prerequisite for S1.4's failure tiers.
3. Land the blocking measurement the spec itself names: failure run-length
   distribution (gate S1-G4 decides whether `penalty_failures=2` ever fires).
4. Add the observability Stage 1 needs to be judged: per-query upstream
   attribution (`upstream_used: bool` → which endpoint) and a
   SERVFAIL-served counter — today a client-visible failure is
   indistinguishable from an ordinary pass (`pipeline.rs:405-418`), so an
   outage's client impact — the thing Adaptive exists to reduce — cannot be
   measured before/after. Both fit `/telemetry` + events; no query-log
   revival (p2-09 stands).
5. Add the SERVFAIL-does-not-increment-failures pinning test (S1-G1 #5) and
   the 8-endpoint config cap (expected new work).

Items 1–2 are small, but skipping them re-runs the exact failure mode the
reset was for: a spec assumption the transport layer silently falsifies.

## 6. Required changes before Phase 3 reboot

1. Fix listener loop failure semantics + real healthcheck (risk 1) — and
   make the retry pattern the template Phase 3 listeners inherit.
2. Content sanity gate on list refresh before overwriting last-good copy;
   expose `parse_errors` (risk 2).
3. Decide cert machinery home (fah-certs) *before* p3-01 code exists (ADR).
4. Redesign the retarget/connector contract for upstream TLS (hostname must
   reach the connector; the literal-IP rebind closure survives in a
   different shape) — before p3-04, not during.
5. Decide DoH listener placement (separate listener or per-path limits) —
   before p3-05.
6. Bind interception opt-in to stable identifiers, not bare IPs (risk 7).
7. Measure first (see §8): TLS handshake on-device, per-connection memory
   under TLS-loaded concurrency, splice probe. These gate the budgets p3-06
   would otherwise discover late.
8. Close the policy fail-open edges (risk 5): order the snapshot republish
   with the matcher swap (or make stale-index fail-closed), move name-based
   assignment off the evictable registry, and make rules/policy wiring
   compile-enforced (or test-enforced per listener) before new listeners
   multiply the convention.
9. Extend `EventKind` (`https-sni`, transport dimension) deliberately — L1 +
   API.md in one change — rather than shoehorning into `Http` later.

## 7. Architectural invariants (must not be violated)

- Rule Engine before cache; cache stores upstream answers only, never
  verdicts (ADR-0001).
- Verdict before resolve/connect: a blocked request costs no lookup and no
  upstream bytes (DNS and HTTP both).
- Egress default-deny judges the *resolved* address; connector never
  resolves again.
- One matcher, one policy snapshot, one client-identity construction point
  (`context_for`) for every pipeline, present and future; typed entry points
  per request model, never per transport (no `lookup_https()`; SNI reuses
  the domain index).
- Bounded everything: every queue try_send-drops with a counted shed, never
  back-pressures the pipeline; every cache/registry has entry *and* byte
  awareness where bytes vary (dual-bound lesson, p1.5-05 — applies to the
  future cert cache and per-connection state).
- RCODE is a transport success; only transport errors are health evidence.
  Adaptive addition: the selector always returns a candidate.
- Libraries hold pure state; the binary owns clocks, timers, tasks, I/O,
  wiring; one spawn/abort list.
- Dependencies point strictly downward; siblings communicate only through
  the binary's channels/ports.
- Boot-vs-live config classification stays test-enforced against actual
  consumers; every key ships a production default.
- Never terminate on the splice path, never splice after terminating; a
  client not opted in can never be intercepted.
- Memory is monitored, not runtime-enforced (deliberate — the memory-high
  incident); the soak cadence is therefore load-bearing infrastructure.

## 8. Measurement gaps (before implementation)

| Gap | Why it can't wait | Method |
| --- | --- | --- |
| Failure run-length distribution | gates S1-G4 | current-build telemetry addition (spec companion doc) |
| Three in-engine p99 rows (block/cache_hit/forward) on RB5009 | budget table claims device verification it doesn't have | on-device probe |
| rustls handshake cost on RB5009 (aws-lc-rs, ARMv8) | 9× factor unproven for crypto; sizes p3-03/04 budgets | micro-bench probe container, before p3-03 |
| Splice throughput | carries every HTTPS byte; 9× proven not to convert syscall work | probe container (slowest measurement type — start early) |
| Per-connection memory, TLS-loaded, at concurrency | 1024 × TLS state can exceed today's entire RSS | probe + counted profile |
| All-cores combined load (DNS+HTTP+splice) | every existing figure is single-subsystem | on-device |
| `PURGE_DELAY=0` above idle qps | falsification signal named, never exercised | load soak |
| Cert-mint latency + cert-cache bound | unbudgeted, unbounded in any plan | design + bench in p3-01 |

## 9. ADR recommendations (do not write yet)

1. One-matcher / typed-entry-point interface (the un-promoted phase2 note —
   overdue; Phase 3 designs must fit it or overturn it explicitly).
2. Certificate machinery home (fah-certs vs fah-common) — decide before
   p3-01.
3. Client identity = canonicalized source IP: its limits (NAT/DHCP/IPv6
   privacy rotation) and what interception opt-in may bind to.
4. Listener failure semantics and supervision policy (retry-in-loop vs
   supervised respawn vs fail-whole-process) — the current abort-only model
   is an undocumented decision with the worst failure path in the system.
5. DoH/DoT listener placement relative to the admin surface.

## 10. Go / no-go

- **Adaptive DNS Stage 1: GO** once §5 items 1–3 are resolved (1–2 are
  small; 3 is already the spec's own gate). Item 4 should land with Stage 1
  — without it the before/after cannot be judged. The selection seam
  (`mod.rs:184` walk, `Arc<[UpstreamServer]>` atomics pattern) is ready.
- **Phase 3 reboot after Adaptive: GO**, conditional on §6 — the decisions
  (1–6, 8–9) are cheap now and expensive mid-p3-04; the measurements (7)
  are the direct lesson of the reset.
- **More architecture work first: NO** — no redesign is warranted. The
  architecture is the right foundation; what it needs is the listed seam
  decisions, two transport fixes, and the measurement debt paid *before*
  the phases that depend on it.
