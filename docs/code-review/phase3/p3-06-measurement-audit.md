# p3-06 Measurement Audit — does p3-06 measure what Phase 3 needs? (read-only)

**Audited:** [p3-06 plan](../../../plan/wip/phase3/p3-06-phase3-verification-plan.md)
§Step 1–5, [p3-06 review](p3-06-phase3-verification-review.md) §Pre-declaration,
§Measurements, §Runbook 1–7, §Post-review work C; the harnesses behind them
(`fah-http/benches/{proxy,intercept}.rs`, `fah-certs/benches/certs.rs`,
`fastadhunter/benches/pipeline.rs`, `fastadhunter/tests/{security_phase3,
e2e_https,encrypted_latency}.rs`, `tests/common/mod.rs`); branch `phase3-06`
at `f8ecad2`, 2026-09-02. Follows [phase3-audit.md](phase3-audit.md).
**Question:** not "is the code right" but "does each measurement exercise the
production path and support the Phase 3 acceptance conclusion it is cited
for". Nothing modified, nothing run beyond the gates already recorded.
**State:** the verdict, map and findings below describe the checkout as of
2026-09-02 and are kept as written; every finding MA-1–MA-11 has a
resolution row in §Fixes applied (2026-09-02/03), and the dev-box re-run of
2026-09-03 is in the p3-06 review §Post-review work D.

## Verdict

**Partly.** Functional and security acceptance (Step 2 suite, Step 3 e2e) is
measured on the real binary and is adequate at the dev-box level. Performance
acceptance is **not measured anywhere yet**: every TLS/DoT/DoH row is `TBD`
(correctly), the on-device harness for P1–P6 does not exist, and the dev-box
benches establish "no regression on the pre-existing paths" plus harness
sanity — nothing more. Two dev-box labels overclaim the path exercised
(MA-1, MA-2), one on-device declaration contradicts its runbook (MA-4), the
A/B skipped two pre-existing budget rows the phase touches (MA-6), and the soak
as declared has no functional gate proving Phase 3 traffic is flowing (MA-3).

## Decisions (audit-level)

- Three tiers kept apart throughout: *runs* / *exercises the production path*
  / *sufficient for the gate*.
- A dev-box loopback figure is never "sufficient" for a TLS/socket row
  (PERFORMANCE.md §Converting); it can only be *diagnostic* or *A/B*.
- The `test-harness` dev-profile binary counts as the production path for
  functional properties, not for timing or RSS.

## Per-measurement map

