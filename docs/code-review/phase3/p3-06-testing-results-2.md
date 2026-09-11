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
| D8 | `https_handshake/spliced` | same, client trusts the origin CA | 7.9066 ms **proxy setup + relayed handshake**, not a TLS handshake — **see §D8 attribution; not a budget row** |
| D8 | `https_handshake/intercepted` | same, client trusts the FAH CA | 8.1878 ms **proxy setup + relayed handshake**, not a TLS handshake — **+3.6 % over spliced** |
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

### D8 attribution — why the spliced arm is 8 × direct

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

`tls − connect` is **not the same quantity per arm**. For the two direct rows it
is the TLS handshake. For the two proxied rows it is **proxy setup + relayed
handshake** — ClientHello read, SNI verdict, the upstream name resolution the
proxy pays and `curl --resolve` skips, the egress check, the upstream TCP
connect, then the relayed handshake. Read the proxied numbers under that name
everywhere, never as "TLS handshake".

**The whole difference is inside that window.** TCP connect is
identical (0.33–0.36 ms). Time to first byte after the handshake is identical
(≈ 0.4 ms). Teardown is *faster* through the splice (0.22 vs 0.28 ms), which
refutes the first hypothesis — EOF propagation through the relay is not the
cost.

Five hypotheses tested; three refuted, two re-opened:

