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

| D8 `https_handshake` — fresh connection, TLS handshake, h1 GET 1 KiB, close (ms) | r1 | r2 | isolated single-arm run |
| --- | --- | --- | --- |
| `direct_to_origin` (control) | 1.130 [1.06 1.21] | 0.983 [0.95 1.01] | 1.141 [1.07 1.22] |
| `spliced` | 5.666 [4.10 7.28] | 5.986 [4.58 7.33] | 4.892 [3.92 5.93] |
| `intercepted` (leaf cache warm: `minted_total = 1`, `unwarmed_misses = 0`) | 2.726 [2.36 3.16] | 4.824 [3.73 6.02] | 4.304 [3.07 5.75] |

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
| **HTTPS** SNI verdict + splice, added per connection | TBD — must be measured during verification (P2 sets it; a loopback figure does not convert, PERFORMANCE.md §Converting) | +3.5–4.9 ms loopback, harness-dominated (D8, old harness — see F6) | TBD — P2 |
| **HTTPS** splice throughput, steady state | ≥ 100 MiB/s (gigabit LAN is 119 MiB/s) | 0.93–1.01 GiB/s at 16 KiB, 1.48–1.60 GiB/s at 64 KiB (D7, unpinned); 1.06–1.13 / 1.56–1.61 GiB/s pinned to four cores (§Post-review work C) | TBD — P1 |
| **HTTPS** interception handshake overhead vs splice | intercepted p50 ≤ 2 × spliced p50 | within intervals of each other unpinned (D8); 1.5–2.2× pinned to four cores (§Post-review work C) | TBD — P2 |
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
   (or remove the envlist) + restart.
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

**Prerequisite decision (p3-04 L5 + TODO):** `non_tls`, `hello_timeouts`,
`upstream_cert_failures` are counted but published nowhere. Proposal (API.md
edit + code, owner yes): `GET /api/v1/telemetry` gains
`"listeners": {"http": <ProxyStats>, "https": <ProxyStats>}`, and on the
terminate leg `requests` counts HTTP requests (as :80 does) while a new
`connections` field carries accepted connections, so `blocked ≤ requests`
holds per listener (L5). Until it lands, watch item (b) has no read path.

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

## Proposed documentation edits (Step 5) — none applied

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
2. **SECURITY.md** — §What a household should do, row 3: "Out of scope until
   Phase 3" → "Import the pair with `POST /api/v1/certificates/import` (Phase
   3); renewal stays an operator action". §Later phases already reads in the
   present tense for p3-03/p3-04/p3-05; no other change.
3. **docs/deploy-rb5009.md** — new §5c "HTTPS (Phase 3, `dns+http+https`)"
   mirroring §5b: turn on the mode (API or a *separate* envlist, never
   `fah-env`), prove the listener, the two v4 rules + the three v6 rules above,
   verify (`nat print stats`, WS `https-sni`), the no-SNI/ECH operator warning
   with the measured `ENOENT`, CA install + list-after-install sequencing,
   Private DNS bootstrap rule, rollback. §5b rollback regex tightened to
   `[find comment="fastadhunter http"]` + `[find comment="fastadhunter http: leave local traffic alone"]`
   (exact) so it stops matching the https rules.
4. **Dashboard re-review (code, owner decision):**
   `dashboard/frontend/src/pages/live-feed/filters.ts` `KINDS = ['dns', 'http']`
   lacks `https-sni` and `https`; `detail.tsx:15` gates the HTTP detail on
   `kind === 'http'` and `:53` the block detail on `kind === 'http' || verdict === 'block'`,
   so an `https` row renders through the DNS-shaped branch with empty
   method/path and an `https-sni` row cannot be filtered. Either add both
   kinds (+ a detail branch each) in this task or record the owner's deferral
   to a dashboard task — **unfiltered kinds are a finding** either way.
5. **README.md §Operating modes** row `dns+http+https`: "…plus HTTPS
   interception, for managed environments" → "…plus SNI-level HTTPS filtering
   for every client, opt-in per-client HTTPS interception, and DoT/DoH
   listeners". Line 44's "on 0.3.0 since 2026-08-29" is stale (0.3.1 deployed).
6. **ROADMAP.md §Phase 3** — bullets to delivered `[x]` wording at phase close;
   "import PEM/PFX" → "import PEM (PFX descoped, ADR-0006)"; add "p3-06
   verification: dev-box suite + soak `<date>`".
7. **API.md** — (a) p3-05 N3: `GET /api/v1/certificates` gains
   `"dot": {"state": "listening" | "closed", "reason": "<the boot error>"}`
   (or `/health` `checks.dot`); code change in `main.rs`/`fah-api`. (b) L5/TODO
   `listeners` block on `/telemetry` as in Runbook item 6. Both owner yes.
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
| (outside the repo) `../FastAdHunter-pre3` | detached worktree at `64be513` with its own `target/`, left in place for re-runs; `git worktree remove ../FastAdHunter-pre3` drops it |

## Known limitations / deferred

