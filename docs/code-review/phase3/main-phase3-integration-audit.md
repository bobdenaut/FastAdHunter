# Audit — main `857865d` into `phase3-06`, and Phase 3 after the merge

Read-only architectural audit of `phase3-06` at `fa9451a`, 2026-09-07/08.
Nothing in the tree was changed; the gates were run as evidence. Companion
documents: the allocation-domain design and findings 1–23 in
[alloc-domains-http-review.md](../phase2.6/alloc-domains-http-review.md), the
earlier Phase 3 audit [phase3-audit.md](phase3-audit.md), the task review
[p3-06-phase3-verification-review.md](p3-06-phase3-verification-review.md).

## Summary

- Two questions: (1) did every relevant `main` change survive the merge
  without a behavioural regression — allocation domains, N=2 default,
  alloc finding 21, the IP-literal fix, adaptive-only (p2.6-12), telemetry,
  lifecycle, config; (2) is Phase 3 still correctly built on the merged tree —
  HTTPS listener → allocation-domain hand-off, TLS thread ownership,
  permits/gauges, error paths, shutdown, `http_runtimes = 0`, shared state,
  cross-runtime spawning.
- Git: `phase3-06..main` is empty; merge `e0c6071` (parents `4eddc39`,
  `857865d`); 20 files three-way merged, 6 main-only. Every main-only file is
  byte-identical to `main` after the merge except
  `crates/fah-http/src/domain.rs` — deliberate (`Handoff` enum, `Accepted`
  helpers).
- Gates on this tree (Windows dev box): `cargo fmt --check` clean, clippy
  `-D warnings` clean, `cargo test --all-features --workspace` 0 failed in
  every suite (§Measurements).
- **Main integration: PASS** — one low concern (F1). **Phase 3 design: PASS**
  — minor concerns (F2–F5). **No blocker in code** before further Phase 3
  work; the process items are in §Remaining TODOs.

## Decisions

- Verdict per part is PASS / CONCERNS / FAIL as asked; the closing status uses
  plan/CLAUDE.md's vocabulary.
- "Proven" = read in this tree, seen in a `git diff`, or a test that ran here.
  "Assumed" = taken from a review or a library's documented behaviour, not
  re-derived — listed in §Assumptions.
- No fix applied, none proposed for approval here; F1–F6 wait for an owner
  decision or the next approved edit of the file they name.
- Not audited: `fah-certs` internals, `dot.rs` beyond the accept path
  (l. 49–155), the API wire shape of `listeners`, `p3-06-testing-results.md`.

## Method

| Step | Evidence gathered |
| --- | --- |
| merge shape | `git merge-base` (old base `64be513`); name-only diffs old base → each side; `git diff 857865d e0c6071` per main-only file; `git diff 857865d HEAD` for the 20 three-way files and the Phase 3 wiring |
| code read | `fah-http/src/{server,domain,tls_server,https,intercept,tls,connections}.rs`; `main.rs` runtime, wiring, `interception()`, `build_tls_proxy`, `shutdown`, perf sampler; `adapters.rs` resolver + telemetry; `dot.rs:49-155`; `config_store.rs` `BOOT_KEYS` + classification test; `schema/{runtime,https}.rs`; `encrypted.rs:81,665`; tests `interception.rs:494-565,2083-2154`, `tls_server.rs` tests |
| spawn inventory | grep `tokio::spawn`, `spawn_blocking`, `Handle::`, `JoinSet`, `TokioExecutor` over `fah-http/src` |
| gates | fmt, clippy, `cargo test --all-features --workspace`, sequential, filtered output |

## Part 1 — main integration

