# p3-06 — testing results (campaign 2)

Figures for the post-merge tip. Campaign 1 —
[p3-06-testing-results.md](p3-06-testing-results.md) — is superseded in full;
nothing from it is carried here.

Declaration: the D-arm table in
[p3-06-phase3-verification-review.md](p3-06-phase3-verification-review.md)
§Pre-declaration, with the campaign-2 changes in §Pre-declaration — campaign 2.
Every row below carries corpus, workload, device and **N**. A row with no N is
diagnostic.

## Session 1 — dev-box benches, 2026-09-08

**Device.** bobdenaut: x86_64, 32 logical cores, Windows 11 IoT Enterprise
LTSC 2024. Owner confirmed the box idle for the whole window; no browser, no
video, no other build. Windows heap for criterion harnesses; the binary carries
mimalloc.

**Checkouts.** **A** = `main` `857865d` (0.3.3) in the worktree
`E:/FastAdHunter-main857`, its own `target/`. **B** = `phase3-06` tip
`ce3c6e6` plus an uncommitted working tree of four **test-only** files —
`crates/fah-api/src/config_store.rs` (a boot-key list inside `#[cfg(test)]`),
`crates/fah-http/tests/sni.rs`, `crates/fastadhunter/tests/common/mod.rs`,
`crates/fastadhunter/tests/e2e_https.rs`. None is on a benched path. Campaign 1
ran the same shape (`877aad2` + working tree).

**Timing.** Criterion A/B `09:26:53Z`–`09:44:01Z`; D13, D14 and the D8
attribution after it. Raw output and the runner in
[p3-06-bench/campaign2-20260908T0905Z/](../phase3/p3-06-bench/campaign2-20260908T0905Z/).

**Pinning.** D3, D4, D5, D11, D12 pinned to one core with
`ProcessorAffinity = 4`, `PriorityClass = High`, applied to the `cargo` process
so the bench binary inherits it. `fah-http` benches unpinned, per the
declaration.

### Session validity

Control arm `http_pass_through/direct_to_origin`, A mean **32.41 µs** vs B mean
**33.39 µs** = **+3.0 %**, inside the ±5 % rule. **Session valid.**

Round-to-round the control spans 31.92–34.86 µs, a **9.0 %** band, and it is
concentrated in one round: B r1 carries both the control outlier (34.86 µs) and
the 1 MiB direct outlier (1138 µs). The band bounds what this session can
resolve; it does not invalidate it.

### D1–D5 — A/B, existing DNS and HTTP benches

All four rounds ran A r1, B r1, A r2, B r2. Values are criterion means.

| Arm | A r1 | B r1 | A r2 | B r2 | A mean | B mean | Δ |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D1 `http_pass_through/direct_to_origin` (control) | 32.754 µs | 34.863 µs | 32.065 µs | 31.915 µs | 32.409 | 33.389 | +3.0 % |
| D1 `http_pass_through/through_proxy` | 65.579 | 68.347 | 62.161 | 63.858 | 63.870 | 66.102 | +3.5 % |
| D2 `http_opaque_body` 8 KiB direct | 33.917 | 35.366 | 33.656 | 34.287 | 33.787 | 34.826 | +3.1 % |
| D2 `http_opaque_body` 8 KiB proxy | 66.532 | 68.683 | 68.106 | 66.436 | 67.319 | 67.560 | +0.4 % |
| D2 1 MiB direct | 835.56 | 1138.20 | 663.81 | 661.45 | 749.69 | 899.83 | **+20.0 %** |
| D2 1 MiB proxy | 945.24 | 853.04 | 766.72 | 787.02 | 855.98 | 820.03 | −4.2 % |
| D2 8 MiB direct | 5.5321 ms | 5.8448 ms | 5.9372 ms | 5.4583 ms | 5.7347 | 5.6516 | −1.4 % |
| D2 8 MiB proxy | 6.2410 ms | 6.4594 ms | 6.3395 ms | 6.3901 ms | 6.2903 | 6.4248 | +2.1 % |
| D3 `dns_cache/cache_hit_in_engine_latency` | 2.6780 µs | 2.7064 | 2.7760 | 2.8446 | 2.7270 | 2.7755 | +1.8 % |
| D4 `full_pipeline/blocked_query` | 3.6309 µs | 3.4631 | 3.3039 | 3.5197 | 3.4674 | 3.4914 | +0.7 % |
| D4 `full_pipeline/forwarded_query_overhead` | 4.5092 µs | 4.7260 | 4.3409 | 4.3726 | 4.4251 | 4.5493 | +2.8 % |
| D5 `matcher_lookup/hit_exact` | 64.221 ns | 63.725 | 63.635 | 64.728 | 63.928 | 64.227 | +0.5 % |
| D5 `matcher_lookup/hit_subdomain` | 197.61 ns | 188.96 | 194.18 | 196.61 | 195.90 | 192.79 | −1.6 % |
| D5 `matcher_lookup/miss` | 48.688 ns | 47.552 | 47.961 | 48.037 | 48.325 | 47.795 | −1.1 % |

