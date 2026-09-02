# Phase 3 Audit — p3-01 → p3-06 cross-phase (read-only)

**Audited:** branch `phase3-06` at `f8ecad2` (base `64be513`), 2026-09-02.
**Method:** fresh read of every Phase 3 source file in `fah-certs`, `fah-http`
(`https`, `intercept`, `sni`, `tls`, `tls_server`, `exclusions`), `fah-api`
(`certs`, `doh`, `routes`, `state`), `fah-dns` (`dot`, `server`, `tcp`),
`fah-config` (`https`, `listen`, `lib`), `fastadhunter` (`main`, `adapters`);
diff-first for the rest; the six review files read for known findings only.
Gates re-run on this checkout: clippy `--all-features -D warnings` clean;
`cargo test --all-features --workspace` 0 failed, no DEGRADED/SKIPPED line.
Three raw bench outputs spot-checked against the p3-06 tables (match).
Nothing modified.

## Verdict

**PASS WITH FINDINGS.** No blocker or high. Two MEDIUM (both documentation
semantics), four LOW, three NIT. Code is internally coherent; the p3-01
security invariants hold after p3-03/04/05/06; all TLS budget rows are
correctly `TBD` pending the RB5009.

## Decisions (audit-level, none binding)

- Findings ranked by consequence for the soak reading, not by code size.
- Known findings re-verified from source, not from review verdicts.
- Android user-CA-store behaviour treated as **unverified**, not as a defect.

## 1. New findings

### MEDIUM