| Area | Intended design (source) | Actual code | Match | Evidence | Risk |
| --- | --- | --- | --- | --- | --- |
| allocation domains | alloc review §Decisions: one acceptor on the base runtime, N `current_thread` runtimes on their own threads, permit before accept, runtime built inside the thread (F13), `watch` stop, dead-domain removal (F1), cap 64 (F2) | `server.rs:93-133` `serve_domains`, `domain.rs:105-165`; the merge replaced `Dispatch::Domains { senders, next }` with `Rotation` (same index arithmetic, same log lines) and the `Handoff` alias with an enum | yes | `server.rs` domain tests green; `git diff 857865d e0c6071 -- domain.rs` | low. Shift: `Server` keeps a `Rotation` copy for `TlsServer`, so aborting the HTTP acceptor no longer closes the domain inboxes; stop relies on the `watch` alone (`shutdown()` flips it, dropping `Server` drops the sender) — both present |
| N=2 default | host-derived `max(1, cores / 2)`; RB5009 → 2; production pins the env to 2 | `schema/runtime.rs` identical to `main`; `MAX_HTTP_RUNTIMES`, the env arm and their tests untouched (`fah-config/src/lib.rs` diff main → HEAD is additions only) | yes | diff | none |
| alloc finding 21 | DoT/DoH exchange spawned on the runtime that built the pool | `encrypted.rs:81` `Handle::try_current()`; the pool is built inside the base `block_on`; both `UpstreamResolver`s clone that pool | yes | test `the_exchange_lives_on_the_runtime_that_built_the_conn_not_the_caller` green; `encrypted.rs` diff main → HEAD is test literals only | assumption A1 |
| IP-literal fix `0a716ec` / `6d591ad` | an allowed IP-literal `Host` is the destination, never resolved | `Proxy::approved_address` hunk intact (`proxy.rs` diff main → HEAD touches only the judge/emit extraction and the counters) | HTTP yes; HTTPS no → **F1** | `tls_server.rs` test `an_ip_literal_sni_is_refused_before_resolution_unless_allowed` pins `allowed → resolve_failures == 1` | low |
| adaptive-only (p2.6-12, `fa9451a`) | one strategy; `"fallback"` rejected at load from TOML, env and API | `UpstreamStrategy { Adaptive }`; `TryFrom<String>` → `REMOVED_FALLBACK`; no `Fallback` remnant in `crates/` (grep); fixtures say `adaptive` | yes | `fah-config` 76/76; `p2.6-12-default-flip-review.md` | none here; that review's open items stand (CONTEXT.md:241, redundant `fah-env` opt-in) |
| telemetry | per-listener counters, refused sum over both listeners, `concurrent_connections.https` | `ProxyCounters` + `connections`, `non_tls`, `hello_timeouts`, `upstream_cert_failures`; `From<ProxyStats> for ListenerCounters`; `TelemetryAdapter::listeners()`; the poll sums both listeners; the sampler reads both gauges (alloc F20's `None` slot now filled); `process.rs`, `memory.rs` identical to `main` | yes | `api` 117/117; code read | assumption A3 |
| lifecycle | alloc 11a: HTTP drained before DNS is aborted | `Engine::shutdown`: https abort → http abort + `watch` + join (≤ 5 s drain + 1 s runtime) → dns → api → tasks (`main.rs:707-720`) | yes | code read | 11b still open, and now also reaches idle spliced sessions (F4) |
| config | `[runtime]` and `[https]` boot-class; every key with a compiled-in default | `BOOT_KEYS` holds `runtime` and `https`; `[https]` defaults `::` / 8444, 1024, 10 s, 60 s, `no_sni = pass`, empty `clients`; https/dot port-collision and zero-timeout validation; `[dns.listen]` DoT/DoH on, 853, env arms | yes | `fah-config` 76/76 | classification test lists `runtime.http_runtimes` but no `https.*` key (F5) |

## Part 2 — Phase 3 design after the merge

| Area | Intended design (source) | Actual code | Match | Evidence | Risk |
| --- | --- | --- | --- | --- | --- |
| HTTPS listener → domain hand-off | CONTEXT.md §Allocation Domain: both listeners feed the same domains; the acceptor never does TLS; `0` serves both on the shared runtime | shared `accept_loop` (permit → accept → `TCP_NODELAY` → gauge → `detach` → channel); `TlsServer::serve_domains(proxy, &http)` clones `http.rotation()` with `Lane::Https`; `serve_one` registers the socket on the domain reactor and spawns `tls.serve_connection` into the domain `JoinSet`; `main.rs:634-644` picks domains only when N > 0 and HTTP is up, after `http.serve_domains`; an empty rotation is a boot error | yes | `an_intercepted_session_is_served_on_an_allocation_domain` (every verdict on thread `fah-http-0`); `a_failed_handshake_on_a_domain_returns_the_permit_and_the_gauge`; `e2e_https` boots with no `[runtime]` key, so the compiled-in default ran the domain lane end to end (A4) | F2, F3 |
| TLS handshake / session thread ownership | hello peek, SNI verdict, resolve, splice or MITM handshake and the hyper session on the domain thread | the whole of `TlsProxy::serve_connection` runs in the domain task; `spawn_blocking(prewarm)` uses the domain runtime's own blocking pool; the upstream driver (`tokio::spawn`) and hyper `auto` + `TokioExecutor` land on the domain; `TlsProxy` is one shared `Arc` with no runtime-bound state (per-connection upstream `Sender`, no pool) | yes | thread-name test; spawn inventory: no `Handle::current` in `fah-http/src`; the only base-runtime spawns are the acceptors and the N=0 `Shared*` arms | assumption A2 |
| permits / gauges | per-listener semaphore and gauge travel in `Accepted` and drop with the task | `Accepted::serve` drops both after the served future, also on abort; every early return (`detach` / `register` failure, empty rotation) drops on the spot; HTTPS has its own semaphore and gauge, peak read by the sampler | yes | `max_connections_actually_blocks_the_second_connection` (tls_server); the interception permit tests | F3 |
| error paths | p3-03 plan §Step 3, p3-04 plan §Decision 4 | hello EOF/deadline → `hello_timeouts`; garbage → `non_tls`; no-SNI or > 16 KiB → classified by `no_sni`, event; block before resolve; resolve/policy/connect failures → counters + `status 0` event; upstream cert failure → 526 and close before our handshake (`certificate_error` = `InvalidCertificate` only); `same_host` mismatch → 421; reconnect once only when the request was never sent; boot degradations logged and `DotListener::Closed` surfaced; a bad `clients` / `exclude_domains` entry fails boot | yes | `security_phase3` 7/7, `interception` 33/33, `sni` 9/9 | none |
| shutdown | acceptor abort; domain-lane sessions drained by `Server::shutdown`; N=0 sessions end with the runtime (p3-04 L4) | as designed | yes | `shutdown_stops_accepting_while_a_live_intercepted_session_keeps_serving` | cosmetic: two acceptors can each log "not accepting" while stopping (alloc F8, twice) |
| `http_runtimes = 0` | both listeners on the shared runtime | `Dispatch::SharedTls` → `tokio::spawn` on the base runtime; the rotation stays empty and is never consulted | yes | tls_server tests; the `interception.rs` / `sni.rs` harness default is `domains: 0` | none |
| shared state / `Arc`s | read-only `Arc`s shared, `Proxy` per domain | `Proxy` + hyper-util pool per domain; shared `TlsProxy`, `UpstreamPool`, `dyn Ruleset`, `PolicyState`, one `ProxyCounters` per listener, `CertStore`, events `Sender`; per connection across runtimes: one `Arc<TlsProxy>` inc/dec, permit and gauge atomics, one `Box<RequestEvent>` (alloc F22) | yes | code read | none |
| cross-runtime spawning | none | none found by the inventory; DoT runs on the base runtime via `dns.serve`, DoH inside the API server | yes | grep | assumption A2 |
| DoT / DoH | p3-05 plan decisions 1–5, 8 | 853 bound before the privilege drop with its own bind error; `dot::run` on the base runtime under fatal supervision; 64 permits before accept; SNI peek, prewarm and handshake inside the spawned task under 10 s; prewarm at every handshake (replaces the plan's re-warm ticker); `/dns-query` mounted only when `api.tls && doh` | yes | `e2e_https` legs 6–7; `security_phase3` closed posture | none |

## Findings

Severity-ranked. No fix applied.

| # | Severity | Site | Finding | Impact if unchanged | Disposition |
| --- | --- | --- | --- | --- | --- |
| F1 | Low | `crates/fah-http/src/https.rs:164-168` + `approved_address`; CONFIGURATION.md:375 | `main`'s IP-literal fix (`0a716ec`) covers `Proxy` only. With `allow_ip_literal_hosts = true` an IP-literal SNI passes the refusal and is handed to `resolver.resolve()`, which fails every time; the tls_server test pins that outcome. CONFIGURATION.md says the same switch governs the HTTPS path. Not a regression from `main` (which had no HTTPS path); an inconsistency the merge did not reconcile. | the allowed case is a 100 % close on HTTPS while HTTP works. Unreachable in production: flag off, RFC 6066 forbids IP-literal SNI, `same_host` blocks it inside TLS | owner decision: port the fix (literal → `policy.check` → connect) or state HTTP-only in CONFIGURATION.md and the test |
| F2 | Low | `crates/fah-http/tests/sni.rs`; `interception.rs` harness (`domains: 0` default) | Splice on a domain has no crate-level test; only `e2e_https` covers it, through the compiled-in N, and only on a box with ≥ 2 cores. The terminate leg on a domain has two tests. | a splice-on-domain regression shows only in e2e | deferred; one `domains: 1` splice test in `sni.rs` when that harness is next touched |
| F3 | Info | alloc review pass 2 §Scope checklist 9; CONFIGURATION.md `[runtime]` | The per-domain `JoinSet` bound is now `http.max_connections + https.max_connections`, and `HANDOFF_QUEUE` (32 per domain) is shared by both lanes, so alloc F9's head-of-line stall blocks both acceptors. Still bounded by config. | the recorded bound reads as one listener | one doc line on the next approved `.md` pass |
| F4 | Info | `Engine::shutdown`; alloc 11b | The 5 s drain waits for connections to end; an idle spliced session (`idle_timeout` 60 s) or an idle intercepted keep-alive holds it to the full 5 s, as keep-alive HTTP already does. DNS answers throughout (11a). | up to 5 s longer stop, inside `stop-time=10s` | open with 11b; owner decision before the full-mode soak |
| F5 | Info | `crates/fah-api/src/config_store.rs` `boot_key_classification_matches_what_actually_applies_the_key` | `https` is in `BOOT_KEYS`, but no `https.*` key is in the test's boot list. | contract right, coverage gap | one line in the test with the next `fah-api` change |
| F6 | Info | `p3-06-phase3-verification-review.md` §Hand-off state 2026-09-05 | "Runbook 6 cannot start before the 0.3.1 soak ends 2026-09-08" is stale: [project-state.md](../../project-state.md) (2026-09-07) records the 0.3.1 soak stopped on day 6 and the 0.3.3 soak running to 2026-09-14. | a reader schedules the Phase 3 deploy a week early | rewrite on the next approved edit of that file |
| F7 | Low | `crates/fah-dns/tests/adaptive_behaviour.rs:1247-1257` (the assert and its silent skip); `:1010-1021` `RecoveryScript::build` with the bucketing at `:1093-1096` (the sample asymmetry); `:1280-1292` (the three-arm loop) | `b5_recovery_and_flapping`'s p99 guard is three construction defects that compound, not one. **(a)** The `2.5` arm can never assert. The flapping window is six phases of `unit(0.1)` against black_hole's single `unit(2.0)`, so flapping collects ~1/3.33 of black_hole's samples by construction; with `CADENCE_MS = 50` its ceiling is 6 × 375 ms / 50 ms = **45 samples against a `>= 100` threshold**. Measured 35-41 across 7 runs. A faster box does not fix it: sampling is wall-clock paced, and `MissedTickBehavior::Delay` only removes ticks (11-20 % of nominal on every arm, both sides), never adds them. **(b)** The `12.5` and `37.5` arms do assert, but `p99_flapping <= 1.1 * p99_black_hole` breaks when the *reference* phase gets faster, not when flapping gets slower — `branch-0` failed on a black_hole of 1.53 ms, the lowest denominator in the whole set, with a numerator (2.27 ms) that passes everywhere else. The ratio is not stable on unchanged code: `branch-0` measured 1.48 and `branch-1` 0.91, same arm, same commit, same box. **(c)** A panic in one arm skips the ones after it — `branch-0` panicked at `12.5` and never ran `37.5`. Composed: in a bad run the first arm skips silently, the second raises a false alarm, and that alarm deletes the third, so the test verifies nothing while reporting a failure. | a gate run with `--include-ignored` fails on a test that cannot discriminate, and the safety net the `2.5` arm is assumed to provide does not exist on any machine. **Open, not closed by this audit:** on the `37.5` arm 2 of 3 `phase3-06` runs exceeded the threshold from the *numerator* side (p99_flapping 2.35 / 2.30 ms against denominators identical to main's) where 0 of 3 `main` runs did. The distributions overlap — the lowest p99_flapping in the set, 1.83 ms, is `phase3-06`'s — and the gap is ~0.3 ms on a ~2 ms p99, at the resolution of the instrument. Not a production signal: mock upstreams on a dev box, and PERFORMANCE.md's budgets are RB5009 figures | no fix proposed here; a merge is not the place to change a test. If the `37.5` question is taken up, the method is more percentiles, not more runs — that arm collects ~575 flapping samples per run, so a genuinely slower flapping phase moves p50 and p90 too, while a tail artefact moves only p99. The test prints p99 per phase and nothing else (`:1169-1177`), so answering it is itself a test change. Owner decides when `adaptive_behaviour.rs` is next touched |

## Assumptions

| # | Assumed | Basis | Verify by |
| --- | --- | --- | --- |
| A1 | `UpstreamPool::clone` shares the `ExchangeConn` state, so a resolve from a domain uses the base-pinned exchange | `adapters.rs:43-45` comment "cheap `Arc` clone"; alloc review finding 21 | read `UpstreamPool`'s `Clone` |
| A2 | hyper-util `TokioExecutor`, the hyper-util client pool and hickory's `TokioRuntimeProvider` spawn on the current handle | alloc review pass 3 §Stage trace | not re-derived |
| A3 | `/api/v1/telemetry` serializes `ListenerTelemetry` as `listeners.http` / `listeners.https` | API.md; `api` tests green | read `fah-api/src/telemetry.rs` |
| A4 | `e2e_https` exercised the domain lane | `tests/common/mod.rs` writes no `[runtime]` key; `available_parallelism` = 32 on this box; the test asserts nothing about threads | a thread-name assertion, or a fixture pinning `http_runtimes` |

## Measurements

Gates on this tree, 2026-09-08, Windows dev box:

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | 0 failed in every suite |
| `fastadhunter` `e2e_https` / `security_phase3` / `e2e` / `http_e2e` / `layering` | 2/2, 7/7, 2/2, 2/2, 1/1 |
| `fah-http` unit / `interception` / `sni` / `proxy` / `filtering` | 90/90, 33/33, 9/9, 12/12, 12/12 |
| `fah-dns` unit / `server_integration` | 209 ok + 2 ignored, 11/11 |
| `fah-api` unit / `api` | 126/126, 117/117 |
| `fah-config` / `fah-certs` unit | 76/76, 85/85 |

`b5_recovery_and_flapping` (F7), 2026-09-13, Windows dev box. Eight runs,
alternating sides so no machine drift lands on one of them: four on
`phase3-06` (`eb693e2`), four on `main` in the `E:/fah-main-bench` worktree
(`ebc46f1`). Each run is `cargo test -p fah-dns --test adaptive_behaviour
b5_recovery_and_flapping -- --include-ignored --test-threads=1 --nocapture`,
serial, never two at once. Logs in `E:/fah-b5-runs/`.

| Run | `2.5` flapping_n / black_hole_n | `12.5` p99 bh / fl / ratio | `37.5` p99 bh / fl / ratio |
| --- | --- | --- | --- |
| main-0 | not captured (run without `--nocapture`) | passed, numbers not printed | passed, numbers not printed |
| main-1 | 37 / 133, undecidable | 2.87 / 1.97 / 0.69 | 2.22 / 2.00 / 0.90 |
| main-2 | 41 / 132, undecidable | 2.18 / 2.28 / 1.05 | 2.06 / 2.00 / 0.97 |
| main-3 | 35 / 120, undecidable | 5.22 / 2.06 / 0.39 | 2.05 / 1.85 / 0.90 |
| branch-0 | 39 / 135, undecidable | 1.53 / 2.27 / **1.48 broke** | not reached — the panic ended the test |
| branch-1 | 37 / 120, undecidable | 2.53 / 2.30 / 0.91 | 2.13 / 1.83 / 0.86 |
| branch-2 | 40 / 133, undecidable | 4.33 / 2.10 / 0.48 | 2.10 / 2.35 / **1.119 broke** |
| branch-3 | 36 / 120, undecidable | 2.88 / 2.08 / 0.72 | 2.08 / 2.30 / **1.106 broke** |

p99 in ms. "undecidable" is the test's own word: below the `>= 100` sample
threshold it prints the counts and skips the assert, so a passing `2.5` arm
means the assert never ran. The decided branch prints nothing on success,
which is why the absence of that line is how a run is classified.

Nominal sample counts, from the windows divided by `CADENCE_MS = 50`:

| Arm | `penalty_max_ms` | flapping window | flapping nominal | black_hole nominal | measured flapping |
| --- | --- | --- | --- | --- | --- |
| 2.5 | 3 750 | 6 × 375 ms | 45 | 150 | 35-41 |
| 12.5 | 18 750 | 11 250 ms | 225 | 750 | ≥ 100 (assert ran) |
| 37.5 | 56 250 | 33 750 ms | 675 | 2 250 | ≥ 100 (assert ran) |

Totals land at 80-89 % of nominal on every arm and both sides (`2.5`: 312-346
queries against 390; `12.5`: 1 559-1 703 against 1 950; `37.5`: 4 670-5 123
against 5 850), so the shortfall is a uniform tick slip, not a phase effect.
How it splits between the two phases has no stable direction across the four
runs that print counts, and no per-phase cause is claimed here. The `2.5` arm's
ceiling does not depend on it: 45 is below 100 before any slip.

## Files changed

None in `crates/` or docs; this file only.

## Remaining TODOs

- F1 owner decision; F3 and F6 doc lines on their next approved edit; F2 and
  F5 with the next touch of those tests; F4 rides alloc 11b.
- Before further Phase 3 work, none of it code: the 0.3.3 soak verdict
  (2026-09-14, ADR-0006 plateau — project-state §Next); the ADR-0006 revisit
  trigger (remeasure N with TLS on the RB5009 before deploying `phase3-06`);
  the `AWAITING SOAK` flip condition in
  [p3-06-phase3-verification-review.md](p3-06-phase3-verification-review.md)
  §Hand-off state 2026-09-05 (24 h full-mode soak, Runbook 1–4 and 7,
  `BASELINE_EXCLUSIONS` final list, a second wired LAN endpoint for P1/P2/P3,
  the P3 on-device rerun).
- Unchanged by this audit, still open elsewhere: dashboard settings metadata
  for `runtime.http_runtimes` (alloc F3); CONTEXT.md:241 (p2.6-12 F2).

**PASS WITH DEFERRED FINDINGS** — main integration PASS (F1 low), Phase 3
design PASS (F2–F6 low/info); no blocker in code.
