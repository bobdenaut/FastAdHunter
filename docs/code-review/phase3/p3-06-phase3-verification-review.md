# P3-06 — Phase 3 Verification — Review

**Task:** `plan/wip/phase3/p3-06-phase3-verification.md` · **Plan:**
`p3-06-phase3-verification-plan.md` · **Status:** **campaign 2 planned, not
executed.** The three p3-06 plans are rewritten for the post-merge tip and
committed; campaign 1's figures are superseded in full and nothing is carried.
No campaign-2 step has run — Steps 1–3 owe a re-run plus three coverage
additions, Step 4 is not started, Step 5 is a list. Phase row `AWAITING SOAK`;
flip condition and per-step state in §Hand-off state, 2026-09-08.

## Pre-declaration (written 2026-09-02, before any measurement ran)

Binding per `docs/measurement-traps.md` and the phase-2.6 lesson. This block
is never edited after the first run; a declaration that had to change is
recorded beneath it with the reason, and the original stays.

**Pre-phase-3 checkout:** `64be513` (`fix(dashboard/live-feed): let every
wide column wrap`), the last commit before `3337aab` (p3-01). Built in a
detached worktree at `../FastAdHunter-pre3`; its own `target/`.
**Post:** the `phase3-06` tip as it stands when each arm runs (hash recorded
beside each table).

**Device (dev box):** x86_64, Windows 11, Windows heap (not musl `mallocng`),
idle machine required; a delta on the control arm outside ±5 % invalidates the
session. **Pinning:** CPU-bound microbenches pinned to one core
(`ProcessorAffinity = 4`, `High`); `fah-http/benches/*` unpinned, range across
runs (PERFORMANCE.md §Measuring reliably). **Control arm:**
`http_pass_through/direct_to_origin` — the proxy is not on that path.

| # | Arm | Workload / corpus | Sample size | Comparison |
| --- | --- | --- | --- | --- |
| D1 | `http_pass_through/{direct_to_origin,through_proxy}` | small fixed payload, warm keep-alive, loopback | criterion default (100) | `64be513` vs tip, **A/B/A/B** (2 pairs), unpinned |
| D2 | `http_opaque_body/{direct,proxy}` × 8 KiB / 1 MiB / 8 MiB | opaque body relay, loopback | 20 per arm | as D1 |
| D3 | `dns_cache/cache_hit_in_engine_latency` | one warm A record, decode → hit → encode | 100 | `64be513` vs tip, A/B/A/B, pinned |
| D4 | `fastadhunter` `pipeline` `blocked_query`, `forwarded_query_overhead` | 1 M-domain synthetic blocklist, mock forwarder | 100 | as D3 |
| D5 | `matcher_lookup/{hit_exact,hit_subdomain,miss}` | bench's own synthetic domain corpus | 100 | as D3 |
| D6 | `https_sni_splice/{direct_to_origin,through_splice}` **per-connection** | raw TCP origin, 1 MiB per connection, synthetic ClientHello `origin.test`, harness rebuilt on `TlsServer::bind/serve` (n5) | 20 | tip only (new in phase 3); `SPLICE_BUF` 16 KiB vs 64 KiB as **two builds of the tip**, A/B/A/B, unpinned |
| D7 | `https_sni_splice/{direct,splice}_steady_state` | one connection, 64 MiB, same origin | 10 | as D6 |
| D8 | new `fah-http/benches/intercept.rs` `https_handshake/{direct_to_origin,spliced,intercepted}` | TLS origin (rcgen test CA, h1), fresh connection per iteration, GET `/` with a 1 KiB body, client trusts the origin CA (direct, spliced) or the FAH CA (intercepted) | 50 | tip only; `direct` is the in-bench control |
| D9 | `intercept.rs` `https_h2_download/{spliced,intercepted}` 8 MiB | h2 origin, one connection, one GET of 8 MiB, shipped `H2_*` limits | 20 | tip only; memory figure for the 32 × 32 KiB alternative is arithmetic, not a reading |
| D10 | `intercept.rs` `prewarm_hop/{inline_cached_leaf,spawn_blocking_prewarm}` | warm host, multi-thread runtime | 100 | tip only (p3-04 N8 diagnostic) |
| D11 | `certs_mint`, `certs_cache_hit`, `certs_prewarm_warm` | p3-01 bench as shipped | 100 | tip only, pinned; absolute |
| D12 | `certs_replay_zipf` (new arm) hit rate | **synthetic** Zipf(s = 1.0) over 4 096 hosts, 100 000 handshakes, LRU 512 — not a real session; a real-session replay is TBD from the soak's `https-sni` feed | one deterministic replay (seed fixed) | tip only; reports `prewarm_hits / handshakes` |
| D13 | `encrypted_latency` UDP / DoT / DoH | release binary, loopback, blocked domain, in-engine; **3 rounds × 2 000 queries per transport, transports interleaved per round** | 6 000 per transport | tip only; UDP is the in-run control; ×9 conversion **not** applied to TLS legs |
| D14 | dev-box RSS, full mode | `process_rss` from `/api/v1/debug/memory` after the offline full-mode e2e traffic | one reading | diagnostic only (Windows heap, debug build) |

**On-device (probe container, owner-executed) — declared here, results TBD:**

| # | Arm | Workload | Sample size |
| --- | --- | --- | --- |
| P1 | splice throughput, `SPLICE_BUF` 16 vs 64 KiB | probe container on `veth3`, LAN host as client and origin, 64 MiB per connection, one connection | 5 runs per build, median + range |
| P2 | TLS handshake cost: spliced vs intercepted vs direct | LAN client to a LAN TLS origin through the probe, 200 fresh connections per arm | min / p50 / p99 |
| P3 | intercepted h2 throughput + per-session RSS under a 64-stream stall | 8 MiB download; 64 stalled streams from one slow client | 3 runs; RSS from `/api/v1/debug/memory` before/during/after |
| P4 | DoT / DoH / UDP per-query latency | LAN client, 2 000 queries per transport, 3 interleaved rounds | 6 000 per transport |
| P5 | leaf mint cost, `spawn_blocking(prewarm)` hop | 512 first-sight hosts over DoT; `minted_total` delta / wall time | one replay |
| P6 | CA generate / API pair import wall time | `POST …/ca/generate` and `POST …/import` on the probe, 5 each | min / median |
| P7 | 24 h soak, production container, `dns+http+https` | household traffic; watch items in §Runbook item 6 | hourly `/telemetry`, `/certificates`, `/debug/memory` pulls |

**Declaration changes, with reasons (the block above is unedited):**

- D8/D9/D10 first run (session `18:13–18:25`) crashed on the `spliced`
  arm: the harness kept every keep-alive connection of the `direct` arm open
  (thousands of sockets), and the next arm's upstream connect got EOF. Fixed
  in the harness (`Connection: close` per request, 10 s measurement time),
  rerun as two rounds at `18:31–18:35`; three **isolated single-arm runs**
  added as a labelled supplement (`isolated-handshake.out.txt`).
- D11/D12 ran after the main session (the script passed an empty filter
  argument; rerun with none), pinned as declared.
- D14 read from the OS (`tasklist`), not `/debug/memory` (X4).
- **P2 (2026-09-02, [phase3-audit](phase3-audit.md) §Fixes applied 3):** the
  declared arm stands — a LAN client through the **probe FAH instance** (the
  real binary), 200 fresh connections per arm, min / p50 / p99 — and Runbook
  5's cross-compiled `intercept` criterion binary is **not** P2: it has no
  verdict, no DNS resolution, no veth hop, and its loopback origin cannot be
  verified by a release build. One change to the declaration: the origin is
  **one fixed public HTTPS origin**, not "a LAN TLS origin" — the intercepted
  arm verifies the origin against the compiled-in webpki roots and the probe
  build has no `test-harness` root hook, so a LAN origin under a private CA
  can never pass that arm (526). The three arms interleave per round so WAN
  RTT drift lands on all of them. P1 and P3 carry the same declared-LAN vs
  runbook-loopback split; **not resolved here — owner decision**.
- **D6–D9 harness (2026-09-02, same audit, items 1–2):** both `fah-http`
  benches gained a compiled ruleset (domain and URL rules, none matching the
  bench origin), the default `PolicyState` and a drained event channel, so the
  SNI verdict, the URL tier and `publish` run on every connection and request.
  The figures in §Measurements and §Post-review work C were taken **without**
  them; the missing term is the D5 / `url_matcher` cost (ns) against ms-scale
  arms. Each bench prints its `ProxyCounters` after the group as the proof the
  verdict path ran. The resolver stays fixed: the production resolver is
  `fah-dns`, an L3 sibling no `fah-http` target may import (`layering.rs`
  checks `dev-dependencies`); the resolve leg is measured only by P2 through
  the real binary.
- **P4 (2026-09-03, [p3-06-measurement-audit](p3-06-measurement-audit.md)
  MA-8):** the acceptance measurement is the `encrypted_latency` harness
  run **in-device** from the probe image (release binary, reused DoT
  connection, DoH keep-alive, blocked domain ⇒ in-engine, 3 × 2 000) — the
  declared "LAN client" run becomes **P4-LAN**, a diagnostic with `kdig
  +keepopen`. Reason: the row is defined in-engine on a reused connection
  (PERFORMANCE.md, D13); a per-invocation `kdig` pays a handshake per query
  and reads at 0.1 ms resolution, neither of which the row can absorb.
- **P6 (MA-9):** the row is `time_starttransfer − time_appconnect` (server
  side, handshake excluded); `time_total` recorded beside it as the
  handshake-inclusive diagnostic. The declared "wall time" did not say which.
- **P3 / P7 (MA-10):** P7's RSS gate is household browsing; it cannot reach
  the 64-stream stall. P3 is the sole authority for the intercepted-session
  memory ceiling; a passing P7 makes no claim about it.
- **P8 (MA-11):** "interception CPU under browsing" (plan §Step 4.5) is
  **withdrawn as an acceptance claim** — per-leg CPU is not separable on the
  RB5009 (`/tool/profile` keys on process name; the telemetry `process`
  block carries version and uptime only). Replaced by P8, a full-mode CPU
  diagnostic: `/tool/profile cpu=all` spot reads on the soak deploy against
  the 0.3.1 `dns+http` container (Runbook 6). The terminate leg's CPU cost
  stays bounded by P2's intercepted arm and the D8/D9 on-device diagnostics.
- **P9 (MA-6):** the A/B filter left the `startup` group out; running it
  now would show nothing by construction — `startup_from_cached_lists` is
  `ListManager::new` + `boot()`, and Phase 3 touched neither (`matcher.rs`
  gained two lookup wrappers, no representation change). The Phase 3 startup
  delta lives in `Engine::start` (`CertStore::open`, DoT + HTTPS binds,
  `dot_tls`) and is visible only in the binary: P9 reads it from the
  container log on the device (Runbook 6), against the "< 3 s hard" row. The
  `steady_state_memory` arm the bench header names **does not exist** in the
  file; the "RAM steady-state, 1 M loaded" row has always been a soak
  figure (PERFORMANCE.md: 46.6–53.6 MiB `dns+http`) and P7 re-affirms it for
  full mode with the same deployed ruleset. Neither row is inside the
  dev-box regression claim; both are covered on the device.
- **D12 (MA-7):** the synthetic Zipf hit rate is struck from the proposed
  hit-rate row; the soak's `leaf_cache` counters are the only evidence.
- **P4 image uid (2026-09-03, testing-plan delta 10):** `Dockerfile.p4`
  runs as `65532:65532` with `/tmp` owned by that uid, not the probe-image
  `USER 0:0` convention. The harness's `tempfile` volumes are 0700 to the
  harness uid and the spawned binary drops to 65532 before its first-boot
  writes, so a root harness yields a predictable `EACCES` and no figure. As
  65532 the binary performs no drop and binds ephemeral loopback ports; the
  per-query latency the row measures never includes the drop. Recorded
  before the first P4 run.
- **Origin-failure-rate budget (2026-09-03, testing-plan delta 11):** the
  plan's "within budget, else `degraded`" rule carried no number. For
  P1-LAN, P2 (rows per arm) and the P3 throughput arm the budget is **2 %**
  of a stage's samples (`--max-fail-pct` default 2): zero completed samples
  ⇒ `INVALID`, at or under 2 % ⇒ `valid` from the completed samples, above
  ⇒ `degraded`. P2's issuer rules stay `INVALID` conditions outside this
  budget. Gate statistics, quantities and counts unchanged.
- **P4 DoH protocol (2026-09-03, testing-plan delta 12):** the harness's DoH
  arm runs over h2 (`reqwest` dev-dependency feature `http2`, every response
  asserted `HTTP/2.0`), the protocol P4-LAN and real DoH clients speak, so
  P4 and P4-LAN describe one protocol on that transport (smoke findings
  F17 / F26). D13's HTTP/1.1 seed is not the comparator for the DoH column;
  UDP and DoT columns, gate statistic and counts unchanged.
- **SNI invalidity rules and gate term (2026-09-03, testing-plan delta 13):**
  `p0-sni.mjs` requires the allowed name to reach ServerHello and
  `listeners.https.blocked` to move by the blocked-attempt count, else
  `INVALID` (smoke F18: a resolve-failure close is not an SNI verdict); the
  gate boolean covers blocked and no-SNI attempts, both closed before any
  certificate. Close latency stays diagnostic; the gate statistic is
  unchanged in kind.
- **P3 origin (2026-09-03, testing-plan delta 14):** the "public h2 origin"
  becomes an h2 origin on the second LAN endpoint under a public name with a
  publicly trusted certificate (Let's Encrypt DNS-01; the release probe
  trusts `webpki-roots` only), served by `smoke/h2-origin.mjs --bytes 8`.
  Reasons: no third-party 64 × 8 MiB burst; LAN bandwidth, without which the
  ≥ 50 MiB/s throughput gate reads the WAN link. Preflight
  (`smoke/h2-preflight.mjs`: HEAD + GET over h2, `PASS` on ALPN h2, 200,
  `content-length` and body of exactly 8 MiB) saved beside the results.
  Barrier, exclusive-window proof, gate statistics and counts unchanged.
- **P1 firewall scoping (2026-09-05, testing-plan delta 15):** the origin host's
  inbound allow on TCP 443 is scoped to **`192.168.10.1`**, not to the probe's
  `172.17.0.4` as the plan's Local firewall table declares. srcnat rule 1 on the
  device is `action=masquerade src-address=172.17.0.0/24` with no
  `out-interface` restriction, so probe traffic to a LAN host arrives from the
  router's LAN address; the declared rule matches nothing and the connection is
  dropped silently. Verified 2026-09-04 from `/ip/firewall/nat print`. LAN →
  container is not masqueraded, so P2's identity precondition and P1-control's
  LAN → LAN path are unaffected. Gate statistics, quantities and counts
  unchanged.

## Pre-declaration — campaign 2 (written 2026-09-08, before any measurement ran)

The block above is campaign 1's and stays unedited. It described measurements
taken on images built at `a2d0802`, which predates merge `e0c6071`; those
figures are superseded in full (header of
[p3-06-testing-results.md](p3-06-testing-results.md)). Campaign 2 re-runs every
arm from zero — owner decision 2026-09-08, nothing carried.

**Binding declaration:**
[p3-06-testing-plan.md](../../../plan/wip/phase3/p3-06-testing-plan.md) — arms,
workloads, sample sizes, invalidity rules and gate statistics live there and
are not restated here. This section carries the campaign-2 declaration changes
only, per that plan's §Declaration deltas.

**What changed in the declaration frame:**

| Item | Campaign 1 | Campaign 2 |
| --- | --- | --- |
| Pre-phase-3 A/B checkout | `64be513`, which also predates the allocation domains | `main` at `857865d` (0.3.3) — same execution model, so the delta isolates Phase 3 (verification plan §Step 1) |
| Execution model | HTTPS on the base runtime | HTTPS on N `current_thread` allocation domains |
| N | not a config key | `runtime.http_runtimes` recorded on **every** HTTPS figure; a figure with no N is diagnostic. Arms run at N = 2 unless the arm sweeps it |
| Results file | `p3-06-testing-results.md` | `p3-06-testing-results-2.md`, created when the first arm produces a figure |

**Declaration changes (recorded before the arm runs; the campaign-1 block above
is unedited):**

1. **P1-LAN's absolute gate withdrawn — owner decision outstanding.** Campaign
   1 declared "≥ 100 MiB/s steady state" from gigabit link speed. Campaign 2
   gates P1-LAN **relative to P1-control** (median ≥ 0.9 × control median),
   absolute MiB/s recorded as a diagnostic. Reason: the phase-2.6 sweep
   measured this device's LAN-through-router ceiling at 58–60 MiB/s single
   stream and 67–70 par8, at every N, with a 380 MB/s origin
   ([alloc-domains-n-sweep.md](../phase2.6/alloc-domains-n-sweep.md)), so the
   declared gate is unreachable by topology. Reinstating an absolute row needs
   a host on the far side of the router — separate decision.
2. **P10 inherits the phase-2.6 rig rather than declaring a new one.**
   Campaign 1 had no N arm. P10's arms, statistics (cores from the container's
   own CPU counters, ΔRSS against the arm-local floor), sampling cadence and
   client discipline are `alloc-domains-n-sweep.md`'s, so the two tables are
   comparable. New: the TLS arms and N = 1. N = 3 is deferred, not dropped.
3. **`oha` 1.16.0 adopted for the arms it reproduces exactly.** It drives
   close-mode HTTP, both TLS connection-rate arms, the P3 throughput arm and
   the transfer arms; `p10-connrate.mjs` keeps the keep-alive arm alone and
   `p10-dnsload.mjs` is ported in full, because `oha` expresses neither
   requests-per-connection nor DNS. **No declared quantity changes** — where
   `oha` would change one (P1's exclusion of setup, P2's bound source
   addresses and handshake p50, P3's stall barrier, P5/SNI's raw TLS work,
   P6's curl phase timings) it is not used, or it runs beside the row labelled
   a cross-check. Version and `--worker-threads` pinned and recorded;
   `--connect-to`'s SNI behaviour is a smoke prerequisite (smoke plan Layer 0).
4. **P2's measured quantity is renamed, not changed.** `p2-handshake.mjs`
   computes `secureConnect − connect`. On the `direct` arm that is a TLS
   handshake. On `spliced` and `intercepted` it is **proxy setup + relayed
   handshake**: ClientHello read, SNI verdict, one **uncached upstream A + AAAA
   resolve** (`https.rs:180` → `upstream/mod.rs:142`, `tokio::join!`, no cache
   by design), egress check, upstream TCP connect, then the handshake — the
   upstream leg's own TLS handshake as well on `intercepted`. Same defect the
   D8 curl figure carried (§Post-review work F, F1); caught here **before** the
   arm runs. Consequences: the ratio gate `intercepted p50 ≤ 2 × spliced p50`
   is **unaffected**, since both arms pay the setup and it cancels; the row
   value `spliced p50 − direct p50` **is** affected and must ship named "SNI
   verdict + splice + one upstream resolve, added per connection". No threshold
   moves and no sample size changes. On the device that resolve is real
   per-connection production cost, so it belongs in the figure — it must not be
   called a handshake.
5. **P2 additionally records the pre-relay split of the `spliced` arm, from
   shipping telemetry.** `https.rs:216` captures `started.elapsed()` after the
   upstream connect and before the relay, and it reaches the wire as
   `duration_ms` on the `https-sni` event. `p2-handshake.mjs` subscribes to
   `WS /api/v1/events` for the run and reports that distribution beside the
   socket-side figure; `spliced p50 − pre-relay p50` is the relayed-handshake
   remainder. **The `spliced` arm only** — `intercept.rs` calls `emit_session`
   on failure paths exclusively, so a successful intercepted session emits no
   pre-relay event, and the `direct` arm never reaches the probe. Rows are
   attributed by client address, which the identity precondition already
   guarantees is unique per arm. **Diagnostic, not a gate and not a row** — it
   adds no arm, changes no threshold and no sample size, and a socket that
   fails to open, drops, or lags degrades to the declared figure alone rather
   than invalidating the run. It exists to settle F2/F3: whether the splice
   arm's extra time is pre-relay (resolve + connect) or inside the relayed
   handshake.

## Measurements

Dev box: x86_64 Windows 11, idle apart from this session; **A** = `64be513`
(pre-phase-3), **B** = `877aad2` + the p3-06 working tree. Raw criterion
output per run in `p3-06-bench/`. Figures are criterion means with the 95 %
interval `[lo hi]`; two interleaved pairs (r1, r2). Control arm
`http_pass_through/direct_to_origin`: A 32.9 → 31.0 µs, B 32.2 → 31.5 µs
between rounds — the session's noise band is ≈ ±6 %, no untouched arm moved
more than 10 %.

**Regression verdict (>10 % rule, existing DNS/HTTP benches): none.** Every
B arm sits inside A's round-to-round spread.

| D1 `http_pass_through` (µs) | A r1 | B r1 | A r2 | B r2 | A mean | B mean | Δ |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `direct_to_origin` (control) | 32.92 [32.24 33.94] | 32.15 [31.93 32.41] | 30.96 [30.76 31.18] | 31.52 [31.22 31.87] | 31.94 | 31.84 | −0.3 % |
| `through_proxy` | 66.50 [65.48 67.74] | 65.52 [64.97 66.13] | 63.36 [63.02 63.73] | 65.17 [64.41 66.07] | 64.93 | 65.35 | +0.6 % |
| added (proxy − direct) | 33.6 | 33.4 | 32.4 | 33.7 | 33.0 | 33.5 | +1.5 % |

| D2 `http_opaque_body` (mean thrpt, MiB/s unless GiB/s) | A r1 | B r1 | A r2 | B r2 |
| --- | --- | --- | --- | --- |
| 8 KiB direct | 218.6 | 228.4 | 230.0 | 229.2 |
| 8 KiB proxy | 111.4 | 112.7 | 116.8 | 114.1 |
| 1 MiB direct | 1.076 GiB/s | 978 | 1.437 GiB/s | 1.360 GiB/s |
| 1 MiB proxy | 1.127 GiB/s | 1.079 GiB/s | 1.225 GiB/s | 1.204 GiB/s |
| 8 MiB direct | 1.340 GiB/s | 1.336 GiB/s | 1.483 GiB/s | 1.379 GiB/s |
| 8 MiB proxy | 1.122 GiB/s | 1.185 GiB/s | 1.227 GiB/s | 1.250 GiB/s |

The 1 MiB direct control moved +10 % on means between A and B with fully
overlapping ranges — that bounds this table's resolution at ~10 %. B's 8 MiB
proxy arm is faster than A's in both pairs (relay overhead 0.74 / 0.58 ms vs
1.14 / 1.10 ms); recorded, not claimed (below the resolution).