| # | Finding | Evidence | Owner | Class |
| --- | --- | --- | --- | --- |
| M1 | `unwarmed_misses` documented meaning is false for DoT: API.md says a miss "failed closed"; the DoT resolver serves the API-pair fallback after the same miss. With the default `dot_enabled = true` and no CA (state after first boot until the runbook generates one), every SNI-bearing DoT hello increments it while succeeding | [API.md:1306-1314](../../../API.md#L1306-L1314); [dot.rs:33-47](../../../crates/fah-dns/src/dot.rs#L33-L47) fallback; [leaf.rs:134](../../../crates/fah-certs/src/leaf.rs#L134), [leaf.rs:331](../../../crates/fah-certs/src/leaf.rs#L331); p3-05 review line 154 records shipped semantics | p3-05 (doc), p3-06 (soak watch item 3) | doc/telemetry drift, not a code defect — **fixed 2026-09-02**, see §Fixes applied |
| M2 | `counters.http.{pass,allow,block,response_bytes}` and `latency.http` absorb HTTPS-listener events: one `https-sni` item per spliced connection (spliced bytes as `response_bytes`, hello-to-connect duration as `forward`), plus session-level `https` items with status 0/526. API.md documents only `refused` as merged; its compatibility contract says existing fields keep meaning | [main.rs:936-938](../../../crates/fastadhunter/src/main.rs#L936-L938) → [registry.rs:183-197](../../../crates/fah-metrics/src/registry.rs#L183-L197); [API.md:120-127](../../../API.md#L120-L127); p3-03 review line 170 ("not split per protocol") | p3-03 | pre-existing decision; doc debt new |

### LOW

| # | Finding | Evidence | Owner | Class |
| --- | --- | --- | --- | --- |
| L1 | DoT accept path takes the CA `std::Mutex` on a tokio worker while `install_ca` holds it across stage/archive/commit file I/O. A CA regeneration stalls DoT handshakes for the I/O duration | [dot.rs:124](../../../crates/fah-dns/src/dot.rs#L124) `has_ca()` → [store.rs:311](../../../crates/fah-certs/src/store.rs#L311); [store.rs:328-375](../../../crates/fah-certs/src/store.rs#L328-L375); p3-05 review line 440 calls it uncontended | p3-05 | new; bounded by regeneration rarity |
| L2 | ×9 factor applied to ECDSA P-256 minting ("CPU-bound, converts"). Factor calibrated on matcher/cache/pipeline; aws-lc-rs uses per-arch assembly, ratio unestablished for crypto. PERFORMANCE.md row correctly `TBD`; only the review wording overclaims | [p3-06 review:221](p3-06-phase3-verification-review.md#L221) | p3-06 | methodology caveat |
| L3 | Dead public interface: `validate_ca_pair` / `install_ca_pair` / `ValidatedCaPair` have no product consumer — no CA-import route exists; used by fah-certs' own tests only | [store.rs:324](../../../crates/fah-certs/src/store.rs#L324), [import.rs](../../../crates/fah-certs/src/import.rs), [lib.rs](../../../crates/fah-certs/src/lib.rs); API.md §Certificates | p3-02 | principle 14 |
| L4 | Stale PFX wording after ADR-0006 descoped it | [ROADMAP.md:78](../../../ROADMAP.md#L78), [ROADMAP.md:307](../../../ROADMAP.md#L307), [plan/wip/phase3/CLAUDE.md:4](../../../plan/wip/phase3/CLAUDE.md#L4) | p3-06 Step 5 | doc debt |

### NIT

| # | Finding | Evidence | Owner |
| --- | --- | --- | --- |
| N1 | `fah-api/src/tls.rs` holds only `probe_local_address`; module name stale since p3-01 | [tls.rs](../../../crates/fah-api/src/tls.rs) | p3-01 |
| N2 | `[https.interception] clients` / `exclude_domains` validated in `main.rs`, not `fah-config::validate`; a bad entry is a binary boot error, not a `ConfigError::Validation` naming the key. No test at any level | [main.rs:810](../../../crates/fastadhunter/src/main.rs#L810) `interception()` | p3-04 |
| N3 | Splice bench ClientHello is ~100 bytes; browser hellos are 1.5–2 KiB with many extensions. Parse cost scales with extension count; small, unmeasured | [proxy.rs](../../../crates/fah-http/benches/proxy.rs) `client_hello` | p3-06 |

## 2. Cross-phase interaction — verified holding

| Seam | Evidence | Result |
| --- | --- | --- |
| p3-01 epoch gate respected by p3-04 and p3-05 | both call `CertStore::prewarm` ([store.rs:376-401](../../../crates/fah-certs/src/store.rs#L376-L401)); `(ca, epoch)` captured under `lock_ca`; `install_ca` swaps CA and bumps epoch under the same lock; `store_minted` rejects a stale epoch | holds |
| Interception fail-closed vs DoT fallback | `MintingResolver { fallback: None }` on the HTTPS listener; API pair on 853 | holds, documented (M1 is the doc gap) |
| `/dns-query` outside auth | route added after `.layer(require_auth)`, gated `state.tls && doh` ([routes.rs:108-125](../../../crates/fah-api/src/routes.rs#L108-L125)); security suite + api tests prove only `/health` and `/dns-query` answer without credentials; peer canonicalised in `Pipeline::handle` | holds |
| Exclusion cannot be bypassed via `Host` | 421 for `Host ≠ SNI` after the block check ([intercept.rs](../../../crates/fah-http/src/intercept.rs) `handle_intercepted`) | holds |
| Verify-before-present | upstream connect + verify precede prewarm and our ServerHello; `InvalidCertificate` → 526, else status 0 | holds |
| `test-harness` root hook stays out of release | `#[cfg(feature)]` at [main.rs:826-829](../../../crates/fastadhunter/src/main.rs#L826-L829); `compile_error!` in fah-api | holds |
| Boot-key gating | `https` and `dns.listen` reject runtime PATCH; CA is the only live thing — matches CONFIGURATION.md | holds |
| `no_sni` closes either way | code, CONFIGURATION.md, SECURITY.md agree | holds |
| CA key isolation | export re-encoded from DER; key never in any response (212-request walk, unit tests); archives 0600, cap 8 | holds |
| Layering / ADR-0006 / hard rule 7 | `layering.rs` green; no Rust comments in Phase 3 files | holds |

## 3. Performance audit

| Claim | Evidence type | Validity | Concern |
| --- | --- | --- | --- |
| No >10 % DNS/HTTP regression | measured A/B/A/B with control arm, dev box | valid | debug-assertions on both arms (X2); D3/D4 ±9–12 % pinned; pinned rerun control moved −5 % |
| Splice steady ≥ 100 MiB/s | loopback 0.93–1.6 GiB/s | diagnostic; TBD P1 | does not convert; `SPLICE_BUF` decision open |
| Per-connection splice 7–9× under direct | measured loopback | expected limitation | connect + parse dominate a 1 MiB transfer |
| Intercepted ≤ 2× spliced handshake | pinned 4-core 1.46–2.23× | straddles; TBD P2 | three-party loopback harness, wide intervals |
| Intercepted h2 ≥ 50 MiB/s | 481–620 MiB/s loopback | diagnostic; TBD P3 | h2 stall RSS unmeasured (p3-04 S1) |
| Leaf hit rate ≥ 90 % | synthetic Zipf 67 % | unsupported for browsing load; TBD soak | correctly labelled synthetic |
| DoT +17 µs / DoH +130 µs vs UDP | loopback, release binary, 3 × 2000, `#[ignore]` | valid dev-box; TBD P4 | not converted (correct) |
| Cold prewarm < 1 ms | 53.5 µs pinned | derived; TBD P5 | L2 |
| CA gen < 100 ms / import < 50 ms | ≈2 / 1.4 ms debug build | derived; TBD P6 | — |
| RAM ≤ 128 MB full mode | 46.1 MiB Windows test build | diagnostic; TBD P7 | steady vs peak not separated; intercepted worst ≈5.5 MiB/session, no per-leg cap (S1) |
| `spawn_blocking` hop 4 µs | measured, warm pool | valid | "≈38 µs on device" labelled inference |
| Pinning rule (F8) | measured probes | sound | PERFORMANCE.md and traps consistent |
| A/B provenance | fingerprints recovered | acceptable | build commands never logged |
| Harness = production path | `TlsServer::bind/serve`, `Interception::new`, `connect_verified_upstream` | holds | bench injects roots; same verify code path |

No root doc makes a Phase 3 claim stronger than its evidence.

## 4. Coverage gaps

| Gap | Level covered today | Class |
| --- | --- | --- |
| Boot rejection of a bad `[https.interception]` entry | none | missing test (N2) |
| Expired CA: terminate leg closes, DoT falls back | store unit only | missing listener-level test |
| TLS 1.2-only client vs interception / DoT | none (only a verifier stub) | missing test |
| DoH over h2 on the wire | none | known open (p3-05) |
| CA regeneration with live DoT clients | resolver-level only | on-device |
| `counters.http` meaning after HTTPS events | none | blocked on M2 |
| L1 lock contention | none | timing-dependent |
| Static-path traversal with a real `/web` root | vacuous on dev box (F18) | on-device curl walk |

## 5. Already-fixed / deferred — confirmed from source

| Item | State |
| --- | --- |
| p3-01 H1 epoch, H2 key-match at load, L1 0600 (`write_private`), L2 interrupted replacement | fixed |
| p3-02 HIGH-1 import serialisation (`lock_api`), LOW-2 `MAX_ARCHIVES = 8` | fixed |
| p3-04 L5 `listeners` block; p3-05 N3 `dot` posture; p3-06 F1(b), F2–F9, F11, F18 | fixed |
| p3-01 L4 expired entries inflate `size` (lazy `take_fresh`) | deferred, still true |
| p3-02 LOW-C no reauth on CA regenerate | deferred, still true, by documented model |
| p3-04 L2 no CA → close; N4 prewarm→evict fail-closed; S1 h2 stall RSS → P3; N8 hop measured, leave | deferred, still true |
| p3-05 N8 mint-rate watch; N1/N11 | deferred |
| p3-06 F10, F16 deferred; F6 withdrawn | as recorded |
| I1 dashboard kinds — [filters.ts:30](../../../dashboard/frontend/src/pages/live-feed/filters.ts#L30) `KINDS = ['dns','http']`; `kind: string` so nothing throws; `https` / `https-sni` rows render through the DNS branch | open, needs a named task |
| X2 bench profile `debug-assertions` (p5-04) | open, follow-up task |
| X3 [project-state.md:7](../project-state.md#L7) dated 2026-09-01, phase 3 absent | open, phase close |
| deploy-rb5009.md has no HTTPS / 853 / `dns-query` section | open, after walkthrough |
| Android Private DNS trusting a user-installed CA (p3-05 review line 153, p3-06 plan line 239) | **unverified**, no evidence either way; highest-risk DoD item; fallback = imported-real-cert route |

## 6. Remaining before RB5009 verification / soak

No code blocker. Owner-side, per p3-06 plan Step 4:

1. Runbook 1 — dst-nat 443 v4 `protocol=tcp` + v6 decision.
2. Runbook 2 — CA install Android + Windows; list the device only after install; ECH check.
3. Runbook 3 — Private DNS hostname mode; settles the user-CA-store question.
4. Runbook 4 — pinned-app check; `BASELINE_EXCLUSIONS` final names (owner input pending).
5. Runbook 5 / 7 — P1–P7 probe measurements, cert-store device checks; P1 decides `SPLICE_BUF`.
6. Before reading the soak — M1 sentence in API.md and soak watch item 3 wording, else `unwarmed_misses` misreads if any DoT client connects before the CA install.
7. Sequencing — a full-mode deploy replaces `fastadhunter-0.3.1`; the p2.6-11 day-7 acceptance runs to 2026-09-08 on the same device. Owner decision, outside Phase 3.

## 7. Safe to leave until after the soak

M2 doc sentence, L1, L3, L4, N1, N2, N3; coverage gaps except DoH-h2 (on-device); I1 named task; X2; project-state rewrite and deploy-rb5009.md HTTPS section at phase close; `FastAdHunter-pre3` worktree removal.

## Fixes applied — M1, 2026-09-02 (owner-approved)

| Change | Where |
| --- | --- |
| `unwarmed_misses` paragraph now states both listeners' behaviour after a miss: HTTPS fails closed, DoT serves the API pair; no CA ⇒ every SNI-bearing DoT hello counts and succeeds; CA installed ⇒ DoT contributes zero, an increase is eviction-between-pre-warm-and-handshake or a mint failure | [API.md:1306-1314](../../../API.md#L1306-L1314) |
| Soak watch item 3 already carried this meaning — no edit | [p3-06 plan:282-290](../../../plan/wip/phase3/p3-06-phase3-verification-plan.md#L282-L290) |

Doc-only; no code, no gate impact. §6 item 6 is closed by this edit.

## Fixes applied — measurement-methodology items 1–4, 2026-09-02 (owner-approved)

The four items are MA-1–MA-4 of
[p3-06-measurement-audit.md](p3-06-measurement-audit.md); recorded here per
instruction, that file is unchanged (its rows still read as open).

| # | Request | What changed | Kind | Where |
| --- | --- | --- | --- | --- |
| 1 | SNI verdict bench | `https_sni_splice` / `_steady_state` proxy is built with `with_rules(BENCH_RULES)` (six rules: three domain, one domain + `$script`, two URL-path — none matches `origin.test`), the default `PolicyState`, and a drained `mpsc` event channel; after each group the bench prints `ProxyCounters`. The resolver stays `FixedResolver` — declared, not fixable here (see below) | benchmark code + labelling | [proxy.rs](../../../crates/fah-http/benches/proxy.rs) `splice_in_front_of`, `splice_arms` |
| 2 | Intercepted relay bench | `tls_server` gets the same wiring; `Rig` carries both listeners' counters and prints them after `https_handshake` and `https_h2_download` | benchmark code + labelling | [intercept.rs](../../../crates/fah-http/benches/intercept.rs) `tls_server`, `Rig::report_verdict_paths` |
| 3 | P2 declaration vs runbook | Declaration-change bullet (the pre-declaration block itself untouched, per its own rule): P2 = LAN client through the **probe FAH instance**, one fixed **public** origin, three arms interleaved, 200 rounds, min / p50 / p99 of `time_appconnect − time_connect`. Runbook 5 gains the full `curl --connect-to` procedure, preconditions, row evaluation, and the counter readings that prove which leg each arm took; the cross-compiled criterion binary is demoted to a "D8 on-device" CPU diagnostic | verification procedure (doc) | [review §Pre-declaration](p3-06-phase3-verification-review.md) declaration changes; §Runbook 5 |
| 4 | Soak reachability gate | Four **gate** rows and one diagnostic row in the Runbook 6 watch table, all on production telemetry: HTTPS reached (`listeners.https.connections` rising in ≥ 20 of 24 windows, `requests ≥ 1`); SNI filtering exercised (unlisted-device `curl` to a blocked domain, `blocked` +1, one `https-sni` block item on a WS tap); interception exercised (`PUT /api/v1/rules/user` URL rule, `leaf_cache.minted_total ≥ 1`, one `https` block item, `requests − connections` growing); DoT exercised (`dot.state == listening`, WS tap `transport: "dot"` from the phone per window); DoH count diagnostic | verification procedure (doc) | [review §Runbook 6](p3-06-phase3-verification-review.md) |

**Pushed back / not done:**

- Item 1 option (a) "exercise the real resolver path" is impossible inside a
  `fah-http` bench: the production resolver is `fah-dns`'s `UpstreamPool`, an
  L3 sibling, and `layering.rs` (line 7) checks `dev-dependencies` too. The
  resolve leg is measured only by P2 through the real binary; the bench
  declares the fixed resolver.
- Item 3: P1 and P3 carry the same declared-LAN vs runbook-loopback split.
  Not resolved — owner decision, outside the four items.
- Item 4: two of the four gates need one deliberate probe at soak start (an
  unlisted device to a blocked SNI; the listed device to a user-rule URL)
  because household traffic cannot guarantee a block inside a window; the DoT
  gate presupposes Runbook 3. Per-item `transport` / `kind` exist only on the
  WS feed — no `/history/*` endpoint carries them — so the tap is a
  `websocat` subscription, named in the row.
- The P2 origin moved from "LAN TLS origin" to a public one: the intercepted
  arm verifies against webpki roots and the probe build has no root hook, so a
  private-CA LAN origin can never pass it.

**Checks re-run (affected targets only):**

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p fah-http --all-targets -- -D warnings` | clean |
| `cargo bench -p fah-http --bench proxy -- https_sni_splice` — smoke: 1 s warm-up, 2 s measurement, unpinned, box not idle | runs; `https_sni_splice` connections 1141 = requests 1141, blocked 0, dropped_events 0; steady-state 45 = 45 |
| `cargo bench -p fah-http --bench intercept` — same smoke settings | runs; spliced 4336 = 4336; intercepted connections 5611, requests 11 222 (one SNI verdict + one judged in-TLS request each); h2 intercepted 1 connection / 274 requests; `minted_total 1`, `unwarmed_misses 0`, blocked 0 |

`requests == connections` on the splice arms and `requests == 2 × connections`
on the intercepted arm are the proof the SNI verdict and the URL tier ran.
Smoke figures are **not measurements** (criterion's "change" lines compare
against a stale stored baseline) and supersede nothing in the p3-06 tables.

**No production-code optimization:** `git diff --stat -- 'crates/*/src'` is
empty; the diff is two bench targets and two `.md` files. Not committed; phase
and status rows untouched.

## Files changed

`API.md` (one paragraph, M1); `crates/fah-http/benches/proxy.rs`,
`crates/fah-http/benches/intercept.rs` (bench wiring + counter print);
`docs/code-review/phase3/p3-06-phase3-verification-review.md` (declaration
change, Runbook 5 P2, Runbook 6 gates); this file.

## Remaining TODOs

Owner decisions: M2 wording; §6 item 7 sequencing; P1/P3 definition (same
split as P2); whether `p3-06-measurement-audit.md` MA-1–MA-4 rows get a
"resolved" pointer.
