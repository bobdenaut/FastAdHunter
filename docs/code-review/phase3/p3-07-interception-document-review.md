# p3-07 — Interception Document — Review

**Task:** [p3-07-interception-document.md](../../../plan/wip/phase3/p3-07-interception-document.md) ·
**Plan:** [p3-07-interception-document-plan.md](../../../plan/wip/phase3/p3-07-interception-document-plan.md) ·
**ADR:** ADR-0008 frozen at `fcc7244` · **Base:** `phase3-06` at `e5bc251` ·
**Gates:** GREEN · **Review:** done 2026-09-10 — **PASS** after two approved fix batches (§Findings)

## Implementation Summary

`clients` and `exclude_domains` left the TOML and the binary. They live in
`/config/interception.json`, are read and replaced through
`GET`/`PUT /api/v1/interception`, and a `PUT` applies on the next accepted
connection — no restart, no `restart_required`. `BASELINE_EXCLUSIONS` is
deleted. Release N of ADR-0008 §Migration ships here; release N+1 (deleting the
two `Option` fields) does not.

### What was implemented

| Group | Change |
| --- | --- |
| A1 | `fah-model::InterceptionDocument` — `deny_unknown_fields`, `default`; the file and wire shape, no logic (hard rule 2) |
| A2 | `AllowedNet` gains `Hash` — client dedupe is a `HashSet` insert, never O(n²) |
| A3 | `fah-rules::interception` (new module): `normalize_host` + `MAX_NAME_LEN`/`MAX_LABEL_LEN` moved verbatim from `fah-http/src/sni.rs`; `ExclusionSet`/`InvalidExclusion` moved from `fah-http/src/exclusions.rs` minus the baseline; `InterceptionScope`, `Active`, `Active::compile`, `DocumentError`, `InterceptionState`, `MAX_CLIENTS = 256`, `MAX_EXCLUDE_DOMAINS = 512` |
| A4 | `InterceptionConfig` fields become `Option<Vec<String>>`; the whole `interception` field is skipped when both are `None`, so a saved TOML carries neither the keys nor an empty table |
| A5 | `fah_config::write_atomic` is `pub`; its two error-context closures are built before the write, so a successful `fs::rename` is followed by literally `Ok(())` |
| B | `fah-http`: `exclusions.rs` deleted; `Interception { server_config, client_config, store, state }`; `with_interception` no longer drops an empty one; `interception_for` takes one `ArcSwap` guard |
| C1 | `ErrorDetail` gains `details: Option<Value>` (`skip_serializing_if`); new `ApiError::ValidationFailedWithDetails` → 422 |
| C2 | `fah-api::interception_store` (new): `DOCUMENT_FILE`, `InterceptionRuntime`, `load_or_migrate`, `InterceptionStore` (`prepare` / `commit`), `InterceptionStoreError` |
| C4 | `GET`/`PUT /api/v1/interception`; `post_config` rejects `https.interception` naming the endpoint |
| D | Binary: `load_or_migrate` on the blocking pool after the privilege drop; `interception()` no longer parses lists or returns `None` for an empty one; runtime status from `https.is_some()` × `certs.is_some()` |

### Design decisions worth recording

- **Types live in `fah-rules::interception`, not behind a port.** `fah-api`
  validates, builds, persists and publishes; the binary wires one `Arc`;
  `fah-api` never names `fah-http`. Mirror of `PolicyState`.
- **One published value.** `Arc<Active { document, scope }>` through one
  `ArcSwap`. `GET` reads `state.current().document`, the hot path reads
  `.scope` — same `Arc`, so the API and the listener cannot disagree, and the
  document strings are stored once rather than twice.
- **`prepare` / `commit` split.** Everything fallible or allocating happens
  async-side before the lock; `commit` is `lock → write_atomic → ArcSwap::store
  → unlock`, run through `spawn_blocking` so a cancelled `PUT` can never leave
  the file ahead of the runtime. Nothing sits between `rename` and `store`.
- **Poisoning is recovered, not fatal.** `clear_poison()` + `error!`; a panic in
  `commit` answers 500 and the next `PUT` proceeds. State is A/A or B/B by
  construction (plan §3.7 proof).
- **Migration saves the file layer, never the effective config.** `load_or_migrate`
  re-reads `fastadhunter.toml` through `Config::from_toml_str` and saves *that*,
  so an `FAH__` override in force at that boot is not baked into the file (F1).
  The document is written on exactly one branch (`NotFound`), so an existing
  file is never overwritten.

### Deviations from the approved plan

All four **approved by the owner** on 2026-09-10, after implementation.