| Area / gate | 1. Validates | 2. Exercises today | 3. Production path? | 4. Sufficient? | 5. Unanswered |
| --- | --- | --- | --- | --- | --- |
| **HTTP pass-through / relay** (D1, D2, p4b) | no >10 % regression on `:80` from the Phase 3 diff (one `fetch_add`/connection, `judge` refactor) | `Proxy::serve_connection` behind a hand-rolled accept loop, `FixedResolver`, no rules, keep-alive, loopback | request path yes; accept loop is not `Server::accept_loop` (pre-existing p2-02 harness, n5 fixed only the splice one) | D1 + 8 KiB: yes (±0.5–1 %, A/B/A/B, control arm). 1 MiB / 8 MiB arms ±10–23 %: cannot support a 10 % verdict — recorded, not claimed | nothing Phase 3 needs; `http_e2e.rs` green covers function |
| **DNS cache / pipeline / matcher** (D3–D5) | no regression from `Pipeline::handle(transport)` + `ClientTransport` on the event | production `Pipeline`, 1 M synthetic blocklist, mock forwarder; `matcher_lookup` synthetic corpus | yes | matcher: yes (<1 % CI). Cache/pipeline: marginal — ±9–12 % intervals against a 10 % rule, verdict rests on interleaving; both arms `debug-assertions = true` (X2) | a 5–10 % pipeline regression would be invisible; `startup` and `steady_state_memory` arms were filtered out of the A/B (MA-6) |
| **SNI verdict + splice** (D6 per-connection, D7 steady; P1, P2) | budget rows "SNI verdict + splice added latency" and "splice throughput ≥ 100 MiB/s"; `SPLICE_BUF` decision | `TlsServer::bind/serve` + `TlsProxy` with **no ruleset, no policy, no events, `FixedResolver`**; synthetic 100-byte ClientHello; raw TCP origin on loopback | accept loop and relay: yes. Verdict, policy context, DNS resolution, egress check on a resolved address, event publish: **not exercised** (`rules: None` ⇒ `Verdict::Pass` before any lookup) | D7 answers "is the relay itself far above 100 MiB/s on x86" — diagnostic. D6 harness-dominated (connect + parse + connect per 1 MiB). Neither converts. P1 not built | added latency *with* a verdict and a resolve (cache hit ≈ 2.5 µs, cache miss = an upstream RTT) is unmeasured on any box (MA-1); on-device rows TBD |
| **TLS interception handshake** (D8; P2) | row "intercepted p50 ≤ 2 × spliced p50" | fresh connection + TLS + h1 GET 1 KiB + close, three arms, client resumption disabled, loopback origin; no rules | relay and both handshakes: yes; the arms measure a whole request, not a handshake | dev box: unpinned arms within each other's intervals; pinned 4-core 1.46–2.23 × — straddles the row, cannot decide it. RTT-independent by construction (the extra origin handshake precedes the local one), so loopback does not understate | device figure (P2); whether P2 runs a LAN client (declaration) or the bench's loopback origin (runbook 5) — MA-4 |
| **Intercepted h1 / h2 relay** (D9; P3) | row "intercepted h2 relay ≥ 50 MiB/s"; S1 memory under a 64-stream stall | one h2 session, 8 MiB GET, shipped `H2_*` limits, no rules | hyper auto server + h2/h1 client, `Upstream::attempt`, `frame_for`: yes. URL-tier `lookup_http_in` skipped (`judge` still allocates url/host/path) | throughput: diagnostic only (481–620 MiB/s loopback). Stall RSS: **not measured anywhere**; the 5.5 MiB figure is arithmetic | P3 on device; h2 client → h1 origin serialisation cost (one upstream connection per session) unmeasured |
| **Prewarm / leaf cache / minting** (D10–D12; P5) | rows "cold prewarm < 1 ms", "hit rate ≥ 90 % browsing load"; N8 hop; N4 detector | `CertStore::prewarm` round-robin over 4 096 hosts (every call a miss + eviction scan); warm-host hop through `spawn_blocking`; synthetic Zipf replay | prewarm/mint/evict: yes. Hop: yes (warm pool). Zipf: not a workload | mint: yes for x86 (53.5 µs, <1 % CI); ×9 conversion unestablished for ECC assembly (phase3-audit L2). Hop: yes. Hit rate: **D12 answers nothing about browsing** — it is a property of the chosen distribution (8 × cache size) | real hit rate: only the soak's `prewarm_hits / (prewarm_hits + minted_total)` on the listed device; 24 h cannot show 7-day expiry churn. P5 (512 first-sight hosts over DoT) measures mint cost, not hit rate |
| **UDP / DoT / DoH latency** (D13; P4) | row "DoT/DoH added latency vs UDP p50" | release binary, loopback, blocked domain (in-engine), 3 × 2 000 interleaved, DoT one reused connection, DoH POST h1 keep-alive | yes — real listeners, real pipeline; DoH via the API server. h2 DoH unexercised | dev box: valid diagnostic, correctly unconverted. P4 (`kdig` from a LAN host, 3 × 2 000): production path incl. the LAN hop; `kdig +https` speaks h2, which would close the p3-05 gap if recorded | P4 must state the query domain (blocked/cached ⇒ in-engine) or the row absorbs upstream RTT; DoT handshake cost on device is not a row |
| **CA generate / import** (p3-02 debug figures; P6) | rows "< 100 ms / < 50 ms" | `store.generate_ca` / `install_api_pair` in-process, debug build | yes for the store; API layer (JSON, `spawn_blocking`, archive copy on the device's flash) only on P6 | not yet. P6 `curl -w %{time_total}` includes the API TLS handshake (≈ 7 ms on device, p5-10) — declare it or subtract `time_appconnect` | device I/O cost of archive copy + two renames |
| **RAM / steady-state** (D14; P7 soak) | hard gate "≤ 128 MB steady, full mode" | `tasklist` reading of the dev-profile `test-harness` binary after the e2e | no (Windows heap, unoptimised, two rules, one leaf) | D14: not evidence. Soak method (5-min `/telemetry`, hourly `/debug/memory`, slope over final third < 2 MB, MiB vs MB) is adequate for household traffic | the soak's 1–2 listed devices browsing normally never stall 64 streams — the interception worst case is bounded only by P3, not by the soak. Startup delta with the cert store present: undeclared (MA-6) |
| **Security suite** (Step 2, six tests) | SECURITY.md promises 1–6 at binary level | dev-profile `test-harness` binary (relaxed login rate limits, upstream-root hook); 212-request route walk with key-material detector; origin-chain check via `peer_certificates`; `minted_total == 0`; 526 path; origin→client byte identity; unauthenticated-route set; DoT closed posture | yes — auth middleware, routes, export, splice, interception gating are the shipped code; the feature changes rate limits only | yes for what it asserts. Static traversal vacuous without `/web` (F18 prints it); client→origin identity not compared (F16); "no ServerHello" inferred from the client error | on-device curl walk (Runbook 5) is the only static-root evidence; nothing measures the WAN-exposure re-check the plan names (read-only router query, not yet run) |
| **Full-mode e2e** (Step 3, seven legs) | definition of done offline: DNS null-IP, HTTP empty 200, SNI closed + `block` event, no-SNI closed + `pass`, intercepted URL block + pass-through, DoT leaf under the exported CA, DoH forward + block | real binary in `dns+http+https`, two user rules, ephemeral ports; leg 5 needs the feature-gated `FAH_TEST_UPSTREAM_ROOT` hook | yes for legs 1–4, 6, 7. Leg 5 runs **only on a build the release cannot be** — the shipped binary never executes the intercepted-URL-block scenario in software | yes as a functional gate. Two rules ≠ production ruleset (matcher scale is covered by D5, not here) | intercepted URL block on the production build exists only as Runbook 2's "browse, blocked images collapse" — an observation, not a recorded event assertion (MA-5) |
| **On-device P1–P6** | every TLS/DoT/DoH budget row; S1; P6 | **nothing** — "the probe image for these arms does not exist" (Runbook 5) | proposed: cross-compiled criterion benches with their own loopback origins inside the probe ⇒ in-device CPU cost, no veth/dst-nat hop | not yet; the declaration is sound for CPU cost | P2 declaration ("LAN client to a LAN TLS origin through the probe, 200 fresh connections") vs Runbook 5 ("they carry their own loopback origins") — two different measurements under one label (MA-4); interception CPU under browsing (plan Step 4.5) has no P-arm at all |
| **24 h soak** (P7, Runbook 6) | RAM gate, uptime gate, curl walk; watch items (a)–(f) | production container, household traffic, hourly pulls | yes | gates: yes for RAM/uptime. Watch items: (b)–(e) have read paths since post-review work A; (a) has none (documented) | **no functional gate**: the declared soak passes if dst-nat 443 is misplaced and the HTTPS listener sees zero connections. Nothing requires `listeners.https.connections > 0`, an `https-sni block` from an unlisted device, `transport: dot` items from the phone, or `https` items from the listed device (MA-3) |

## Findings — measurement validity, ranked

| # | Finding | Evidence | Class | Owner |
| --- | --- | --- | --- | --- |
| MA-1 | "SNI verdict + splice added latency" is measured without a verdict, a policy context, a resolution or an event: `TlsProxy::new` in both benches is never given `with_rules` / `with_policies` / `with_events`, and the resolver is `FixedResolver`. The per-connection production path adds `matcher.lookup_host_in` + `context_for` (ns, known from D5), `resolver.resolve` (a DNS cache hit ≈ 2.5 µs, a miss = one upstream RTT), `policy.check` and `publish`. The row label and the P2 arm inherit the omission | [proxy.rs](../../../crates/fah-http/benches/proxy.rs) `splice_in_front_of`; [intercept.rs](../../../crates/fah-http/benches/intercept.rs) `tls_server`; grep `with_rules` in benches: none | methodology (label overclaims) | p3-06 |
| MA-2 | "Intercepted relay" benches skip the URL tier: `rules: None` ⇒ `judge` returns `Pass` before `lookup_http_in`. Throughput is dominated by TLS + hyper, so the number is not wrong, but the row "intercepted h2 relay" is not "filtered intercepted relay". `url_matcher` microbench holds the missing term separately | same harness; [proxy.rs:judge](../../../crates/fah-http/src/proxy.rs) | methodology (label) | p3-06 |
| MA-3 | The soak has no functional gate. Gates are RSS, uptime, curl walk; every Phase 3 behaviour is a diagnostic watch item or absent. A misplaced dst-nat rule (the exact trap `CLAUDE.md` §Working agreement records for `add`) yields a clean 24 h soak with no HTTPS traffic ever reaching the container | [review §Runbook 6](p3-06-phase3-verification-review.md) watch table; `listeners.https.connections`, `kind: https-sni` / `https`, `transport: dot` are all readable since post-review work A but none is a gate | acceptance gap | p3-06 |
| MA-4 | P2 is declared as "LAN client to a LAN TLS origin through the probe, 200 fresh connections" but Runbook 5 runs the criterion `intercept` binary with its own loopback origin inside the probe. The first measures the user-visible path (veth, dst-nat, LAN RTT); the second measures in-device CPU. Both are useful; they are not the same row, and the declaration is binding | [review §Pre-declaration P2](p3-06-phase3-verification-review.md) vs §Runbook 5 | declaration/runbook conflict | p3-06 |
| MA-5 | The definition-of-done item "a managed client with the CA installed gets full URL-level filtering inside HTTPS" is proven in software only on the `test-harness` build (leg 5's root hook) and on the device only as "browse, blocked images collapse". No on-device step records an `https` block event for a named URL from the listed device, nor an `https-sni block` from an **unlisted** device through dst-nat | [e2e_https.rs](../../../crates/fastadhunter/tests/e2e_https.rs) leg 5; Runbook 2 line "HTTPS ad-heavy page"; Runbook 1 has no unlisted-device check | acceptance evidence gap (production build) | p3-06 |
| MA-6 | The A/B filter `full_pipeline/(blocked_query\|forwarded_query_overhead)$` excluded the `startup` and memory arms of the same bench, so the two pre-existing rows the phase can move — "Startup to serving < 3 s" (`CertStore::open`, `load_or_generate`, three more binds) and "RAM steady-state, 1 M loaded" — were not A/B'd. The plan's own diagnostic "startup delta with the cert store present" is undeclared | [run-ab.ps1](p3-06-bench/run-ab.ps1) pipeline line; [pipeline.rs](../../../crates/fastadhunter/benches/pipeline.rs) `startup`, `startup_phases` groups | regression coverage gap | p3-06 |
| MA-7 | D12 (synthetic Zipf hit rate 67 %) is cited beside the "≥ 90 % browsing load" row. The figure is a function of the chosen `ZIPF_HOSTS = 4096` (8 × the cache), not of browsing; a different constant gives any hit rate. Correctly labelled synthetic, but it answers no acceptance question and should not appear in a budget column | [certs.rs](../../../crates/fah-certs/benches/certs.rs) `certs_replay_zipf`; review §Proposed rows | not an acceptance measurement | p3-06 |
| MA-8 | P4 does not name the query domain. D13 used a blocked domain (in-engine); if P4 queries a forwarded name the "added vs UDP" row absorbs upstream RTT variance and the interleaved UDP control no longer isolates the listener cost | [review §Runbook 5](p3-06-phase3-verification-review.md) `kdig` line | declaration incomplete | p3-06 |
| MA-9 | P6 `curl -w %{time_total}` includes the API TLS handshake (≈ 7 ms on the device, p5-10) against a 50 ms import row — 14 % of the budget is the client's handshake, not the import | same | declaration incomplete | p3-06 |
| MA-10 | The intercepted worst case (64 stalled streams ≈ 5.5 MiB/session, S1) is bounded only by P3 (one session). The soak's RAM gate is taken under normal browsing by 1–2 listed devices and cannot stand in for it; a passing soak says nothing about the ceiling `[https] max_connections` documents | plan §Step 1 p3-04 carry-over; CONFIGURATION.md `[https] max_connections` | expected limitation — say so in the row | p3-06 |
| MA-11 | "Interception CPU + RSS under browsing" (plan Step 4.5) has no P-arm: P3 is RSS under a stall, P5 is mint cost. On-device CPU of the terminate leg is the number the RB5009's four cores actually need | plan §Step 4.5 vs review §Pre-declaration P1–P7 | undeclared measurement | p3-06 |

## Measured, but answering no Phase 3 acceptance question

| Measurement | Why it is not acceptance evidence |
| --- | --- |
| D6 per-connection splice (1 MiB) | harness-dominated; the row is steady-state (D7); its 16-vs-64 KiB reversal did not survive pinning |
| D8 dev-box handshake | cannot resolve intercept-vs-splice on x86 (intervals overlap unpinned; 1.5–2.2 × pinned); loopback does not convert |
| D10 prewarm hop | answers p3-04 N8 (design), not a row |
| D12 Zipf hit rate | MA-7 |
| D14 RSS 46.1 MiB | dev-profile Windows build |
| F8 pin probes | methodology; fixed PERFORMANCE.md's rule — valuable, not a gate |
| D2 1 MiB / 8 MiB arms | ±10–23 % intervals; below the 10 % rule's resolution |
| `certs_cache_hit`, `certs_prewarm_warm` | no row consumes them |

## Acceptance questions with no adequate measurement today

| Question | Nearest evidence | Status |
| --- | --- | --- |
| Every TLS / DoT / DoH budget row on the RB5009 | dev-box diagnostics | harness unbuilt (Runbook 5) |
| Leaf-cache hit rate under browsing load | D12 synthetic | soak `leaf_cache` counters, 24 h window only |
| RAM ≤ 128 MB steady, full mode | D14 | soak (P7) |
| h2 stall memory per intercepted session | arithmetic | P3 |
| Intercepted p50 ≤ 2 × spliced | straddles on x86 | P2 (after MA-4 is settled) |
| Interception CPU under browsing on four cores | none | undeclared (MA-11) |
| Android Private DNS validates against a user-installed CA | none either way | Runbook 3 (the one DoD item with no evidence) |
| Pinned / banking app unaffected | `an_excluded_sni_splices_even_for_a_listed_client` and `a_baseline_bank_is_never_intercepted_even_for_a_listed_client` (fah-http harness) — the second drives a **baseline** name, `homebanking.unicredit.ro`, through the wire path with an empty user list | Runbook 4. The app is UniCredit, covered through `unicredit.ro`, so the check has an excluded arm. A harness proves the contract, not that the app's own API hosts sit under that parent — only the device does |
| HSTS transparent on a real browser | none | Runbook 2 (implicit) |
| SNI block reaches an **unlisted** LAN device through dst-nat | e2e leg 3 (loopback) | no on-device step (MA-5) |
| DoH over h2 on the wire | none | P4 with `kdig +https` would close it if recorded |
| Startup with the cert store present; 1 M-loaded RSS after the phase | none (filtered out) | MA-6 |
| Connection ceiling under browser preconnect storms (`hello_timeout_ms` × permits) | permit-return tests | watch item (b) observes counts, not saturation |

## Fixes applied — MA-1 … MA-11, 2026-09-02/03 (owner-approved)

MA-1–MA-4 landed on 2026-09-02 under
[phase3-audit.md §Fixes applied — measurement-methodology items 1–4](phase3-audit.md);
the rows below restate them in this file's terms and add MA-5–MA-11 (2026-09-03).
Every edit outside this file is in
[p3-06-phase3-verification-review.md](p3-06-phase3-verification-review.md)
unless a path says otherwise.

| # | Fix / what changed | Kind | Production path now exercised | TBD for RB5009 |
| --- | --- | --- | --- | --- |
| MA-1 | `https_sni_splice*` proxy built with a six-rule compiled ruleset, the default `PolicyState` and a drained event channel; counters printed per group (`requests == connections`, `blocked 0` proves the verdict ran). Resolver stays `FixedResolver` — `fah-dns` is an L3 sibling, `layering.rs` checks `dev-dependencies`; declared in §Pre-declaration | benchmark code + labelling | accept loop, hello parse, SNI verdict, policy context, egress check, event publish, splice | resolve leg (DNS cache hit / miss) and the whole row: P2 |
| MA-2 | `intercept.rs` `tls_server` same wiring; `Rig` prints both listeners' counters (`requests == 2 × connections` on the intercepted arm proves one judged in-TLS request per connection) | benchmark code + labelling | terminate leg incl. `handle_intercepted` → `judge` → `lookup_http_in` (URL tier), h1/h2 relay | on-device figures: P2 (handshake), D9 on-device (relay) |
| MA-3 | Four gate rows on production telemetry in Runbook 6: HTTPS reached (`listeners.https.connections` rising ≥ 20/24 windows, `requests ≥ 1`); SNI exercised (unlisted-device probe, `blocked` +1, `https-sni block` item); interception exercised (`PUT /api/v1/rules/user` probe, `minted_total ≥ 1`, `https block` item, `requests − connections` growing); DoT exercised (`dot.state`, WS tap `transport: "dot"` per window). DoH diagnostic | runbook / acceptance procedure | production container, household traffic + two deliberate probes | the soak itself |
| MA-4 | Declaration change (block untouched): **P2 = user-visible LAN path** through the probe FAH instance, one fixed public origin, three arms interleaved, 200 rounds, min/p50/p99 of `time_appconnect − time_connect`; the cross-compiled criterion binary is **"D8 on-device"**, an in-device CPU diagnostic, not P2. Runbook 5 carries the `curl --connect-to` procedure, preconditions and the counter readings that prove each arm's leg. P1 and P3 keep the same declared-LAN vs runbook-loopback split — owner decision, not made here | declaration + runbook | real binary, veth, verdict, DNS resolution, egress, both handshakes | P2 and D8-on-device both unrun |
| MA-5 | Runbook 1 step 6: unlisted-device `curl -sv` to a blocked domain ⇒ no certificate, `listeners.https.blocked` +1, `https-sni block` item; control to an allowed domain ⇒ the origin's issuer. Runbook 2 step 5: three objective checks on the listed device — issuer `CN=FastAdHunter CA` vs the origin's issuer on the unlisted device; `Via: 1.1 fastadhunter` on a relayed response; a `PUT /api/v1/rules/user` URL rule ⇒ `200` empty body + `kind: https`, `verdict: block`, `path`, `client` item, while the unlisted device gets the origin's `404`. Browsing demoted to observation | runbook / acceptance procedure | production build, dst-nat'd 443, splice and terminate legs, URL tier inside TLS | all of it — owner-executed |
| MA-6 | Declaration bullet + Runbook 6 rows: the `startup` group A/B would be vacuous by construction (bench = `ListManager::new` + `boot()`, no Phase 3 code; `matcher.rs` gained two wrappers only); the `steady_state_memory` arm named in the bench header does not exist. Both rows are stated **outside** the dev-box regression claim and covered on the device: **P9** boot-to-serving from the container log (full mode vs the 0.3.1 `dns+http` container, "< 3 s" row); P7's RSS gate re-affirms the 1 M-loaded RAM row for full mode | declaration + runbook / acceptance procedure | `Engine::start` on the device (`CertStore::open`, DoT/HTTPS binds, `dot_tls`) | P9, P7 |
| MA-7 | Proposed-rows table: the 67 % Zipf figure struck; dev-box cell reads "none"; on-device cell names the soak counters (`prewarm_hits / (prewarm_hits + minted_total)`, 24 h) and the `https-sni` domain list as the 7-day replay corpus | declaration / labelling | — (no path) | soak counters |
| MA-8 | Declaration change: **P4** = `encrypted_latency` harness in-device from the probe image (release binary, reused connection, blocked domain ⇒ in-engine, 3 × 2 000, µs resolution — D13's quantity). **P4-LAN** = `kdig +keepopen` from a LAN host, same blocked domain, diagnostic (LAN hop, one handshake per batch, 0.1 ms resolution). Runbook 5 rewritten accordingly | declaration + runbook | real listeners (UDP, DoT, DoH on the API server), pipeline, Rule Engine block path | both unrun; probe image not built |
| MA-9 | P6 row defined as `time_starttransfer − time_appconnect` (server side, handshake excluded; archive copy, staging, renames included); `time_total` recorded as the handshake-inclusive diagnostic; bearer auth | declaration + runbook | `POST …/ca/generate`, `POST …/import` through the API on the device's storage | P6 |
| MA-10 | RSS gate row, RAM proposed-row cell and a declaration bullet state it: the soak establishes household-browsing RSS only; **P3 is the sole authority** for the 64-stream intercepted-session ceiling | declaration + runbook / acceptance procedure | — | P3 |
| MA-11 | "Interception CPU under browsing" **withdrawn** as an acceptance claim: not separable on the RB5009 (`/tool/profile` keys on process name; telemetry `process` block has no CPU seconds). Replaced by **P8**, a full-mode CPU diagnostic (`/tool/profile cpu=all` spot reads, soak deploy vs the 0.3.1 container, interpreted with `listeners.https` mix) in Runbook 6 | declaration + runbook (diagnostic) | production container under household traffic | P8; per-leg CPU stays unmeasurable |

**Checks re-run.** 2026-09-02 (MA-1/2, bench code): `cargo fmt --all --
--check` clean; `cargo clippy -p fah-http --all-targets -- -D warnings`
clean; smoke runs of both benches printed the counter proof lines (figures
not measurements). 2026-09-03 (MA-5–MA-11): documentation only, no Rust
target touched — no check applies beyond `git diff --stat -- 'crates/*/src'`
being empty. No production code changed in either round; no commit; phase
and status rows untouched.

**Pushed back:** MA-1's "real resolver path" cannot live in a `fah-http`
bench (sibling import); MA-6's A/B option is vacuous by construction, so the
documented route was taken; MA-11's measurement is definable only for full
mode, not per leg, hence the withdrawal plus P8; P1/P3 inherit MA-4's split and
remain an owner decision.

## Files changed

`crates/fah-http/benches/{proxy,intercept}.rs` (MA-1/2, 2026-09-02);
`docs/code-review/phase3/p3-06-phase3-verification-review.md` (declaration
changes, Runbook 1 step 6, Runbook 2 step 5, Runbook 5 P2/P4/P6, Runbook 6
gate and diagnostic rows, proposed-rows cells); `phase3-audit.md` (MA-1–4
record); this file.

## Remaining TODOs

Owner: P1/P3 definition (same split as P2); the probe image (criterion
`proxy`/`intercept`/`certs` + the `encrypted_latency` harness + the release
binary) before P1–P6 can run; P9 baseline read from the 0.3.1 container log
before the full-mode deploy replaces it.
