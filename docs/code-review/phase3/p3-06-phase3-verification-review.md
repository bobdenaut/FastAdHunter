# P3-06 — Phase 3 Verification — Review

**Task:** `plan/wip/phase3/p3-06-phase3-verification.md` · **Plan:**
`p3-06-phase3-verification-plan.md` · **Status:** agent-side work complete
(Steps 1–3 on the dev box, Step 4 runbook proposed, Step 5 edits listed);
on-device items and every doc edit await the owner; review not started.
Phase row untouched (`WAITING` until the owner flips it; `AWAITING SOAK` is
the expected next state).

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
the socket/task path is. Diagnostic only; P2 on-device decides.

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

### Proposed PERFORMANCE.md rows (owner decides; on-device column stays `TBD — must be measured during verification`)

| Metric | Proposed budget | Dev-box figure (this file) | On-device |
| --- | --- | --- | --- |
| **HTTPS** SNI verdict + splice, added per connection | p50 < 5 ms on-device (proposal from P2, not from D8) | +3.5–4.9 ms loopback, harness-dominated (D8) | TBD — P2 |
| **HTTPS** splice throughput, steady state | ≥ 100 MiB/s (gigabit LAN is 119 MiB/s) | 0.93–1.01 GiB/s at 16 KiB, 1.48–1.60 GiB/s at 64 KiB (D7) | TBD — P1 |
| **HTTPS** interception handshake overhead vs splice | intercepted p50 ≤ 2 × spliced p50 | within intervals of each other (D8) | TBD — P2 |
| **HTTPS** intercepted h2 relay | ≥ 50 MiB/s | 481–485 MiB/s (D9) | TBD — P3 |
| Minted-leaf cache hit rate, browsing load | ≥ 90 % (real replay decides) | 67 % synthetic Zipf (D12) | TBD — soak feed |
| **DoT** / **DoH** added latency vs UDP, p50 | DoT < 0.5 ms, DoH < 2 ms on-device | +17 µs / +130 µs loopback (D13) | TBD — P4 |
| Cold `prewarm` per first-sight host (whole path, not raw keygen) | < 1 ms | 53.5 µs (D11) ⇒ ≈ 0.48 ms by the ×9 factor (CPU-bound, converts) | TBD — P5 |
| CA generate / API-pair import wall time | < 100 ms / < 50 ms | ≈ 2 ms / ≈ 1.4 ms (p3-02, debug) | TBD — P6 |
| RAM steady-state, full mode | ≤ 128 MB (existing row, re-affirmed) | 46.1 MiB test build (D14) | TBD — P7 |

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
| p3-04 carry-over integration tests (M4 rows 5–9, N10, IPv6) | `crates/fah-http/tests/interception.rs`: `Setup.max_connections` / `Setup.listen`, `OriginSpec.count_body` / `cut_after_first`, `Trickle` streaming body, 6 new tests, N10 permit assertion added to the idle test |
| p3-03 n5 + M4: splice bench on `TlsServer::bind/serve`, shared payload origin, loop-read drain, steady-state arm (`https_sni_splice_steady_state`, 64 MiB) | `crates/fah-http/benches/proxy.rs` |
| New bench: handshake overhead (direct / spliced / intercepted), h2 8 MiB download through terminate vs splice (p3-04 S1), `prewarm` hop (p3-04 N8) | `crates/fah-http/benches/intercept.rs` (new), `[[bench]]` in `crates/fah-http/Cargo.toml` |
| Leaf-cache replay arm `certs_replay_zipf` (synthetic Zipf, hit rate printed) | `crates/fah-certs/benches/certs.rs` |
| `encrypted_latency` promoted: 3 interleaved rounds × 2 000 per transport, per-round p50 printed, pooled percentiles | `crates/fastadhunter/tests/encrypted_latency.rs` (still `#[ignore]`, release binary via `FAH_E2E_BINARY`) |
| A/B session script + raw criterion logs | `docs/code-review/phase3/p3-06-bench/` (`run-ab.ps1`, `session.log`, `r*-*.out.txt`) |
| Dev-dependency | `crates/fastadhunter/Cargo.toml`: `rcgen` (dev-only; the origin the binary must *not* be able to verify) — `Cargo.lock` gains the edge |