**Regression verdict (>10 % rule): none.** Every arm sits within ±4.2 %.

The one exception is the 1 MiB **direct** arm at +20 %. That arm is the control
for its own size class, so the 1 MiB class is simply unresolved in this
session — its proxy arm moved −4.2 %, well inside its own control's noise.
Campaign 1 saw the same shape at +10 % and recorded it the same way. **Nothing
is claimed for the 1 MiB size class.**

**D4 build note.** `cargo bench -p fastadhunter` does not compile at either
checkout: `fastadhunter`'s dev-dependency pulls `fah-api/test-harness` into the
bench profile, which is a release profile, and
`crates/fah-api/src/lib.rs:17`'s `compile_error!` fires. Both arms were built
with `--config 'profile.bench.debug-assertions=true'` passed on the command
line — campaign 1's X2 caveat, but with **no edit to `Cargo.toml`**. Both sides
carry it, so the A/B is unaffected; absolute D4 values are inflated by debug
assertions and overflow checks and are not comparable across campaigns.

### D6–D12 — tip-only, the new budget candidates

No A/B by construction — these paths do not exist at `857865d`. All at
**N = 2** where N applies; the criterion benches drive `TlsServer::serve`
(shared dispatch), so N is not a factor for D6–D10.

| # | Arm | Workload | Measured |
| --- | --- | --- | --- |
| D6 | `https_sni_splice/direct_to_origin` | raw TCP origin, 1 MiB per connection, synthetic ClientHello `origin.test` | 640.72 µs = **1.5242 GiB/s** |
| D6 | `https_sni_splice/through_splice` | same | 6.0345 ms = **165.71 MiB/s** = 10.6 % of direct |
| D7 | `https_sni_splice_steady_state/direct_to_origin` | one connection, 64 MiB | 22.576 ms = **2.7684 GiB/s** |
| D7 | `https_sni_splice_steady_state/through_splice` | same | 56.676 ms = **1.1028 GiB/s** = **39.8 % of direct** |
| D8 | `https_handshake/direct_to_origin` | TLS origin (rcgen CA, h1), fresh connection per iteration, `Connection: close` | 959.91 µs |
| D8 | `https_handshake/spliced` | same, client trusts the origin CA | 7.9066 ms — **see §D8 attribution; not a budget row** |
| D8 | `https_handshake/intercepted` | same, client trusts the FAH CA | 8.1878 ms — **+3.6 % over spliced** |
| D9 | `https_h2_download/direct_to_origin` | h2 origin, one connection, one 8 MiB GET | 6.1806 ms = **1.2640 GiB/s** |
| D9 | `https_h2_download/spliced` | same | 8.7907 ms = **910.05 MiB/s** |
| D9 | `https_h2_download/intercepted` | same | 15.755 ms = **507.78 MiB/s** (÷ direct 0.39, ÷ spliced 0.56) |
| D10 | `prewarm_hop/inline_cached_leaf` | warm host, multi-thread runtime | 91.431 ns |
| D10 | `prewarm_hop/inline_prewarm_warm` | same | 118.42 ns |
| D10 | `prewarm_hop/spawn_blocking_prewarm` | same | 4.7132 µs — the hop costs **40×** inline |
| D11 | `certs_mint` | p3-01 bench as shipped, pinned | 53.692 µs |
| D11 | `certs_cache_hit` | same | 48.554 ns |
| D11 | `certs_prewarm_warm` | same | 75.016 ns |
| D12 | `certs_replay_zipf` hit rate | **synthetic** Zipf(s = 1) over 4 096 hosts, 100 000 handshakes, LRU 512, seed fixed | `prewarm_hits` 67 323, `minted_total` 32 677, `evictions` 32 165, **hit rate 0.6732** |