| # | Plan said | Shipped | Why |
| --- | --- | --- | --- |
| 1 | C2: `#[cfg(test)] panic_after_lock`, route test reaches it because "`routes.rs` tests build `AppState` themselves" | `#[cfg(any(test, feature = "test-harness"))]` | The premise was false: `routes.rs`'s test module holds pure functions only and builds no `AppState`; the sole route-test surface is `crates/fah-api/tests/api.rs`, an integration crate linking the library built **without** `cfg(test)`. Owner approved Option A and directed the Decision 11 rewording, which is applied in the plan. `fah-api` already enables `test-harness` for its own integration tests; `lib.rs` already `compile_error!`s on that feature in a release profile |
| 2 | C4 route line carries no body limit | `/interception` wrapped in `bounded_body` (256 KiB, the existing `certs::MAX_BODY_BYTES`) | `body_error`'s over-size arm names that constant. Without the layer the real limit is axum's 2 MiB default and the message would be false. Also bounds the body (hard rule 4); the cap is ~2× the largest legal document (§12: ≤ ~130 KiB) |
| 3 | — | `requests/interception.http` added | `crates/fah-api/tests/request_coverage.rs` fails any router route with no request file. Not optional |
| 4 | §14.1 listed `bad-.example` among rejected hosts | replaced with `bad.example-` | `normalize_host` was moved **verbatim**; it rejects `-` only at label start or at name end, so `bad-.example` is accepted. The plan's test expectation was wrong, not the function. No behaviour changed |

`ExclusionSet::new` was carried over as the plan specifies (A3) but now has no
production caller — `Active::compile` builds the set directly because it must
report per-entry indices. It is exercised by the moved
`a_malformed_entry_is_rejected_by_name` test. Flagged for the review, not
changed. **Deleted after the review (F-04, §Fixes applied).**

### Tests

| Suite | Count | Notes |
| --- | --- | --- |
| `fah-model` unit | 4 | document round-trip, unknown key, missing key |
| `fah-rules::interception` unit | 15 | moved exclusion tests, `normalize_host`, all four `Active::compile` rejections, `details` serialization, guard/store semantics, `lookup_cost_is_independent_of_list_size` (10 000 misses on a 512-entry set) |
| `fah-config` unit | +6 | env rejection for both keys, absent-by-default TOML, `from_toml_str` file-layer contract, `write_atomic` error paths, presence round-trip, `take` |
| `fah-api::interception_store` unit | 21 | 11 migration cases (incl. F1 file-layer, unreadable document, second boot, over-cap, invalid TOML), commit/publish, two-thread serialization, poison recovery, `panic_after_lock` |
| `fah-api` routes (`tests/api.rs`) | +13 | `GET`, whole-document `PUT`, no `restart_required`, all four 422 `details` shapes, 400 envelope, 503 store-closed, 500 write failure, **500 commit panic through the real handler**, `post_config` rejection, `get_config` omission |
| `fah-http/tests/interception.rs` | 36 | harness on `InterceptionState`; 3 new: empty-document bootstrap → store → intercepted, removed client keeps its session, stored exclusion splices the next connection |
| `fastadhunter` e2e | +1 | `listing_a_client_through_the_api_applies_on_the_next_connection` — real binary, empty document, splice, `PUT`, next connection intercepted with a minted leaf, no restart |

Deleted, not adapted: `the_baseline_exclusions_ship_without_any_configuration`,
`the_baseline_is_present_with_an_empty_user_list`,
`every_baseline_entry_is_a_valid_hostname`,
`an_empty_client_list_intercepts_nobody`.
`a_baseline_bank_is_never_intercepted_even_for_a_listed_client` became
`a_parent_entry_in_the_document_splices_its_subdomain_for_a_listed_client`.

## Measurements

### Gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --all-features --workspace` | 53 binaries, 0 failures |
| `crates/fastadhunter/tests/layering.rs` | green — `serde` is external, `fah-rules` gains no internal edge (F2) |

### Bench — `fah-http/benches/intercept.rs`

Migrated to the new `Interception::new` signature. Workload unchanged in
intent: one listed client `127.0.0.1`, zero exclusions — identical to the
previous `AllowedNet::host(LOCALHOST)` + `ExclusionSet::empty()`. No new
benchmark added: the only hot-path delta is one `ArcSwap::load`, which the
existing `intercepted` leg already covers. `cargo bench -- --test` passes every
case; the leaf-cache and verdict-path counters are unchanged
(`minted_total=1`, spliced 1 request/connection, intercepted 2).

**No performance verdict is drawn, and none is owed (plan §11, §14.7).**
Criterion's `change:` column was checked and rejected as evidence for the
`https_handshake` group, per CLAUDE.md §Before prescribing or measuring rule 2.

**`https_handshake` is not a trustworthy benchmark on this host.** Five runs of
**identical code**, each compared by Criterion to the run before it. Runs A–C
had a fullscreen browser video playing; Q1 and Q2 were on a quiet box:

| leg | A | B | C | Q1 | Q2 | point estimate (ms) |
| --- | --- | --- | --- | --- | --- | --- |
| `direct_to_origin` | +113.6% | −50.5% | +123.7% | −52.8% | +3.9% (p = 0.41) | 2.05 → 1.01 → 3.06 → 0.99 → 1.15 |
| `spliced` | −68.4% | −20.8% | +8.2% (n.s.) | +113.0% | **+70.6%** (p = 0.00) | 2.62 → 2.14 → 2.35 → 5.74 → 7.10 |
| `intercepted` | −53.4% | −22.7% | +86.4% | +57.8% | **−28.4%** (p = 0.04) | 3.27 → 2.20 → 5.26 → 6.72 → 5.25 |