| Hypothesis | Test | Result |
| --- | --- | --- |
| EOF / teardown propagation through the relay | `total − firstbyte` per arm | Refuted — splice is faster (0.22 vs 0.28 ms) |
| Nagle on the upstream leg | code read, `crates/fah-http/src/https.rs:230` | Refuted — `set_nodelay(true)` is set on the upstream socket, and `server.rs:184` sets it on the accepted one |
| The domain hand-off (detach → channel → re-register on another runtime's IO driver) | interleaved N = 0 / 2 / 0 / 2, phases each time | Refuted — N = 0 produced both the fastest (7.033 ms) and the slowest (21.314 ms) reading |
| Windows ephemeral-port / TIME_WAIT pressure (the p10 trap) | `netstat` count vs `netsh int ipv4 show dynamicport tcp` | ~~Refuted — 165 TIME_WAIT against 16 384 ephemeral ports, 1 %~~ **Withdrawn 2026-09-08: the reading was taken after the run had drained. Re-opened as the leading candidate — see below** |
| The SNI resolve the splice pays and `curl --resolve` skips | raw UDP query to `192.168.10.1`, 12 samples | ~~Refuted — min 0.65, median 0.78, max 1.24 ms~~ **Weakened 2026-09-08: the samples were one warmed record type against the LAN router. `resolve_host` issues A *and* AAAA under `tokio::join!`, so the resolve costs max of the two, and `127-0-0-2.nip.io` has no AAAA — that leg is a negative answer from a public zone whose SOA minimum decides how often it leaves the LAN. The leg that can carry the spread was never sampled. Not refuted; untested** |

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

**Status: UNRESOLVED, and bounded.** The cost is localised to the proxy setup +
relayed handshake window, is independent of N, and is not explained by connect,
Nagle or teardown. **DNS and port pressure are both open**, not eliminated —
their refutations were withdrawn and weakened respectively on 2026-09-08. What
is established: the box and the origin are
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

## Session 2 — RB5009, 2026-09-08

**Device:** RB5009UG+S+, RouterOS 7.21.5, 4× ARMv8, 1 GB shared with RouterOS.
**Probe:** `fah-probe` container on `veth3` / `172.17.0.4`, tip build, image
`fah-probe-arm64.tar`, `envlists=fahprobe-env`, `cpu-list=""`,
`mode=dns+http+https`, **N = 2** (`http_runtimes=2`, read back from
`/api/v1/config`). Production `fastadhunter` 0.3.3 stayed up on `veth1`
throughout and was never touched.

**Corpus:** `oisd-basic`, 60 748 active DNS rules, refreshed 2026-09-08T17:12Z.
**Client:** the dev box over the LAN, idle-checked per run.
**Raw output:** `p3-06-probe/campaign2/<arm>-<ts>/`, one directory per arm,
each with `run.log` and the arm's JSON.

Setup context worth carrying: the container's first list refresh fires before
its network is up, so `oisd-basic` failed at every boot with `upstream timed
out` and would not have retried for 24 h. `POST /api/v1/lists/oisd-basic/refresh`
fixes it. Upstreams themselves are healthy — the boot warning is a startup
race, not an upstream fault.

### SNI — the gate

| Statistic | Value |
| --- | --- |
| blocked closed before any certificate | **true** |
| no-SNI closed | **true** |
| allowed reached ServerHello | **true** |
| `listeners.https` delta | connections 15, requests 15, **blocked 5**, refused_claim 0, refused_destination 0, upstream_failures 0 |

Blocked name `analytics.google.com` (`||analytics.google.com^`, oisd-basic),
allowed `example.com`, 5 attempts each of blocked / allowed / no-SNI.
**PASS** — the everyone path of the definition of done, confirmed on target
hardware.

`blocked = 5 ≤ requests = 15` holds here too, consistent with the p3-04 L5
refutation recorded in the review file.

### P4-LAN — DNS per-query latency, three transports

18 000 queries, 3 rounds × 2000 per transport, transports interleaved within
each round. Blocked name, so every answer is `0.0.0.0` from the rule engine.

| Transport | n | min | **p50** | p99 | max | unanswered | unmatched |
| --- | --- | --- | --- | --- | --- | --- | --- |
| udp | 6000 | 0.184 | **0.341** | 0.733 | 2.641 | 0 | 0 |
| dot | 6000 | 0.226 | **0.442** | 0.828 | 3.744 | 0 | 0 |
| doh | 6000 | 0.460 | **0.951** | 1.612 | 55.94 | 0 | 0 |

All in ms. DoT costs **+0.101 ms** over UDP, DoH **+0.610 ms**, handshakes
excluded. DoT handshakes, recorded separately and not in the per-query figures:
19.534 / 10.293 / 4.596 ms across the three rounds — warming as the session
cache fills.

`served_issuer`: DoT `FastAdHunter CA`, DoH `FastAdHunter` (the API
certificate — DoH rides the API listener). Both as designed.

DoH's 55.94 ms max is a single event; p99 at 1.612 ms says it is not a
pattern. Recorded, not attributed.

### P5 — leaf mint, first-sight vs repeat

16 hosts over DoT, `--hosts 16`.

| Arm | n | min | **p50** | p99 | max | issuer |
| --- | --- | --- | --- | --- | --- | --- |
| first-sight | 16 | 3.503 | **3.696** | 6.404 | 6.404 | FastAdHunter CA |
| repeat | 16 | 1.354 | **1.579** | 2.686 | 2.686 | FastAdHunter CA |

Correctness, all met: `minted_total` +16, `evictions` 0, `unwarmed_misses` 0,
`superseded` 0, cache 1 → 17 of 512.

**Gate reports fail: 3.696 − 1.579 = 2.117 ms against `< 1 ms`. It does not
overturn the budget row, for two reasons.**

1. **It is not the budget's measurement.** [PERFORMANCE.md](../../../PERFORMANCE.md)
   §Budgets sets `< 1 ms` for cold prewarm and records **450.88 µs** from
   `certs_mint` criterion **on this device**, 2026-09-04. P5 measures a DoT
   handshake delta from a LAN client and carries handshake and network
   variance the criterion bench does not.
2. **It scales as expected.** The dev box measured 0.275 ms for the same
   statistic (smoke Layer 3). 2.117 / 0.275 = **7.7×**, against the measured
   ~9× x86 → RB5009 factor. Consistent scaling, not a regression.

**Disposition: RESOLVED — see §The one-shot containers below.** `fah-certs`
ran the same criterion bench on this device at **447.12 µs**, 0.8 % from the
450.88 µs budget row. The budget holds; P5's 2.117 ms is the DoT handshake
delta, not the mint path. **This is not a budget failure and must not be
recorded as one.**

### P6 — certificate generate and import, wall time

| Operation | n | **median** | budget | verdict |
| --- | --- | --- | --- | --- |
| CA generate | 2 | **5.895 ms** | < 100 ms | pass, 6 % of budget |
| certificate import | 2 | **11.127 ms** | < 50 ms | pass, 22 % of budget |

Statistic is `starttransfer − appconnect`, so it excludes connection and TLS
setup. Four rows, all `200`; `api_certificate_after.source = "imported"`.

P6 replaces the CA on every generate, so any CA exported before it is stale.

### P7 — certificate store, traversal and leak checks

| Check | Result |
| --- | --- |
| `ca/export?format=pem` | 200, `application/x-pem-file`, blocks `["CERTIFICATE"]`, **no key material** |
| `ca/export?format=der` | 200, `application/pkix-cert`, 382 bytes, valid DER |
| `GET /api/v1/config` | 200, no leaks |
| traversal, 20 paths / 40 requests | 24 SPA shell, **0 `other_200`**, 16 rejected, `failing: []` |

**PASS.** No private key is reachable over the API on the device. The image
ships `/web`, so unmatched routes answer the SPA shell — the suite asserts
needles, never status, which is why that is not a finding.

The `--ca-key` precondition was satisfied with an owner-taken SFTP copy of
`/config/ca-key.pem`, held outside the repository and deleted after the run.


### P10 — the N sweep, and the ADR-0006 revisit criterion

Two sweeps were run. **Only the second is a valid comparison.**

#### Sweep A — underloaded, superseded

`--concurrency 16 --worker-threads 4`, four N values, four arms each. Raw
output in `p3-06-probe/campaign2/p10-N{0,1,2,4}-*/`.

It reported N=0 and N=4 at roughly twice the throughput of N=1 and N=2, and
was briefly read here as evidence against N=2. **That reading was wrong and is
withdrawn.** The probe drew 0.78–1.00 cores across the whole sweep, against
2.1–3.6 cores in the phase-2.6 rig that set N=2
([alloc-domains-n-sweep.md](../phase2.6/alloc-domains-n-sweep.md) §Rig, client
`connrate.py`, 6 processes × 8 threads = 48 concurrent). At roughly a third of
the load the domain hand-off has no contention to amortise, so the shared
runtime wins on raw rps. Sweep A also measured neither DNS-under-load nor
CPU-per-request — two of the four axes that decided N=2 in the first place.

Kept as raw evidence; **no figure from sweep A belongs in a verdict.**

#### Sweep B — phase-2.6-comparable confirmation

`--concurrency 48 --worker-threads 6`, 60 s per arm, arms
`close,mixed,tls-spliced,transfers`, N ∈ {0, 1, 2}. Raw output in
`p3-06-probe/campaign2/c48-N{0,1,2}-*/`. N=4 was not re-run: phase 2.6 already
tested and rejected it (+33 % keep-alive rate for HTTP p95 of 49–53 ms against
28 ms, and +45 MiB held after the burst), and nothing here challenges that.

**The rig reproduces phase 2.6 at N=2**, which is what makes the rest of the
table comparable:

| | phase 2.6 | sweep B |
| --- | --- | --- |
| probe CPU at N=2 | 2.1–2.2 cores | **2.14 cores** |
| DNS p50 under load at N=2 | 0.96 ms | **0.973 ms** |
| CPU per request, N=2 vs N=0 | 23–26 % less | **28 % less** |

| Axis | N=0 | N=1 | N=2 |
| --- | --- | --- | --- |
| close rps | 2077.2 **(degraded — see below)** | 922.8 | **1476.9** |
| close p50 ms | 26.4 | 50.1 | **31.6** |
| close p95 ms | 48.2 | 70.3 | **50.8** |
| close p99 ms | 61.3 | 80.0 | **62.5** |
| probe cores, close | 3.08 | **1.18** | 2.14 |
| CPU ms per request | 2.016 | **1.284** | 1.454 |
| ΔRSS max, close (MiB) | 13.62 | 8.48 | **7.88** |
| DNS p50 under load (ms) | 0.955 | 1.172 | **0.973** |
| DNS p95 under load (ms) | 14.775 | **12.756** | 12.982 |
| DNS p99 under load (ms) | 21.625 | 17.969 | **17.261** |
| DNS timeouts | 0 | 0 | 0 |
| 502s, all arms | 0 | 0 | 0 |
| **client-side failures, close arm** | **33 004** | 48 | 43 |

**N=0's rps is not a throughput result.** Its close arm logged 33 004
client-side failures against 43–48 for N=1 and N=2 — Windows ephemeral-port
exhaustion on the driving host, at 10 680 TIME_WAIT and rising. A rate
measured while a third of the attempts never reached the listener is
diagnostic, not comparable. Its CPU figure is still usable, and it is the
figure that matters: **3.08 cores against N=2's 2.14, for work that was
partly not performed.**

#### The keep-alive arm — `p10-connrate`

`oha` cannot express 20 requests per connection with the last carrying
`Connection: close`, and phase 2.6's decision rested partly on keep-alive rate.
Run separately at the same rig settings — concurrency 48, 6 workers, 20
requests per connection, 60 s, plaintext through `:8080`.

| N | requests/s | connections/s | p50 ms | p95 ms | p99 ms | errors |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 1112.8 | 56.0 | 41.537 | 56.634 | 63.952 | **0** |
| 2 | **1829.4** | **91.9** | **25.201** | 52.544 | 60.780 | **0** |

Both runs are clean — zero errors, so unlike the `close` arm at N=0 these are
directly comparable. N=2 carries **64 % more requests/s and 64 % more
connections/s at 40 % lower p50**. This is an independent confirmation of N=2
over N=1, on the axis the `close` arm does not cover.

N=0 was not run on this arm: the owner closed the sweep once N=2 was confirmed
(2026-09-08).

**The TLS half of this arm did not produce a figure.** It was invoked with
`--ca probe-ca.pem`, which asserts the FastAdHunter CA issued the served
certificate — the *terminate* leg. With `https.interception.clients` empty the
probe splices, so the origin's own self-signed certificate is served and every
attempt failed `DEPTH_ZERO_SELF_SIGNED_CERT` (15 216), with 84 435
`EADDRINUSE` on top. Operator error in the flag, not a probe fault; the
correct invocation on the spliced path passes the origin's certificate. Owed,
and it does not affect the N verdict, which rests on the plaintext rows.

#### Verdict

1. **N=1 is rejected.** It is the cheapest per request (1.284 ms against
   1.454) but does not carry the workload: 37 % less throughput and
   **p95 70.3 ms against 50.8 ms**. The owner's criterion is the smallest N
   that carries the tested workload *and* keeps HTTP p95 in bounds; N=1 fails
   the second clause. This closes a gap — phase 2.6 swept {0, 2, 3, 4} and
   never tested N=1.
2. **N=2 is the smallest N that satisfies the workload and latency criteria.**
   The keep-alive arm agrees independently: 1829.4 req/s and 91.9 conn/s at
   N=2 against 1112.8 and 56.0 at N=1, both with zero errors.
3. **N=0's apparent throughput advantage is not a valid comparison** — see the
   client-failure qualification above — and it consumed materially more CPU:
   3.08 cores against 2.14, and 2.016 ms per request against 1.454.
4. **N=2 is confirmed under phase-2.6-comparable load.**
5. **ADR-0006 stands unchanged. Production remains N=2.**

DNS never degraded at any N: 300 qps sustained, 0 timeouts, p50 between 0.955
and 1.172 ms. The `transfers` arm held ΔRSS at 0–0.8 MiB across 900 MiB
relayed at every N — bounded memory holds on target hardware regardless of N.

#### What this sweep does not establish

- **Absolute throughput.** The origin (`static-web-server` 2.44.0) and the
  `oha` client share the dev box, and the client hit port exhaustion on every
  arm. Arm-to-arm at a fixed N is fair; the numbers are not a capacity figure
  for the RB5009.
- **N=3.** Untested here; phase 2.6 covers it.
- **The `mixed` and `tls-spliced` arms at N=0 and N=2**, which logged 43k–120k
  client failures. Only the `close` arm is clean enough to compare across N,
  which is why the table is built from it.


### The one-shot containers — `fah-certs`, `fah-splicebench`, `fah-p4`

Both run on `veth3`, which carries one container at a time, so each cost the
probe's downtime as well as its own runtime. Output is read from the container
log; neither writes a file.

#### `fah-certs` — criterion on the device, and P5's open question closed

Criterion at **default** 3 s warm-up and 5 s measurement, exit 0.

| Bench | RB5009 | dev box (amd64, smoke Layer 3) | factor |
| --- | --- | --- | --- |
| `certs_mint` | **447.12 µs** | 51.651 µs | 8.7× |
| `certs_cache_hit` | 382.31 ns | 52.910 ns | 7.2× |
| `certs_prewarm_warm` | 554.15 ns | 79.325 ns | 7.0× |
| `certs_replay_zipf` | 143.86 µs | 17.040 µs | 8.4× |

**The cold-prewarm budget holds.** [PERFORMANCE.md](../../../PERFORMANCE.md)
§Budgets records 450.88 µs from `certs_mint` on this device, 2026-09-04; this
run reproduces it at 447.12 µs — **0.8 % apart**, at 45 % of the `< 1 ms`
budget.

**This closes P5's open question.** P5's gate read 2.117 ms against the same
`< 1 ms` budget and was recorded above as an unresolved diagnostic. It is now
resolved: P5 measures a **DoT handshake delta from a LAN client**, not the mint
path, and carries handshake and network variance the criterion bench does not.
The mint path itself is 447 µs. **P5's gate failure is not a budget failure and
must not be recorded as one.**

The four factors — 7.0× to 8.7× — also confirm the measured ~9× x86 → RB5009
conversion on four independent benches.

Zipf replay, 100 000 handshakes over 4096 hosts against an LRU of 512:
`prewarm_hits=67323`, `minted_total=32677`, `evictions=32165`,
**hit rate 0.6732** at 8× oversubscription.

#### D6 — `SPLICE_BUF`, settled

Two runs. The first, `--reps 1 --size-mib 8`, produced a non-monotonic down
curve (16/32 below 16/16) and is a shakedown, not a figure. The second,
**`--reps 5 --size-mib 64`**, is the D6 result.

| up / down KiB | median MiB/s | min | max | worst case MiB at 1024 conns | in budget |
| --- | --- | --- | --- | --- | --- |
| loopback origin | 957.3 | 840.2 | 998.8 | — | — |
| **16 / 16 — shipped** | **351.1** | 341.3 | 362.3 | **32** | **true** |
| 16 / 32 | 450.4 | 417.2 | 481.5 | 48 | false |
| 16 / 64 | 484.8 | 465.7 | 515.8 | 80 | false |
| 16 / 128 | 493.2 | 422.5 | 541.2 | 144 | false |
| 64 / 64 | 498.5 | 495.3 | 506.3 | 128 | false |

`pick: none — no in-budget candidate reaches 0.9 x best 498.5`.

**Verdict, owner decision 2026-09-08: keep 16/16 as shipped. `SPLICE_BUF` is
not changed on this benchmark.**

1. **Up-buffer is 16 KiB.** 64/64 over 16/64 is **+2.8 %**, well below the
   +10 % threshold, and two independent runs agree (−4.1 % at 1 rep, +2.8 % at
   5). Raising `up` buys nothing.
2. **The down curve is clean and monotonic**, saturating after 64 KiB: +28 % at
   32, **+38 % at 64**, +40 % at 128. The step from 64 to 128 is worth about
   two percentage points for 64 MiB more worst-case memory.
3. **Only 16/16 fits the declared 32 MiB worst-case budget** at
   `max_connections = 1024`. Holding that ceiling costs 38 % of splice
   throughput, and that is the trade being accepted.
4. The alternatives — a higher memory budget, a lower `max_connections`, or
   accepting the current throughput — are **configuration and product
   decisions requiring an explicit call**, not conclusions this benchmark can
   draw. Changing `SPLICE_BUF` is a `src` edit with its own gates.
5. The LAN is 1 GbE and 351 MiB/s is about 2.9 Gbit/s, so the ceiling is
   unlikely to bind in practice. **This is context only and is not a reason to
   relax the declared memory ceiling.**

Counters were clean on every candidate: 6 connections, 6 requests, 0 blocked,
0 `refused_destination`, 0 `resolve_failures`, 0 `dropped_events`.

#### `fah-p4` — DNS latency from a loopback client, in-container

The harness boots its own `/fah-probe` via `FAH_E2E_BINARY`, so both the client
and the resolver are inside the container and no network is involved. 3 rounds
× 2000 sequential queries per transport, transports interleaved, blocked
domain. `test result: ok` in 8.97 s, exit 0, no `EACCES`.

| Transport | n | min | **p50** | p90 | p99 | max |
| --- | --- | --- | --- | --- | --- | --- |
| udp | 6000 | 92 | **120** | 198 | 299 | 5836 |
| dot | 6000 | 108 | **174** | 309 | 391 | 1798 |
| doh-post | 6000 | 512 | **1025** | 1351 | 1967 | 6763 |

All µs. DoH negotiated `HTTP/2.0`. DoT handshakes, excluded from the
per-query figures: 2.076 / 2.744 / 1.933 ms.

**The LAN hop costs about 220 µs.** In-container UDP p50 is 120 µs against
P4-LAN's 341 µs over the wire — a clean separation of engine cost from network
cost, which is what this container exists to give.

**DoH's cost is not transport.** In-container DoH is 1025 µs against P4-LAN's
951 µs — *slower with no network at all*. The cost is TLS plus HTTP/2 framing
on the API listener. Against the dev box it is 7.9× (1025 µs vs 129 µs), close
to the measured ~9× factor, while UDP is only 2.3× — UDP is dominated by fixed
overhead that does not scale with CPU. DoH at **8.5× UDP** on this hardware is
recorded as a shape, not a fault.

One incidental result: the probe was removed and re-added around these three
containers, and its `/config` survived intact — N=2 and `dns+http+https` read
back after the restore. The `fahprobe-config` and `fahprobe-data` mount lists
are therefore real mounts, not the silently-ineffective kind
([routeros-traps.md](../../routeros-traps.md) §Container configuration).

### Not run in this session

| Arm | Blocker |
| --- | --- |
| P1-LAN, P1-control | needs an origin under a publicly resolvable name on a **second** host; the origin currently shares the dev box with the client, which P1 cannot use |
| P3 | needs a **publicly trusted** h2 origin; still owed, unchanged since smoke Layer 3 |
| P2 | runs on the Mac; preconditions not yet met |

**The Mac is out of the rig this session, owner decision 2026-09-08.** It has
no wired path (no USB-C, so the adapter cannot be used), Homebrew could not
write to `/usr/local`, and Remote Login could not be enabled without Full Disk
Access — so no ssh and no scriptable origin. P10 therefore ran with
`static-web-server` 2.44.0 on the dev box at `192.168.10.10`, reached through
`192-168-10-10.nip.io` and the `192.168.10.10/32` egress exception, the same
arrangement phase 2.6 used. The Mac is still required for P2, which must run
*on* the second endpoint.

**Wi-Fi delta, if the Mac is used later.** P1/P2/P3 would run over Wi-Fi. This is
tolerable for P1 because its gate is already **relative** (median ≥ 0.9 ×
control median) and the absolute row was withdrawn as unmeasurable on this
topology; both arms cross the same air, so the ratio largely survives. It is
**not** tolerable for any absolute MiB/s figure, and it raises variance enough
that medians need more runs. Every Wi-Fi row carries this delta.

## Session 3 — RB5009, 2026-09-11 — tip deploy, N1, N4

**Device.** RB5009UG+S+, RouterOS 7.21.5 (long-term, build 2026-07-03), 4×
ARM64, 1024 MiB total. The production 0.3.3 container ran on `veth1`
throughout; its soak was **not** interrupted and no load arm ran, so nothing
here competes with it. The probe is `fah-probe` on `veth3`, 172.17.0.4.

**Tip.** `phase3-06` at `4365932`, tracked tree clean. Nothing in this session
is a measurement — every row is a functional check.

### Images built from the tip

Built on the dev box (Docker 29.7.2, containerd snapshotter on, QEMU arm64),
each `buildx -o type=docker` then converted with skopeo to legacy
docker-archive, because the containerd store emits OCI layout and RouterOS
hangs at `extracting` on that (deploy-rb5009.md §1). `fah-probe` carries
`--build-arg SPLICE_BUF_KIB=16`, the shipped const.

| Image | Bytes | sha256 | Build |
| --- | --- | --- | --- |
| `fah-probe-4365932-rosready.tar` | 16 886 784 | `da3b947db8bb0c82ec2f990feaefb8180216f56a9c2d4cd18a5a134d3ce2d60c` | 26 min |
| `fah-splicebench-4365932-rosready.tar` | 7 438 848 | `d8f17fad25ee50369e9cea4166537fb5df177e10342bd5acc5af275b3b79fe67` | 12 min |
| `fah-p4-4365932-rosready.tar` | 21 892 096 | `a14e0ab7f1fdad7195f470f40c100f8d5074bf5d9acf0d7b2298c4e0d5aaf269` | 55 min |
| `fah-certs-4365932-rosready.tar` | 7 101 440 | `b6c599556d3d928d8ce701c466557e50b8a3e0e943f218f23088a1856024ca1d` | 10 min |

Per image, verified from the tar before upload: `manifest.json` `Layers` are
flat `<hash>.tar`; `architecture` arm64, `os` linux; and the shipped layers
carry **no** `fastadhunter` entry — `/fah-probe`, `/fah-splicebench`,
`/fah-p4` + `/fah-probe`, `/fah-certs` respectively. `fah-splicebench` is
exactly 15 characters, at the `comm` limit, not over it.

Only `fah-probe` was started. The three one-shot benches are uploaded and
unused — they are CPU-saturating and the 0.3.3 soak is still running.

### Router steps as actually run

`R4` uploaded the **tip-stamped** names, not the runbook's
`fah-<name>-arm64.tar`; `R5`'s `file=` was adjusted to match. `/file/print`
confirmed all four at the byte counts above. RouterOS prints no hash, so size
is the only device-side check available.

`R4b`'s directories were created with `/file/add name=… type=directory`
(parent first — it does not create intermediate levels), not over SFTP.
Mount lists `fahprobe-config` / `fahprobe-data` added; production's
`fah-config` / `fah-data` untouched.

`R3` created `fahprobe-env` with the four keys, re-checked against `fah-env`
first and identical to it. The boot log's `http_runtimes=2` confirms it
attached.

`R5` added and started `fah-probe` from
`kingston/fah-probe-4365932-rosready.tar`. The container's `image-id`
`f7d1a8aa…d62d7c73fa` equals the config blob name inside the locally built
tar, so the image on the device is the one built from `4365932`.

`R2` set `engine.mode=dns+http+https` only. **`egress.allow_destinations` was
left empty** — there is no Mac in the rig (owner decision 2026-09-11,
continuing the 2026-09-08 decision), and no origin host has been chosen. Every
proxy arm and the N2/N3 device path need it set first.

The CA was generated afterwards: `CN=FastAdHunter CA`, SHA-256
`AE:6E:9A:…:F9:1C`, `not_after` 2036-09-07, `archived_previous:false`.

### N1 — migration on the probe: **PASS**

**Fixture is constructed, not found.** R0 proved on 2026-09-08 that no probe
directory, container or tar existed, and `Dockerfile.fahprobe` seeds `/config`
from an empty directory — so the campaign-1 config the R2 addendum assumes has
never existed on this device. A 98-byte `fastadhunter.toml` carrying only

```toml
[https.interception]
clients = ["10.0.0.5", "192.168.88.0/24"]
exclude_domains = ["bank.example"]
```

was placed in the config mount before the first start. The values are
deliberately off-LAN (this LAN is 192.168.10.0/24) and `bank.example` is a
reserved TLD, so no real client or host was affected. The migration path
exercised is genuine; its input was seeded by us, and no row below should be
read as evidence about a real 0.3.x upgrade.

First boot, all four reads pass:

1. `/file/print` shows `interception.json`, 108 B, in the config mount.
2. `GET /api/v1/interception` returns the seeded `clients` and
   `exclude_domains` exactly.
3. `GET /api/v1/config` carries no `https.interception`.
4. The log carries `migrated [https.interception] into interception.json
   document=/config/interception.json`.

The stored TOML went 98 → 1282 bytes — re-saved with defaults expanded and the
legacy block stripped, confirmed by reading its contents back.

**Privilege-drop path.** The migration line is timestamped *after*
`dropped privileges after binding uid=65532 gid=65532`, so the write landed on
the re-owned mount as the service user. A later `PUT` adding `probe.invalid`
answered 200 and `GET` reflected it, and the second boot read that document
back — the behavioural proof N1 asks for, ownership not being observable from
RouterOS.

Second boot: no migration line, no `ignored` line, no regenerated keys.

### N4 — negative `PUT`, atomicity: **PASS**

Run in two parts. **API half, before any device was listed.**
`PUT {"clients":["10.0.0.5"],"exclude_domains":[]}` → **200**, body echoes.
`PUT` with `"10.0.0.300"` appended → **422**,
`{"reason":"invalid_entry","list":"clients","index":1,"entry":"10.0.0.300"}`,
message `clients[1]: "10.0.0.300" is not an IP address or CIDR block`.
`GET` afterwards returns the previous document unchanged — the primary proof,
no mutation.

**Device leg, after N2/N3, with the phone listed and the CA reinstalled.**
Document before: `clients ["192.168.10.11", "2a02:2f04:5400:cc00::/64"]`,
`exclude_domains ["mob-ro.unicreditbanking.eu"]`; counter 1167, connections
1311. A `PUT` appending `"10.0.0.300"` as the third client answered **422**
with `index: 2` — the index tracks the entry's real position, not a fixed one.
`GET` returned the document byte-identical to the snapshot, and the phone's
next Chrome connection to a non-excluded host still read
`Issued by: FastAdHunter CA`. **That last read is what the API check alone
cannot give: the rejected `PUT` moved neither the stored file nor the running
policy.**

### Two observations, neither a defect

**Upstream failure at boot is a startup transient.** Both first boots logged
`all upstreams failed … upstream timed out` about 2 s in, and the scheduled
list refresh failed with it. Container networking is not ready that early.
`veth3` is bridged on `CONTAINERS` exactly like `veth1`, `srcnat` masquerades
all of `172.17.0.0/24`, and nothing in `forward` matches `172.17.0.4`;
`nslookup example.com 172.17.0.4` answered normally minutes later, and
`1.1.1.1` showed 3 attempts / 0 failures. The probe's compiled-in defaults
`1.1.1.1` and `9.9.9.9` are the same two production uses.

**The failed boot refresh does not retry on its own inside the window we
watched.** `rules` stayed 0 until `POST /api/v1/lists/refresh` was called by
hand, which returned `{"refreshed":1,"failed":0,…"rules_active_dns":55490}`.
Any later boot that loses its upstreams for the first seconds starts with an
empty ruleset until the next scheduled refresh; worth knowing before reading
any arm that assumes rules are loaded.

### Device path — steer, CA install, N2, N3

**Device.** OnePlus 15 (OxygenOS, Android), 192.168.10.11 and
`2a02:2f04:5400:cc00::/64`, static lease. Owner's own phone, owner-operated
throughout.

**R7, scoped.** Owner decision 2026-09-11: *only 192.168.10.11 is steered to
the probe; no other LAN client is affected.* The runbook's R7 steers all LAN
tcp/443; it was narrowed with `src-address=192.168.10.11`. The v6 half was
added because RA advertises `2a02:2f04:5400:cc00::1/64` on the LAN and the
phone prefers v6 — unsteered, it bypasses the probe entirely. v6 uses an
address-list `p3-06-probe-client` holding the phone's two global addresses,
and mirrors production's guards: accepts for `fah-http-skip6` and `fah-lan6`
(`2a02:2f04:5400:cc00::/56`, so LAN-internal v6 is never steered) ahead of the
dst-nat, with `to-address=…/128` and `dst-address=!…/128`.

Both rules showed the `I` invalid flag on the print immediately after `add` and
cleared on the next print, v4 and v6 alike. **Transient, twice observed** — do
not act on an `I` seen in the same breath as the `add`.

**Two traps found in sequence, both about scope living in the right place.**
Interception first failed: `example.com` served Cloudflare's real certificate
although the NAT counters showed traffic reaching the probe (v4 26 packets, v6
65). The connection went over v6, so the probe saw a v6 source, and `clients`
held only `192.168.10.11` — not a match, so it spliced. Correct behaviour, wrong
document. Fixed by listing `2a02:2f04:5400:cc00::/64` alongside the v4 address.
**The `/64` cannot over-intercept here because the steer is already scoped to
one device** — no other client's packets reach the probe, so scope lives in the
NAT rule and the document need not restate it. Exact addresses were rejected as
a design: Android rotates temporary addresses and they go stale mid-test. After
the fix, Chrome on the phone read `Issued by: FastAdHunter CA` — interception
confirmed end to end.

### N2 — the ADR-0008 path: **PASS**

App: UniCredit mobile banking (`mob-ro.unicreditbanking.eu`). Baseline
`listeners.https.client_cert_rejections` **303** before the app was opened, on
393 connections.

1. The host appeared in the Live Feed's "Certificate rejected by client" view,
   grouped by client and host, count 3, from the **v4** address — while Spotify
   and the app's Adobe/Microblink SDK hosts arrived over **v6**. One app
   straddles both families.
2. Excluded through the view's own confirmation, exact host only.
3. `GET /api/v1/interception` then held
   `exclude_domains: ["mob-ro.unicreditbanking.eu"]`, `clients` unchanged.
4. No restart, no `restart_required` anywhere in the flow.
5. After a force-close the app logged in and performed actions normally.
6. Counter 303 → **443** across the window.

**Two scope limits on this row.** Item 3's specified proof is reading the
issuer for that host and seeing it is no longer `FastAdHunter CA`; what was
observed is the app working, which is strong indirect evidence — a pinned
banking app succeeds only against the real upstream chain — but it is inference,
not the issuer read. And the +140 delta is phone-wide, not the banking app's:
Spotify, Brave, Adobe, Microblink, Allawn and Heytap were rejecting throughout.
**Per-host attribution from the view is the evidence; the counter only
corroborates.**

### N3 — `UnknownCA` is not a rejection: **FAIL — design finding**

Pass criterion: with the CA removed from the device and the probe store
untouched, the app's connections fail as `https` **status 0**, the rejection
view stays **empty** and `client_cert_rejections` stays **flat**.

Observed after removing `FastAdHunter CA` from the phone's user trust store:

| State | `client_cert_rejections` | Elapsed |
| --- | --- | --- |
| CA installed, before the banking app | 303 | — |
| after the N2 window | 443 | ~6 min |
| CA uninstalled | 855 | ~2.5 min |
| CA uninstalled | 930 | ~1 min later |

The counter did not stay flat; it accelerated roughly six-fold. The rejection
view held **56 rows** where the criterion requires zero — `login5.spotify.com`
(12), `go-updater.brave.com` (8), `links.tospotify.com` (4),
`z-m-gateway.facebook.com`, `variations.brave.com`, `i.scdn.co`,
`image-cdn-fa.spotifycdn.com`, the Heytap and Allawn hosts. The view is
cumulative and some rows predate the removal, so **the counter delta is the
clean evidence**, not the row count.

**The defect is in the premise, not the code.**
[`intercept.rs:49-56`](../../../crates/fah-http/src/intercept.rs#L49-L56)
classifies exactly as CONTEXT.md and API.md declare: `BadCertificate`,
`CertificateUnknown` and `AccessDenied` → 525; `UnknownCA` → status 0,
uncounted. What does not hold is the assumption that a client which has never
seen our CA sends `UnknownCA`.

**The finding, stated to the evidence:**

> On OnePlus 15 / OxygenOS / BoringSSL, absence of the FAH client CA produced a
> 525-class client-certificate rejection. The probe was running at INFO level,
> while the alert identity is logged only at DEBUG
> ([`intercept.rs:158`](../../../crates/fah-http/src/intercept.rs#L158)), so the
> specific alert is **not established**. The rejection is classified as 525
> `ClientCertRejected`. Because `bad_certificate`, `certificate_unknown` and
> `access_denied` all map to the same 525 classification, the current rejection
> view cannot reliably distinguish application certificate pinning from an
> untrusted or missing interception CA.

The counter moving is what proves a 525-class alert was sent; no read path
carries the alert name — `GET /telemetry` and the event stream both surface only
the status. Naming the alert needs a deliberate re-run at `log.level=debug` with
the steer restored, and the design consequence above does not depend on it.

The tell was already in the N2 baseline and was read too generously at the
time: `client_cert_rejections` stood at **303 while the CA was installed and
trusted**, because Android apps targeting API 24+ ignore the user trust store.
Removing the CA did not change the alert the device sends — it only widened the
set of apps sending it. N2 and N3 were measuring **one phenomenon**, not two.

**Consequence, and it is design-level.** The rejection view cannot distinguish
*this app pins and refuses our leaf* from *this client never trusted our CA*.
Both render as 525 rows offering the same `Exclude` action, and excluding is the
wrong remedy for the second: it permanently surrenders interception for a host
in order to paper over a device-side install problem. Recorded against p3-08
(classification) and p3-09 (the operator action), not against this run.

**Scope.** One device — OnePlus 15, OxygenOS, Android's BoringSSL. Nothing here
shows how iOS, Windows or other TLS stacks behave; they may send `UnknownCA` and
work exactly as designed. Superseded by a second device disagreeing.

### D1 — dashboard at 390 px, both themes: **run, three defects found and fixed**

Driven by Playwright against **the probe itself** — the real dashboard and API
on the RB5009 at `https://172.17.0.4:8443` — not a dev server. p3-09's §Known
limitations had recorded this check as owed precisely because "it needs the
dashboard served against a running API, which was not stood up"; the probe
built in this session is that API.

**Method note.** Screenshots the MCP browser wrote landed outside the
workspace, so the arms below are **measured** through `getComputedStyle` and
`getBoundingClientRect` rather than eyeballed, with screenshots read back only
to confirm the visible result. Measurement is the stronger form here: p3-09's
standing claim was a *reuse argument* ("every class used is already carried by
a surface verified at that width"), and only numbers could falsify it.

| Arm | Result |
| --- | --- |
| Rejection view, empty, 390, light | PASS — `scrollWidth` 375 ≤ 390, no element past the viewport, all controls 44 px |
| Rejection view, 8 cards, 390, light | PASS — cards `display: flex`, page scrolls (1747 vs 844), zero overflow, longest domain 300 px unwrapped |
| Rejection view, 8 cards, 390, dark | PASS — domain 13.29:1, secondary 6.63:1 against the card, both above AA |
| Interception card, 390, dark | `.set-field` collapses to one 321 px column as claimed; Reset/Save 52 px; no page overflow |
| Interception card, 1280, light + dark | editor text 15.04:1; defects 1 and 3 below |

**Three defects, all fixed in `dashboard/frontend/src/styles/components.css`
(+29 lines, no deletions; frontend suite 1043 tests / 58 files green).**

| # | What | Before | After |
| --- | --- | --- | --- |
| 1 | The line editor never grew past the textarea's intrinsic `cols=20`. `.editor-area` is already `width: 100%`, but the wrapper between it and `.set-field-control` is a flex item, and a flex item shrinks to content unless told to grow | 161 px of 473 at 1280; 161 px of 321 at 390; `2a02:2f04:5400:cc00::/64` and `mob-ro.unicreditbanking.eu` truncated behind a sideways scrollbar | 429 px at 1280, 287 px at 390, no sideways scroll, both values fully visible |
| 2 | `Exclude` missed the phone touch-target rules, which name `.feed-actions .btn` and `.feed-chipset .chip`; it is a `.btn` inside `.ev-meta` | 34 px, against 44 px for Pause/Clear/the view chips in the same view | 44 px, matching Pause |
| 3 | The focused textarea painted over the sticky save bar while a long editor scrolled. `.editor-area` is `position: relative; z-index: 1`; `.set-bar` was `position: sticky; z-index: auto`; same stacking context, `auto` loses to `1` | textarea bottom 852.9 vs bar top 779.8 — 73 px of overlap, drawn over `Save changes` | `.set-bar` topmost under `elementFromPoint` at its own centre with the textarea focused |

**Defect 3 was found by the owner, not by the sweep.** It only appears when the
card is tall enough to scroll *and* the textarea holds focus, which no
systematic width/theme pass reaches. Recorded because the lesson generalises:
this arm's grid is width × theme, and a state axis — focused, scrolled, dirty —
is not in it.

Defects 1 and 3 are **not** specific to the Interception card. `.set-bar` is the
shared settings save bar and `.editor` the shared line editor, so every section
with a tall editor had both; `[egress]`'s `allow_destinations` was measured
showing defect 1 identically (161 px of 473 at 1280). The fixes are shared in
the same way.

**Scope limit.** The rejection-view rows were **injected into the DOM** using
the component's own card markup, because the R7 steer was removed after the
N-rows and no live rejections arrive at the probe. Those arms therefore verify
the CSS and layout, **not** the data path or the grouping logic. The
Interception card was measured entirely as served, with the real document
(`192.168.10.11`, `2a02:2f04:5400:cc00::/64`, `mob-ro.unicreditbanking.eu`).
The fixes are in source only — the probe still serves the pre-fix bundle, so
confirming them on the device needs a rebuild and redeploy.

### Standing state at session end — device restored, steer removed

The phone is **restored**. `FastAdHunter CA` was reinstalled in its user trust
store after N3, and the R7 steer was removed once the N-rows were done, so the
device is off the interception path entirely — Spotify, which failed throughout
N3, works again. That last check is the practical confirmation, not the counter.

Why the steer came off rather than staying for the soak: **it was breaking the
owner's daily phone, and not only because of N3**. The 303 baseline accumulated
while the CA was installed and trusted, because apps targeting API 24+ ignore
the user store — so under interception only Chrome and the excluded UniCredit
host worked. The soak has not started, so nothing was being measured from that
device; three days of broken apps would have bought nothing.

| What | State at session end |
| --- | --- |
| R7 v4 | **Removed** 2026-09-11 after the N-rows. `/ip/firewall/nat/print where comment~"p3-06"` returns nothing |
| R7 v6 | **Removed** the same way; `/ipv6/firewall/nat` likewise clean |
| Address list | `p3-06-probe-client` **retained**, two global v6 addresses, inert without a rule referencing it. Kept because it is the fiddly part to reconstruct |
| Interception Document | **Retained**: `clients ["192.168.10.11", "2a02:2f04:5400:cc00::/64"]`, `exclude_domains ["mob-ro.unicreditbanking.eu"]`. Inert — no traffic reaches the probe |
| Probe | `fah-probe` **running** on `veth3`, `dns+http+https`, CA present, 55 490 rules, `start-on-boot=no` |
| Production | 0.3.3 on `veth1`, untouched throughout, soak running to 2026-09-14 |

**To resume at soak start**, re-add the two `add` pairs recorded in §Router
steps — v4 with `src-address=192.168.10.11`, v6 with
`src-address-list=p3-06-probe-client` behind the `fah-http-skip6` and `fah-lan6`
accepts. Watch-list row (i) then captures the posture fresh, which is why the
document and address list being retained does not weaken that baseline: the
snapshot is taken after the steer is back, not inherited from here.

One caveat carried forward: the phone's v6 addresses in `p3-06-probe-client`
were read on 2026-09-11 and Android rotates temporary addresses. **Re-read them
before trusting the list at soak start** — the document's `/64` entry covers the
interception side, but the address list drives the steer and a stale entry means
the phone is simply never steered.
### Not run in this session

| Arm | Blocker |
| --- | --- |
| P1, P2, P3, Runbook 1–4 | load arms. The 0.3.3 production soak runs to 2026-09-14; running them now risks invalidating it, as the 2026-09-08 flood did |
| N5, N6 | inside the 24 h full-mode soak (R11), which has not started |
| The three one-shot benches | uploaded, never started, same soak reason |

## Owed

| Item | Why it is not here |
| --- | --- |
| D6's `SPLICE_BUF` 16 vs 64 KiB comparison | Declared as **two builds of the tip**; the 64 KiB build needs a `src` edit, which is a code change awaiting owner approval |
| D12 against a real host distribution | The shipped arm is synthetic. A hashed real-traffic capture is running — see §Corpus |
| Every P-series arm | On-device, owner-executed. The probe preconditions are met since Session 3 — the blockers are now the 0.3.3 soak running to 2026-09-14 and an unset `egress.allow_destinations` with no origin host chosen |
| N2, N3, and N4's device leg | **All run 2026-09-11** (§Session 3): N2 PASS, N4 PASS, **N3 FAIL** — a design finding against p3-08/p3-09, not a defect in this run |
| N5, N6 | Live inside the 24 h full-mode soak (R11), not yet started |

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