Mint costs **≈ 1 100 ×** a leaf-cache hit (53.692 µs vs 48.554 ns). That ratio,
not the hit rate, is the case for the cache.

D12 reproduces campaign 1's 0.673 exactly — the replay is deterministic, so
this is a build check, not new information. **The corpus is still synthetic**;
a real-distribution replay is owed (§Owed below).

**Verdict path ran on every HTTPS arm.** `connections == requests`,
`dropped_events = 0`, `blocked = 0`, `refused_destination = 0`,
`resolve_failures = 0` on D6/D7; `refused_claim = 0`,
`upstream_cert_failures = 0` on D8/D9. The SNI verdict, the URL tier and
`publish` are therefore inside every figure above, per the campaign-1 harness
fix.

### D13 — DoT / DoH added latency, in-engine

Release binary via `FAH_E2E_BINARY` (so the harness stays in the dev profile
and drives a real `cargo build --release` binary), loopback client, blocked
domain ⇒ answered in-engine, 3 interleaved rounds × 2 000 queries per
transport. N not applicable — the DNS listener does not run on the HTTP
allocation domains.

Per-round p50, showing no drift: udp 28 / 29 / 29 µs, dot 36 / 37 / 37 µs,
doh 260 / 263 / 282 µs.

| Transport | n | min | p50 | p90 | p99 | max | added vs UDP (p50) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| UDP/53 (in-run control) | 6 000 | 13 µs | **29 µs** | 38 µs | 107 µs | 1 040 µs | — |
| DoT, one reused connection | 6 000 | 24 µs | **37 µs** | 42 µs | 94 µs | 199 µs | **+8 µs** |
| DoH POST, **HTTP/2.0** keep-alive | 6 000 | 237 µs | **268 µs** | 351 µs | 584 µs | 1 424 µs | **+239 µs** |

DoT handshakes 1.9166 / 1.8761 / 1.8875 ms, **excluded** from the per-query
figures.

**Not comparable to campaign 1's D13.** That run negotiated HTTP/1.1 on the DoH
leg (158 µs p50); this one asserts and gets HTTP/2.0 (268 µs p50). Different
protocol, different row.

**Not converted.** The ×9 dev→RB5009 factor is not applied to the TLS legs.
P4 sets the budget row.

### D14 — dev-box RSS, full mode

`engine.mode = "dns+http+https"`, **N = 2**, release binary, one user rule
(`||ads.smoke.test^`), **no compiled blocklists**. Workload: 300 iterations of
{one uncached DNS query, one blocked DNS query, one spliced HTTPS request, one
HTTP request}.

| Reading | Value |
| --- | --- |
| RSS at boot (`tasklist`) | 25 468 K = **24.9 MiB** |
| RSS after the workload | 25 912 K = **25.3 MiB** |
| ΔRSS over 300 connections per lane | **+444 K = 0.43 MiB** |
| `allocator_committed_bytes` | 39.6 MiB |
| `allocator_committed_peak_bytes` | 47.2 MiB |
| `ruleset_bytes` | 1.95 MB |