Q2 vs Q1 is the controlled pair — same code, same quiet box, back to back — and
it still reports a *statistically significant* +70.6% "regression" on `spliced`
and a −28.4% "improvement" on `intercepted`. That is the group's noise floor,
not a signal.

Why, structurally:

| group | connections | Q2 vs Q1 | trustworthy |
| --- | --- | --- | --- |
| `prewarm_hop` | none (in-process) | +3.1% / +2.7% / +6.6% | yes |
| `https_h2_download` | one h2 session, 676 requests over it | p = 0.58 / 0.59 / 0.37 — all n.s. | yes |
| `https_handshake` | **one fresh TCP connection per iteration** | +3.9% n.s. / +70.6% / −28.4% | **no** |

Only the connection-per-iteration group is unstable. Its iteration counts fall
across the session at fixed measurement time (`spliced`: 3573 connections in
Q1, 1786 in Q2), which is consistent with Windows loopback ephemeral-port /
TIME_WAIT pressure accumulating over consecutive runs — a host effect that hits
the two-connection proxy legs (client→proxy→origin) about twice as hard as the
one-connection `direct_to_origin`.

Two further reasons the column is not evidence:

- **Criterion records no commit.** `new/` holds `benchmark.json`,
  `estimates.json`, `sample.json`, `tukey.json` — no VCS metadata. `base/` is
  the previous run's `new/`, moved. Run A's "baseline" was whatever an earlier
  `cargo bench` left in `target/criterion/`, which is gitignored and
  unattributable to any checkout.
- **Two of the three legs cannot be affected by p3-07.** `direct_to_origin`
  connects straight to the TLS origin (`rig.origin`), never touching `TlsProxy`.
  `spliced` runs `tls_server(origin, None)`, so `interception_for` returns at
  its first `?` before and after the change. Both nonetheless posted the
  largest swings.
- **Dispersion swamps the delta.** Coefficient of variation on this host:
  `handshake/spliced` 55.8% (mean 2.117 ms, median 1.742 ms, sd 1.182 ms),
  `handshake/intercepted` 40.4%, `handshake/direct` 15.5%; 2–4 outliers per 50
  samples in every leg. The change under test is one `ArcSwap::load` plus a
  field offset per accepted connection — nanoseconds against a multi-millisecond
  handshake, ~4 orders of magnitude below a 1 ms sd.

**What the trustworthy groups say:** `https_h2_download` shows no significant
change on any leg across the quiet pair, `intercepted` included (15.36–16.95 ms
Q1, 15.33–17.09 ms Q2). No p3-07 effect is observable where the instrument can
observe anything.

A real verdict would still need an A/B against a pre-change checkout on a quiet
box, and `https_handshake` would need its TIME_WAIT sensitivity fixed first
(`docs/measurement-traps.md`). Neither is owed by this task.

### Reviewer A/B — base `e5bc251` vs tip, 2026-09-10

Run on the owner's question whether the table above is a regression. Base:
`e5bc251` in a detached worktree. Tip: `db14c95` plus the approved fixes (no
hot-path change). Bench: `fah-http/benches/intercept.rs`, same workload on both
arms (one listed client `127.0.0.1`, no exclusions). Device: dev box
(i9-13980HX, Windows 11); `tasklist` showed no browser or player. Order
base/tip/base/tip in both sessions. `TIME_WAIT` (`netstat -an`) drained below
200 before every arm — session 1: 2 / 71 / 148 / 85; session 2: 4 / 10 / 60 /
126 — each arm leaving 2 200–6 800 behind. Control: `direct_to_origin` (never
enters the proxy). Mid estimates, run order left to right.

**Session 1 — pinned per PERFORMANCE.md (`0x55`, `TOKIO_WORKER_THREADS=4`, High)**

| leg | base | tip | base | tip |
| --- | --- | --- | --- | --- |
| `https_handshake/direct_to_origin` (control) | 1.53 ms | 1.03 ms | 2.07 ms | 1.70 ms |
| `https_handshake/spliced` (untouched by p3-07) | 20.14 ms | 14.44 ms | 10.66 ms | 13.73 ms |
| `https_handshake/intercepted` | 12.18 ms | 10.92 ms | 18.31 ms | 13.08 ms |
| `https_h2_download/direct_to_origin` | 5.53 ms | 5.56 ms | 5.99 ms | 6.26 ms |
| `https_h2_download/spliced` | 8.20 ms | 7.50 ms | 7.97 ms | 8.29 ms |
| `https_h2_download/intercepted` | 12.65 ms | 12.80 ms | 12.80 ms | 12.69 ms |
| `prewarm_hop/inline_cached_leaf` | 91.5 ns | 90.5 ns | 91.4 ns | 90.5 ns |
| `prewarm_hop/inline_prewarm_warm` | 117.0 ns | 116.2 ns | 115.6 ns | 116.3 ns |
| `prewarm_hop/spawn_blocking_prewarm` | 4.68 µs | 3.97 µs | 4.56 µs | 3.44 µs |