| Item | Owner |
| --- | --- |
| X1–X5 above | owner decision |
| Every on-device row (P1–P7, Runbook 1–7): dst-nat, CA install, Private DNS, pinned app, probe measurements, soak, cert-store checks, DoH-h2-on-the-wire, DoT/DoH device latency, N1/N11 `prewarm` profile — **nothing changed in `dot.rs`/`intercept.rs`** per the plan's "measure before touching" | owner + agent after approval |
| N3 `dot` listener state, L5/TODO `listeners` block, dashboard kinds — proposed, not built | owner yes |
| p3-05 N2/N4/N9 won't-fix, N12/N16 closed — untouched | — |
| Real browsing-session host replay for the leaf-cache row (D12 is synthetic) | soak feed |

## Findings — consolidated 2026-09-03; full history: `git show 0a74ac3:docs/code-review/phase3/p3-06-phase3-verification-review.md`

One review (F1–F18, X1–X5) and one fix round; F1 closed by option (b), F2–F9,
F11, F18 fixed, F6 withdrawn, F8's pinning-rule conflict settled by
§Post-review work C. Fixed and withdrawn items are omitted — git has them.
The measurement-validity findings MA-1–MA-11 and their resolutions live in
[p3-06-measurement-audit.md](p3-06-measurement-audit.md). Still open:

| id(s) | Issue | Status | Where |
| --- | --- | --- | --- |
| F10 | the key-material detector searches PEM base64, its first 48 chars, raw DER and four headers; hex, base64url and JSON `\u` encodings are not searched | deferred | no owner — note for the day a route emits those |
| F16 | splice byte-identity is proven origin→client only; the client→origin direction is sunk by the raw origin | deferred | no owner — low value, low cost |
| I1 | dashboard live-feed kinds `https-sni` / `https` are unfiltered and render through the DNS branch | deferred | needs a named task (phase3-audit §5) |
| I3 | `E:/FastAdHunter-pre3` worktree (`64be513`) still registered | deferred | drop at phase close |
| X2 | `cargo bench -p fastadhunter` compiles only with `CARGO_PROFILE_BENCH_DEBUG_ASSERTIONS=true` (the `test-harness` dev-dependency unifies into the bench profile; p5-04 leftover) | deferred | follow-up task; workaround in PERFORMANCE.md §Measuring reliably |
| X3 | `docs/project-state.md` is dated 2026-09-01 and does not mention Phase 3 | deferred | phase-close rewrite |
| Step 4 | Runbook 1–7 on the device, P1–P9, the 24 h soak; `BASELINE_EXCLUSIONS` final names; P1/P3 LAN-vs-loopback definition | deferred | owner — §Runbook, §Hand-off state |
| Step 5 | doc sweep remainder: deploy-rb5009.md §5c after the walkthrough, README modes row, SECURITY.md row 3, ROADMAP wording, project-state | deferred | owner — §Proposed documentation edits |

**PASS WITH DEFERRED FINDINGS** — 8 open rows (8 deferred, 0 won't-fix). `AWAITING SOAK`; flip condition in §Hand-off state.

### GAR §5 items 7–14 — the Phase 3 gate map (plan §TASK START 8, F11)

| # | Item | Where it is discharged | Status |
| --- | --- | --- | --- |
| 7 | cert home / ADR-0007 | `docs/decisions/0007-certificate-machinery-home.md`; `fah-certs` (L2) consumed by `fah-http` and `fah-api` (L3); `layering.rs` green | closed (p3-01) |
| 8 | connector redesign — hostname-verified upstream TLS | `crates/fah-http/src/tls.rs:65` `connect_verified_upstream`, roots from `client_config()` (`tls.rs:36`); suite `bad_upstream_cert_is_not_masked` proves fail-closed at the binary level | closed (p3-04 decision 4) |
| 9 | DoH/DoT placement | DoT inside `fah-dns` (`main.rs:377` `dns.dot_addr()`); DoH on the API listener, route gated on `state.tls && state.doh` (`routes.rs:118-125`, `main.rs:548-582`); shared 64-permit consequence → soak watch item (a) | closed (p3-05 decision 4); watch item open |
| 10 | event/telemetry taxonomy | `fah-model/src/request_event.rs:126-127` `EventKind::{HttpsSni, Https}`; `fah-model/src/client_transport.rs:5` `ClientTransport`; suite and e2e assert `kind: https-sni` / `https` on the WS feed | closed (p3-03/04/05) |
| 11 | memory caps per new state owner | leaf LRU `fah-certs/src/leaf.rs:16` `LEAF_CACHE_CAPACITY = 512`; splice `https.rs:29` `SPLICE_BUF` × 2 × `max_connections` (D6/D7); terminate leg `intercept.rs:37-40` `H2_*` (D9); DoT 64 permits (p3-05) | closed on paper; stall RSS → P3 |
| 12 | 443 steering v4 + v6 | Runbook item 1 (proposed, not run) | open — owner |
| 13 | on-device TLS measurements | P1–P6 (declared, not run) | open — owner |
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

## Hand-off state, 2026-09-08 — current

Supersedes both sections above. Tree clean on `phase3-06` at **`61ea35c`**
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
- P10 (Step 4.1) cannot run while the probe is attached to envlist `fah-env`:
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