No release code changed. `crates/fah-http/src/https.rs` `SPLICE_BUF` was
toggled to 64 KiB for one bench build and restored (`git diff` clean).
`layering.rs` green. `http_e2e.rs` unchanged and green.

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
| X1 | Step 3: "intercepted HTTPS URL block (client trusting the test CA, opted in)" through the full binary | **Not buildable offline.** The terminate leg verifies the upstream against **compiled-in webpki roots only** (`fah_http::client_config()`, `main.rs` `interception()`); no config key adds a root. A loopback origin can never verify, so the binary closes the connection (526) before minting. Leg 5/7 therefore asserts: listed client enters the terminate leg (event `kind: https`), unverifiable origin ⇒ `status 526`, no ServerHello, `minted_total == 0`. The URL-level judge inside TLS stays pinned at the `fah-http` harness (p3-04 `a_listed_client_gets_url_level_filtering_over_*`, which injects the origin root) | (a) accept as is — the wiring is proven end-to-end, the judge is proven one layer down; (b) a `test-harness`-gated extra-roots hook in `main.rs` (feature already exists for the login limiter; test-built binaries carry it, release cannot); (c) a real `[https.interception] upstream_roots` key (product feature, also serves private CAs — out of this task's scope) |
| X2 | Step 1: A/B the `fastadhunter` pipeline bench | `cargo bench -p fastadhunter` **cannot build** at either checkout: the dev-dep `fah-api/test-harness` hits `compile_error!` under the release profile (since p5-04). D4 ran both arms with `--config profile.bench.debug-assertions=true` — equal treatment, but not the shipped codegen | record; or move the harness feature off the bench build (separate task) |
| X3 | plan/CLAUDE.md: `wip` holds at most one phase | `plan/wip/` holds `phase2.6-adaptive-stage1` **and** `phase3`; `docs/project-state.md` (2026-09-01) still says Phase 3 opens after 2.6 closes | pre-existing; project-state rewrite at phase close |
| X4 | Step 1 D14: dev-box RSS from `/api/v1/debug/memory` | `process_rss` / `process_peak_rss` are `null` on Windows (`getrusage` is unix-only); the reading was taken from the OS (`tasklist`) for the test-built binary | label as done in §Measurements |
| X5 | Step 2 item 1: path-traversal probes aimed at `/config` | Meaningful only where `/web` and `/config` are siblings. On this box `/web` does not exist (static falls to 404) and the config volume is a temp directory on another drive, so the probes prove "never the key" but cannot prove "the traversal was attempted and refused". `web.rs` unit tests pin refusal with a fixture root. Runbook item 5 repeats the probe list with `curl` against the production container (read-only GETs) | run on-device |

## Tests

| Suite | New | Pins |
| --- | --- | --- |
| `fastadhunter` `security_phase3.rs` | 6 | `ca_key_unreachable_via_every_route` — 70 documented routes × 3 credentials (none / bearer / session cookie from the first-boot password) + `logout-all` + `apikey/rotate` = 212 requests, 0 leaks (CA key, API key); `non_listed_client_is_never_minted_a_leaf` — non-listed client sees the origin's own certificate through the splice, byte-identical payload, `https-sni pass` event; trusting only our CA fails; the listed client from another loopback IP gets `kind: https` and fails; `minted_total == 0`; `bad_upstream_cert_is_not_masked` — listed client + self-signed origin ⇒ handshake error, event `status 526`, `bytes 0`, no ServerHello even for an accept-anything client, `minted_total == 0`; `exports_contain_no_private_material` — PEM export is one CERTIFICATE block whose DER equals the DER export, no key material in either, in the status document, in the import response, in `api-cert.pem` after a real import, or in `GET /api/v1/config`; `splice_is_byte_identical_when_interception_is_off` — 3 samples × 256 KiB pseudo-random payload, direct vs spliced byte-equal, events `pass` with `bytes` = payload; `dns_query_is_the_only_new_unauthenticated_route` — every `/api/**` route answers 401 without credentials (login exists), `/health` + `/dns-query` are the only non-static public paths, `/api/v1/dns-query` and `/api/dns-query` are 401, plaintext DNS on the DoT port is never answered; **closed posture** (`api.tls = false` + unloadable API pair, booted over `http://`): :53 answers, the DoT port is bindable (nothing listens), plaintext gets nothing, the log names `dot_enabled = true`, `/dns-query` is absent (N13), `/health` carries no DoT field (N3, see §Proposed doc edits) |
| `fastadhunter` `e2e_https.rs` | 1 | `full_mode_blocks_at_every_layer` — 1 DNS null-IP over UDP; 2 HTTP `/track.js` ⇒ empty 200; 3 SNI block closed before ServerHello, `https-sni block` event; 4 no-SNI hello closed, classified `pass`, listener still judging; 5 terminate leg entered, 526 (X1); 6 DoT answers the block for a client trusting only the exported CA; 7 DoH forwards and blocks. `minted_total == 1` (the DoT hostname), `unwarmed_misses == 0`. 0.9 s |
| `fah-http` `interception.rs` | 6 (+1 extended) | `eight_parallel_h2_requests_share_one_verified_upstream_session` (8 × 200, `connections == 1`); `a_streamed_request_body_reaches_the_origin_before_the_client_finishes_sending` (4 MiB POST, 1 MiB window); `a_client_that_disconnects_mid_response_returns_its_permit` and `an_upstream_that_disconnects_mid_response_ends_the_session_and_returns_its_permit` (`max_connections = 1`, second session served); `shutdown_stops_accepting_while_a_live_intercepted_session_keeps_serving` (p3-04 L4 semantics pinned: accept loop aborted, live session answers, no new session); `an_ipv6_listed_client_is_intercepted_end_to_end` (`[::1]` listed and listening, block inside TLS, event client `::1`, one mint); `an_idle_intercepted_session_is_closed_and_its_permit_returned` now asserts the permit (N10) |
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
5. **Browse** from the device: the WS feed shows `kind: https` items with real
   `method`/`path`/`status` for that client and `https-sni` for everyone else;
   `GET /api/v1/certificates` `leaf_cache.minted_total` climbs with first-sight
   hosts, `unwarmed_misses` stays 0. Record what the device shows on an
   HTTPS ad-heavy page (blocked images collapse, pages load).
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
origins, so they measure in-device CPU cost, which is the open question — the
LAN hop is not), plus the `SPLICE_BUF` 64 KiB variant of `proxy`. Then the
owner: `scp` the tar (ask first), `/container/add … interface=veth3
root-dir=/kingston/probe-bench/root comment="fah-bench"`, `/container/start`,
`/log print where topics~"container"`, `/container/remove`
(`docs/routeros-traps.md` §On-device measurement). Do **not** pin the probe
(`cpu-list` empty). P4 (DoT/DoH/UDP per query) runs from a LAN host with
`kdig` against the probe FAH instance (`kdig @172.17.0.4 +tls-host=dns.fastadhunter.lan …`,
`+https`), 3 interleaved rounds × 2 000. P6 with
`curl -w '%{time_total}'` against the probe's `/api/v1/certificates/ca/generate`
and `/import`, 5 each. Also on the probe: re-run the security suite's
traversal list with `curl -sk` against `https://172.17.0.4:8443` (X5).

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
| RSS ≤ 128 MB steady, flat (slope over the final third < 2 MB) | **gate** | `/api/v1/history/perf` `rss_bytes`, `/api/v1/debug/memory` `process_rss` (MiB vs MB) | acceptance |
| No crash, no restart, `uptime_seconds` continuous | **gate** | `/health`, container log | acceptance |
| Security suite green on the deployed build (traversal probes, X5) | **gate** | curl walk | acceptance |
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