`GET /api/v1/debug/memory` returns **`process_rss: null`** on Windows, along
with `process_peak_rss`, the page-fault counters and `cpu_user_ms` /
`cpu_system_ms` — those readings are `/proc`-backed and exist in-container
only. The RSS numbers above come from `tasklist`, as campaign 1's X4 recorded.
`allocator_committed_*` **is** available on Windows and is the more portable of
the two.

**This is not the 128 MB budget check.** The budget assumes compiled
blocklists; this instance carries a single user rule. It is a floor reading and
a bounded-growth check: 600 proxied connections moved RSS by 0.43 MiB. The
soak on the device is the authority.

Listener counters after the workload: `https` 300 connections / 300 requests /
0 failures of any kind — the splice served 300/300. `http` 300 connections with
`upstream_failures: 300`, expected: the HTTP lane proxies to port 80 and
nothing listens there.

### D8 attribution — why the spliced handshake is 8 × direct

Owner-directed, **attribution only; no implementation changed and the figure is
not promoted to a budget or gate row.**

The three D8 arms share one timing boundary — `fetch_h1_once`, from TCP connect
to body drained — and differ only in destination and client trust root. So the
harness is not charging setup differently per arm. The cost was localised with
`curl` phase marks against the **shipped binary**, not the bench harness, on
the same quantity.

Phase medians, ms, cumulative from request start, N = 30, `Connection: close`,
origin = node HTTPS on `127.0.0.2:443` with an RSA-2048 leaf:

| Arm | connect | **tls − connect** | firstbyte − tls | total − firstbyte |
| --- | --- | --- | --- | --- |
| direct to origin | 0.334 | **4.116** | 0.461 | 0.278 |
| through splice | 0.341 | **20.087** | 0.431 | 0.223 |
| direct, keep-alive | 0.344 | **4.224** | 0.386 | 0.034 |
| splice, keep-alive | 0.360 | **12.067** | 0.385 | 0.037 |

**The whole difference is inside the TLS handshake window.** TCP connect is
identical (0.33–0.36 ms). Time to first byte after the handshake is identical
(≈ 0.4 ms). Teardown is *faster* through the splice (0.22 vs 0.28 ms), which
refutes the first hypothesis — EOF propagation through the relay is not the
cost.

Four hypotheses tested and refuted:

| Hypothesis | Test | Result |
| --- | --- | --- |
| EOF / teardown propagation through the relay | `total − firstbyte` per arm | Refuted — splice is faster (0.22 vs 0.28 ms) |
| Nagle on the upstream leg | code read, `crates/fah-http/src/https.rs:230` | Refuted — `set_nodelay(true)` is set on the upstream socket, and `server.rs:184` sets it on the accepted one |
| The domain hand-off (detach → channel → re-register on another runtime's IO driver) | interleaved N = 0 / 2 / 0 / 2, phases each time | Refuted — N = 0 produced both the fastest (7.033 ms) and the slowest (21.314 ms) reading |
| Windows ephemeral-port / TIME_WAIT pressure (the p10 trap) | `netstat` count vs `netsh int ipv4 show dynamicport tcp` | ~~Refuted — 165 TIME_WAIT against 16 384 ephemeral ports, 1 %~~ **Withdrawn 2026-09-08: the reading was taken after the run had drained. Re-opened as the leading candidate — see below** |
| The SNI resolve the splice pays and `curl --resolve` skips | raw UDP query to `192.168.10.1`, 12 samples | Refuted — min 0.65, median 0.78, max 1.24 ms |

The N sweep readings, interleaved, `tls − connect` for the splice arm:
**7.033 → 11.686 → 21.314 → 20.031 ms** across four consecutive arms, while the
direct control in the same four arms stayed flat at **4.116 → 4.228 → 4.157 →
4.466 ms**. The proxied path drifts by 3 × within one session; the direct path
holds an 8 % band. The FAH process was restarted between every arm, so nothing
accumulates inside it.

**Correction, 2026-09-08 (later the same day).** The TIME_WAIT row above is
**withdrawn**. That reading of 165 was taken after the run had drained, which
does not support a refutation. During the smoke session's `p10` close arm this
box reached **15 543 TIME_WAIT — 95 % of the 16 384-port ephemeral range** —
and was watched draining to 12 in about 100 seconds. The D8 attribution runs
were four back-to-back `Connection: close` arms inside roughly two minutes,
and the spliced path opens two sockets per request where direct opens one,
which fits the observed 7 -> 12 -> 21 ms drift against a flat direct control.
Port pressure is therefore the **leading candidate**, not a refuted one. Not
chased further (owner instruction); recorded because a wrong refutation is
worse than an open question.

**Status: UNRESOLVED, and bounded.** The cost is localised to the TLS handshake
window, is independent of N, and is not explained by connect, DNS, Nagle,
teardown or port pressure. What is established: the box and the origin are
healthy (flat direct control), and the variance belongs to the proxied path.

**Consequences.**

1. D8's absolute `spliced` and `intercepted` values are **not budget rows** on
   this box.
2. The **`intercepted − spliced` delta of +3.6 % survives**, because both legs
   pay the unexplained cost and it cancels.
3. P2 on the device — a LAN client through the probe binary, 200 fresh
   connections per arm, min / p50 / p99 — is the declared authority for the
   handshake row and is unaffected by this. The declaration already says
   TLS/HTTP figures do not convert from the dev box.

## Owed

| Item | Why it is not here |
| --- | --- |
| D6's `SPLICE_BUF` 16 vs 64 KiB comparison | Declared as **two builds of the tip**; the 64 KiB build needs a `src` edit, which is a code change awaiting owner approval |
| D12 against a real host distribution | The shipped arm is synthetic. A hashed real-traffic capture is running — see §Corpus |
| Every P-series arm | On-device, owner-executed. Blocked on the probe preconditions (verification plan §Step 4.0) |

## Corpus — D12's real-distribution replay

Owner decision 2026-09-08: derive the distribution from the production
container's live event feed rather than from a synthetic Zipf or from
`GET /api/v1/history/top`, which caps at `n = 100` domains and so cannot
exercise a 512-entry LRU at all.

`docs/code-review/phase3/p3-06-corpus/capture-hosts.mjs` subscribes to
`WS /api/v1/events` on the 0.3.3 production container (read-only) with
`{"subscribe":["query"]}` and records `{ts, kind, host}` per query. **The host
is hashed at capture time** — SHA-256, first 16 hex characters — so no
household domain name is ever written to disk. Cardinality and repeat rate,
which is all the replay needs, are preserved exactly; the names are not
recoverable.

The question the corpus answers is not the hit rate directly but **how many
distinct hosts this deployment names against a capacity of 512**. If the
working set is far below 512 the D12 row is trivially met and the synthetic
0.6732 understates it; if it is far above, the row is a real constraint.

State: capture paused at 119 rows for the bench window, to be resumed.
Production is `dns+http`, so every row is `kind: dns` — a DNS-query
distribution standing in for SNI names. **That substitution is a declaration
change and is recorded as such**, not presented as an `https-sni` corpus.

## Smoke

Layer 0 of [p3-06-smoke-plan.md](../../../plan/wip/phase3/p3-06-smoke-plan.md)
ran on 2026-09-08; output in
[p3-06-probe/smoke-20260908T0905Z/layer0/](p3-06-probe/smoke-20260908T0905Z/layer0/).
Nothing in it is a measurement. Layers 1–3 are not yet run.