**Session 2 — unpinned, the regime the table above came from**

| leg | base | tip | base | tip |
| --- | --- | --- | --- | --- |
| `https_handshake/direct_to_origin` (control) | 1.03 ms | 1.05 ms | 1.00 ms | 1.03 ms |
| `https_handshake/spliced` (untouched) | 1.60 ms | 9.33 ms | 9.06 ms | 8.69 ms |
| `https_handshake/intercepted` | 4.56 ms | 9.01 ms | 8.25 ms | 6.83 ms |
| `https_h2_download/direct_to_origin` | 7.10 ms | 6.17 ms | 6.34 ms | 6.34 ms |
| `https_h2_download/spliced` | 9.33 ms | 8.85 ms | 8.60 ms | 8.60 ms |
| `https_h2_download/intercepted` | 86.4 ms (CI 53–128) | 15.28 ms | 14.98 ms | 16.67 ms |
| `prewarm_hop/inline_cached_leaf` | 90.7 ns | 91.4 ns | 93.0 ns | 92.3 ns |
| `prewarm_hop/inline_prewarm_warm` | 119.0 ns | 116.5 ns | 118.2 ns | 117.6 ns |
| `prewarm_hop/spawn_blocking_prewarm` | 4.61 µs | 3.48 µs | 3.72 µs | 4.75 µs |