| D3–D5 pinned microbenches | A r1 | B r1 | A r2 | B r2 | A mean | B mean | Δ |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `dns_cache/cache_hit_in_engine_latency` (µs) | 2.656 [2.42 2.89] | 2.640 [2.33 2.95] | 2.408 [2.18 2.62] | 2.755 [2.50 3.01] | 2.53 | 2.70 | +6.5 % — intervals ±9 % even pinned; overlapping |
| `full_pipeline/blocked_query` (µs) | 3.363 [2.96 3.77] | 3.265 [2.88 3.64] | 3.623 [3.21 4.03] | 3.083 [2.72 3.43] | 3.49 | 3.17 | −9 % (±12 % intervals) |
| `full_pipeline/forwarded_query_overhead` (µs) | 4.068 [3.57 4.54] | 4.204 [3.76 4.63] | 4.684 [4.18 5.20] | 4.394 [3.92 4.85] | 4.38 | 4.30 | −1.8 % |
| `matcher_lookup/hit_exact` (ns) | 63.90 | 63.58 | 64.10 | 64.66 | 64.00 | 64.12 | +0.2 % |
| `matcher_lookup/hit_subdomain` (ns) | 194.0 | 193.1 | 195.3 | 188.2 | 194.7 | 190.6 | −2.1 % |
| `matcher_lookup/miss` (ns) | 47.39 | 47.70 | 47.15 | 48.96 | 47.27 | 48.33 | +2.2 % (placement class; CIs < 1 %) |

D4 note: both arms built with `debug-assertions = true` in the bench profile
(X2). The tokio-runtime benches (D3, D4) held ±9–12 % intervals pinned to
one core — not the sub-1 % the µs benches show; the A/B is still resolved
within the 10 % rule by interleaving, not by interval width.

| D6 `https_sni_splice`, 1 MiB per connection (thrpt mean [range]) | 16 KiB r1 | 16 KiB r2 | 64 KiB r1 | 64 KiB r2 |
| --- | --- | --- | --- | --- |
| `direct_to_origin` | 1.209 GiB/s [1.02 1.45] | 1.335 GiB/s [1.14 1.58] | 1.502 GiB/s [1.30 1.75] | 1.128 GiB/s [0.88 1.52] |
| `through_splice` | 139.3 MiB/s [113 180] | 178.9 MiB/s [147 227] | 97.2 MiB/s [87.7 108.6] | 98.4 MiB/s [83.1 119.6] |

| D7 `https_sni_splice_steady_state`, one connection, 64 MiB | 16 KiB r1 | 16 KiB r2 | 64 KiB r1 | 64 KiB r2 |
| --- | --- | --- | --- | --- |
| `direct_to_origin` | 2.840 GiB/s | 2.556 GiB/s | 2.891 GiB/s | 2.837 GiB/s |
| `through_splice` | 1009.6 MiB/s [887 1118] | 931.4 MiB/s [825 1043] | 1.602 GiB/s [1.50 1.73] | 1.479 GiB/s [1.42 1.59] |

Harness now runs the shipped `TlsServer::bind/serve` accept loop (permit +
`TCP_NODELAY`), so p3-03 n5 is closed and these supersede the p3-03 figures.
Reading (loopback, dev box): per connection the splice is 7–9× under direct
(p3-03 M4 confirmed — connect + hello parse + upstream connect + teardown
dominate a 1 MiB transfer); **steady-state** the 16 KiB splice reaches
~0.97 GiB/s, 2.7× under direct, and 64 KiB buffers lift it to ~1.54 GiB/s
(+59 %, both pairs) while costing more per connection (97 vs 139–179 MiB/s,
two 64 KiB allocations per session). Memory axis: `2 × SPLICE_BUF ×
max_connections` = 32 MiB at 16 KiB, **128 MiB at 64 KiB** at the default
1024 — the whole RAM budget at saturation. Decision deferred to P1 on the
device; the p3-03 25 MiB/s worry does not survive steady-state even at 16 KiB
on this box, but loopback figures do not convert (PERFORMANCE.md §Converting).

| D8 `https_handshake` — fresh connection, connection setup through to handshake complete, h1 GET 1 KiB, close (ms) | r1 | r2 | isolated single-arm run |
| --- | --- | --- | --- |
| `direct_to_origin` (control) | 1.130 [1.06 1.21] | 0.983 [0.95 1.01] | 1.141 [1.07 1.22] |
| `spliced` | 5.666 [4.10 7.28] | 5.986 [4.58 7.33] | 4.892 [3.92 5.93] |
| `intercepted` (leaf cache warm: `minted_total = 1`, `unwarmed_misses = 0`) | 2.726 [2.36 3.16] | 4.824 [3.73 6.02] | 4.304 [3.07 5.75] |

The `direct_to_origin` row is a TLS handshake. The `spliced` and `intercepted`
rows are **proxy setup + relayed handshake** and must not be quoted as handshake
figures (§Post-review work F, F1).

Reading: on this box a proxied connection costs ~+3.5 ms per connection
**whichever leg**; splice and terminate are within each other's (wide)
intervals, so the TLS termination itself is not what the dev box resolves —
the socket/task path is. Diagnostic only; P2 on-device decides. Harness
unchanged after review: the F6 `iter_custom` variant measured 3–6× slower
under every pin, unpinned included, and was reverted (§Post-review work C,
which also holds the pinned-four-core figures for these arms).

| D9 `https_h2_download`, one h2 session, 8 MiB per GET, shipped `H2_*` limits | r1 | r2 |
| --- | --- | --- |
| `direct_to_origin` | 1.202 GiB/s (6.50 ms) | 1.160 GiB/s (6.73 ms) |
| `spliced` | 906.7 MiB/s (8.82 ms) | 903.7 MiB/s (8.85 ms) |
| `intercepted` | 484.9 MiB/s (16.50 ms) | 481.3 MiB/s (16.62 ms) |

Intercepted h2 relays at ~0.53× the splice on this box (decrypt + hyper + re-encrypt
twice). Memory of the shipped limits per stalled session ≈ 5.5 MiB
(CONFIGURATION.md); the named alternative 32 × 32 KiB ≈ 1.5 MiB is
arithmetic, not a reading — P3 measures the stall on-device. No edit.

| D10 `prewarm_hop` (p3-04 N8, p3-05 N1/N11) | r1 | r2 |
| --- | --- | --- |
| `inline_cached_leaf` | 91.7 ns | 92.0 ns |
| `inline_prewarm_warm` | 118.8 ns | 117.7 ns |
| `spawn_blocking_prewarm` (warm host) | 4.19 µs | 4.17 µs |

The blocking-pool hop costs ≈ 4 µs per handshake against a ≥ 1 ms handshake
(0.1–0.4 % here; by the factor ≈ 38 µs against a multi-millisecond on-device
handshake — *inference*). Recommendation: **leave `dot.rs` / `intercept.rs`
alone** unless P5 on the device contradicts this; N1/N11 stay deferred.
The `spawn_blocking` arm measures a **warm** blocking pool (the pool thread
exists after the first iteration); the cold-pool thread spawn N8 named is not
in this figure and is paid once per idle pool, not per handshake (F7).

| D11 `fah-certs` (pinned) | r1 | r2 | p3-01 |
| --- | --- | --- | --- |
| `certs_mint` — cold `prewarm`, whole path incl. eviction scan | 53.64 µs | 53.39 µs | 55.56 µs |
| `certs_cache_hit` | 48.78 ns | 48.20 ns | 62.96 ns |
| `certs_prewarm_warm` | 75.45 ns | 75.14 ns | — |
| `certs_replay_zipf` steady state (32.7 % misses) | 17.50 µs | 17.60 µs | — |

D12 (synthetic): 100 000 handshakes, Zipf(s = 1) over 4 096 hosts, LRU 512 —
`prewarm_hits` 67 323, `minted_total` 32 677, `evictions` 32 165, **hit rate
0.673**, deterministic (seed fixed, both runs identical). A real household
session has far fewer distinct hosts per 7-day leaf lifetime than this
distribution assumes; the soak's `https-sni` feed supplies the real replay.

D13 `encrypted_latency`, release binary (`FAH_E2E_BINARY`), loopback, blocked
domain, in-engine, 3 interleaved rounds × 2 000 per transport (per-round p50
udp 28/28/28, dot 46/45/45, doh 158/157/159 µs — no drift):

| Transport | min | p50 | p90 | p99 | max | added vs UDP (p50) |
| --- | --- | --- | --- | --- | --- | --- |
| UDP/53 (control) | 23 µs | 28 µs | 31 µs | 80 µs | 218 µs | — |
| DoT, one reused connection | 42 µs | 45 µs | 54 µs | 111 µs | 540 µs | +17 µs |
| DoH POST, HTTP/1.1 keep-alive | 137 µs | 158 µs | 227 µs | 312 µs | 2 371 µs | +130 µs |

DoT handshakes 1.82 / 1.97 / 1.80 ms (excluded). Reproduces p3-05's seed
(+17 / +132 µs) on the promoted harness. **Not converted**: TLS/HTTP legs do
not take the ×9 factor; P4 on-device supplies the budget row.

D14: `full_mode_blocks_at_every_layer` binary after the scenario — **46.1 MiB**
working set (Windows `tasklist`, test build, Windows heap, rules = two user
rules, CA + one minted leaf). Diagnostic only; the ≤ 128 MB row is
re-affirmed by P7.

### On-device campaign, 2026-09-04 — figures live in `p3-06-testing-results.md`

Everything above this line is x86. The first on-device figures for this task
were taken on 2026-09-04 against `a2d0802` on the RB5009, driven from bobdenaut;
they are recorded in
[p3-06-testing-results.md](p3-06-testing-results.md), one section per
measurement ID, and are **not** duplicated here.

