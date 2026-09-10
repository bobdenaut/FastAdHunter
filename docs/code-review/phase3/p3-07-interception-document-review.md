# p3-07 — Interception Document — Review

**Task:** [p3-07-interception-document.md](../../../plan/wip/phase3/p3-07-interception-document.md) ·
**Plan:** [p3-07-interception-document-plan.md](../../../plan/wip/phase3/p3-07-interception-document-plan.md) ·
**ADR:** ADR-0008 frozen at `fcc7244` · **Base:** `phase3-06` at `e5bc251` ·
**Gates:** GREEN · **Review:** not started (awaiting `start code review`)

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
changed.

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
  and `prewarm_hop` are unaffected and reproduce. Candidate for
  `docs/measurement-traps.md` if the owner wants it recorded there.

## Findings

Not started. Awaiting `start code review`.