| Claim | Evidence |
| --- | --- |
| No regression signal | handshake `intercepted`: pinned pairs −10 % / −29 % (tip below base twice), unpinned +98 % / −17 %. The untouched `spliced` leg moved −28 % / +29 % pinned and **+483 % / −4 %** unpinned in the same pairs; a delta of that size on code p3-07 does not touch invalidates the group for any verdict (measurement-traps §Calibration). Ranges overlap in both sessions, no consistent direction |
| Nothing moved where the instrument can measure | pinned h2 `intercepted` 12.65 / 12.80 base vs 12.80 / 12.69 tip, intervals ±1–3 %; `inline_cached_leaf` and `inline_prewarm_warm` within ±1 % over all eight runs. The pinned −1 % on `inline_cached_leaf` is placement-sized, on `fah-certs` code p3-07 does not touch; a null-edit arm would be needed to attribute it |
| The one delta the other way | unpinned pair 2 h2 `intercepted` +11 % with a ±9 % interval and both controls flat. Contradicted by the four pinned runs (±1 %) and by mechanism: p3-07 adds one `ArcSwap::load` per *connection*; that leg is 676 requests over one connection per iteration |
| The `TIME_WAIT` explanation above is refuted | every arm started under 200 sockets and the proxy legs still ran 5× slower from the second run on (`spliced` 1.60 → 9.33 / 9.06 / 8.69 ms; Criterion's iteration budget 3825 → 1275) while the control held 1.00–1.05 ms. Whatever accumulates across runs, it is not the count `netstat` reports; it hits the two-connection legs and spares the one-connection control. The first run was also the only one where h2 `intercepted` blew up (86 ms), so the group is bimodal per leg per run, not "first run fast" |
| The four-core pin does not fit this bench | pinned, every handshake proxy leg read 10–20 ms against 1.6–9 ms unpinned, ±10 % intervals: client, proxy and origin share one runtime and contend inside the mask. PERFORMANCE.md lists handshake/h2 under the four-core recipe and, four bullets later, says proxy-in-one-runtime benches must not be pinned; for `intercept.rs` the second rule is the one the numbers back |

Raw logs: `%TEMP%\fah-p307-bench\` on the dev box, not committed. Worktree
removed after the runs.

## Files changed

| File | Change |
| --- | --- |
| `crates/fah-model/src/interception.rs` | new — `InterceptionDocument` |
| `crates/fah-model/src/lib.rs` | module + export |
| `crates/fah-common/src/egress.rs` | `AllowedNet` derives `Hash` |
| `crates/fah-rules/src/interception.rs` | new — validator, matcher, compiled scope, published value, holder, error contract |
| `crates/fah-rules/src/lib.rs`, `Cargo.toml` | `pub mod interception`; `serde` dependency, `serde_json` dev-dependency |
| `crates/fah-config/src/schema/https.rs` | `InterceptionConfig` optional + `is_absent`/`take`; tests |
| `crates/fah-config/src/lib.rs` | `write_atomic` public, closures hoisted; 4 new tests |
| `crates/fah-http/src/exclusions.rs` | **deleted** (`BASELINE_EXCLUSIONS` with it) |
| `crates/fah-http/src/sni.rs` | `normalize`/consts removed, imported from `fah-rules` |
| `crates/fah-http/src/intercept.rs` | `Interception` holds `state`, not lists |
| `crates/fah-http/src/https.rs` | one guard per accepted connection; empty machinery kept |
| `crates/fah-http/src/lib.rs` | exports dropped |
| `crates/fah-api/src/interception_store.rs` | new — store, migration, commit |
| `crates/fah-api/src/error.rs` | `details` on the envelope |
| `crates/fah-api/src/routes.rs` | route pair, handlers, `post_config` guard |
| `crates/fah-api/src/state.rs`, `lib.rs`, `certs.rs`, `config_store.rs` | `AppState.interception`; exports; `body_error` shared; stale classification row dropped |
| `crates/fastadhunter/src/main.rs` | migration, runtime status, wiring |
| `requests/interception.http` | new |
| tests | `fah-api/tests/api.rs`, `fah-http/tests/interception.rs`, `fah-http/benches/intercept.rs`, `fastadhunter/tests/{common/mod.rs,e2e_https.rs,history_e2e.rs}` |
| `plan/wip/phase3/p3-07-interception-document-plan.md` | Decision 11 / C2 / F3 reworded for the approved gate change |

## Documentation (plan §15) — written, owner-approved 2026-09-10

| Doc | Change |
| --- | --- |
| CONFIGURATION.md | `[https.interception]` block deleted from §Reference (with the 34-name baseline listing); new **§Interception Document** — shape, caps, precondition, no-baseline warning, error/503 behaviour, `POST /config` refusal, `FAH__` refusal, migration table; §Mutability classes paragraph rewritten; `/config` volume row gains `interception.json`; `max_connections` cross-reference retargeted |
| API.md | §Error format gains the optional `details` object; new **§Interception** with `GET`/`PUT`, status table, caps, closed `reason` set and the four `details` shapes; `GET /config` omission and `POST /config` 422 recorded; the `https` event's `[https.interception]` reference retargeted |
| SECURITY.md | opt-in bullet names the document, the volume, the authenticated-`PUT`-only writer and the layering argument that no traffic path can write it; **"Exclusions always splice"** rewritten — baseline deleted, empty list excludes nothing, with the trade-off stated plainly |
| CONTEXT.md | §Interception pinned to two meanings; new term **§Interception Document**; §Exclusion loses the baseline |
| README.md | ADRs 0001–0007 → 0001–0008 |
| `p3-06-phase3-verification-runbook.md` | `R2` drops the `https` half of its body (now three keys → two); `R8` becomes `PUT /api/v1/interception`, no restart, rollback rows replaced by an undo `PUT`, plus the warning that the boot log line no longer proves a client is listed |
| `p3-06-measurement-audit.md` | Runbook 4 precondition: the excluded entry comes from the document, not the constant, and must be `PUT` before the arm runs |

Not touched, deliberately: `docs/project-state.md` (owner's file, and outside
the approved list), and the p3-04/p3-06 review files plus
`main-phase3-integration-audit.md` — historical records of what was true when
written; §15 named only the two p3-06 rows above.

## Remaining TODOs

- Release N+1: delete `InterceptionConfig` and the `interception` field.
- Follow-ups carried unchanged from plan §16: `ConfigStore::apply_patch`
  saving synchronously on an executor worker and baking effective-config values;
  `post_config`'s plain-text rejection on a non-JSON body; `fsync` in
  `write_atomic`; stale `.tmp.<pid>` cleanup.
- p3-08 consumes the `Interception`/`InterceptionState` contract and the
  accept-arm invariant (`intercept()` never reads `state`); p3-09 consumes the
  endpoint pair and the `details` contract.
- **`https_handshake` needs a measurement fix before it can gate anything**
  (not p3-07 scope): one fresh TCP connection per iteration makes it swing
  ±70–120% between identical-code runs on Windows loopback. `https_h2_download`
  and `prewarm_hop` are unaffected and reproduce. The reviewer's A/B
  (§Measurements) narrowed it: the swing is **not** the `TIME_WAIT` count —
  arms started under 200 sockets and the proxy legs still ran 5× slower from
  the second run on while the control held ±3 %; and the four-core pin makes
  the group worse, not better. Two candidates for `docs/measurement-traps.md`
  and one for PERFORMANCE.md §Measuring reliably (handshake/h2 listed under the
  pin recipe that its own later bullet forbids), if the owner wants them
  recorded.

## Findings

Reviewer: Fable 5.1, 2026-09-10. Scope: `3ae6230` (code) + `db14c95` (docs)
against the plan at `e5bc251` plus its Decision 11 rewording. Every line below
was checked against the tree, not against the summary above. Verbatim moves
were diffed against `e5bc251` (`sni.rs:266-300`, `exclusions.rs`).

### Gates re-run by the reviewer

| Gate | Result |
| --- | --- |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --all-features -p fah-model -p fah-rules -p fah-config -p fah-api -p fah-http` | green |
| `cargo test --all-features -p fastadhunter` (all binary suites) | green — 57 tests; `listing_a_client_through_the_api_applies_on_the_next_connection` ran, did not skip |
| `crates/fastadhunter/tests/layering.rs` | green; `layer()` keys on the `fah-` prefix only, so `serde` on `fah-rules` is invisible to it (F2 holds) |

### Severity-ranked

Blockers: **none**.

**F-01 · should-fix · docs** — [deploy-rb5009.md:825-827](../../deploy-rb5009.md#L825-L827)
- Evidence: "Install the CA before adding a device to `[https.interception] clients`".
- Why: the key no longer exists; that patch now answers 422
  ([routes.rs:1429-1440](../../../crates/fah-api/src/routes.rs#L1429-L1440)) and
  boot strips it. An operator following this page fails. Plan §15 did not list
  the file, so `db14c95` missed it (CLAUDE.md: a doc that contradicts the change
  is updated in the same change).
- Fix: point the sentence at `PUT /api/v1/interception`. `.md` — needs an owner yes.

**F-02 · note · docs** — [requests/README.md:33-46](../../../requests/README.md#L33-L46)
- Evidence: the file table stops at `dns-query.http`; `interception.http` is absent
  while line 3 claims every API.md endpoint is covered.
- Why: `request_coverage.rs` guards route reachability, not this table; the index rots.
- Fix: one row. `.md` — owner yes.

**F-03 · note · undocumented deviation** — [interception.rs:211-213](../../../crates/fah-rules/src/interception.rs#L211-L213)
- Evidence: `entry.trim().parse::<AllowedNet>()`. The boot parser it replaces
  (`main.rs@e5bc251:849-855`) called `entry.parse()` untrimmed, and
  `AllowedNet::from_str` ([egress.rs:106-127](../../../crates/fah-common/src/egress.rs#L106-L127))
  does not trim. Plan A3: "parsed with `AllowedNet::from_str`".
- Why: `" 10.0.0.1"` was a boot error and is now accepted, stored padded, and
  deduplicated against `"10.0.0.1"`. Pinned by
  `compile_keeps_the_document_spelling_out_of_the_scope`, so it is intended, just
  unrecorded. Symmetric with the hostname path, harmless.
- Fix: state it in plan A3 and API.md §Interception ("entries are trimmed"), or drop the trim.

**F-04 · note · obsolete code** — [interception.rs:56-83](../../../crates/fah-rules/src/interception.rs#L56-L83)
- Evidence: `InvalidExclusion` and `ExclusionSet::new` have no caller outside the
  module's own tests (workspace grep: none). `new` silently deduplicates; the one
  production validator, `Active::compile`, rejects duplicates — two validators for
  one input (principles 4, 14). Plan A3 asked for the carry-over, so plan-compliant.
- Same family, plan-named, no production reader: `InterceptionStore::runtime()`
  ([interception_store.rs:180](../../../crates/fah-api/src/interception_store.rs#L180))
  and `Loaded.migrated` ([:56](../../../crates/fah-api/src/interception_store.rs#L56));
  `main.rs:459` reads `loaded?.active` only.
- Fix: delete `new`/`InvalidExclusion`, re-pin `a_malformed_entry_is_rejected_by_name`
  and the four `set(&[..])` tests on `Active::compile`. Decide `runtime()` at p3-09
  (its likely consumer); drop `migrated` or use it (F-07).

**F-05 · note · undocumented behaviour change** — [main.rs:874-880](../../../crates/fastadhunter/src/main.rs#L874-L880)
- Evidence: the store-closed `warn!` now fires with `count = 0`. At `e5bc251`
  `interception()` returned `None` on an empty list *before* the store check, so
  the line never printed without listed clients; `CertStore::open` failure already
  logs `error!` ([main.rs:533-538](../../../crates/fastadhunter/src/main.rs#L533-L538)).
  Plan D1: "warn as today".
- Why: one misleading line per store-closed boot ("listed clients are spliced" with none listed).
- Fix: gate on `clients > 0`, as the no-CA branch at `:883` already does.

**F-06 · note · logging** — [routes.rs:1544](../../../crates/fah-api/src/routes.rs#L1544)
- Evidence: `payload = ?error.into_panic()` formats `Box<dyn Any + Send>`, whose
  `Debug` is the literal `Any { .. }`. The line carries no panic text; the message
  survives only through the default panic hook on stderr (no custom hook in the tree).
- Why: plan C4 specifies this exact form, so plan-compliant — but the log line the
  500 body points at ("see the log") says nothing.
- Fix: downcast to `&str` / `String` before logging.

**F-07 · note · migration log** — [interception_store.rs:117-122](../../../crates/fah-api/src/interception_store.rs#L117-L122)
- Evidence: the `NotFound` arm logs `info!` only `if carried`; a fresh install
  writes `interception.json` with no line. Plan §3.5 pseudo-code logs unconditionally on that arm.
- Why: the only `/config` file created silently at boot; one line, boot-only.
- Fix: log the write on both paths, or state the silence in §7's fresh-install row.

**F-08 · note · prepare-path allocation** — [interception.rs:233-257](../../../crates/fah-rules/src/interception.rs#L233-L257)
- Evidence: each normalised host is boxed into `hosts` and cloned into `order`
  (line 244) only so `duplicate_of` can be reported; likewise `seen_clients` +
  `index_of_client` (205-208). ≤ 512 extra `Box<str>` (~130 KiB worst case), transient.
- Why: operator path, not hot; principle 3 asks for a justification and there is a one-copy form.
- Fix (optional): `HashMap<Box<str>, usize>` / `HashMap<AllowedNet, usize>`.

**F-09 · note · regression surface, plan-mandated** — [main.rs:449-459](../../../crates/fastadhunter/src/main.rs#L449-L459)
- Evidence: `load_or_migrate` compiles the legacy TOML lists in every `engine.mode`.
  At `e5bc251` the lists were parsed only inside `if https.is_some()`, so a DNS-only
  install carrying an invalid `[https.interception]` entry booted; on release N it
  fails boot naming the list.
- Why: plan §7 ("TOML values invalid or over cap → boot fails") and CONFIGURATION.md
  §Migration say so, mode-agnostic. Correct per plan; recorded for the 0.3.x → N upgrade note.
- Fix: none in code. Mention in the release note.

**F-10 · note · test hygiene** — [e2e_https.rs:412](../../../crates/fastadhunter/tests/e2e_https.rs#L412)
- Evidence: `let _ = &origin;` is a no-op; `origin` is already a named binding.
- Fix: delete the line.

### Checked and found acceptable

| Category | Evidence |
| --- | --- |
| Plan compliance | A1–A5, B1–B5, C1–C5, D1–D2, E1–E2 each located in the diff. `normalize_host` and `ExclusionSet` byte-identical to `e5bc251` minus the baseline and `empty()`. `write_atomic`: closures hoisted at [lib.rs:110-111](../../../crates/fah-config/src/lib.rs#L110-L111); after `rename` only `map_err(path_err)` → `Ok(())`. The four recorded deviations verified: hook gate [interception_store.rs:156-157](../../../crates/fah-api/src/interception_store.rs#L156-L157) + `compile_error!` at `fah-api/src/lib.rs:17`; `bounded_body` at [routes.rs:90-93](../../../crates/fah-api/src/routes.rs#L90-L93) with the over-size arm at `certs.rs:237`; `request_coverage.rs:32`; §14.1 host list. Undocumented deviations are F-03, F-05, F-07 only. Out-of-scope boundaries respected: no accept-arm change beyond `interception_for`, no dashboard file, no `Option` field deletion, no event kind |
| Correctness | Critical section is exactly lock → `write_atomic` → `store` → unlock → `info!` ([interception_store.rs:202-235](../../../crates/fah-api/src/interception_store.rs#L202-L235)); `?` at 222 returns before 228; `Arc::clone` at 228 is a refcount, not an allocation; poison path recovers, clears and logs (202-212). `JoinError` both branches ([routes.rs:1541-1557](../../../crates/fah-api/src/routes.rs#L1541-L1557)). Cancellation: the only side effect is the `spawn_blocking` unit. Shutdown: the runtime is dropped after `block_on` (`main.rs:255`) with no `shutdown_background`, so a started commit completes. Boot order: migration after the privilege drop (446-459), before `CertStore::open` (528) and `ConfigStore::new` (623). Migration writes only on `NotFound` (104-124), any other read error returns (126), TOML re-read from the file layer (71-83) and saved only when carried (128-130). Runtime status table §3.4 matches `main.rs:544-548` |
| Architecture | `fah-api/Cargo.toml` has no `fah-http`; layering green; L2 home per §3.1; `fah-model` type is data only; the only new dependency is external `serde` (+ `serde_json` dev) on `fah-rules`; no port trait, channel or event; `TlsProxy.interception` readers are `with_interception`, `intercepts`, `interception_for` only |
| Performance | Hot path: one `ArcSwap::load` per accepted connection at [https.rs:100-104](../../../crates/fah-http/src/https.rs#L100-L104), guard dropped before `intercept()` (183-193); `TlsProxy::intercepts` has no production caller; `excludes` walk and `normalize_host` unchanged; no lock, allocation or log on the path. `prepare` is CPU-only on the executor (≤ 768 entries), `commit` on the blocking pool; error logs fire only on operator-triggered failures. The rejection of the `https_handshake` column above is sound (fresh-connection group, no VCS baseline); no bench owed (§11). Run anyway on the owner's question — §Measurements → Reviewer A/B: no p3-07 effect where the instrument can measure (pinned h2 `intercepted` and `prewarm_hop` within ±1–3 % over eight runs); the handshake group cannot produce a verdict in either direction on this host, and its `TIME_WAIT` explanation is refuted |
| Memory | `Active` bounded by the two caps; document strings once, normalised copies once; the retiring `Arc<Active>` frees when guards drain; `text` and `Prepared` live for one commit; no task, timer or channel; stale tmp file on crash is the documented limitation |
| Rust quality | No `unwrap`/`expect` on production paths (`unwrap_or(0)` at 220/247 is unreachable); `Send + Sync` by construction; `Prepared` fields private; `InterceptionStoreError::Parse` reused for a serialize failure in `document_text` (139-149) — cosmetic |
| Tests | §14.1–14.6 present by name (renames: `interception_keys_still_parse_as_present_when_empty`, `normalize_host_lowercases_and_bounds_what_it_accepts` merges the `normalize_host_*` cases, `put_is_503_when_the_certificate_store_did_not_open`). The panic → 500 test drives the real handler; the two-thread test proves serialisation; `a_removed_client_keeps_its_session_and_loses_the_next` proves the in-flight invariant on h2. `lookup_cost_is_independent_of_list_size` keeps the 1 s bound the plan asked to carry over (timing-dependent, generous). The `NotFound` migration branch is covered at unit level only — `boot_full_in` seeds the document — and the binary calls the same function |
| Regression | F-05, F-09. `GET /api/v1/config` drops `https.interception` (documented, tested); `BOOT_KEYS` keeps `"https"`; `healthcheck` untouched; dashboard has no reference to the key (grep); `security_phase3.rs` boots through the seeded document unchanged |

### Fixes applied — owner go 2026-09-10 (on the batch proposed in chat)

| Finding | Status | Where |
| --- | --- | --- |
| F-01 | **fixed** (second batch, owner go) — the sentence now points at `PUT /api/v1/interception` and CONFIGURATION.md §Interception Document | `docs/deploy-rb5009.md` §Interception and the CA |
| F-02 | **fixed** (second batch) — `interception.http` row added | `requests/README.md` file table |
| F-03 | **recorded** in plan A3; trim kept, behaviour unchanged | `plan/wip/phase3/p3-07-interception-document-plan.md` A3 |
| F-04 | **fixed** — `ExclusionSet::new` and `InvalidExclusion` deleted; the `set()` helper and `a_malformed_entry_is_rejected_by_name` re-pinned on `Active::compile` (`InvalidEntry { list: "exclude_domains", index: 0, entry }`). `runtime()` and `Loaded.migrated` kept: plan-named, p3-09 and the tests read them | `crates/fah-rules/src/interception.rs` |
| F-05 | **fixed** — store-closed `warn!` gated on `clients > 0` | `crates/fastadhunter/src/main.rs` `interception()` |
| F-06 | **fixed** — payload downcast to `&str` / `String`, `"non-string panic payload"` otherwise; non-panic branch unchanged | `crates/fah-api/src/routes.rs` `commit_join_error` |
| F-07 | **fixed** — fresh-install write logs `info!("wrote an empty interception document")`; carried branch unchanged | `crates/fah-api/src/interception_store.rs` `load_or_migrate` |
| F-08 | **fixed** (second batch, revised on owner review) — `exclude_domains`: the output `HashSet<Box<str>>` is the only structure; `contains` + `insert` per entry, no clone, no second table. `duplicate_of` is recomputed on the error path only, by re-normalising the earlier entries (`normalized_exclusion`, O(index), one allocation each — a rejected `PUT`, never the success path); `unwrap_or(0)` stays, unreachable since an earlier entry put the host in the set. `clients`: `HashMap<AllowedNet, usize>` is the dedupe structure and the `Vec` is the output, nothing built from the map. First revision (`HashMap` + `into_keys().collect()`) paid a second table and a rehash on every success — withdrawn. Hot path untouched | `crates/fah-rules/src/interception.rs` `Active::compile` |
| F-09 | **fixed** (second batch) — migration table gains the "invalid TOML entry, no document" row naming the every-mode change from 0.3.x | `CONFIGURATION.md` §Migration from 0.3.x |
| F-10 | **fixed** — `_origin` binding, no-op line gone | `crates/fastadhunter/tests/e2e_https.rs` |

Verification, both batches: `cargo fmt --all -- --check` clean;
`cargo clippy --workspace --all-targets --all-features -- -D warnings` clean;
`cargo test --all-features -p fah-rules -p fah-api -p fah-http -p fastadhunter`
774 passed, 0 failed after each batch — `a_malformed_entry_is_rejected_by_name`,
`compile_rejects_duplicates_after_normalization`,
`put_with_a_duplicate_is_422_with_duplicate_details`,
`lookup_cost_is_independent_of_list_size`,
`a_commit_panic_is_500_and_the_next_put_succeeds` and
`listing_a_client_through_the_api_applies_on_the_next_connection` all ran, none
skipped. No fix touches the hot path (`interception_for`, `intercepts`,
`excludes`, `normalize_host` unchanged). Working tree only — not committed.

### Verdict

**PASS.** No blocker. Fixed in the working tree: F-01, F-02, F-04, F-05, F-06,
F-07, F-08, F-09, F-10; recorded in plan A3: F-03. Nothing deferred. The plan
§16 follow-ups stay separate tasks by owner decision and are unchanged.