| Section | Result |
| --- | --- |
| [SNI](p3-06-testing-results.md#sni) | pass — every blocked and no-SNI attempt closed before a certificate |
| [P1-loopback](p3-06-testing-results.md#p1-loopback) | no pick; the CPU axis is missing (the sweep outran no profile window). **Buffer decision taken 2026-09-05: `SPLICE_BUF` stays 16 KiB per direction, budget stays 32 MiB, `max_connections` unmoved** — no CPU-per-relayed-byte advantage is established for a larger buffer. That is the shipped configuration only; **P1-LAN still owes the ≥ 100 MiB/s confirmation** and stays parked |
| [P4](p3-06-testing-results.md#p4) | **row-setter withdrawn 2026-09-05.** Three valid in-device sessions; the declared `p50(transport) − p50(UDP)` statistic is not robust — the UDP control moved 101 / 175 / 148 µs and carried DoT added from +61 to +17 / +16 µs. DoT and DoH rows return to `TBD` |
| [P4-reruns](p3-06-testing-results.md#p4-reruns--diagnostic-session-to-session-stability-of-the-p4-statistic) | diagnostic — the two reruns that withdrew the row-setter, and the failed CPU attribution. Transport paths healthy: both beat the ×9 prediction (DoT 162–192 µs vs ≈ 405 predicted, DoH 1 016–1 044 µs vs ≈ 1 422); DoH is the most stable quantity in the set at ~3 %. **No code change follows** |
| [P4-LAN](p3-06-testing-results.md#p4-lan) | diagnostic; its DoH figure is less than half P4's. The attribution was attempted 2026-09-05 and is **unresolved** — `/tool/profile` charges all container work to one aggregate task and never named the harness and the server apart |
| [P5](p3-06-testing-results.md#p5) | **FAIL** — 1.389 ms incremental against < 1 ms |
| [D11-on-device](p3-06-testing-results.md#d11-on-device--certs-criterion-suite-on-the-rb5009) | **PASS** — `certs_mint` 450.88 µs, inside the < 1 ms row |
| [P5-conc](p3-06-testing-results.md#p5-conc--diagnostic-p5-path-segmentation-by-concurrency) | diagnostic — P5's incremental reproduces at ~1.29 ms; 8× concurrency removes ~0.26 ms, bounding the concurrency-sensitive component without identifying it; ~0.6 ms unattributed |
| [P5-diag](p3-06-testing-results.md#p5-diag--diagnostic-listener-side-segmentation-of-the-p5-gap) | diagnostic — listener-side timestamps reconcile P5 to 3 %: `dispatch_wait` +4.5 µs (refutes the `spawn_blocking` hypothesis), `prewarm` +940.5 µs, `handshake_after_prewarm` +647 µs (cause not identified) |
| [P6](p3-06-testing-results.md#p6) | pass — 4.937 ms generate, 6.015 ms import |
| [P7-store](p3-06-testing-results.md#p7-store) | pass — no key material over the API, traversal list clean |
| [P8-probe](p3-06-testing-results.md#p8-probe) | diagnostic — the rename works, `fah-probe` is never summed with `fastadhunter` |
| [P9-probe](p3-06-testing-results.md#p9-probe) | diagnostic — ~0.18 s boot-to-serving, a lower bound |

**Two certificate results that must not be conflated.** Mint performance on
target hardware **passes**; P5's LAN-observed incremental cost **fails**. The
~0.94 ms between them is explained by neither the crypto nor the budget, and the
next step on P5 is segmenting its path, not changing `fah-certs`. Any reading of
this file that still attributes P5 to "minting is too slow" is superseded by
D11-on-device.

**P1-LAN, P1-control, P2 and P3 were not run** — no second LAN endpoint, and for
P3 no h2 origin under a public name with a publicly trusted certificate. Their
`TBD` rows below stand.

### Proposed PERFORMANCE.md rows (owner decides; on-device column carries a figure only where the 2026-09-04 campaign measured one)

| Metric | Proposed budget | Dev-box figure (this file) | On-device |
| --- | --- | --- | --- |
| **HTTPS** SNI verdict + splice **+ one upstream resolve**, added per connection | TBD — must be measured during verification (P2 sets it; a loopback figure does not convert, PERFORMANCE.md §Converting). **Not a handshake figure**: the quantity is proxy setup + relayed handshake and contains one uncached A + AAAA resolve per connection (campaign-2 declaration change 4) | +3.5–4.9 ms loopback, harness-dominated (D8, old harness — see F6) | TBD — P2 |
| **HTTPS** splice throughput, steady state | ≥ 100 MiB/s (gigabit LAN is 119 MiB/s) | 0.93–1.01 GiB/s at 16 KiB, 1.48–1.60 GiB/s at 64 KiB (D7, unpinned); 1.06–1.13 / 1.56–1.61 GiB/s pinned to four cores (§Post-review work C) | TBD — P1 |
| **HTTPS** interception overhead vs splice, per connection | intercepted p50 ≤ 2 × spliced p50 | within intervals of each other unpinned (D8); 1.5–2.2× pinned to four cores (§Post-review work C) | TBD — P2. **Ratio unaffected by declaration change 4** — both arms pay the same proxy setup and resolve, so it cancels; what the ratio isolates is the interception leg |
| **HTTPS** intercepted h2 relay | ≥ 50 MiB/s | 481–485 MiB/s (D9, unpinned); 571–620 MiB/s pinned to four cores | TBD — P3 |
| Minted-leaf cache hit rate, browsing load | ≥ 90 % (real replay decides) | **none** — D12 is synthetic (a property of `ZIPF_HOSTS = 4096`, not of browsing) and is not evidence for this row (MA-7) | TBD — soak: `leaf_cache.prewarm_hits / (prewarm_hits + minted_total)` on the listed device over 24 h; the `https-sni` domain list is the corpus for a 7-day replay |
| **DoT** / **DoH** added latency vs UDP, p50 | TBD — must be measured during verification (P4 sets it; TLS/HTTP legs do not convert) | +17 µs / +130 µs loopback (D13) | **TBD — the 2026-09-04 figures (DoT +61 µs, DoH +915 µs) are withdrawn as row-setters** (§P4-reruns, 2026-09-05). Three valid sessions of the same workload on the same build give DoT added +61 / +17 / +16 µs: the declared statistic subtracts a UDP control that itself moved 73 % between sessions. The transport paths are healthy — DoT 162–192 µs and DoH 1 016–1 044 µs absolute, both faster than the ×9 conversion predicts — so the fix is the statistic (pool the control across sessions), not the code. No replacement value is picked from the three |
| Cold `prewarm` per first-sight host (whole path, not raw keygen) | < 1 ms | 53.5 µs (D11) ⇒ ≈ 0.48 ms by the ×9 factor (CPU-bound, converts) | **450.88 µs** (D11-on-device, 2026-09-04) — inside the row, and 8.4× the dev box, so the ×9 prediction of ≈ 0.48 ms was right. **P5's end-to-end arm fails the same row at 1.389 ms**; the two are different measurements and both are recorded |
| CA generate / API-pair import wall time | < 100 ms / < 50 ms | ≈ 2 ms / ≈ 1.4 ms (p3-02, debug) | **4.937 ms / 6.015 ms** (P6, 2026-09-04) — both inside the row |
| RAM steady-state, full mode | ≤ 128 MB (existing row, re-affirmed) | 46.1 MiB test build (D14) | TBD — P7 (household browsing; the 64-stream intercepted-session ceiling is P3's question, not this row's — MA-10) |

## Implementation Summary

Verification task: no product feature. Adds the phase-3 security suite, the
offline full-mode e2e, the p3-03/p3-04/p3-05 carry-over tests and benches,
runs the dev-box A/B against the pre-phase-3 checkout, and writes the
on-device runbook the owner executes. Steps 1–3 done on the dev box; Step 4 is
proposed below and **stops there**; Step 5 is a list, nothing landed.

| Area | Where |
| --- | --- |
| Security suite, six scenarios with the plan's exact names | `crates/fastadhunter/tests/security_phase3.rs` (new) |
| Offline full-mode e2e `full_mode_blocks_at_every_layer` | `crates/fastadhunter/tests/e2e_https.rs` (new) |
| Shared harness: `boot_with(scheme)`, `Ports::https()`, full-mode config, TLS/raw loopback origins on `127.0.0.x:443`, synthetic ClientHello (with/without SNI), key-material detector (`Needles`), WS event waiter, first-boot password + session cookie | `crates/fastadhunter/tests/common/mod.rs` |
| p3-04 carry-over integration tests (M4 rows 5–9, N10, IPv6) | `crates/fah-http/tests/interception.rs`: `Setup.max_connections` / `Setup.listen`, `OriginSpec.count_body` / `hold_page` (+ `Origin.cut` / `Origin.release`, `Held` body), `Trickle` streaming body, 6 new tests, N10 permit assertion added to the idle test |
| p3-03 n5 + M4: splice bench on `TlsServer::bind/serve`, shared payload origin, loop-read drain, steady-state arm (`https_sni_splice_steady_state`, 64 MiB) | `crates/fah-http/benches/proxy.rs` |
| New bench: handshake overhead (direct / spliced / intercepted), h2 8 MiB download through terminate vs splice (p3-04 S1), `prewarm` hop (p3-04 N8) | `crates/fah-http/benches/intercept.rs` (new), `[[bench]]` in `crates/fah-http/Cargo.toml` |
| Leaf-cache replay arm `certs_replay_zipf` (synthetic Zipf, hit rate printed) | `crates/fah-certs/benches/certs.rs` |
| `encrypted_latency` promoted: 3 interleaved rounds × 2 000 per transport, per-round p50 printed, pooled percentiles | `crates/fastadhunter/tests/encrypted_latency.rs` (still `#[ignore]`, release binary via `FAH_E2E_BINARY`) |
| A/B session script + raw criterion logs | `docs/code-review/phase3/p3-06-bench/` (`run-ab.ps1`, `session.log`, `r*-*.out.txt`) |
| Dev-dependency | `crates/fastadhunter/Cargo.toml`: `rcgen` (dev-only; the origin the binary must *not* be able to verify) — `Cargo.lock` gains the edge |

Product code: one `test-harness`-gated hook in `main.rs` `interception()`
(F1, owner decision (b), §Fixes applied) — nothing else under `crates/*/src`.
`crates/fah-http/src/https.rs` `SPLICE_BUF` was toggled to 64 KiB for one
bench build and restored (`git diff` clean). `layering.rs` green.
`http_e2e.rs` unchanged and green.

## Decisions

- **Origins live on `127.0.0.x:443`, one address per test.** `HTTPS_ORIGIN_PORT`
  is the compiled-in 443 (as `HTTP_ORIGIN_PORT` is 80 for `http_e2e.rs`), so
  every test that needs an origin binds `<its own loopback IP>:443`; the mock
  upstream answers that IP and `[egress] allow_destinations` names it. Tests
  therefore run in parallel without sharing a port; a refused bind
  (`AddrInUse` / `PermissionDenied`) skips with a message, as `http_e2e.rs`
  does. On this box all bound.
- **Traversal probes go over raw HTTP/1.0 on a rustls stream, not reqwest.**
  reqwest's URL parser resolves `..`, `%2e%2e` and `\` client-side, so the
  first run sent `/config/ca-key.pem` for `/api/v1/../../config/ca-key.pem`.
  The raw sender puts the request line on the wire byte-for-byte.
- **Key-material detector** searches every response body for the CA key's and
  the API key's whole base64 payload and its first 48 characters after
  whitespace and JSON `\n` escapes are stripped, the raw DER bytes, and the
  four PEM private-key headers. Built from `ca-key.pem` / `api-key.pem` read
  off the config volume; self-checked against the files it came from. The
  key never leaves the test process and is never printed.
- **Streaming proof is ordering, not RSS.** The client body releases each
  256 KiB chunk only after the origin has confirmed receipt of everything more
  than 1 MiB behind it; a proxy that buffered the 4 MiB body could never let
  the client finish (20 s timeout). RSS cannot be attributed to the proxy in
  an in-process test.
- **Zipf replay is labelled synthetic.** No real browsing-session host list
  exists offline; the real replay comes from the soak's `https-sni` feed (D12).

## Discrepancies with the plan — owner decisions needed

| # | Plan said | Found | Options |
| --- | --- | --- | --- |
| X1 | Step 3: "intercepted HTTPS URL block (client trusting the test CA, opted in)" through the full binary | **Resolved 2026-09-02 — owner chose (b), see §Fixes applied — F1.** Correction to the option text below: a plain `cargo test` build does **not** carry the binary's `test-harness` feature (the self dev-dep enables only `fah-api/test-harness`); the hook exists under `--all-features`, which is the documented gate. Original finding: **Not buildable offline.** The terminate leg verifies the upstream against **compiled-in webpki roots only** (`fah_http::client_config()`, `main.rs` `interception()`); no config key adds a root. A loopback origin can never verify, so the binary closes the connection (526) before minting. Leg 5/7 therefore asserts: listed client enters the terminate leg (event `kind: https`), unverifiable origin ⇒ `status 526`, no ServerHello, `minted_total == 0`. The URL-level judge inside TLS stays pinned at the `fah-http` harness (p3-04 `a_listed_client_gets_url_level_filtering_over_*`, which injects the origin root) | (a) accept as is — the wiring is proven end-to-end, the judge is proven one layer down; (b) a `test-harness`-gated extra-roots hook in `main.rs` (feature already exists for the login limiter; test-built binaries carry it, release cannot); (c) a real `[https.interception] upstream_roots` key (product feature, also serves private CAs — out of this task's scope) |
| X2 | Step 1: A/B the `fastadhunter` pipeline bench | `cargo bench -p fastadhunter` **cannot build** at either checkout: the dev-dep `fah-api/test-harness` hits `compile_error!` under the release profile (since p5-04). D4 ran both arms with `--config profile.bench.debug-assertions=true` — equal treatment, but not the shipped codegen | record; or move the harness feature off the bench build (separate task) |
| X3 | plan/CLAUDE.md: `wip` holds at most one phase | `plan/wip/` holds `phase2.6-adaptive-stage1` **and** `phase3`; `docs/project-state.md` (2026-09-01) still says Phase 3 opens after 2.6 closes | pre-existing; project-state rewrite at phase close |
| X4 | Step 1 D14: dev-box RSS from `/api/v1/debug/memory` | `process_rss` / `process_peak_rss` are `null` on Windows (`getrusage` is unix-only); the reading was taken from the OS (`tasklist`) for the test-built binary | label as done in §Measurements |
| X5 | Step 2 item 1: path-traversal probes aimed at `/config` | Meaningful only where `/web` and `/config` are siblings. On this box `/web` does not exist (static falls to 404) and the config volume is a temp directory on another drive, so the probes prove "never the key" but cannot prove "the traversal was attempted and refused". `web.rs` unit tests pin refusal with a fixture root. Runbook item 5 repeats the probe list with `curl` against the production container (read-only GETs) | run on-device |

## Tests

| Suite | New | Pins |
| --- | --- | --- |
| `fastadhunter` `security_phase3.rs` | 6 | `ca_key_unreachable_via_every_route` — 70 documented routes × 3 credentials (none / bearer / session cookie from the first-boot password) + `logout-all` + `apikey/rotate` = 212 requests, 0 leaks (CA key, API key); `non_listed_client_is_never_minted_a_leaf` — non-listed client sees the origin's own certificate through the splice, byte-identical payload, `https-sni pass` event; trusting only our CA fails; the listed client from another loopback IP gets `kind: https` and fails; `minted_total == 0`; `bad_upstream_cert_is_not_masked` — listed client + self-signed origin ⇒ handshake error, event `status 526`, `bytes 0`, no ServerHello even for an accept-anything client, `minted_total == 0`; `exports_contain_no_private_material` — PEM export is one CERTIFICATE block whose DER equals the DER export, no key material in either, in the status document, in the import response, in `api-cert.pem` after a real import, or in `GET /api/v1/config`; `splice_is_byte_identical_when_interception_is_off` — 3 samples × 256 KiB pseudo-random payload, direct vs spliced byte-equal, events `pass` with `bytes` = payload; `dns_query_is_the_only_new_unauthenticated_route` — every `/api/**` route answers 401 without credentials (login exists), `/health` + `/dns-query` are the only non-static public paths, `/api/v1/dns-query` and `/api/dns-query` are 401, plaintext DNS on the DoT port is never answered; **closed posture** (`api.tls = false` + unloadable API pair, booted over `http://`): :53 answers, the DoT port is bindable (nothing listens), plaintext gets nothing, the log names `dot_enabled = true`, `/dns-query` is absent (N13), `/health` carries no DoT field (N3, see §Proposed doc edits) |
| `fastadhunter` `e2e_https.rs` | 1 | `full_mode_blocks_at_every_layer` — 1 DNS null-IP over UDP; 2 HTTP `/track.js` ⇒ empty 200; 3 SNI block closed before ServerHello, `https-sni block` event; 4 no-SNI hello closed, classified `pass`, listener still judging; 5 (under `--all-features`, F1) listed client trusting only our CA completes our handshake, `GET /track.js` inside TLS ⇒ empty 200 + `https` event `verdict block`, `path /track.js`; second session `GET /page` ⇒ origin body byte-identical + `https` event `pass`, `status 200`; one leaf for `shop.example.com` reused by the second session; origin `accepts` grew per session (verify-before-mint) — without the feature the leg degrades to the fail-closed 526 and says so; 6 DoT answers the block for a client trusting only the exported CA; 7 DoH forwards and blocks. `minted_total == 2` (page host + DoT hostname), `unwarmed_misses == 0`. 0.9 s |
| `fah-http` `interception.rs` | 6 (+1 extended) | `eight_parallel_h2_requests_share_one_verified_upstream_session` (8 × 200, `connections == 1`); `a_streamed_request_body_reaches_the_origin_before_the_client_finishes_sending` (4 MiB POST, 1 MiB window); `a_client_that_disconnects_mid_response_returns_its_permit` and `an_upstream_that_disconnects_mid_response_ends_the_session_and_returns_its_permit` (`max_connections = 1`; the origin holds `/page` open after a first 64 KiB frame, the client reads that frame, then one side cuts — deterministic mid-body, F2; second session served); `shutdown_stops_accepting_while_a_live_intercepted_session_keeps_serving` (p3-04 L4 semantics pinned: accept loop aborted, live session answers, no new session); `an_ipv6_listed_client_is_intercepted_end_to_end` (`[::1]` listed and listening, block inside TLS, event client `::1`, one mint); `an_idle_intercepted_session_is_closed_and_its_permit_returned` now asserts the permit (N10) |
| `fah-certs` bench | 1 arm | `certs_replay_zipf` |
| `fah-http` bench | 1 file, 3 groups | `https_handshake`, `https_h2_download`, `prewarm_hop` |

Gates on the Windows dev box, 2026-09-02: `cargo fmt --all -- --check` clean;
`cargo clippy --workspace --all-targets -- -D warnings` clean;
`cargo test --all-features --workspace` green — every suite 0 failed
(`security_phase3` 7/7 incl. the harness self-test, `e2e_https` 2/2,
`interception` 27/27, `http_e2e` 2/2 unchanged, `e2e` 2/2, `layering` 1/1).

## Runbook — on-device, owner-executed (propose only; nothing below was run)

Every router line is a write unless marked read-only. Read-only facts below
were collected 2026-09-02 over `ssh rb5009` and are quoted, not assumed.

**Read-only facts used.** `/ip/firewall/nat print`: `dstnat` rules 5–10, no
terminal drop; rule 9 = `fastadhunter http: leave local traffic alone`
(`dst-address-list=fah-http-skip dst-port=80`), rule 10 = `fastadhunter http`
(`dst-nat … to-ports=8080 in-interface-list=LAN src-address=!172.17.0.0/24`);
disabled rules 5/6 forward WAN :80/:443 to `192.168.10.10`. `/ip/firewall/filter
print where chain=forward`: rule 12 is the only catch-all drop and matches
`in-interface-list=WAN` new non-dstnat only — LAN → `172.17.0.2` falls through
to the default accept, so **no forward rule is needed**, as for :8080.
`/ipv6/firewall/nat print`: rules 3–5 steer v6 :80 via `fah-http-skip6`,
`fah-lan6`, `to-address=fd6c:7f32:8e91:1::2/128 to-ports=8080`. `/ip/dhcp-server
lease print where !dynamic`: `192.168.10.11` OnePlus 15 and `192.168.10.10`
bobdenaut are **static leases** (GAR §5.14 precondition holds for both test
devices). `/container print detail`: `fastadhunter-0.3.1`, `envlists=fah-env`
(three `MIMALLOC_*` keys, no `FAH__ENGINE__MODE`), `cpu-list=cpu0..cpu3`,
`memory-current=112.9MiB` (cgroup incl. page cache). `/interface/list/member`:
`LAN` = `BRIDGE` + `CONTAINERS`; the WireGuard interface is not in `LAN`, so
VPN clients (`10.10.10.0/24`) are **not** steered by any rule below.

**Deployed build first.** The container runs 0.3.1 (`db2f9b2`); the phase-3
listeners exist only from `3337aab` onward. Items 1–7 presuppose a phase-3
build deployed to the production container (a deploy with its own approval) —
or, for items 5 and 7, to the probe container.

### 1. dst-nat 443 (v4) + v6 decision

1. **Confirm the ports and the mode** (read-only, FAH API): `GET /api/v1/config`
   → `engine.mode`, `https.listen.port` (default 8444), `api.port` (8443).
   Mode is boot-class: set it with `POST /api/v1/config`
   `{"engine":{"mode":"dns+http+https"}}` (answers `restart_required: true`),
   then restart the container (`/container/stop` + `/container/start`,
   router write). Alternative without the API: a **new** envlist
   (`/container/envs/add name=fah-mode key=FAH__ENGINE__MODE value=dns+http+https`
   + `/container/set [find comment="fastadhunter"] envlists=fah-env,fah-mode`)
   — **never** add the key to `fah-env`, which the probe shares.
   Effect: at the restart. Rollback: `POST /api/v1/config` back to `dns+http`
   (or detach the envlists entry) + restart.
2. **Prove the listener before steering** (read-only):
   `/log print where message~"HTTPS SNI listener bound"` → `addr=[::]:8444`;
   from a LAN host, `openssl s_client -connect 172.17.0.2:8444 -servername neverssl.com </dev/null | openssl x509 -noout -subject`
   must print neverssl's subject (spliced), and `WS /api/v1/events` shows a
   `kind: https-sni` item.
3. **Steer v4 :443** — append in this order; the skip must precede the dst-nat,
   and `add` appends to a chain with no terminal drop (read-only fact above),
   so positions 11 and 12 are correct:

   ```routeros
   /ip/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=443 \
     dst-address-list=fah-http-skip \
     comment="fastadhunter https: leave local traffic alone"
   /ip/firewall/nat/add chain=dstnat action=dst-nat protocol=tcp dst-port=443 \
     in-interface-list=LAN src-address=!172.17.0.0/24 \
     to-addresses=172.17.0.2 to-ports=8444 \
     comment="fastadhunter https"
   ```

   `protocol=tcp` only: UDP/443 (QUIC) stays unsteered so browsers fall back
   to TCP instead of black-holing HTTP/3 (p3-03 non-goal). Effect: new
   connections immediately; established flows finish on their conntrack entry.
   Verify: `/ip/firewall/nat/print stats where comment~"fastadhunter https"`
   (packet counters move), and the WS feed carries `https-sni` items from LAN
   clients. **Rollback (one line):**
   `/ip/firewall/nat/remove [find comment~"fastadhunter https"]`.
   **Hazard:** deploy-rb5009.md §5b's rollback regex `comment~"fastadhunter http"`
   now also matches the two https rules — tighten it (see §Proposed doc edits).
4. **v6 decision (GAR §5.12).** Recommended: mirror the :80 v6 steering so
   SNI filtering covers the ~22 % of traffic that is v6 —

   ```routeros
   /ipv6/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=443 \
     dst-address-list=fah-http-skip6 comment="fastadhunter https v6: leave local traffic alone"
   /ipv6/firewall/nat/add chain=dstnat action=accept protocol=tcp dst-port=443 \
     dst-address-list=fah-lan6 comment="fastadhunter https v6: leave the delegated LAN prefix alone"
   /ipv6/firewall/nat/add chain=dstnat action=dst-nat to-address=fd6c:7f32:8e91:1::2/128 \
     to-ports=8444 protocol=tcp dst-address=!fd6c:7f32:8e91:1::2/128 \
     in-interface-list=LAN dst-port=443 comment="fastadhunter https v6"
   ```

   Rollback: `/ipv6/firewall/nat/remove [find comment~"fastadhunter https v6"]`.
   **Recorded consequence either way:** interception identity is IP/CIDR and a
   phone's v6 source rotates (privacy addresses), so a v6-steered connection
   from a listed device is **spliced, not intercepted** — interception rides
   v4 only. If the owner leaves v6 :443 unsteered instead, v6 HTTPS bypasses
   SNI filtering entirely and only the DNS layer covers it; record which.
5. **Operator warning — no-SNI / ECH.** Once :443 is steered, every TCP
   connection without a plaintext SNI (legacy clients, ECH outer-only hellos
   that the browser does not retry, IP-literal HTTPS) is **closed**: the
   container cannot recover the destination (measured 2026-08-31,
   `getsockopt(SO_ORIGINAL_DST)` = `ENOENT`, `docs/routeros-traps.md`).
   `[https.sni] no_sni` decides only how it is reported. Confirm the deployed
   lists cover the domains the household relies on before step 3, and keep
   step 3's rollback line at hand for the first hour.
6. **Prove the steering on an unlisted device** (MA-5 — the "any client,
   zero setup" half of the definition of done, production build). From a LAN
   device that is **not** in `[https.interception] clients`, with nothing
   installed: `curl -sv https://<a domain the deployed lists block>/` ⇒ the
   TLS connection fails before any certificate (curl prints no `subject:` /
   `issuer:` line; the error is a reset or unexpected EOF, not a certificate
   error). Then read: `GET /api/v1/telemetry` `listeners.https.blocked` +1,
   `connections` +1, and one WS `query` item `kind: "https-sni"`,
   `verdict: "block"`, `domain: <that domain>`, `client: <that device>`.
   Control: the same `curl` to an allowed domain ⇒ the origin's own
   certificate (`issuer:` is public, never `FastAdHunter CA`) and a
   `https-sni` `pass`/`allow` item. Record all four readings; the same two
   commands are the soak's SNI gate (Runbook 6).

### 2. CA install walkthrough (Android OnePlus 15 `192.168.10.11`, Windows `192.168.10.10`)

1. **Generate the CA** (FAH API, not a router write):
   `POST /api/v1/certificates/ca/generate {"confirm": true}` → note
   `fingerprint_sha256`. DoT picks it up on the next handshake (no restart).
2. **Export on the device itself** — the export is authenticated (p3-02
   decision; two-exemption rule stands). On the device: open
   `https://172.17.0.2:8443/`, accept the self-signed warning, log in
   (session cookie), then open
   `https://172.17.0.2:8443/api/v1/certificates/ca/export?format=der` — the
   cookie rides the request and the browser saves `fastadhunter-ca.crt`
   (`Content-Disposition`). Fallback if the browser drops cookies on download:
   from bobdenaut,
   `curl -sk -H "Authorization: Bearer $FAH_KEY" -o fastadhunter-ca.crt "https://172.17.0.2:8443/api/v1/certificates/ca/export?format=der"`
   and move the file over (USB / adb push). **Record which path was used.**
3. **Install.** Android: Settings → Security & privacy → More security →
   Encryption & credentials → Install a certificate → **CA certificate** →
   "Install anyway" → pick `fastadhunter-ca.crt`; verify under Trusted
   credentials → User. Windows (admin shell):
   `certutil -addstore -f Root fastadhunter-ca.crt`; verify with
   `certutil -store Root | findstr FastAdHunter`. Screenshots →
   `docs/images/p3-06-ca-install-android-*.png`, `…-windows-*.png` (image files,
   listed here for the owner; not written by the agent).
4. **Only now list the device** (p3-04 L2: a listed client with no CA installed
   is closed, not spliced): `POST /api/v1/config`
   `{"https":{"interception":{"clients":["192.168.10.11"]}}}` → boot-class →
   restart the container. Boot log must show
   `HTTPS interception active for the listed clients — each must hold a static lease`
   (`count=1`). Rollback: `clients: []` + restart.
5. **Assert interception on the production build** (MA-5,
   [p3-06-measurement-audit](p3-06-measurement-audit.md) §Fixes applied) —
   three objective checks, recorded verbatim, before any browsing:
   - **Issuer.** From the listed device (Windows: `openssl s_client -connect
     <allowed origin>:443 -servername <allowed origin> </dev/null | openssl
     x509 -noout -issuer`; Android: the browser's certificate viewer) ⇒
     `issuer=CN=FastAdHunter CA`. The same command from an **unlisted**
     device ⇒ the origin's public issuer, never ours.
   - **Relay header.** From the listed device
     `curl -sSI --cacert fastadhunter-ca.pem https://<allowed origin>/ | findstr /i via`
     ⇒ `Via: 1.1 fastadhunter`; absent from the unlisted device.
   - **URL block inside TLS.** `PUT /api/v1/rules/user` adds
     `||<allowed origin>/p3-06-probe.js`; from the listed device
     `curl -sS -D - --cacert fastadhunter-ca.pem https://<allowed origin>/p3-06-probe.js`
     ⇒ `200` with an empty body, and the WS feed carries one item
     `kind: "https"`, `verdict: "block"`, `path: "/p3-06-probe.js"`,
     `client: "192.168.10.11"` (or `.10`); from the unlisted device the same
     URL ⇒ the origin's own `404` and no `https` item. Remove the rule after.
   Then **browse**: the WS feed shows `kind: https` items with real
   `method`/`path`/`status` for that client and `https-sni` for everyone else;
   `GET /api/v1/certificates` `leaf_cache.minted_total` climbs with first-sight
   hosts, `unwarmed_misses` stays 0. Record what the device shows on an
   HTTPS ad-heavy page (observation only — the three checks above are the
   evidence).
6. **ECH check (p3-04 L7):** from the listed device open
   `https://cloudflare-ech.com/cdn-cgi/trace` (or another ECH-enabled origin
   the owner prefers) — expect the browser to retry without ECH and the page to
   load; the feed shows one `status 0` `https` item for the outer name plus the
   filtered retry under the real name. Record whether the retry was visible to
   the user and how many extra upstream handshakes it cost.

### 3. Private DNS (Android hostname mode)

1. **Pick the hostname:** `dns.fastadhunter.lan` (private, never resolvable
   publicly).
2. **Bootstrap answer** (FAH API): add the user rules
   `||dns.fastadhunter.lan^$dnsrewrite=172.17.0.2` and
   `||dns.fastadhunter.lan^$dnsrewrite=fd6c:7f32:8e91:1::2` via
   `PUT /api/v1/rules/user`; confirm with `nslookup dns.fastadhunter.lan 172.17.0.2`
   from a LAN host. The phone resolves the hostname over plain DNS while it
   validates the DoT leaf.
3. **Phone:** Settings → Network & internet → Private DNS → Private DNS
   provider hostname → `dns.fastadhunter.lan` → Save.
4. **Verify:** the WS feed shows `kind: dns` items from `192.168.10.11` with
   `transport: dot`; `GET /api/v1/certificates` shows one more minted leaf
   (the hostname). Browse; then toggle airplane mode to force reconnects and
   confirm the phone stays on Private DNS (no "couldn't connect" banner).
5. **Record the tested assumption either way:** whether this device's Private
   DNS validation consults the **user** CA store. If the phone shows
   "Private DNS server cannot be accessed", the imported-real-certificate route
   (`POST /api/v1/certificates/import` with a publicly trusted pair for the
   hostname, restart; the DoT fallback then serves it) is the remaining path —
   record that outcome instead.
6. **Also on-device from p3-05:** DoH over h2 on the wire — from the listed
   Windows device, `curl --http2 -sk -H 'accept: application/dns-message' "https://172.17.0.2:8443/dns-query?dns=AAABAAABAAAAAAAAB2V4YW1wbGUDY29tAAABAAE" -o /dev/null -w '%{http_version}\n'`
   must print `2`; and a browser's secure-DNS setting pointed at
   `https://172.17.0.2:8443/dns-query` (after the CA install) shows
   `transport: doh` items. Per-query DoT/DoH vs UDP latency: P4 below.

### 4. Pinned-app spot check

1. **Owner settles `fah_http::BASELINE_EXCLUSIONS` first** (code change, own
   gates). Shipped list: Apple (4), Google/Android (8), Microsoft (6),
   WhatsApp (2), Signal, PayPal, Revolut, Wise, N26, eight Romanian banks
   (`bancatransilvania.ro`, `btrl.ro`, `ing.ro`, `bcr.ro`, `george.ro`,
   `brd.ro`, `raiffeisen.ro`, `unicredit.ro`). The app used below must be
   covered by the baseline or by `[https.interception] exclude_domains`, else
   the check proves nothing about exclusions. Record the final list here and
   the CONFIGURATION.md `exclude_domains` sentence that names it.
2. **Static-lease precondition per listed IP** — read-only, already verified
   2026-09-02: `192.168.10.11` and `192.168.10.10` hold static leases. Re-run
   `/ip/dhcp-server/lease print where !dynamic` before adding any other client.
3. **Check:** with the device listed and the CA installed, open the banking
   app, log in, view a balance. Expected: unchanged behaviour; the feed shows
   the bank's hosts as `https-sni pass` (spliced), never `https`. Record the
   app, the hosts observed and the outcome.

### 5. Probe-container measurements (P1–P6, pre-declared above)

Not built yet — the probe image for these arms does not exist. Proposed
agent work (dev box only, no router write): cross-compile the criterion bench
binaries `proxy`, `intercept`, `certs` for `aarch64-unknown-linux-musl` into a
`Dockerfile.probe`-style single-layer image (they carry their own loopback
origins, so they measure in-device CPU cost — the "D8 / D9 on-device"
diagnostics; the acceptance figures P2 and P4 are the LAN path and the
in-device harness defined below, not these binaries), plus the `SPLICE_BUF`
64 KiB variant of `proxy`, the `encrypted_latency` test binary and the
release `fastadhunter` (P4). Then the
owner: `scp` the tar (ask first), `/container/add … interface=veth3
root-dir=/kingston/probe-bench/root comment="fah-bench"`, `/container/start`,
`/log print where topics~"container"`, `/container/remove`
(`docs/routeros-traps.md` §On-device measurement). Do **not** pin the probe
(`cpu-list` empty).

**P2 — the declared arm, on the probe FAH instance (`172.17.0.4`, the real
binary), never the criterion binary** (declaration change of 2026-09-02,
§Pre-declaration). Preconditions, probe config (owner): `[engine] mode =
"dns+http+https"`; `[https.interception] clients` lists the intercepted LAN
client (boot key); a CA generated on the probe and its PEM on the client
(`GET /api/v1/certificates/ca/export`). One fixed public origin `ORIGIN`
(small page; record the name, its TLS version and ALPN) chosen before the
run. The spliced and intercepted arms come from **two client addresses**
(one listed, one not), or from one client in two passes with a restart
between. From the client(s), 200 rounds, the three arms in order per round:

```sh
curl -sS -o /dev/null -w '%{time_connect} %{time_appconnect} %{time_starttransfer}\n' https://ORIGIN/
curl -sS -o /dev/null -w '%{time_connect} %{time_appconnect} %{time_starttransfer}\n' --connect-to ORIGIN:443:172.17.0.4:8444 https://ORIGIN/
curl -sS -o /dev/null -w '%{time_connect} %{time_appconnect} %{time_starttransfer}\n' --connect-to ORIGIN:443:172.17.0.4:8444 --cacert fastadhunter-ca.pem https://ORIGIN/
```

Line 1 direct, line 2 spliced (unlisted client), line 3 intercepted (listed
client). `--connect-to` keeps SNI and `Host` at `ORIGIN` — the terminate leg
answers `421` to a `Host` naming another port — and connects to the probe's
8444. Per arm: handshake = `time_appconnect − time_connect`, first byte =
`time_starttransfer − time_connect`; min / p50 / p99 over the 200. Windows
curl (schannel) takes `--cacert` since 7.60; add `--ssl-no-revoke` if the
probe CA carries no CRL. **Row evaluation:** "SNI verdict + splice, added
latency per connection" = spliced p50 − direct p50 (handshake and first-byte
columns both recorded); "intercepted p50 ≤ 2 × spliced p50" on the handshake
column. Before/after each arm read the probe's `/api/v1/telemetry`
`listeners.https` and `/api/v1/certificates` `leaf_cache`: `connections`,
`requests` and `blocked = 0` prove the verdict ran; `minted_total = 1`,
`unwarmed_misses = 0` and `requests > connections` prove the intercepted arm
took the terminate leg. The cross-compiled `intercept` criterion binary stays
useful as an in-device CPU diagnostic — label it "D8 on-device"; it is not P2.

**P4 (DoT/DoH/UDP per query)** — two measurements, one row (MA-8,
declaration change of 2026-09-03):

- **P4, the acceptance measurement (in-engine, reused connection):** the
  `encrypted_latency` harness cross-compiled into the probe image beside the
  release binary (`FAH_E2E_BINARY=/fastadhunter`, `--ignored --nocapture`),
  run **in-device** — the same quantity as D13 (µs resolution, one reused DoT
  connection, DoH keep-alive, 3 interleaved rounds × 2 000, handshakes
  excluded). Query: `ads.example.com` A, blocked by the harness's own user
  rule ⇒ answered by the Rule Engine, no cache, no upstream. Feeds the
  "DoT / DoH added latency vs UDP, p50" row directly; in-engine by
  construction, as PERFORMANCE.md defines the row.
- **P4-LAN, diagnostic (user-visible):** from a LAN host, `kdig +keepopen`
  (one connection for the batch) against the probe: UDP `kdig @172.17.0.4
  <blocked domain> A` ×2 000; DoT `kdig @172.17.0.4 +tls
  +tls-ca=fastadhunter-ca.pem +tls-hostname=<dot hostname> +keepopen …`; DoH
  `+https +keepopen …`; 3 interleaved rounds. Same blocked domain, so the
  reply is in-engine and the UDP control isolates the listener cost from
  upstream RTT. kdig's per-query time has 0.1 ms resolution — record p50/p99
  from it **and** the wall-clock mean of each 2 000-batch; the LAN hop and
  the one handshake per batch are in this figure, which is why it is not the
  row.

**P6 (CA generate / API-pair import wall time)** — `curl -sS -o /dev/null
-H "Authorization: Bearer $FAH_KEY" -w '%{time_appconnect} %{time_starttransfer} %{time_total}\n'`
against the probe's `/api/v1/certificates/ca/generate` and `/import`, 5 each
(MA-9). **The row is `time_starttransfer − time_appconnect`**: server-side
wall time from the TLS session being up to the first response byte — the
archive copy, staging, two renames and the JSON are in it, the client's TCP
and TLS handshake (≈ 7 ms on the device, p5-10) are not. Record
`time_total` beside it as the handshake-inclusive diagnostic. Bearer auth,
so no Argon2 in the reading.

Also on the probe: re-run the security suite's traversal list with
`curl -sk` against `https://172.17.0.4:8443` (X5).

### 6. 24 h soak (production container — a deploy with its own approval)

**Prerequisite decision (p3-04 L5 + TODO) — SHIPPED 2026-09-02, `44f3c83`.**
`non_tls`, `hello_timeouts` and `upstream_cert_failures` were counted and
published nowhere. `GET /api/v1/telemetry` now carries
`"listeners": {"http": <ListenerCounters>, "https": <ListenerCounters>}`, and
on the terminate leg `requests` counts judged units while `connections`
carries accepted connections, so `blocked ≤ requests` holds per listener.
Watch item (b) has its read path. Do not re-propose this as work; the change
is described in §Post-review work A and the code is in
`fah-model/src/engine.rs`, `fah-http/src/{proxy,https}.rs` and
`fah-api/src/telemetry.rs`.

| Watch item | Class | Read | Decides |
| --- | --- | --- | --- |
| RSS ≤ 128 MB steady, flat (slope over the final third < 2 MB) | **gate** | `/api/v1/history/perf` `rss_bytes`, `/api/v1/debug/memory` `process_rss` (MiB vs MB) | acceptance for **household browsing with the deployed 1 M-scale ruleset** — this reading also re-affirms the pre-existing "RAM steady-state, 1 M loaded" row for full mode (MA-6). It does **not** establish the 64-stream intercepted-session ceiling (`[https] max_connections` ≈ 5.5 MiB/session worst case): the soak's listed devices never stall 64 streams; **P3 is the only authority for that question** (MA-10) |
| Boot-to-serving, full mode (MA-6, "P9"): container log timestamps from `/container/start` to `API listening` on the soak deploy, against the same interval on the 0.3.1 `dns+http` container (read-only `/log print where topics~"container"`, available now). Phase 3 adds `CertStore::open`, the DoT and HTTPS binds and `dot_tls` to `Engine::start`; the `startup` bench does not contain that code, so this is the only place the delta is visible | **gate** against the existing "< 3 s hard" row | container log | the pre-existing startup row for full mode |
| Full-mode CPU under household browsing (MA-11, "P8"): `/tool/profile cpu=all` (read-only), 10 spot reads at matched hours on the soak deploy and, beforehand, on the 0.3.1 `dns+http` container; report the `fastadhunter` process share per read and the median delta. Interpret with `listeners.https.connections` and `requests − connections` from the same hour (splice vs terminate-leg mix). **Per-leg interception CPU is not separable on this platform** — `/tool/profile` keys on process name and the shipped telemetry carries no CPU seconds — so "interception CPU under browsing" is **withdrawn** as an acceptance claim; the terminate leg's CPU cost is bounded by P2's intercepted arm and the D8/D9 on-device diagnostics | diagnostic | `/tool/profile`, `/telemetry` | whether full mode changes the device's CPU envelope; descriptive |
| No crash, no restart, `uptime_seconds` continuous | **gate** | `/health`, container log | acceptance |
| Security suite green on the deployed build (traversal probes, X5) | **gate** | curl walk | acceptance |
| **Traffic reached the container — HTTPS** ([phase3-audit](phase3-audit.md) §Fixes applied 4): `listeners.https.connections` strictly increasing between consecutive hourly pulls in ≥ 20 of the 24 windows, `listeners.https.requests ≥ 1` at the first pull | **gate** | `/api/v1/telemetry` | dst-nat 443 steers the LAN; without it the RSS and uptime gates pass on an idle listener |
| **SNI filtering exercised**: at soak start, from an **unlisted** LAN device, `curl -sv https://<a domain the deployed lists block>/` ⇒ the connection closes before any certificate (curl reports a TLS connect failure, no `subject:` line); `listeners.https.blocked` +1 on the next pull; one `kind: "https-sni"`, `verdict: "block"` item from that device on a WS `query` tap. Then `blocked` keeps growing over the window | **gate** | curl, `/telemetry`, WS `query` | the everyone-path of the definition of done, on the production build |
| **Interception exercised**: at soak start, from the **listed** device (CA installed), one URL a **user rule** blocks on an otherwise allowed host (`PUT /api/v1/rules/user`, the e2e's `\|\|host/track.js` shape; add it for the check, remove it after) ⇒ empty `200`; `leaf_cache.minted_total ≥ 1`; one `kind: "https"`, `verdict: "block"` item naming that URL and device on the WS tap; over the window `listeners.https.requests − connections` grows (requests judged inside TLS) | **gate** | `/certificates`, WS `query`, `/telemetry` | the managed-client path of the definition of done on the production build — the software leg 5 runs only on the `test-harness` build |
| **DoT exercised**: `dot.state == "listening"` on every `/certificates` pull, and a 60 s WS `query` tap per hourly pull (`websocat`, `{"subscribe":["query"]}`, bearer header) counts ≥ 1 item with `transport: "dot"` from the phone's address in every window the phone is on Wi-Fi (Runbook 3 first). The tap is the only read path — no `/history/*` endpoint carries a per-item transport | **gate** | `/certificates`, WS `query` | Private DNS path of the definition of done |
| **DoH**: `transport: "doh"` items on the same taps | diagnostic | WS `query` | DoH is not a definition-of-done item; record the count |
| (a) peak concurrent DoH sessions vs the shared 64-permit API ceiling | diagnostic | **no direct read exists** — proxy: `transport: doh` items per minute on the WS feed and `/api/v1/stats` per-client counts; a session gauge would be a code change | whether the escape hatch (const bump / separate semaphore) is ever built |
| (b) `non_tls` vs `hello_timeouts` over the window | diagnostic | `listeners.https` once published | whether `hello_timeout_ms` (10 s of permit per silent socket) is revisited |
| (c) `leaf_cache.unwarmed_misses` with a CA installed | **attribute before filing** | `GET /api/v1/certificates` hourly | non-zero = p3-04 prewarm-then-evict window (> 512 first-sight hosts inside one handshake) or a DoT mint failure — attribute, then decide |
| (d) mint rate: `minted_total`/h, peak `inflight`, `superseded`, `evictions` | diagnostic | same | a rate far above the DoT hostnames in use or churning terminate-leg leaves ⇒ the per-client mint limit escape hatch |
| (e) idle-cut sessions vs RSS trend (p3-04 L4) | **attribute before filing** | `listeners.https.hello_timeouts` growth against `rss_bytes` | RSS trending with idle cuts is the L4 leak signature; flat RSS closes L4 |
| (f) per-transport traffic split | diagnostic | WS `transport` counts | descriptive |

Collection as deploy-rb5009.md §9 (5-minute `/telemetry` + `/stats` pulls)
plus hourly `GET /api/v1/certificates` and `/api/v1/debug/memory`; log every
extra pull. If the owner defers the soak, the phase row goes `AWAITING SOAK —
flips when the 24 h full-mode soak on the RB5009 records RSS ≤ 128 MB steady
and the watch items above`.

### 7. Certificate-store device checks (probe container, all mandatory)

| Check | Command (owner) | Expect / record |
| --- | --- | --- |
| Import-then-restart | `POST /api/v1/certificates/import` with a real pair (dev-box `openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -subj /CN=fah-probe -days 30`), then `/container/stop` + `start` on `fah-probe`; `openssl s_client -connect 172.17.0.4:8443 </dev/null \| openssl x509 -noout -subject`; `GET /api/v1/certificates` | subject `CN=fah-probe`, `api_certificate.source == "imported"`; boot log clean. Then the MEDIUM-2 recovery: copy the archived pair back from `api-archive/<ts>/` and restart |
| `0600` on every private key (first execution anywhere of `write_private`'s unix mode) | `sftp rb5009` → `ls -l kingston/probe/config/ kingston/probe/config/ca-archive/*/ kingston/probe/config/api-archive/*/` (read-only) after: first boot, import, `ca/generate` ×2 | `-rw-------` on `api-key.pem`, `ca-key.pem`, every archived `*-key.pem`; `*.pem.tmp` only transiently |
| Archive cap | `POST /api/v1/certificates/ca/generate {"confirm":true}` nine times | the ninth answers `409 conflict` `archive_full:`, the live pair unchanged (`fingerprint_sha256` stable); record the operator's retention story (move directories out of `ca-archive/`) |
| Generate / import wall time (P6) | `curl -w '%{time_total}\n'` ×5 each | min / median → PERFORMANCE.md rows |

## Proposed documentation edits (Step 5) — items 2, 3 and 4 applied 2026-09-09

Status per item below. Items 1 and 5–9 remain proposals; 1 is blocked on
figures that do not exist yet, and the rest await the owner or phase close.

1. **PERFORMANCE.md §Budgets** — new rows, each `Measured on the RB5009` =
   `TBD — must be measured during verification` until Step 4 lands, dev-box
   figures pointing here: HTTPS SNI verdict + splice added latency; splice
   throughput; interception handshake overhead (terminate + re-originate vs
   splice); minted-leaf cache hit rate under browsing load; DoT / DoH added
   latency vs UDP; **cold `prewarm` per first-sight host** (the whole cold path
   incl. the eviction scan, not raw keygen); CA generate / API-pair import wall
   time. Proposed budget values in §Measurements once the dev-box figures are
   in. Plus one line under §Converting dev-box numbers: TLS/splice/handshake
   figures do not convert.
   **Two of those row names are superseded by campaign-2 declaration change 4**
   — take the wording from §Proposed PERFORMANCE.md rows, not from this
   sentence. "SNI verdict + splice added latency" gains "+ one upstream
   resolve", and "interception **handshake** overhead" drops the word, because
   on the proxied arms the measured quantity is proxy setup + relayed
   handshake, never a TLS handshake (§Post-review work F, F1).
2. **SECURITY.md** — **already applied before 2026-09-09; this row was stale.**
   §What a household should do, row 3 already reads "Import the pair with
   `POST /api/v1/certificates/import` (Phase 3, API.md §Certificates); the DoT
   listener serves it too. Renewal stays an operator action", and §Later phases
   already reads in the present tense. Verified, no edit made. Recorded because
   a proposal list that still asks for an applied change wastes the next
   session's time re-deciding it.
3. **docs/deploy-rb5009.md** — new §5c "HTTPS (Phase 3, `dns+http+https`)"
   mirroring §5b: turn on the mode (API or a *separate* `envlists` entry, never
   `fah-env`), prove the listener, the two v4 rules + the three v6 rules above,
   verify (`nat print stats`, WS `https-sni`), the no-SNI/ECH operator warning
   with the measured `ENOENT`, CA install + list-after-install sequencing,
   Private DNS bootstrap rule, rollback. §5b rollback regex tightened to
   `[find comment="fastadhunter http"]` + `[find comment="fastadhunter http: leave local traffic alone"]`
   (exact) so it stops matching the https rules.
   **Applied 2026-09-09, commit `29ae326`** — 150 lines. Two departures from
   the sketch: the no-SNI/ECH warning is its own section placed *before* the
   redirect rather than after it, because it is the one step that can break
   sites the DNS layer never touched; and the Private DNS bootstrap rule is
   **not** included — it belongs to §4 of the runbook, is device-side, and
   restating it here would duplicate a document that already owns it. The CA
   walkthrough is linked, not copied, for the same reason.
4. **Dashboard re-review (code, owner decision):**
   `dashboard/frontend/src/pages/live-feed/filters.ts` `KINDS = ['dns', 'http']`
   lacks `https-sni` and `https`; `detail.tsx:15` gates the HTTP detail on
   `kind === 'http'` and `:53` the block detail on `kind === 'http' || verdict === 'block'`,
   so an `https` row renders through the DNS-shaped branch with empty
   method/path and an `https-sni` row cannot be filtered. Either add both
   kinds (+ a detail branch each) in this task or record the owner's deferral
   to a dashboard task — **unfiltered kinds are a finding** either way.
   **Fixed 2026-09-09, commit `bac7454`.** All three sites branched on
   `kind === 'http'` where they meant "not DNS"; each now tests DNS instead, so
   the three request-shaped kinds share one path. A third defect was found
   while fixing it and is not in the sketch above: `FeedCache` drew `MISS` on a
   Phase 3 row, because the wire sets `cached: false` on a pipeline that never
   asked the cache — the cell's own doc comment forbids exactly that. One
   judgment call: `session_event` fills an `https-sni` row's method and path
   with empty strings and its status with 0, so the row draws only its relayed
   byte count and the shared branch drops empty parts and a zero status.
   996 dashboard tests pass, four added. Verified against a live engine with
   user rules installed, not only in unit tests: the SNI block row reads
   `BLOCK` with its rule and list, `0 B`, no cache outcome.
5. **README.md §Operating modes** row `dns+http+https`: "…plus HTTPS
   interception, for managed environments" → "…plus SNI-level HTTPS filtering
   for every client, opt-in per-client HTTPS interception, and DoT/DoH
   listeners". Line 44's "on 0.3.0 since 2026-08-29" is stale (0.3.1 deployed).
6. **ROADMAP.md §Phase 3** — bullets to delivered `[x]` wording at phase close;
   "import PEM/PFX" → "import PEM (PFX descoped, ADR-0006)"; add "p3-06
   verification: dev-box suite + soak `<date>`".
7. **API.md — DONE 2026-09-02, `44f3c83`.** (a) p3-05 N3: `GET
   /api/v1/certificates` carries
   `"dot": {"state": "listening" | "closed", "reason": "<the boot error>"}`,
   `address` only when listening and `reason` only when closed; `/health` is
   unchanged, per SECURITY.md. (b) L5/TODO: the `listeners` block is on
   `/telemetry`. Both are documented in API.md and covered by tests — see
   §Post-review work A. Nothing owed here.
8. **docs/project-state.md** — rewrite at phase close (X3).
9. **CONFIGURATION.md** — only if the owner changes `BASELINE_EXCLUSIONS`
   (Runbook item 4).

## Files changed

| File | Change |
| --- | --- |
| `crates/fastadhunter/tests/security_phase3.rs` | new — 6 scenarios |
| `crates/fastadhunter/tests/e2e_https.rs` | new — `full_mode_blocks_at_every_layer` |
| `crates/fastadhunter/tests/common/mod.rs` | `ApiScheme`, `boot_with`, `Ports::https`, full-mode config + `Instance`, origins, hellos, `Needles`, `await_event`, password/cookie helpers |
| `crates/fastadhunter/tests/encrypted_latency.rs` | interleaved rounds |
| `crates/fastadhunter/Cargo.toml`, `Cargo.lock` | dev-dep `rcgen` |
| `crates/fah-http/tests/interception.rs` | harness fields, `Trickle`, 6 tests, N10 |
| `crates/fah-http/benches/proxy.rs` | splice harness on `TlsServer`, steady-state arm |
| `crates/fah-http/benches/intercept.rs`, `crates/fah-http/Cargo.toml` | new bench |
| `crates/fah-certs/benches/certs.rs` | `certs_replay_zipf` |
| `docs/code-review/phase3/p3-06-bench/` | `run-ab.ps1`, `session.log`, `r{1,2}-{A,B}-{cache,matcher,pipeline,proxy}`, `r{1,2}-splice{16,64}`, `r{1,2}-{intercept,certs}`, `isolated-handshake.out.txt` — raw criterion output |
| `docs/code-review/phase3/p3-06-phase3-verification-review.md` | this file |
| `docs/code-review/phase3/p3-06-testing-results{,-2}.md` | campaign 1 (superseded) and campaign 2 figures |
| `docs/code-review/phase3/p3-06-probe/` | the probe script set, its `lib.mjs`, and the `smoke-*` / `results-*` / `campaign2` artifact directories |
| `dashboard/frontend/src/pages/live-feed/{filters.ts,detail.tsx}`, `live-feed.test.tsx` | the two Phase 3 event kinds render and filter (Step 5 item 4, commit `bac7454`) |
| `docs/deploy-rb5009.md` | new §5c HTTPS steering; §5b rollback tightened to exact comments (Step 5 item 3, commit `29ae326`) |
| (outside the repo) `../FastAdHunter-pre3` | detached worktree at `64be513` with its own `target/`, left in place for re-runs; `git worktree remove ../FastAdHunter-pre3` drops it |

Rows above the review file are Steps 1–3 (code, tests, benches). The four below
it are Step 5 documentation and dashboard work, added 2026-09-09 — the table
listed only crate and bench files until then, so it did not show that this task
had changed the deploy guide or the dashboard at all.

## Known limitations / deferred

| Item | Owner |
| --- | --- |
| X1–X5 above | owner decision |
| Every on-device row (P1–P7, Runbook 1–7): dst-nat, CA install, Private DNS, pinned app, probe measurements, soak, cert-store checks, DoH-h2-on-the-wire, DoT/DoH device latency, N1/N11 `prewarm` profile — **nothing changed in `dot.rs`/`intercept.rs`** per the plan's "measure before touching" | owner + agent after approval |
| ~~N3 `dot` listener state, L5/TODO `listeners` block, dashboard kinds~~ — all three built: N3 and L5 on 2026-09-02 (`44f3c83`, §Post-review work A), dashboard kinds on 2026-09-09 (`bac7454`). Row kept struck rather than deleted: it was re-proposed as open work twice after shipping | closed |
| p3-05 N2/N4/N9 won't-fix, N12/N16 closed — untouched | — |
| Real browsing-session host replay for the leaf-cache row (D12 is synthetic) | soak feed |

## Findings — consolidated 2026-09-03; full history: `git show 0a74ac3:docs/code-review/phase3/p3-06-phase3-verification-review.md`

One review (F1–F18, X1–X5) and one fix round; F1 closed by option (b), F2–F9,
F11, F18 fixed, F6 withdrawn, F8's pinning-rule conflict settled by
§Post-review work C. Fixed and withdrawn items are omitted — git has them.
The measurement-validity findings MA-1–MA-11 and their resolutions live in
[p3-06-measurement-audit.md](p3-06-measurement-audit.md). I1 was closed
2026-09-09 by `bac7454` — §Proposed documentation edits item 4 carries the
record. Still open:

| id(s) | Issue | Status | Where |
| --- | --- | --- | --- |
| F10 | the key-material detector searches PEM base64, its first 48 chars, raw DER and four headers; hex, base64url and JSON `\u` encodings are not searched | deferred | no owner — note for the day a route emits those |
| F16 | splice byte-identity is proven origin→client only; the client→origin direction is sunk by the raw origin | deferred | no owner — low value, low cost |
| I3 | `E:/FastAdHunter-pre3` worktree (`64be513`) still registered | deferred | drop at phase close |
| X2 | `cargo bench -p fastadhunter` compiles only with `CARGO_PROFILE_BENCH_DEBUG_ASSERTIONS=true` (the `test-harness` dev-dependency unifies into the bench profile; p5-04 leftover) | deferred | follow-up task; workaround in PERFORMANCE.md §Measuring reliably |
| X3 | `docs/project-state.md` is dated 2026-09-01 and does not mention Phase 3 | deferred | phase-close rewrite |
| Step 4 | Runbook 1–7 on the device, P1–P8, the 24 h soak; `BASELINE_EXCLUSIONS` final names; P1/P3 LAN-vs-loopback definition. P9 recorded 2026-09-09 (§Post-review work H); P8 baseline open, 1 read of 10 | deferred | owner — §Runbook, §Hand-off state |
| Step 5 | doc sweep remainder: deploy-rb5009.md §5c after the walkthrough, README modes row, SECURITY.md row 3, ROADMAP wording, project-state | deferred | owner — §Proposed documentation edits |

**PASS WITH DEFERRED FINDINGS** — 7 open rows (7 deferred, 0 won't-fix). `AWAITING SOAK`; flip condition restated in §Hand-off state, 2026-09-08 (session 3, RB5009) — current. Its wording originates in the 2026-09-08 morning hand-off, which is marked superseded for everything else.

### GAR §5 items 7–14 — the Phase 3 gate map (plan §TASK START 8, F11)

| # | Item | Where it is discharged | Status |
| --- | --- | --- | --- |
| 7 | cert home / ADR-0007 | `docs/decisions/0007-certificate-machinery-home.md`; `fah-certs` (L2) consumed by `fah-http` and `fah-api` (L3); `layering.rs` green | closed (p3-01) |
| 8 | connector redesign — hostname-verified upstream TLS | `crates/fah-http/src/tls.rs:65` `connect_verified_upstream`, roots from `client_config()` (`tls.rs:36`); suite `bad_upstream_cert_is_not_masked` proves fail-closed at the binary level | closed (p3-04 decision 4) |
| 9 | DoH/DoT placement | DoT inside `fah-dns` (`main.rs:377` `dns.dot_addr()`); DoH on the API listener, route gated on `state.tls && state.doh` (`routes.rs:118-125`, `main.rs:548-582`); shared 64-permit consequence → soak watch item (a) | closed (p3-05 decision 4); watch item open |
| 10 | event/telemetry taxonomy | `fah-model/src/request_event.rs:126-127` `EventKind::{HttpsSni, Https}`; `fah-model/src/client_transport.rs:5` `ClientTransport`; suite and e2e assert `kind: https-sni` / `https` on the WS feed | closed (p3-03/04/05) |
| 11 | memory caps per new state owner | leaf LRU `fah-certs/src/leaf.rs:16` `LEAF_CACHE_CAPACITY = 512`; splice `https.rs:29` `SPLICE_BUF` × 2 × `max_connections` (D6/D7); terminate leg `intercept.rs:37-40` `H2_*` (D9); DoT 64 permits (p3-05) | closed on paper; stall RSS → P3 |
| 12 | 443 steering v4 + v6 | Runbook item 1 (proposed, not run) | open — owner |
| 13 | on-device TLS measurements | SNI, P4-LAN, P5, P6, P7 and P10 ran on the RB5009 2026-09-08 (`p3-06-testing-results-2.md` §Session 2); P9 recorded 2026-09-09 (§Post-review work H); P8 baseline 1 read of 10. **Only P1, P2 and P3 are outstanding**, all three on the second wired endpoint | partly open — owner; not a flip-condition gate |
| 14 | opt-in bound to stable identity | static leases for `192.168.10.11` / `.10` read-only verified 2026-09-02 (§Runbook read-only facts) | closed for the two test devices; re-verify per added client |


## Gate posture — strict by default, 2026-09-02 (owner decision)

Owner instruction: the official gate must be exactly
`cargo test --all-features --workspace`, with no environment variable to
remember, and the docs must state the real behaviour — the full-mode e2e needs
`test-harness`, and without it the test fails rather than passing on the
fail-closed path.

| Change | Where |
| --- | --- |
| Skips are opt-out, not opt-in: `bind_origin` panics on an unbindable `127.0.0.x:443` and leg 5 panics when built without `test-harness`, unless `FAH_SECURITY_ALLOW_SKIP=1` (`skips_allowed()`). `FAH_SECURITY_STRICT` removed | `crates/fastadhunter/tests/common/mod.rs`, `crates/fastadhunter/tests/e2e_https.rs` |
| Why `--all-features` matters and what `FAH_SECURITY_ALLOW_SKIP` does, one paragraph each | root `CLAUDE.md` §Quality gates; `CONTRIBUTING.md` §`test-harness` (new bullet) |

| Verification (Windows dev box, 2026-09-02) | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings`, and with `--all-features` | both clean |
| **`cargo test --all-features --workspace`** — no environment variable set (`FAH_SECURITY_ALLOW_SKIP`/`_STRICT` explicitly unset) | every suite green, 0 failed, no `DEGRADED`/`SKIPPED` line; `security_phase3` 7/7, `e2e_https` 2/2 with the full leg 5 |
| `cargo test -p fastadhunter --test e2e_https` (no features, no env) | **fails as designed**: `5/7 intercepted: the binary was built without the test-harness feature … run cargo test --all-features. The URL judge inside TLS did not run; set FAH_SECURITY_ALLOW_SKIP=1 to accept the fail-closed path only` |
| `FAH_SECURITY_ALLOW_SKIP=1 cargo test -p fastadhunter --test e2e_https` (no features) | 2/2, leg 5 prints `DEGRADED — …` and asserts the fail-closed 526 |

Files changed by this step: `crates/fastadhunter/tests/common/mod.rs`,
`crates/fastadhunter/tests/e2e_https.rs`, `CLAUDE.md`, `CONTRIBUTING.md`,
this file. Verdict unchanged.

## Post-review work A — DoT listener state (p3-05 N3) + listener telemetry (p3-04 L5 / TODO), 2026-09-02

Owner decisions: `dot` state on `GET /api/v1/certificates` (not `/health`);
`listeners` block on `GET /api/v1/telemetry`; `requests` = judged units,
new `connections` = accepted connections, so `blocked ≤ requests` holds per
listener. Both are soak prerequisites (watch items (b), (e)).

| Change | Where |
| --- | --- |
| `ListenerCounters`, `ListenerTelemetry` (L1 data, no logic) | `fah-model/src/engine.rs`, exported |
| `ProxyCounters.connections` / `ProxyStats.connections`; `From<ProxyStats> for ListenerCounters` (one conversion, L3→L1) | `fah-http/src/proxy.rs` |
| `:80`: `connections` +1 per accepted connection (`requests` unchanged, per HTTP request) | `proxy.rs` `serve_connection` |
| HTTPS listener: `connections` +1 per accept (was `requests`); `requests` +1 per SNI verdict (incl. no-SNI, which `[https.sni] no_sni` decides) and +1 per HTTP request judged inside TLS; `non_tls` / `hello_timeouts` never count as requests | `https.rs:117, :153, no_sni_observed`; `intercept.rs` `handle_intercepted` |
| `TelemetrySource::listeners()` port; `TelemetrySnapshot`/`TelemetryResponse.listeners` | `fah-api/src/ports.rs`, `telemetry.rs` |
| Binary adapter reads both `ProxyCounters` handles on demand (no poll lag; metrics registry untouched) | `fastadhunter/src/adapters.rs` `TelemetryAdapter::new(.., http, https)`, `main.rs` |
| `DotListener { Listening { address } \| Closed { reason } }` on `AppStateBuilder`/`AppState`; `dot` block on the certificates document (`address` only when listening, `reason` only when closed) | `fah-api/src/state.rs`, `certs.rs`, `lib.rs` |
| `dot_tls()` returns `Result<Option<DotTls>, String>` — `Ok(None)` disabled, `Err(reason)` closed (same sentences the boot log carries); the `dot_pair_loaded == false` path names the unloadable pair | `main.rs` |
| API.md: `listeners` block (example + semantics paragraph replacing "not published yet"), certificates `dot` (example + paragraph, `/health` unchanged per SECURITY.md) | `API.md` |
| Tests: `the_terminate_leg_counts_connections_and_judged_requests_separately` (1 connection, 3 requests, 1 block); `sni.rs` EOF-before-hello now `connections 1 / requests 0`; e2e asserts `dot.state == listening`, per-listener `blocked ≤ requests` and that `connections ≠ requests` on HTTPS; closed-posture test asserts `dot.state == closed` with a reason naming the pair, `address` absent, `/health` unchanged; fah-api unit + integration tests carry `dot` and the stub `listeners` | `fah-http/tests/{interception,sni}.rs`, `fastadhunter/tests/{e2e_https,security_phase3,history_e2e}.rs`, `fah-api/{src/certs.rs,tests/api.rs}` |

Hot path: one relaxed `fetch_add` per accepted connection on both listeners
and one per intercepted HTTP request; the DNS path is untouched. Memory: two
`AtomicU64` per listener, one enum on `AppState`. Publish cost: `/telemetry`
reads 24 more atomics per call.

| Verification (Windows dev box, 2026-09-02) | Result |
| --- | --- |
| `cargo fmt --all -- --check`; clippy with and without `--all-features` | clean |
| `cargo test -p fah-model -p fah-http -p fah-api --all-features` | fah-model 57, fah-http 81 + 12 + 28 + 12 + 9, fah-api 126 + 116 + 2 — 0 failed |
| `cargo test -p fastadhunter --all-features` (security_phase3, e2e_https, http_e2e, e2e, history_e2e, outcome_telemetry, layering, healthcheck) | all green, 0 failed, no degraded leg |

## Post-review work B — `BASELINE_EXCLUSIONS` before the pinned-app check, 2026-09-02

- **p3-04 L6 is already closed** (p3-04 review §"Fixes applied — cleanup L1 ·
  L6 · …"): `ExclusionSet::new` runs every `exclude_domains` entry through
  `sni::normalize` and `main.rs` turns an invalid one into a startup error, the
  same posture as `clients`. The rider in this review's analysis was stale; no
  code change.
- **Shipped list, unchanged, proposed as final** (`fah-http/src/exclusions.rs`
  `BASELINE_EXCLUSIONS`, 34 entries; CONFIGURATION.md `exclude_domains` already
  names every one): Apple `apple.com icloud.com mzstatic.com apple-cloudkit.com`;
  Google/Android `android.com googleapis.com play.google.com
  android.clients.google.com clients.google.com mtalk.google.com gvt1.com
  gvt2.com gvt3.com`; Microsoft `windowsupdate.com update.microsoft.com
  delivery.mp.microsoft.com login.microsoftonline.com notify.windows.com
  wns.windows.com`; messaging `whatsapp.net whatsapp.com signal.org`; payments
  `paypal.com revolut.com wise.com n26.com`; Romanian banks
  `bancatransilvania.ro btrl.ro ing.ro bcr.ro george.ro brd.ro raiffeisen.ro
  unicredit.ro`. A name covers itself and every subdomain.
- **Owner input still needed:** the banking app used in Runbook item 4 and its
  hosts. If they are outside this list, either extend the constant (one line +
  `the_baseline_exclusions_ship_without_any_configuration` + the
  CONFIGURATION.md sentence) or put them in `exclude_domains` on the device —
  both are recorded here when done. Until then the pinned-app check proves
  nothing about exclusions (plan §Step 4.4).

## Post-review work C — F8: the pinning rule, settled by measurement, 2026-09-02

Owner instruction: resolve the `docs/measurement-traps.md` ("four cores")
vs PERFORMANCE.md §Measuring reliably ("one core", `fah-http` unpinned)
conflict and re-run the `fah-http` arms pinned. Dev box: i9-13980HX, 8 P-cores
with HT + 16 E-cores = 32 logical CPUs; logical 0–3 are two P-cores' sibling
pairs. Raw output: `p3-06-bench/p4-*`, `probe-*`, `p4b-*`; scripts
`run-pinned4.ps1`, `run-pin-probe.ps1`; every run in `session.log`.

**Probe — what "pin to four cores" must mean** (handshake ms
direct / spliced / intercepted; pass-through µs direct / proxy):

| Variant | pass-through | handshake |
| --- | --- | --- |
| unpinned, 32 workers, original harness (18:35 session, reference) | 32.2 / 65.5 | 1.13 / 5.67 / 2.73 |
| mask 15 (CPUs 0–3 = 2 P-cores' HT siblings), 32 workers, F6 harness (`p4-*`) | 70 ± 20 % / 160 | 13.8 / 30.3 / 28.7 |
| mask 15, `TOKIO_WORKER_THREADS=4`, F6 harness | 70.7 / 195 | 17.9 / 31.9 / 28.4 |
| mask 0x55 (CPUs 0,2,4,6 = 4 distinct P-cores), 4 workers, F6 harness | **31.7 ± 0.6 % / 65.1 ± 1 %** | 10.2 / 17.4 / 16.9 |
| mask 0x55, 32 workers, F6 harness | — | 14.3 / 15.7 / 22.5 |
| unpinned, 32 workers, F6 harness | — | 6.61 / 13.9 / 10.3 |
| mask 0x55, 4 workers, **original harness** (`p4b-*`, r1 / r2) | 31.4 / 63.7 (B) | 2.35 / 11.6 / 16.9 · 2.51 / 8.85 / 19.7 |

Three findings, each measured, not inferred:

1. **A four-core mask alone measures oversubscription.** tokio sizes the
   runtime by machine CPUs (32) regardless of the mask; with 32 spinning
   workers on four logical CPUs the control arm doubled and its interval went
   from ± 1 % to ± 20 %. `TOKIO_WORKER_THREADS=4` must accompany the mask.
2. **`ProcessorAffinity = 15` is two physical cores on this box.** One logical
   CPU per physical core (`0x55`) with four workers reproduces the unpinned
   pass-through means with ~5× tighter intervals — the rule now written in
   PERFORMANCE.md §Measuring reliably and the new traps.md row.
3. **F6 was wrong and is withdrawn.** The reviewed `rt.block_on`-per-connection
   harness is 3–6× faster than the `iter_custom` batch under every pin, so the
   criterion-thread wake was not the cost the review assumed. Reverted;
   `benches/intercept.rs` is byte-identical to the reviewed commit.

The handshake arms are legitimately slower on four cores than on 24 (a
three-party relay: client, proxy, origin, plus hyper connection tasks): direct
2.4 ms vs 1.1 ms. That is the realistic four-core envelope, not a regression.

**p4b A/B, `^http_`, mask 0x55 + 4 workers, A/B/A/B** (`64be513` vs tip with
the post-review fixes; criterion means, intervals ± 0.5–1 % unless noted):

| D1 `http_pass_through` (µs) | A r1 | B r1 | A r2 | B r2 | A mean | B mean | Δ |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `direct_to_origin` (control) | 32.94 | 31.48 | 33.35 | 31.31 | 33.15 | 31.40 | −5.3 % |
| `through_proxy` | 62.70 | 64.36 | 62.02 | 63.00 | 62.36 | 63.68 | +2.1 % |
| added (proxy − direct) | 29.8 | 32.9 | 28.7 | 31.7 | 29.2 | 32.3 | +10.6 % — rides on the control moving −5 %; resolution bounded by that |

| D2 `http_opaque_body` (8 KiB µs; 1 MiB / 8 MiB ms) | A r1 | B r1 | A r2 | B r2 |
| --- | --- | --- | --- | --- |
| 8 KiB direct / proxy | 35.81 / 66.45 | 33.61 / 68.27 | 33.69 / 66.85 | 34.29 / 66.06 |
| 1 MiB direct / proxy | 0.762 / 0.804 | 0.755 / 0.909 | 0.748 / 0.890 | 0.698 / 1.165 (± 10 %) |
| 8 MiB direct / proxy | 5.43 / 6.16 | 5.39 / 7.94 (± 23 %) | 5.19 / 6.00 | 5.16 / 6.00 |

Reading: `through_proxy` +2.1 % and 8 KiB proxy +0.8 % — inside the 10 %
rule with tight intervals. The 1 MiB / 8 MiB proxy arms alternate direction
with ± 10–23 % intervals at four workers (the relay is scheduler-bound there)
and the unpinned session had B ≥ A on them; not resolved, not claimed. The
B-side code on this path differs from A by one relaxed `fetch_add` per
*connection* (keep-alive: once per bench), nothing per byte.

| D6 / D7 splice, mask 0x55 + 4 workers (`through_splice`, mean [range]) | 16 KiB r1 | 16 KiB r2 | 64 KiB r1 | 64 KiB r2 |
| --- | --- | --- | --- | --- |
| per connection, 1 MiB (MiB/s) | 71.5 [66.7 76.6] | 71.8 [60.6 93.0] | 87.6 [80.6 96.8] | 110.7 [100.8 124.6] |
| steady state, 64 MiB (GiB/s) | 1.13 | 1.06 | 1.61 | 1.56 |
| `direct_to_origin` steady (GiB/s) | 3.14 | 2.92 | 2.98 | 3.03 |

On four cores the 64 KiB buffer wins on both arms (+22–54 % per connection,
+45 % steady) — the unpinned per-connection reversal (16 KiB faster) does not
survive the pin. Memory axis unchanged: `2 × SPLICE_BUF × max_connections`,
128 MiB at 64 KiB and the default 1024. Decision stays with P1 on the device.

| D8–D10, mask 0x55 + 4 workers, original harness | r1 | r2 | unpinned (18:35) |
| --- | --- | --- | --- |
| `https_handshake/direct_to_origin` (ms) | 2.35 [2.02 2.69] | 2.51 [2.17 2.89] | 1.13 / 0.98 |
| `https_handshake/spliced` (ms) | 11.6 [10.6 12.3] | 8.85 [7.79 9.96] | 5.67 / 5.99 |
| `https_handshake/intercepted` (ms) | 16.9 [14.5 18.7] | 19.7 [17.6 21.5] | 2.73 / 4.82 |
| intercepted ÷ spliced | 1.46 | 2.23 | within intervals |
| `https_h2_download` 8 MiB direct / spliced / intercepted (MiB/s) | 1 205 / 989 / 571 | 1 304 / 984 / 620 | 1 231 / 907 / 485 |
| `prewarm_hop/spawn_blocking_prewarm` (µs) | 6.47 | 3.29 | 4.19 / 4.17 |

Reading: on four cores the terminate leg's second handshake is visible —
intercepted is 1.5–2.2× the splice, straddling the proposed "≤ 2× spliced"
row; P2 on the device decides it. h2 relay ratios hold (intercepted ≈ 0.6×
spliced). The `spawn_blocking` hop is 3–6 µs against a ≥ 9 ms handshake here;
N8 "leave alone" stands.

Files changed by this step: `PERFORMANCE.md` §Measuring reliably (the
two-rule paragraph), `docs/measurement-traps.md` §Calibration (new row),
`p3-06-bench/run-pinned4.ps1`, `run-pin-probe.ps1`, raw `p4-*`, `probe-*`,
`p4b-*` outputs, `session.log`, this file. `crates/fah-http/benches/intercept.rs`
net unchanged (F6 reverted). `SPLICE_BUF` toggled to 64 KiB for one build and
restored (diff verified: counters only).

## Post-review work D — dev-box 01–06 re-run (session S3), 2026-09-03

Owner instruction: run the six declared dev-box bench targets from the current
checkout with the declared methodology, no arm changed. Script
`p3-06-bench/run-s3.ps1`, raw `S3-*.out.txt` / `.err.txt`, `session.log`
lines 64–88. Tip `f8ecad2` + working tree (proxy/intercept carry the
2026-09-02 ruleset + event wiring); A = `64be513` exes of the declared session.
Pinning per §Measuring reliably: 01–03, 06 one core (`ProcessorAffinity = 4`,
High); 04–05 mask `0x55` + `TOKIO_WORKER_THREADS=4`. `pipeline` built with
`CARGO_PROFILE_BENCH_DEBUG_ASSERTIONS=true` on both sides (X2). Not run: the
64 KiB splice variant (needs a `src` edit), D13, D14. 22 arms, every exit
clean, no stderr beyond criterion's own.

**Session was not idle** (owner, after the fact): a browser and a fullscreen
video on the second monitor ran through the window. Control arm
`http_pass_through/direct_to_origin` moved **+0.4 %**, so the A/B verdict
stands by the declared ±5 % rule; the **tip-only series is contaminated** and
is not tabled — the identical A-side matcher exe read +5.9 % against
2026-09-02, `certs_mint` +6–7 %, steady splice 10–25 % under p4b with the
direct control unchanged. Re-run of splice16 / intercept / certs on an idle
box pending.

| A/B arm (means of two interleaved pairs) | A | B | Δ | Interval |
| --- | --- | --- | --- | --- |
| `dns_cache/cache_hit_in_engine_latency` µs | 2.964 | 2.730 | −7.9 % | ±9–10 %; round spread 40 % |
| `matcher_lookup/hit_exact` ns | 67.31 | 67.01 | −0.4 % | < 1 % |
| `matcher_lookup/hit_subdomain` ns | 208.3 | 206.4 | −0.9 % | < 1 % |
| `matcher_lookup/miss` ns | 49.02 | 48.85 | −0.3 % | < 1 % |
| `full_pipeline/blocked_query` µs | 3.344 | 3.162 | −5.4 % | ±6–12 % |
| `full_pipeline/forwarded_query_overhead` µs | 5.147 | 5.169 | +0.4 % | pairs +5.4 / −5.5 |
| `http_pass_through/direct_to_origin` µs (control) | 32.58 | 32.72 | +0.4 % | ±0.5 % |
| `http_pass_through/through_proxy` µs | 65.83 | 66.00 | +0.3 % | ±0.5–1 % |
| added (proxy − direct) µs | 33.25 | 33.28 | +0.1 % | — |
| `http_opaque_body` 8 KiB direct / proxy µs | 35.26 / 69.29 | 35.02 / 70.76 | −0.7 % / +2.1 % | ±0.5–1 % |
| 1 MiB direct / proxy ms | 0.973 / 1.663 | 0.897 / 1.271 | −7.8 % / −23.6 % | ±10–15 %, unresolved; B faster both pairs |
| 8 MiB direct / proxy ms | 5.667 / 6.554 | 5.422 / 6.625 | −4.3 % / +1.1 % | ±3–5 %; pairs −9.5 / +12.2 |

**Regression verdict (>10 % rule, two-pair means): none.** The 8 MiB proxy
pair r2 (+12.2 %) reverses in r1 (−9.5 %) — scheduler-bound at four workers,
as p4b recorded. Verdict-path proof lines held on every tip arm:
splice `connections == requests`, intercepted `requests == 2 × connections`,
h2 intercepted 1 connection / 676 requests, `blocked 0`, `dropped_events 0`,
`minted_total 1`, `unwarmed_misses 0`. `certs_replay_zipf` hit rate 0.6732,
deterministic (synthetic; not evidence, MA-7).

## Post-review work E — review of the smoke fixes F7 (p3-04 S2) and F19 (p3-05 N12), 2026-09-03

Scope: the uncommitted working-tree diff of `fah-dns/src/{tcp,dot}.rs`,
`fah-http/src/intercept.rs`, `fah-http/tests/interception.rs`,
`CONFIGURATION.md` §`[https] max_connections`, p3-04 review rows S1/S2, p3-05
review row N12. Gate on the touched crates: `cargo fmt --check` clean,
`cargo clippy -p fah-dns -p fah-http --all-targets -- -D warnings` clean,
`fah-dns tcp::` 2/2, `interception` stall/watchdog tests 3/3 (3.3 s).

### Verified

- `Upstream` (the h2 sender to the origin) is created per intercepted session
  (`intercept.rs:174`), so an upstream h2 connection carries at most the 64
  streams of its one client session; `H2_CONNECTION_WINDOW = 64 × 64 KiB`
  covers the full stall set. Any smaller multiple reintroduces starvation at
  that many stalled streams — 64× is the only value that removes the
  head-of-line block.
- `sixty_four_stalled_h2_streams_…` fails on the old constant by
  construction: `drain_in_order` collects stream 0 while 63 siblings hold up
  to 64 KiB each (> 256 KiB). Not re-run against the old constant (no code
  edits in this review).
- `Accept for TcpListener` refined to `async fn` compiles `Send`; DoT calls
  the inherent `TcpListener::accept`, so `TCP_NODELAY` is set exactly once per
  socket on both listeners.
- Encrypted upstreams (hickory `tls_exchange`) already frame length + message
  in one buffer and set `TCP_NODELAY`; not affected by F19.
- p3-04 S1/S2, p3-05 N12 and the `CONFIGURATION.md` wording match the code.

### Findings

| # | Severity | Finding | Disposition |
| --- | --- | --- | --- |
| E1 | medium | F7 doubles the per-session stalled ceiling: old ≈ 4.25 MiB (64 × 64 KiB send + 256 KiB receive), new ≈ 8 MiB with downloads stalled, ≈ 12 MiB with uploads stalled too (client-leg receive window is 4 MiB as well), × `max_connections` 1024. Reached only by a listed client that stops reading. The alternative (32 KiB stream window → 2 MiB) halves single-stream throughput at RTT and was not weighed | **owner decision 2026-09-03: keep 64 × 64 KiB; documented here as the decision, no further code change. P3 on the RB5009 (§Runbook 5, 64 stalled streams) is the only authority for the real footprint** |
| E2 | low | same defect class as F19 unfixed on the upstream TCP fallback: `upstream/plain.rs:105-106` writes length and message separately, no `TCP_NODELAY`; the truncation fallback pays the same ≈ 40 ms on Linux. Rare path (TC=1 replies only) | deferred; file as p3-05 follow-up row or Phase 1 fix — not part of F19 |
| E3 | low | `TCP_NODELAY` lives in two places (`tcp::Accept for TcpListener` and inline in `dot::run_with:89`); principle 4 | deferred; `tcp::Accept::accept(&listener)` in `dot::run_with` removes the inline block |
| E4 | low | `tcp::frame_reply` splice may realloc + memmove per reply when the pipeline `Vec` has no spare capacity; sub-µs against the syscall and TLS record it saves. Pre-existing `unwrap_or(u16::MAX)` clamp would desync the stream on a > 65 535-byte reply; unreachable, the pipeline bounds replies | accepted |
| E5 | low | `poll_once` classifies a frame later than 250 ms as `Pending`; the first-poll assert `with_data == 64` fails if any first DATA lands after 250 ms on a loaded box. Second-poll asserts tolerate it | accepted; no CI, dev-box only |
| E6 | info | the Node-default-window test proves h2-crate round-robin scheduling under a 65 535 client window, not relay code; still guards "the relay never ends a starved stream" | keep |

**Verdict: PASS WITH DEFERRED FINDINGS** (E1 decided, E2/E3 deferred, E4–E6 accepted).

## Hand-off state, 2026-09-02 (end of session)

**Final gate on the whole tree** (Windows dev box, no environment variable
set): `cargo fmt --all -- --check` clean; `cargo clippy --workspace
--all-targets -- -D warnings` clean with and without `--all-features`;
`cargo test --all-features --workspace` — every suite green, 0 failed, no
`DEGRADED`/`SKIPPED` line (`security_phase3` 7/7, `e2e_https` 2/2 with the
full leg 5, `interception` 28/28, `api` 116/116); `cargo check --release -p
fastadhunter` clean.

**Superseded 2026-09-05 — read §Hand-off state, 2026-09-05 below first.** The
list that follows was true at the end of the 2026-09-02 session and is kept as
history: **all of it has since been committed.** Nothing in it is outstanding.

**Uncommitted at the time, per owner instruction ("do not commit"):** product code
`fah-model/src/{engine,lib}.rs`, `fah-http/src/{proxy,https,intercept}.rs`,
`fah-api/src/{certs,lib,ports,state,telemetry}.rs`,
`fastadhunter/src/{main,adapters}.rs`, `fastadhunter/Cargo.toml`, `Cargo.lock`;
tests `fah-http/tests/{interception,sni}.rs`, `fah-api/tests/api.rs`,
`fastadhunter/tests/{common/mod,e2e_https,security_phase3,history_e2e}.rs`;
docs `API.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `PERFORMANCE.md`,
`docs/measurement-traps.md`; evidence `p3-06-bench/` (`p4-*`, `probe-*`,
`p4b-*`, two scripts, `session.log`); this file. **Phase row flipped to
`AWAITING SOAK`** (owner instruction, 2026-09-02): dev-box work complete,
Step 4 not executed. Flip condition: the 24 h full-mode soak on the RB5009
records RSS ≤ 128 MB steady and the §Runbook 6 watch items, with Runbook 1–5
and 7 done and recorded here. Committed on `phase3-06` in the same
instruction; not pushed.

**Open, owner side:** `BASELINE_EXCLUSIONS` final names (Post-review work B);
dashboard kinds deferral needs a named task (I1); Step 5 doc sweep items not
covered above (deploy-rb5009.md §5c after the walkthrough, README modes row,
SECURITY.md row 3, ROADMAP, project-state); X2 follow-up task
(`cargo bench -p fastadhunter`); `FastAdHunter-pre3` worktree removal at phase
close; Runbook 1–7 on the device, P1–P7, the 24 h soak.

## Hand-off state, 2026-09-05 (superseded)

**Superseded 2026-09-08 — read §Hand-off state, 2026-09-08 below first.** Kept
as history. Two things in it are now wrong: the P-arm dispositions below belong
to campaign 1 and do not carry to the tip, and the ordering constraint reading
"the 0.3.1 soak ends 2026-09-08" is stale — the 0.3.1 soak was stopped on day 6
for the 0.3.3 deploy of 2026-09-07, and the 0.3.3 soak runs to **2026-09-14**
(integration audit F6, [project-state.md](../../project-state.md) §Now).

Supersedes the 2026-09-02 section above, whose "uncommitted" list is history.
Tree clean on `phase3-06` apart from the owner's own `docs/code-review/phase2.6/`
files; `origin` and `backup` both at the same commit.

**Decided this session, all recorded in
[p3-06-testing-results.md](p3-06-testing-results.md):**

| Item | Disposition |
| --- | --- |
| `SPLICE_BUF` | **16 KiB per direction stays, budget stays 32 MiB**, `max_connections` unmoved. The sweep establishes no CPU-per-relayed-byte advantage. Shipped configuration, **not** a P1 pass — P1-LAN still owes the ≥ 100 MiB/s confirmation and now carries one build, not two |
| P4 | **row-setter withdrawn.** Three valid sessions; the declared `p50(transport) − p50(UDP)` statistic is not robust — the UDP control moved 73 % and carried DoT added from +61 to +17 / +16 µs. Both PERFORMANCE.md rows return to `TBD`, no replacement picked. Transport paths healthy, both beat their ×9 prediction. Client/server CPU attribution **unresolved** — `/tool/profile` charges all container work to one aggregate task |
| P5 | **closed.** FAIL at 1.389 ms under the frozen statistic, which is not reinterpreted. The increment tracks the CPU speed regime, not the mint; paired per host 0.728 ms, 74 of 240 pairs still over 1 ms. `certs_mint` passes at 450.88 µs and **is** the PERFORMANCE.md row-setter for the cold-`prewarm` row, now filled. **A recorded budget miss, not a demonstrated defect — it does not block Phase 3 closure.** No further experiment, no `fah-certs` change |

**Flip condition, restated.** The 2026-09-02 wording ("Runbook 1–5 and 7 done")
predates these dispositions and should not be read as requiring every P-arm.
`AWAITING SOAK` flips when the 24 h full-mode soak on the RB5009 records
RSS ≤ 128 MB steady and the §Runbook 6 watch items, with Runbook 1–4 and 7 done
and recorded here. **P5 does not gate it.** Runbook 5's parked arms — P1-LAN,
P1-control, P2, P3 — are blocked on a second wired LAN endpoint and are their
own decision, not a soak precondition.

**Ordering constraints a later session should not rediscover:**

- Runbook 6 cannot start before the 0.3.1 soak ends **2026-09-08**, needs a
  phase-3 deploy (its own approval), and needs N3 (`dot` state) plus the L5
  `listeners` telemetry block first — watch item (b) has no read path without
  them. It carries **P7, P8 and P9 proper**.
- Runbook 4 needs `BASELINE_EXCLUSIONS` settled first, or it proves nothing
  about exclusions.
- Runbook 7's ninth `ca/generate` is the `409 archive_full` check and the
  archive sits at **7 of 8** — do not spend it. Restart the probe to purge the
  leaf cache instead.
- P1-LAN, P1-control, P2 and P3 all unblock from one thing: a second **wired**
  LAN endpoint with Node. P2 additionally needs two same-family IPv4 addresses
  on it, P3 an h2 origin under a public name with a publicly trusted
  certificate (delta 14).

**Deferred, accepted:** F10, F16. **Withdrawn:** F6.

## Hand-off state, 2026-09-08 (morning, superseded)

Superseded by §Hand-off state, 2026-09-08 (session 2) at the end of this file.
Tree clean on `phase3-06` at **`61ea35c`**
("docs(phase3/p3-06): re-plan the probe campaign for the post-merge tip");
`origin` and `backup` both at that commit.

**What this session changed:** merge `e0c6071` re-homed the HTTPS listener onto
the HTTP allocation domains, so campaign 1's on-device figures — taken on
images built at `a2d0802`, where `fah-http/src/domain.rs` does not exist and
`runtime.http_runtimes` is not a config key — measure an execution model that
is no longer shipped. Owner decision: **full re-run from zero, nothing
carried.** The three p3-06 plans were rewritten for campaign 2, this file
gained §Pre-declaration — campaign 2, and
[p3-06-testing-results.md](p3-06-testing-results.md) gained a supersession
header. No figure and no `results-*/` directory was edited.

**Nothing has been executed.** State per step of the leading document
([p3-06-phase3-verification-plan.md](../../../plan/wip/phase3/p3-06-phase3-verification-plan.md)):

| Step | State |
| --- | --- |
| 1 — dev-box benches | **re-run owed.** Campaign 1's D1–D14 A/B'd against `64be513`, which also predates the allocation domains; the baseline is now `main` `857865d`. Setup first: the `https_sni_splice` harness rebuilt on `TlsServer::bind/serve`, a second checkout at `857865d`, a recorded browsing-host corpus for the leaf-cache arm |
| 2 — security suite + coverage gaps | suite green at the tip (`security_phase3` 7/7, `interception` 33/33, `sni` 9/9 — integration audit §Measurements). Owed: F2 (`domains: 1` splice test in `fah-http/tests/sni.rs`), F5 (one `https.*` key in the `fah-api` boot-key classification test), and the F3 hand-off saturation property |
| 3 — offline full-mode E2E | `e2e_https` 2/2 at the tip. Owed: A4 — pin `runtime.http_runtimes` in the fixture and assert the thread name, so the green line means "full mode on the domain lane" rather than an accident of the box's core count |
| 4 — on-device | not started. 4.0 (probe preconditions) blocks everything below it; 4.1 (P10) must precede 4.2 because it fixes the N every later figure is taken at |
| 5 — documentation sweep | not started |

**Flip condition, unchanged in substance.** `AWAITING SOAK` flips when the 24 h
full-mode soak on the RB5009 records RSS ≤ 128 MB steady and the §Runbook 6
watch items, with Runbook 1–4 and 7 done and recorded here. P5 does not gate
it (2026-09-05 disposition stands as a disposition; its figure does not).

**Ordering constraints a later session should not rediscover:**

- The 24 h full-mode soak (Step 4.8) cannot start before **2026-09-14** — the
  0.3.3 soak owns the production container until then. It needs its own deploy
  approval, and it needs N3 (`dot` state) plus the L5 `listeners` telemetry
  block first, or watch item (b) has no read path. It carries P7, P8 and P9
  proper.
- P10 (Step 4.1) cannot run while the probe is attached to `envlists=fah-env`:
  that list pins production's `FAH__RUNTIME__HTTP_RUNTIMES=2`, and precedence
  is `defaults < file < FAH__ env`, so neither the probe's TOML nor
  `POST /api/v1/config` can move N. The probe needs its own env list.
- The probe's stored config carries `strategy = "fallback"`, rejected at load
  since `fa9451a`. **The probe will not boot on a tip build until it is
  fixed.**
- Runbook 4 needs `BASELINE_EXCLUSIONS` settled first, or it proves nothing
  about exclusions.
- Runbook 7's ninth `ca/generate` is the `409 archive_full` check — do not
  spend it. Restart the probe to purge the leaf cache instead.
- P1-LAN, P1-control, P2 and P3 need the Mac as second LAN endpoint. P2
  additionally needs two same-family IPv4 addresses on it; P3 an h2 origin
  under a public name with a **publicly trusted** certificate, which the Mac
  does not supply. The Mac is currently on Wi-Fi, which the testing plan's Mac
  precondition 1 does not allow.
- `p2-handshake.mjs` has no Darwin branch — it calls `ip -4 addr`, which macOS
  does not have — so the Mac cannot run P2 until that lands.
- The smoke plan's Layer 0 `--connect-to` SNI proof gates every `oha` arm and
  has never been run.

**Owner decisions outstanding:**

| # | Decision |
| --- | --- |
| Delta 1 | P1-LAN's absolute ≥ 100 MiB/s gate withdrawn in favour of a relative one (§Pre-declaration — campaign 2). Needs sign-off |
| Audit F1 | `main`'s IP-literal fix covers `Proxy` only; on the HTTPS path an allowed IP-literal SNI goes to the resolver and fails every time, while CONFIGURATION.md says the switch governs both. Port the fix, or narrow the doc and the `tls_server` test to HTTP-only |
| Wi-Fi vs wired | whether the Mac's Wi-Fi link stands for P1/P2/P3 or the plan's wired precondition holds. The deciding measurement — 5 P1-control runs, min/max spread against the 10 % relative gate — needs no probe and no router |
| p3-04 h2-stall | campaign 1 found one h2 stream of 64 answering through the terminate leg and reproduced it on the dev box (control 64/64, stall 5/64). **Never filed as a finding.** The plan requires it filed before P3 runs on the device |

**Deferred, accepted:** F10, F16. **Withdrawn:** F6.

## Hand-off state, 2026-09-08 (session 2, dev box) — superseded

Supersedes every hand-off section above. Working tree on `phase3-06` at
`ce3c6e6`, **dirty**: four test-only files changed, plus three untracked
directories. `origin` and `backup` are still at `ce3c6e6` — **nothing was
committed or pushed this session.**

**What this session did.** Steps 2 and 3 closed, smoke Layer 0 run for the
first time, and the Step 1 dev-box bench session run end to end. Figures live
in [p3-06-testing-results-2.md](p3-06-testing-results-2.md), created this
session with owner approval.

### Step state

| Step | State |
| --- | --- |
| 1 — dev-box benches | **D1–D14 run; session valid** (control +3.0 %, inside ±5 %). No regression on the >10 % rule. **Owed:** D6's `SPLICE_BUF` 16 vs 64 KiB half (needs a `src` edit — owner approval), and D12 against a real distribution (capture running) |
| 2 — security suite + coverage gaps | **closed.** Suite green at the tip; F2, F5 and the F3 saturation property all landed |
| 3 — offline full-mode E2E | **closed.** A4 landed; `e2e_https` 2/2 |
| 4 — on-device | not started. 4.0 blocks everything below it; 4.1 (P10) must precede 4.2 |
| 5 — documentation sweep | not started |

### Code landed (uncommitted)

- **F2** — `an_allowed_sni_is_spliced_on_an_allocation_domain` in
  `crates/fah-http/tests/sni.rs`. Builds a one-domain rig (`Server::bind` +
  `serve_domains(NonZeroUsize::MIN, …)`, then
  `TlsServer::serve_domains(proxy, &http)`), asserts a byte-identical splice
  and that the session ran on `fah-http-0`. The thread name is observed through
  a `RecordingResolver` that records `std::thread::current().name()` — the
  resolve runs on the domain thread, so this is falsifiable: on
  `TlsServer::serve` the name would be libtest's.
- **F3** — `a_saturated_https_lane_leaves_the_http_lane_bounded_and_leaks_no_permit`,
  same file. HTTPS lane held at `max_connections = 2` with 40 sockets queued
  behind it; asserts the gauge never exceeds the ceiling, the HTTP lane still
  answers on the same domain, both gauges return to 0, and all 42 sockets are
  judged once the ceiling frees.
- **F5** — `https.listen.port` and `https.interception.clients` added to the
  boot list in `crates/fah-api/src/config_store.rs`.
- **A4** — `[runtime] http_runtimes = 2` pinned in `full_mode_config`
  (`crates/fastadhunter/tests/common/mod.rs`), which puts **`security_phase3`
  on the domain lane too**, and `e2e_https` now asserts the binary logged
  `HTTPS proxy serving on the HTTP allocation domains` and `http_runtimes=2`.

**A4 deviation, recorded.** The plan says "assert the thread name". Thread names
are not observable across the process boundary — `fah-logging` sets no
`with_thread_names` — so `e2e_https` asserts the dispatch log line instead, and
F2 carries the thread-name assertion in-process. Together they discharge A4's
intent; separately, neither does.

**Gates green** at the end of the session: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --all-features --workspace`. `sni` 11/11, `interception` 33/33,
`security_phase3` 7/7, `e2e_https` 2/2.

### Smoke Layer 0 — first run, all five checks

Output in
[p3-06-probe/smoke-20260908T0905Z/layer0/](p3-06-probe/smoke-20260908T0905Z/layer0/).

| Check | Result |
| --- | --- |
| removed strategy | **PASS** — boot refused, exit 1, `"fallback" was removed after 0.3.3; "adaptive" is the only strategy`. The message names `strategy` and gives line/column but not the dotted path `dns.upstreams.strategy` the plan quotes — cosmetic |
| N readable | **PASS** — `http_runtimes = 1` reads back `1` from `GET /api/v1/config` |
| N default on this box | **PASS** — **16** (32 cores, `max(1, cores/2)`), not 2 |
| `oha` pinned | **PASS here, NOT RUN on the Mac** — bobdenaut has 1.16.0; the Mac half is owner-owed |
| **`--connect-to` preserves SNI** | **PASS.** Blocked name goes to `listeners.https.blocked` 0 to 1, `oha` 0 % success, `resolve_failures` and `refused_destination` both 0. Allowed name over the **same socket target** answers 200 with `blocked` unchanged. Splice served the origin's own certificate, identical to direct |

**Every `oha` arm is unblocked by that last row.** Layers 1–3 are not yet run;
they need an idle box, since a browser makes every script `INVALID` by design.

### Ordering constraints a later session should not rediscover

Unchanged from the previous hand-off except where noted:

- The 24 h full-mode soak (Step 4.8) cannot start before **2026-09-14**. It
  needs its own deploy approval. **Its stated prerequisites are already met** —
  see §Telemetry prerequisite, verified below. Watch item (b) is unblocked.
- P10 (Step 4.1) cannot run while the probe is on `envlists=fah-env`.
- The probe's stored config carries `strategy = "fallback"` and **will not boot
  on a tip build**. Layer 0 confirmed the exact failure shape. **The fix is to
  delete the line, not set it to `"adaptive"`** — `UpstreamStrategy` is a
  single-variant enum, the default is already adaptive, and the key now exists
  only as a migration shim that turns an old value into a named error.
- Runbook 4 needs `BASELINE_EXCLUSIONS` settled first.
- Runbook 7's ninth `ca/generate` is the `409 archive_full` check — do not
  spend it.
- P1-LAN, P1-control, P2 and P3 need the Mac. The Mac is on Wi-Fi, which the
  testing plan's precondition 1 does not allow.
- `p2-handshake.mjs` has no Darwin branch.
- **`cargo bench -p fastadhunter` does not compile at either checkout** — the
  `fah-api/test-harness` dev-dependency reaches the bench (release) profile and
  trips `compile_error!`. Use `--config 'profile.bench.debug-assertions=true'`
  on the command line rather than editing `Cargo.toml`. Same for
  `cargo test --release -p fastadhunter`; `encrypted_latency` instead keeps the
  harness in the dev profile and drives a release binary via `FAH_E2E_BINARY`.

### Open findings from this session

**D8's spliced arm is 8 × direct, and unresolved.** Owner-directed
attribution (bounded, no implementation change): the whole difference sits in
the proxy setup + relayed handshake window — a TLS handshake only on the direct
control; TCP connect, time-to-first-byte and teardown are all
identical or faster through the splice. Five hypotheses tested and refuted —
EOF propagation, Nagle on the upstream leg, the domain hand-off (N = 0 gave
both the fastest and the slowest reading), Windows TIME_WAIT pressure (1 % of
the ephemeral range), and the SNI resolve (0.78 ms median). The proxied path
drifts 3 × within a session while the direct control holds an 8 % band. Full
evidence in [p3-06-testing-results-2.md](p3-06-testing-results-2.md) §D8
attribution. **Consequence:** D8's absolute values are not budget rows on this
box; the `intercepted` minus `spliced` delta of **+3.6 %** survives because
both legs pay the unexplained cost; **P2 on the device is the authority.**

### Owner decisions outstanding

| # | Decision |
| --- | --- |
| Delta 1 | P1-LAN's absolute ≥ 100 MiB/s gate withdrawn in favour of a relative one. Needs sign-off |
| Audit F1 | `main`'s IP-literal fix covers `Proxy` only; on the HTTPS path an allowed IP-literal SNI goes to the resolver and fails every time, while CONFIGURATION.md says the switch governs both. Port the fix, or narrow the doc and the `tls_server` test to HTTP-only |
| Wi-Fi vs wired | whether the Mac's Wi-Fi link stands for P1/P2/P3 |
| p3-04 h2-stall | campaign 1's finding (control 64/64, stall 5/64) is **still never filed**. The plan requires it filed before P3 runs on the device |
| D6 64 KiB | the `SPLICE_BUF` 16-vs-64 KiB half of D6 needs a `src` edit to build the second variant — a code change, not yet made |
| D8 unresolved | whether to spend more on the 8 × handshake now, or let P2 on the device settle it |

**Deferred, accepted:** F10, F16. **Withdrawn:** F6.

### Telemetry prerequisite — verified already met, no API.md edit needed

The verification plan's §Step 4.8 watch item (b) names two prerequisites for
the soak: an API.md edit for the per-listener block, and settling p3-04 L5
("on the terminate leg `requests` is per connection while `blocked` /
`refused_claim` are per request, so `blocked > requests` is possible on one
listener"). **Both were already discharged by §Post-review work A on
2026-09-02.** Checked this session rather than assumed:

- **API.md already documents the shipped block.** `GET /api/v1/telemetry` in
  API.md carries a full `listeners` JSON sample with all twelve fields —
  `connections`, `requests`, `blocked`, `refused_claim`,
  `refused_destination`, `resolve_failures`, `upstream_failures`,
  `upstream_cert_failures`, `non_http`, `non_tls`, `hello_timeouts`,
  `dropped_events` — plus prose on which fields are HTTPS-only, which are
  `:80`-only, and how `refused_claim + refused_destination` sums to
  `counters.http.refused`. A live read from the smoke instance returns exactly
  that field set, same names. **Nothing to add.**
- **p3-04 L5's claim is refuted, by code and by measurement.** API.md states
  `blocked ≤ requests` holds per listener. Every site that increments `blocked`
  is immediately preceded by a `requests` increment on the same path:
  `https.rs:163`/`:172` (SNI verdict), `https.rs:287`/`:291` (no-SNI),
  `intercept.rs:263`/`:287` (inner request on the terminate leg),
  `proxy.rs:352`/`:376` (HTTP). The invariant holds by construction.
  Independently, the D8 bench counters show the terminate leg counting
  **per request, not per connection**: `https_handshake/intercepted`
  `connections=3061 requests=6122` — exactly 2 × (one SNI verdict plus one
  inner request per connection) — and `https_h2_download/intercepted`
  `connections=1 requests=676` (one SNI verdict plus 675 h2 GETs). This file
  already records the corrected invariant at §Post-review work A.

**Stale text to fix, owner approval needed.** The claim survives in
[p3-06-phase3-verification-plan.md](../../../plan/wip/phase3/p3-06-phase3-verification-plan.md)
§Step 4.8 watch item (b), which still names the API.md edit as owed and still
states `blocked > requests` is possible. That is the only place the stale
wording remains; it is a plan-file edit, not a review-file one, so it is
proposed rather than made.

## Smoke session, 2026-09-08 — Layers 0–3 complete

Output root
[p3-06-probe/smoke-20260908T0905Z/](p3-06-probe/smoke-20260908T0905Z/), one
directory per layer and posture. Per the smoke plan, **nothing in this section
is a measurement** — no number here is cited, budgeted or copied into
[p3-06-testing-results-2.md](p3-06-testing-results-2.md). Runs after
2026-09-08 10:45Z carry `host not idle: brave` because the owner resumed
browsing; Layer 1 passes on properties (issuers, counters, byte counts), not
timings, so the label is recorded and does not weaken a row.

`run.log` in every directory carries `tip=b53968ca3b1b-dirty` — the campaign
must not start from a dirty tree, and this session's script fixes are why it
is dirty.

### Layer 0 — the two new boot paths, first run ever

| Check | Result |
| --- | --- |
| removed strategy | **PASS** — boot refused, exit 1, `"fallback" was removed after 0.3.3; "adaptive" is the only strategy`. This is the shape the probe hits on the router. The message names `strategy` with line/column but not the dotted path the plan quotes — cosmetic |
| N readable | **PASS** — `[runtime] http_runtimes = 1` reads back `1` from `GET /api/v1/config` |
| N default on this box | **PASS** — **16** (32 cores, `max(1, cores/2)`), not 2 |
| `oha` pinned | **PASS here, NOT RUN on the Mac** — bobdenaut has 1.16.0; the Mac half is owner-owed |
| **`--connect-to` preserves SNI** | **PASS.** Blocked name drove `listeners.https.blocked` 0 to 1 with `oha` at 0 % success and `resolve_failures`/`refused_destination` both 0; an allowed name over the **same socket target** answered 200 with `blocked` unchanged. Splice served the origin's own certificate, identical to direct |

That last row is the prerequisite every `oha` arm depends on. It is met.

### Layer 1 — all 11 scripts, N ∈ {0,1,2}, both client postures

| Script | Result |
| --- | --- |
| `p0-sni` | **valid, gate pass at N=0/1/2.** Blocked and no-SNI 5/5 `closed_silent`, allowed 5/5 `server_hello`, `telemetry_delta.blocked = 5`. N=0 confirms the pre-merge shared-runtime path still works |
| `p1-lan --direct` | **valid.** Control band 668.95 / 863.84 / 1170.62 MiB/s, 5/5 runs |
| `p1-lan` spliced | **valid, gate pass at N=0/1/2** after fix 1 — 1.43 / 1.44 / 1.34 × control, floor 777.45. `served_issuer = smoke-origin`, so the splice served the origin's own certificate |
| `p1-lan --connections 8` | `p1-aggregate.json` written, `degraded` — `/tool/profile` is router-only. Correct |
| `p2-handshake` | **INVALID as designed** — `ip -4 -o addr: spawnSync ip ENOENT`. Confirms the missing Darwin/Windows branch on the real script |
| `p3-h2stall` throughput | **valid, gate pass at N=0/1/2** — 242.2 / 192.0 / 198.7 MiB/s, exactly 8 388 608 bytes each |
| `p3-h2stall` RSS | **INVALID as designed** — `process_rss is null on this probe`. See §The S2 barrier below |
| `p4-lan` | **valid.** All transports n=200, `unanswered`/`unmatched` 0, `first_answer.answers = ["0.0.0.0"]`. DoT `served_issuer = FastAdHunter CA`, `tls TLSv1.3`; DoH `doh over h2` |
| `p5-mint` | **valid, gate pass.** `minted_total = 16`, `evictions = 0`, both arms `FastAdHunter CA`; repeat pass 16 prewarm hits and 0 mints |
| `p6-certs-time` | **valid, gate pass** on a fresh store. Four rows 200, `api_certificate_after.source = "imported"`, generate 2.541 ms / import 10.376 ms, `ca-after-p6.pem` written with a new fingerprint |
| `p7-store` | **valid, gate pass.** 20 traversal paths × 40 requests, all rejected, `leaks: []` on every row |
| `p10-domains` | all five arms **valid** at N=2 (close, mixed, tls-spliced, tls-intercepted, transfers) and close/mixed/tls-spliced at N=0/1. `oha_version 1.16.0` and `--worker-threads` recorded in every file; N read back from `/config` matches `--n` every time. Issuer samples before and after: spliced legs `smoke-origin`, intercepted leg `FastAdHunter CA` / subject `127-0-0-1.nip.io` |
| `p10-connrate` | keep-alive arm produced **both `rps` and `conns_per_s`** — the field that makes the phase-2.6 comparison possible — plus percentiles and 20 requests per connection (full 2816/2860) |
| `p10-dnsload` | sent 5997/6000 at 299.8 qps, `timeouts = 0`, `unmatched = 0`, blocked share 1233/1233 answering `0.0.0.0` |

Every arm is `degraded` for one reason that is not a defect: `cpu_user_ms` /
`cpu_system_ms` are null off-container, so no `cores` figure exists on this
box. Same class as `process_rss`.

### Layer 2 — 21 negative-path rows, every one produced its expected refusal

`no API key`, `/health unreachable`, `N mismatch`, `--worker-threads unset`,
`oha for the keep-alive arm`, `oha version drift`, `--ca-key unreadable`,
`--ca-key not a private key`, `allowed name does not open`,
`p1 byte count wrong`, `p1 control missing` (after fix 3),
`p5 cache headroom`, `p5 no CA`, `p6 archive cap`,
`engine.mode without https`, `origin outside egress`,
`p3 host not listed`, `busy host`, `issuer sample skipped`,
`identity precondition (p2)`, `dirty checkout`.

Two rows fired without being asked for, which is the better kind of evidence:
**busy host** caught the owner's browser (`host not idle: brave`), and the
**toy-origin** row reproduced itself — see §The toy-origin trap.

Not runnable here, recorded rather than skipped silently: `N unreadable`
(needs a build whose `/config` omits `runtime`), `both addresses unlisted`
(needs the Mac), `barrier not met` (needs `process_rss`).

### Script fixes — three, all under the smoke plan's own remedy

The plan's rule: *a script bug found here is fixed in the script, re-run, and
listed; nothing in `p3-06-testing-plan.md` changes.* That plan is untouched.

1. **`p1-lan.mjs` carried campaign 1's gate.** It tested
   `median ≥ 100 MiB/s AND inside P1-control's min–max`. The min–max half is
   two-sided, so it **failed a run for being faster than the control**: N=1
   measured 1213.224 MiB/s and was reported `pass: false`. Testing-plan
   delta 1 withdrew that gate for `median ≥ 0.9 × control median`, absolute
   MiB/s demoted to a diagnostic column. Now implemented as declared, with
   `floor_mib_s` and `ratio_to_control` recorded. Re-run at all three N: pass.
2. **`p10-domains.mjs` counted failures that never reached the listener.** The
   blocked-arm check required `blocked ≥ requests + failed`, so
   `tls-spliced-blocked` was INVALID on a 4-socket shortfall out of 318 196,
   and again at N=0 on **19 `os error 10048`** — Windows ephemeral-port
   exhaustion. Both classes are client-side and cannot appear in `blocked`.
   Now excluded by kind (deadline aborts, `os error 10048`,
   `usage of each socket address`). Re-run at N=0 and N=2: valid.
3. **`p1-lan.mjs` was silent without a control.** With no `--control` it
   reported `pass: null` and carried on — a row that looks like a result and
   is none. Under delta 1 the gate is purely relative, so no control is not a
   weaker gate but no gate. Now it auto-discovers `p1-control.json` from
   `--out` (which is what the plan's Layer 2 row means by "no prior `--direct`
   result in the run directory") and is INVALID when neither the flag nor the
   file is present. Both branches verified.

**New file, not a fix:** `smoke/static-origin.mjs`. `p10-domains.mjs` names
the origin it wants — `static-web-server`, as in phase 2.6, "never a toy one"
— which is an external binary, not a repo script. The stand-in serves the five
objects from memory over plaintext and TLS so the p10 rows can run on this box
at all. Its header says plainly that it is not that server and that no figure
may be carried from it.

### The toy-origin trap, reproduced by accident

`p10-connrate` at 48 concurrent keep-alive loops drove **11 486 502s out of
68 200 (17 %)** through `static-origin.mjs`, and the script flagged the arm
`degraded` exactly as the plan's §Traps row says it should. Two things follow:
the detection works, and the plan's insistence on a real static server for
anything load-bearing is now backed by a measurement on this box rather than
by assertion.

### The S2 barrier is still owed

The p3-04 S2 stall barrier (control 64/64) lives inside `p3-h2stall`'s **RSS**
arm, and that arm invalidates on `process_rss is null` before producing a
barrier figure — the kernel reading is `/proc`-backed and in-container only.
The warm-up succeeded on both the stall and control runs (`200`, 3 B, issuer
`FastAdHunter CA`), so the terminate leg is healthy, but **Layer 1 did not
discharge the regression check.**

Layer 3's `fah-probe` container is the first place `process_rss` is non-null.
**It was not enough, and that is now measured rather than predicted** — see
the Layer 3 S2 subsection below, where the probe answered
`upstream_cert_failures = 1`. The image is a plain release build, so
`FAH_TEST_UPSTREAM_ROOT` is inert and a local self-signed origin fails
certificate validation on the upstream leg. The plan requires a **publicly
trusted** h2 origin under a public name, proven first with
`smoke/h2-preflight.mjs`. That is the same outstanding blocker as the
device-side P3. The barrier therefore moves to the device campaign and stays
owed.

### Correction — one D8 refutation is withdrawn

[p3-06-testing-results-2.md](p3-06-testing-results-2.md) §D8 attribution lists
Windows TIME_WAIT pressure as refuted, on a reading of 165 TIME_WAIT against
16 384 ephemeral ports. **That reading was taken after the run had drained and
does not support the conclusion.** During Layer 1's `p10` close arm this box
reached **15 543 TIME_WAIT — 95 % of the ephemeral range** — and was observed
draining to 12 within about 100 seconds. The D8 attribution runs were four
back-to-back `Connection: close` arms inside roughly two minutes, and the
spliced path burns two sockets per request where direct burns one, which fits
the observed 7 → 12 → 21 ms drift against a flat direct control.

**D8 therefore stands at four hypotheses refuted, not five, with port
pressure re-opened as the leading candidate.** Not chased further, per the
owner's instruction; recorded because a wrong refutation is worse than an open
question. P2 on the device remains the authority for that row.

### Layer 3 — the four images, x86 Docker on the dev box, 2026-09-08

All four built at the tip for `linux/amd64`; campaign 1's `a2d0802` images are
retired. Raw output in `p3-06-probe/smoke-20260908T0905Z/layer3/` (image runs)
and `layer3-l1/` (the Layer 1 re-run). Neither directory is committed — they
are generated artifacts, and `work/` additionally holds a CA private key, an
API key and a session secret.

| Image | Verdict | Evidence |
| --- | --- | --- |
| `Dockerfile.certs` | pass | four benches at **default** 3.0000 s warm-up and 5 s measurement; `certs_mint` point estimate 51.651 µs; `docker top` shows `/fah-certs --bench`; exit 0 |
| `Dockerfile.splicebench` | pass | five `rep=1 arm=splice` lines, one `loopback_origin`, the candidate table, `pick: none — no in-budget candidate reaches 0.9 x best 5598.3`, five `counters` lines; exit 0 |
| `Dockerfile.p4` | pass | `FAH_E2E_BINARY override active: /fah-probe`; three transports x 3 rounds x 2000 (n = 6000 each); `DoH over Some(HTTP/2.0)`; no `EACCES`; `docker inspect` shows `User=65532:65532`; `test result: ok` in 1.56 s |
| `Dockerfile.fahprobe` | pass (boot) | all four listeners bound on `0.0.0.0`; `dropped privileges after binding uid=65532 gid=65532`; `docker top` shows `/fah-probe` as 65532; health `healthy`; oisd refreshed, 60 673 rules — so both the upstream at `192.168.10.1:53` and outbound HTTPS work from inside |

`docker top` cannot sample the documented `splicebench --reps 1 --size-mib 8`
run: it completes in under a second. The line above was taken from
`--reps 6 --size-mib 512`, which changes nothing that row asserts.

#### Layer 1 re-run against the container

| Script | Verdict | Note |
| --- | --- | --- |
| `p0-sni` | valid | `blocked` moved 5 on 15 connections / 15 requests, gate `pass: true`. The **first** attempt was correctly `INVALID` — `resolve_failures 5`, "`--blocked` is not blocked by the probe" — because a fresh `/config` carries no user rule. Seeding the blocking user rule and a CA fixed it. The invalidity rule doing its job, not a defect |
| `p5-mint` | valid | `minted_total` 0 to 16, `evictions = 0`, both arms `served_issuer = FastAdHunter CA` |
| `p4-lan` | valid | udp / dot / doh each `n = 200`, `unanswered = 0`, `unmatched = 0`, `["0.0.0.0"]`, dot issuer `FastAdHunter CA`. Docker Desktop's UDP relay held at `--queries 200`, as the plan requires |
| `p7-store` | valid | 20 paths / 40 requests, `other_200: 0`, `leaks: []`, gate `pass: true`; the image ships `/web`, so 24 rows answer the SPA shell |
| `p6-certs-time` | valid | four rows `200`, `api_certificate_after.source = "imported"`, `ca-after-p6.pem` written and differs |
| `p10-domains` (N = 1) | degraded | spliced halves 5/5 and 40/40, 0 failed, 0 x 502. Control halves 0/40 — the host cannot route the bridge subnet, the same limitation the plan already records for `p1-lan --direct` |
| `p3-h2stall` | INVALID | see the S2 subsection below — `upstream_cert_failures = 1` |
| `p1-lan --direct` | not run | plan's own instruction |
| `p2-handshake` | not run | Mac only |

#### The two things only the container shows

**`fahprobe-env` precedence — proven.** The file says
`runtime.http_runtimes = 2`; the container was started with
`-e FAH__RUNTIME__HTTP_RUNTIMES=1`. The boot log reads `http_runtimes=1` and
`GET /api/v1/config` reads back `1`. Env beats file, and `p10` reads the value
back from the API — the dress rehearsal the router's `fahprobe-env` needs.
What remains owed for item 3 of the campaign checklist is the attachment, not
the mechanism.

**`process_rss` is non-null — proven.** `p10`'s close arm reported
`cores=0.15`, derived from `cpu_user_ms + cpu_system_ms` deltas, and
`ΔRSS end=11.23 max=12.26 MiB (floor 41.33 MiB)`. All three are null on
Windows. This is the reading the P3 RSS arm needs; it is the *origin*, not the
telemetry, that still blocks that arm.

#### Environment deviations — every one deliberate, none silent

1. The image is tagged `fah-fahprobe:smoke`; the plan's table says
   `fah-probe:smoke`. Tag only — the entrypoint is `/fah-probe`.
2. The config is **not** only the Layer 1 TOML with `0.0.0.0` addresses. Two
   further keys changed: `egress.allow_destinations` gained
   `"172.16.0.0/12"`, without which the container reaches no origin at all;
   and `https.interception.clients = ["127.0.0.1", "172.30.0.1"]`. The second
   entry is what the probe actually sees after Docker's SNAT; the first is
   there only because `p3-h2stall` tests its own precondition against the
   **host-side** local address and would otherwise refuse before connecting.
   On the device neither entry is needed in this form.
3. A user-defined network `fah-l3` (172.30.0.0/16) with fixed addresses —
   probe `.10`, origin `.20` — replaces the default bridge, so the origin has
   a stable name (`172-30-0-20.nip.io`, which the LAN resolver returns
   unfiltered). The default bridge gives no address control.
4. Port 8080 is published in addition to the plan's four, so the HTTP lane can
   be driven.
5. The origin is `smoke/static-origin.mjs` in a `node:22-alpine` container —
   the stand-in, never `static-web-server`. No figure is carried from it, per
   that file's own header and the toy-origin subsection above.
6. `/config` was seeded in-container exactly as smoke plan section 1.1
   requires of a fresh boot: `PUT /api/v1/rules/user` with
   `||ads.smoke.test^`, then `POST /api/v1/certificates/ca/generate` and a CA
   export.

#### Harness robustness finding — `p10-domains` loses completed arms on a mid-run throw

`p10-domains` ended its first N = 1 run with
`INVALID: uncaught exception: ENOBUFS bind ENOBUFS 0.0.0.0`, thrown after the
close arm had finished. The arm's figures had been computed and were written
to `run.log` — `requests=11250 failed=8 rps=562.7 p50/p95/p99=4.706/24.518/41.788 ms 502=72`,
plus the `cores` and RSS line quoted above — but `p10-N1.json` was written
with `arms: {}`. Any error between two arms currently discards every arm
already completed.

The trigger was host-side: this box was carrying **12 530 TIME_WAIT** sockets
against an ephemeral range of 16 384 when the throw happened, the same
exhaustion recorded in the D8 correction subsection above. `netstat` itself
failed first, with `spawnSync netstat ENOBUFS`.

**Recorded, not fixed, on the owner's instruction.** No gate failed, and the
condition is specific to Windows ephemeral-port exhaustion — the RB5009 will
not reproduce it. If it is ever fixed, the change is to persist each arm as it
completes rather than at the end.

#### The S2 barrier — still owed, now with the cause on record

Layer 3 was the first place the P3 RSS arm could have been discharged, and it
was not. It did not fail: it could not run.

Against the container, `p3-h2stall`'s throughput arm reported
`FAILED connect: ECONNRESET`, and the probe's own telemetry names the reason —
`upstream_cert_failures = 1`, `refused_claim = 0`. Interception was claimed and
the terminate leg served the FastAdHunter CA correctly; the probe then refused
the **origin's** self-signed certificate on the upstream leg. There is no way
around it in a release image: `FAH_TEST_UPSTREAM_ROOT` is read inside
`#[cfg(feature = "test-harness")]` in `crates/fastadhunter/src/main.rs`
(`test_harness_upstream_client_config`), and `CONFIGURATION.md` exposes no
file-level equivalent.

**So the barrier is neither met nor missed — it is unexercised, and the
publicly trusted h2 origin is genuinely still owed.** It must not be recorded
as a pass, and a `barrier: not met` reading from this topology would be an
artefact of the origin, not a p3-04 regression. The requirement is unchanged:
a publicly trusted h2 origin under a public name, proven first with
`smoke/h2-preflight.mjs` (`alpn h2`, exactly 8 388 608 bytes with
`content-length`), with the origin, path and that line recorded. Campaign
checklist items 6 and 7 both remain open on it.

#### Layer 3 verdict

The container-only mechanisms the campaign depends on are proven: env-over-file
precedence, `process_rss`, and every Layer 1 script that does not need a
routable origin. What Layer 3 cannot supply is an origin — neither a publicly
trusted one for P3, nor a host-routable one for any `--direct` control arm.
Both move to the device campaign.

## Step 4 runbook — separate file

The owner-executed on-device procedure lives in
[p3-06-phase3-verification-runbook.md](p3-06-phase3-verification-runbook.md).
Entries `R0`–`R11`, each with the exact command, the resulting state, when it
takes effect, whether a restart is required, and the rollback. Prepared
2026-09-08; nothing in it has been run.

## Hand-off state, 2026-09-08 (session 3, RB5009) — current

### Flip condition — restated here because the older copies are superseded

`AWAITING SOAK` flips when the 24 h full-mode soak on the RB5009 records
**RSS ≤ 128 MB steady** and the §Runbook 6 watch items, with **Runbook 1–4
and 7** done and recorded here. P5 does not gate it — the 2026-09-05
disposition stands as a disposition, its figure does not.

**P1, P2 and P3 are not in the flip condition.** They are measurements in the
Step 4 deferred row, and they need a second wired LAN endpoint that does not
exist. A reader who treats them as gating will conclude the phase is blocked
on hardware. It is not: it is blocked on Runbook 1–4 and 7, the deploy
approval, and the clock.

The soak's two code prerequisites — N3 (`dot` state) and the L5 `listeners`
telemetry block — **shipped 2026-09-02 in `44f3c83`** (§Post-review work A).
Watch item (b) has its read path. Do not re-propose them.

**The campaign is on the device.** Step 4 R0–R6 and R10 are executed; nine arms
are measured and recorded in
[p3-06-testing-results-2.md](p3-06-testing-results-2.md) §Session 2. Production
`fastadhunter` 0.3.3 on `veth1` was never touched at any point.

### What exists on the router now

| | |
| --- | --- |
| `fah-probe` | container on `veth3` / `172.17.0.4`, **stopped** at end of session, not removed |
| its config | `/kingston/fah-probe/config`, mount list `fahprobe-config` — survived three remove/add cycles, so it is a real mount |
| its data | `/kingston/fah-probe/data`, mount list `fahprobe-data` |
| its env | `fahprobe-env`, four keys, `FAH__RUNTIME__HTTP_RUNTIMES=2` |
| tars uploaded | `fah-probe`, `fah-splicebench`, `fah-p4`, `fah-certs`, all `-arm64.tar` under `kingston/` |
| probe state | `mode=dns+http+https`, N=2, CA generated, oisd-basic 60 748 rules, egress `192.168.10.10/32` and `192.168.10.22/32` |
| firewall | **untouched.** R7 was never run; nothing steers tcp/443 |
| production | `fastadhunter` 0.3.3, `veth1`, `fah-env`, running throughout |

Restart is one command — the `add` line is in
[the runbook](p3-06-phase3-verification-runbook.md) R5, or just
`/container/start [find comment="fah-probe"]` while the container still exists.

### Settled this session, do not re-open without new evidence

- **N=2 confirmed.** ADR-0006 unchanged. Two independent axes agree: the
  `close` arm and the keep-alive arm. N=1 is now tested and rejected — a gap
  phase 2.6 left open. The first sweep, at concurrency 16, is **withdrawn**;
  it was underloaded and measured neither DNS-under-load nor CPU-per-request.
- **D6 settled.** `SPLICE_BUF` stays 16/16. Up buffer settled at 16 (+2.8 %,
  under the +10 % threshold, two runs agree). The down curve saturates after
  64 KiB but only 16/16 fits the 32 MiB budget at `max_connections = 1024`.
  Changing it is a product decision, not a benchmark conclusion.
- **P5's gate failure is not a budget failure.** `fah-certs` measured
  `certs_mint` at 447.12 µs against the 450.88 µs budget row. P5 measures a DoT
  handshake delta, not the mint path.
- **The ~9× x86 → RB5009 factor is confirmed** on four independent criterion
  benches, 7.0× to 8.7×.

### Owed, and why

| Item | Blocker |
| --- | --- |
| **P1-LAN / P1-control** | the origin must live on a host that is not the client. Today both were the dev box, which P1 cannot use — its control arm fetches the origin directly |
| **P3** | a **publicly trusted** h2 origin under a public name (Let's Encrypt DNS-01). Unchanged since smoke Layer 3, which proved no local substitute exists: `FAH_TEST_UPSTREAM_ROOT` is behind `#[cfg(feature = "test-harness")]` and the release image ignores it. **Longest-standing open item in the campaign** |
| **P2** | runs *on* the second endpoint. The Mac is out — no USB-C so no wired path, Homebrew cannot write `/usr/local`, and Remote Login needs Full Disk Access, so no ssh |
| TLS keep-alive figure | operator error: `--ca probe-ca.pem` asserts the terminate leg, but interception is empty so the probe splices and serves the origin's own certificate. Re-run with the origin's certificate |
| P10 `mixed` / `tls-spliced` at N=0 and N=2 | 43k–120k client-side failures, Windows port exhaustion. Only the `close` arm was clean |
| Step 4 items 4, 5, 6 | CA install walkthrough, Private DNS, pinned-app check — need a test device, not a laptop |
| R11, the 24 h soak | blocked until the 0.3.3 soak ends **2026-09-14** |
| R7, dst-nat 443 | **owner only, optional.** No measured arm needs it; it exists for transparent interception of real browsing |

A second laptop is expected 2026-09-09, which unblocks P1 and P2.

### Traps this session found, worth not rediscovering

- **The container's first list refresh fires before its network is up.** Every
  boot logs `all upstreams failed — upstream timed out` and `list refresh
  failed`, and would not retry for 24 h. `POST /api/v1/lists/{id}/refresh`
  fixes it. The upstreams themselves are healthy; the warning is a startup
  race, not a fault. After the first successful refresh the ruleset is restored
  from cache on later boots and the warning stops.
- **`oisd-basic` does not block the obvious names.** `doubleclick.net`,
  `ads.doubleclick.net`, `googleadservices.com` and `adservice.google.com` all
  pass. `analytics.google.com` and `pagead2.googlesyndication.com` block —
  verify with `POST /api/v1/rules/test` before assuming a name is blocked.
- **Windows is the campaign's throughput ceiling, not the RB5009.** Every
  concurrency-48 arm exhausted the 16 384 ephemeral ports; the `close` arm at
  N=0 logged 33 004 client failures. Drain to under ~100 TIME_WAIT between
  arms and read `failed` before trusting any rate.
- **`/container/add` parameter names**: `mountlists`, `envlists`, `workdir`,
  `cpu-list`. `/container/mounts/add` and `/container/envs/add` take `list=`,
  not `name=`, while `[find …]` also matches on `list=`.
- **The dev box browser breaks runs.** Three arms invalidated on
  `host not idle: brave running`. That is the idle check working.

## Post-review work F — targeted review of the splice TLS-handshake path, 2026-09-08

Scope: read-only. Nothing was modified — not `https.rs`, not the benches, not
`phases.sh`, not config, not the pre-declarations. Trigger: D8's spliced arm at
7–20 ms `tls − connect` against a flat ~4.1 ms direct control
(§D8 attribution, `p3-06-testing-results-2.md`), cause still open after the
2026-09-08 TIME_WAIT withdrawal (§Correction).

### Path traced

| Step | Site | In the curl `tls − connect` window? |
| --- | --- | --- |
| accept, `set_nodelay(true)` on the client socket | `server.rs:180` | no (before `connect` completes) |
| `started = Instant::now()` | `https.rs:127` | window opens here |
| `read_client_hello`, under `hello_timeout` | `https.rs:129` | **yes** |
| `scan_client_hello`, IP-literal refusal | `https.rs:150`, `:164` | **yes** |
| `judge` — `matcher()` + `policies.current()`, arc-swap, no lock | `https.rs:169` | **yes** |
| `approved_address` → `resolver.resolve(host.to_string())` | `https.rs:180`, `:298` | **yes** |
| `UpstreamPool::resolve_host` — `tokio::join!(A, AAAA)` | `upstream/mod.rs:142` | **yes** |
| `plain::query` — fresh `UdpSocket::bind(:0)` per query, ×2 | `upstream/plain.rs:52` | **yes** |
| `DestinationPolicy::check` | `egress.rs:169` | **yes** |
| `TcpStream::connect`, under `hello_timeout` | `https.rs:193` | **yes** |
| `duration = started.elapsed()` — window measurable here | `https.rs:216` | boundary |
| `set_nodelay(true)` upstream, `write_all(hello)` | `https.rs:224`, `:230` | **yes** |
| `copy_bidirectional_with_sizes`, 2 × 16 KiB | `https.rs:245` | yes (relays the handshake) |

### Answers to the ten questions

| # | Question | Answer |
| --- | --- | --- |
| 1 | downstream TLS handshake | **none exists.** The splice never terminates TLS. Only the ClientHello is read, scanned and re-emitted; every later record is opaque |
| 2 | upstream TCP + TLS | TCP at `https.rs:193`; **no upstream TLS handshake** on this path (the interception branch returns at `:187` before it) |
| 3 | serialisation | ClientHello → judge → resolve → connect is structurally ordered (SNI feeds both the verdict and the address). One avoidable point: A and AAAA are `join!`ed, so resolve costs max(A, AAAA) |
| 4 | DNS on the path | **one full `resolve_host` per spliced connection**, no cache, two queries, two fresh UDP sockets |
| 5 | runtime / domain crossing | two modes: `SharedTls` (N=0) and `Domains` (N≥1: `into_std` → `mpsc(32)` → per-domain `new_current_thread` runtime → `from_std`). The D8 criterion rig uses `SharedTls` only; the N sweep already refuted the handoff |
| 6 | socket setup | `TCP_NODELAY` on both sockets, once each. No flush before the hello forward, and none is needed on TCP |
| 7 | certificate work | none — leaf mint, cache lookup and `prewarm` are all behind the interception branch |
| 8 | retry / fallback in the window | `walk_adaptive` walks upstreams on failure with a per-attempt timeout; `plain::query` retries over TCP on TC=1; `hello_timeout` bounds both the hello read and the connect |
| 9 | benchmark boundary | **does not isolate the handshake** — see F1 |
| 10 | allocation / lock / spawn | per connection: ~2 KiB hello `Vec`, `host.to_string()` ×2, one `Box::pin`, one `Vec<IpAddr>`, two ~1232 B recv buffers, two 16 KiB copy buffers, one `try_send`. No mutex, no `spawn_blocking`, no regex. µs scale |

### Findings

| # | Severity | Class | Finding |
| --- | --- | --- | --- |
| F1 | high | **confirmed** | `phases.sh`'s `time_appconnect − time_connect` is not the same quantity in the two arms. Direct = TLS handshake only, with DNS removed by `--resolve`. Splice = hello read + verdict + **full recursive DNS** + egress check + **upstream TCP connect** + hello forward + relayed handshake. The two arms were compared as if they measured one thing |
| F2 | high | **plausible, needs measurement** | The SNI host is `127-0-0-2.nip.io`, a public wildcard zone. Every spliced connection resolves it upstream, A and AAAA, uncached inside FAH. `x-x-x-x.nip.io` carries no AAAA, so that leg is a negative answer whose SOA-minimum TTL decides how often it leaves the LAN. A WAN recursion of 10–30 ms with high variance fits 7 → 12 → 21 ms against a flat direct control |
| F3 | medium | **confirmed** | `resolve_host` uses `tokio::join!`, so the resolve waits for both families even though its own doc comment says either one carrying addresses is enough. Cost is max(A, AAAA), not first-usable. Directly amplifies F2 |
| F4 | medium | **rules out DNS for the criterion figure** | `intercept.rs:35` uses `FixedResolver` — the D8 criterion arms pay no DNS at all. So `spliced` 5.67 ms vs `direct_to_origin` 1.13 ms and the curl 7–20 vs 4.1 ms are **two different gaps**. F2 cannot explain the criterion one; merging them mis-attributes both |
| F5 | medium | **plausible, competing, not established** | Port pressure. Per iteration the splice opens 2 TCP sockets to direct's 1, plus 2 UDP binds in the shipped binary. But the 15 543 TIME_WAIT reading came from the `p10` close arm at concurrency 48, not from the D8 attribution run, which was serial `curl`. Nothing yet ties it to these four arms — it stays a hypothesis, not the answer |
| F6 | low | **confirmed, magnitude unknown** | DNS retry paths sit inside the measured window: the upstream walk on failure, and the RFC 1035 §4.2.2 TCP retry on TC=1. Neither was excluded when the arms ran |
| F7 | low | **no issue for latency** | `approved_address` returns the first policy-approved address and never tries the next if `connect` fails. Robustness gap, not a cost |
| F8 | low | **design question, owner** | The resolver bypasses the DNS cache as well as the Rule Engine. The documented reason (a blocklist blocking the host serving its own next copy) is about the Rule Engine only; hard rule 3 says the cache never stores verdicts, so a cache read carries none. `UpstreamResolver` is L4 and already sits beside the pipeline, so consulting the cache is layering-legal |
| F9 | info | **no issue found** | No lock, no `spawn_blocking`, no certificate work, no regex on the splice path. The allocation set is µs-scale against a ms-scale gap |
| F10 | info | **no issue found** | `TCP_NODELAY` is correct on both legs and set once each; the earlier Nagle refutation holds |

### Most likely explanations

1. **The curl 7–20 ms figure (F1 + F2 + F3).** The splice window contains a
   recursive lookup the direct window does not, and that lookup waits on a
   negative AAAA answer from a public zone. The 2026-09-08 refutation measured
   one warmed query against `192.168.10.1`; it did not test the AAAA leg, the
   two-socket bind, or a cold TTL.
2. **The criterion 5.67 vs 1.13 ms figure (F4).** Not DNS. What remains is
   doubled connection setup on loopback under criterion's per-iteration
   `block_on`, which is the proxied-path constant §Measurements already reads as
   "the socket/task path, not TLS termination".
3. **Port pressure (F5)** stays live for both and is settled neither way.

### Smallest experiment

No code change, no rig change. The instrumentation already ships.

`https.rs:216` captures `started.elapsed()` **after** the upstream connect and
**before** the relay, and it reaches the wire as `duration_ms` on the
`https-sni` record streamed by `WS /api/v1/events`. Run the splice arm of
`phases.sh` with that socket open and read the two halves:

| `duration_ms` reads | Conclusion |
| --- | --- |
| ≈ curl's `tls − connect` | cost is pre-relay — DNS plus connect. Follow with 30 back-to-back `AAAA 127-0-0-2.nip.io @192.168.10.1` to see whether the AAAA leg carries the spread |
| ≈ 1 ms | cost is inside the relayed handshake; F5 and scheduling move to the front |

Cheaper still, and needing no socket: re-run `phases.sh` unchanged except for an
SNI host whose A and AAAA both answer from the router's cache on a long TTL. If
the spread collapses, F2 owns it.

### Is a code change justified?

**No — not yet.** Nothing found is a correctness or security defect, and the
splice path does what it is specified to do. Two changes become justified only
if the experiment implicates them, and both are hot-path behaviour changes that
principle 8 says must follow the measurement:

- race A and AAAA instead of `join!` (F3);
- let the L4 resolver read the DNS cache before going upstream (F8) — owner
  decision, since it edits a documented deliberate bypass.

F1 is a reading correction, not a code change: the spliced `tls − connect`
figure must be labelled "proxy setup + relayed handshake" wherever it appears,
never "TLS handshake". P2 on the device is unaffected and stays the authority
for the handshake row.

**Verdict: PASS WITH DEFERRED FINDINGS** (F1/F4 are reading corrections owed to
the D8 narrative; F2/F3/F5 await the experiment above; F7/F8 deferred to owner;
F6 noted; F9/F10 clean). No implementation was touched by the review itself.

### Acted on after the review — owner-approved, same day

The review ran read-only. Everything below was approved separately afterwards.
No crate source, no criterion bench and no runtime configuration changed; F3 and
F8 remain unimplemented, as the verdict requires.

| Action | Where | Note |
| --- | --- | --- |
| F1 relabel of the D8 narrative | `p3-06-testing-results-2.md` §D8 attribution + declaration rows; this file's D8 table and open-finding restatement | `direct_to_origin` keeps the TLS-handshake label; only the proxied arms are renamed |
| The SNI-resolve refutation marked **weakened**, not refuted, and the status line's "not explained by DNS" withdrawn | `p3-06-testing-results-2.md` §D8 attribution | the 12 samples were one warmed record type; the AAAA leg `resolve_host` waits on was never sampled |
| P2's quantity renamed and the split declared | campaign-2 declaration changes 4 and 5, above | recorded **before** the arm runs; no threshold, arm or sample size moved |
| Pre-relay split instrumented | `p3-06-probe/p2-handshake.mjs` | probe-side telemetry reader only, `--no-events` opts out, socket failure degrades rather than invalidates |

The events reader was exercised against a running probe before being trusted:
upgrade, subscribe, ping/pong and three decoded `query` events with
`duration_ms` populated. That test caught a defect in it — `servername` set to
the probe address, which Node rejects for an IP, so the run would have crashed
on the device where `--probe` is `172.17.0.4`. Fixed. **Not yet exercised: a
frame whose `kind` is `https-sni`**; producing one means firing TLS connections
at a live rig mid-campaign, so it is left to P2's first run, which degrades
rather than fails if the filter misses.

## Post-review work G — P3's certificate requirement satisfied, 2026-09-09

`p3-06-testing-plan.md` §The Mac endpoint held P3 closed on a hard
precondition: *"P3's origin needs a publicly trusted certificate under a public
name (Let's Encrypt DNS-01), because the release probe verifies upstreams
against `webpki-roots` only. […] Until that name exists, P3 does not run."*

That name now exists. The certificate is obtained and verified.

| Item | Value |
| ---- | ----- |
| Domain | `localbox.ro`, registered 2026-09-09 at NameBox, auto-renew on, expires 2027-09-09 |
| DNS | delegated to Cloudflare (`gracie` / `fattouche.ns.cloudflare.com`); ROTLD published the change in under 60 s |
| Zone signing | **unsigned** — no DS at the parent, confirmed by DoH query |
| Certificate | `*.localbox.ro` + `localbox.ro`, EC256, 2026-09-09 → 2026-12-08 |
| Issued by | `lego 5.4.1`, ACME DNS-01 via the Cloudflare provider |
| Stored | `.vscode/lego/` — gitignored and untracked, private key included |

The zone being unsigned matters: it removes the failure class that ended the
earlier deSEC attempt, where five independent validators agreed the zone was
DNSSEC-bogus. There is no DS record to go stale here.

### The chain had to be chosen, not accepted

The first issued certificate would have **failed the probe it was obtained
for**. Let's Encrypt's default chain now terminates at `ISRG Root YE`
(Generation Y hierarchy, live since 2026-01-07), and `webpki-roots` carries only
`ISRG Root X1` and `ISRG Root X2` — checked in 0.26.11, 1.0.8 (the version
`fah-http` resolves) and 1.0.9. `crates/fah-http/src/tls.rs:38` would have
answered `UnknownIssuer` on a genuine, publicly trusted certificate.

`openssl verify` said `OK` throughout, because the OS trust store already has
the Y roots. Only the root set the code actually uses refuses it.

Resolved by taking the cross-signed alternate chain, which terminates at
`ISRG Root X2`. Verified the way that matters — X2 pinned as the sole trusted
root, not the OS store:

```sh
openssl verify -CAfile isrg-root-x2.pem -untrusted chain.pem leaf.pem
# leaf.pem: OK
```

Renewals must carry `--preferred-chain "ISRG Root X2"` or they silently revert
to the Y chain; `--preferred-chain "ISRG Root X1"` matches nothing for an ECDSA
leaf and is dropped without a warning. Full write-up, including how to read the
ACME alternates without spending the 5-per-week duplicate-certificate limit:
[`docs/solutions/environment/lets-encrypt-gen-y-chain-vs-webpki-roots.md`](../../solutions/environment/lets-encrypt-gen-y-chain-vs-webpki-roots.md).

### What this does and does not unblock

**Does not unblock P3.** The plan names two preconditions and this clears one.
P3 still needs the second wired LAN endpoint to host the h2 origin, and P1, P2
and P3-throughput need it too. No arm moves on this alone.

**Does unblock the p3-05 listeners for real use.** DoT and DoH have shipped
since p3-05 but had no publicly trusted name, so Android Private DNS — which
fails closed against a local CA — could not accept them. A client-facing name
under this certificate now can be served. That is a shipped feature becoming
usable, not a test artifact.

**Scope of this record.** The certificate is verified against a pinned root and
is **serving the production API since 2026-09-09**: the pair was copied onto the
container's `/config` volume and loaded at the `14:15:15` restart, and
`https://fah-api.localbox.ro:8443` now passes strict verification. The Phase 3
import endpoint was not used — `POST /api/v1/certificates/import` does not exist
in the deployed 0.3.3, which predates Phase 3, so the files were replaced on
disk.

Two names exist under the zone — `router.localbox.ro` → `192.168.10.1`
(RouterOS WebFig, its own certificate) and `fah-api.localbox.ro` → `172.17.0.2`
(the API) — and neither serves a DoT listener; port 853 is closed. The address a
listener will answer on is undetermined and depends on Runbook 1, which has not
run. **P3's origin is still not served**: that needs the second LAN endpoint,
which this certificate does not provide.

### Declaration change 6 — P3's origin name

P3's origin was declared as a name on the Mac endpoint. It is now a name under
`localbox.ro`, resolving to whichever host serves the h2 origin, with the
wildcard certificate above. No threshold, arm, sample size or ceiling moves;
only the name and its certificate source. Recorded before P3 runs.

## Post-review work H — P9 baseline, and P8's first read, 2026-09-09

Both halves of MA-6/MA-11 need a `dns+http` reading taken **before** the
full-mode deploy replaces the container. This records P9's, which is final,
and opens P8's, which is not.

### P9 — boot-to-serving, `fastadhunter` 0.3.3 `mode=DnsHttp`

Source: `soak-0.3.3/soak-0.3.3-t0-container-log.txt`, captured at the soak's
T0 and already on disk — the reading below is derived from it, not from a new
read. Device RB5009, `veth1`, `FAH__RUNTIME__HTTP_RUNTIMES=2`, warm caches.

| Mark | Time |
| --- | --- |
| `/container/start` — router log, one-second granularity | `11:15:15Z` |
| `fastadhunter starting … mode=DnsHttp` | `11:15:16.071381Z` |
| `ruleset compiled from cache rules=756493` | `11:15:19.007301Z` |
| DNS listeners bound | `11:15:19.036495Z` |
| HTTP listener bound | `11:15:19.036576Z` |
| privileges dropped | `11:15:19.039659Z` |
| `API listening url=https://0.0.0.0:8443` | `11:15:19.046690Z` |

**Boot-to-serving, process-internal: 2.975309 s.** Both ends are the binary's
own log, sub-millisecond, so this is the statistic to carry forward. The
container-observed figure — `/container/start` to `API listening` — is
~3.05 s, bounded 3.047–4.047 s because the router log stamps to the second;
it cannot be sharpened without a finer clock on the start mark.

Composition: 2.935920 s from the first log line to a compiled ruleset, of
which `compile_duration_seconds` reports **2.889515 s** for 756 492 rules
after 451 062 duplicates were removed. The three binds and the privilege drop
cost 29 ms together, and `API listening` follows 7 ms later.

**Finding P9-a: the margin against the `< 3 s hard` startup row is 25 ms,
and the composition says Phase 3 is not what decides it.** Ruleset compile is
97.1 % of boot; every listener the phase adds — `CertStore::open`, the DoT
and HTTPS binds, `dot_tls` — lands in the 39 ms that is left. A full-mode
boot that misses the row misses it because of compile time, and tuning the
Phase 3 startup path cannot buy back a budget that compile has already spent.
The row should be read against compile-time work, not against this phase.

**Scope.** Warm boot only: `refresh schedule restored from cached copies
lists=12` and a ruleset compiled from cache. A cold boot fetches twelve lists
over the network first and is not comparable to this figure. Superseded by
any reading whose start mark carries sub-second resolution, or by a cold-boot
series, neither of which exists today.

### P8 — CPU share under household browsing, read 1 of 10

`/tool/profile cpu=all duration=10s`, read-only over SSH, with
`/api/v1/telemetry` and `/api/v1/stats` from the same moment. Artifacts in
`soak-0.3.3/p8/`, one bucket per local hour per phase; the capture script is
`requests/p8-read.ps1`.

| Bucket | `fastadhunter` peak | Load at the time |
| --- | --- | --- |
| `before-h22`, `2026-09-09T19:58:52Z` | 0 % on all four cores, every sample | 43 350 queries / 31 395 s ≈ 1.38 q/s |

**Finding P8-a: the baseline sits at the instrument's floor, so the declared
statistic may not be reachable.** `/tool/profile` resolves 0.5 % per core. At
household load the `dns+http` container reports 0 %, and a validation read one
hour later peaked at exactly one quantum, 0.5 % — so the baseline is not
uniformly zero, but it occupies the bottom quantum of the scale. A median
delta between two readings that both live there is not a measurement. If the
remaining reads confirm it, P8's honest result is a **bound** — "below 0.5 %
of one core at household load, before and after" — not the delta the plan
declares, and the plan should be redeclared rather than the bound dressed up
as a delta.

Nine reads owed, at 09, 13, 19 and 21 local, repeated after the deploy. The
hours are the comparison: a before-read at 19:00 and an after-read at 03:00
compare household habits, not the build.
