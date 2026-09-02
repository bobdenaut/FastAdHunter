# p3-02 — Certificates API — Review

**Task:** `plan/wip/phase3/p3-02-certificates-api.md` · **Plan:**
`p3-02-certificates-api-plan.md` · **Depends on:** p3-01 (commit `3337aab`)
· **Status:** third fix round applied, gates green — see §Verdict after the
third fix round.

## Implementation Summary

`/api/v1/certificates` is now a served surface over the `fah-certs` core p3-01
shipped. No certificate machinery was added, moved or duplicated: `fah-api`
holds an `Arc<CertStore>` and calls its existing entry points.

| Area | Where |
| --- | --- |
| Handlers + wire shapes + their unit tests | `crates/fah-api/src/certs.rs` (new) |
| Route registration, `no_store`, body limit | `crates/fah-api/src/routes.rs` |
| `certs` handle on the state | `crates/fah-api/src/state.rs` |
| `CertStore` re-export | `crates/fah-api/src/lib.rs` |
| Store opened at boot, degrade on failure | `crates/fastadhunter/src/main.rs` |
| Lifecycle / auth / security tests | `crates/fah-api/tests/api.rs` |
| Manual request suite | `requests/certificates.http` (new) |

### Routes

| Route | Success | Notes |
| --- | --- | --- |
| `GET /api/v1/certificates` | `200` | CA block, `api_certificate.source`, `leaf_cache` |
| `POST /api/v1/certificates/ca/generate` | `200` | `{"confirm": true}` required; `archived_previous` |
| `GET /api/v1/certificates/ca/export?format=pem\|der` | `200` | public certificate only |
| `POST /api/v1/certificates/import` | `200` | PEM API-server pair; `restart_required: true` |

All four carry `Cache-Control: no-store`; both `POST` routes carry a 256 KB
`DefaultBodyLimit`. Auth is the standard middleware — bearer key **or** session
cookie, no exemption (plan decision 2).

### Wire shapes

```json
{
  "ca": { "present": true, "fingerprint_sha256": "AB:CD:…",
          "not_before": "…Z", "not_after": "…Z", "subject": "CN=FastAdHunter CA" },
  "api_certificate": { "source": "self_signed" },
  "leaf_cache": { "size": 0, "capacity": 512, "inflight": 0, "hits": 0,
                  "unwarmed_misses": 0, "prewarm_hits": 0, "coalesced": 0,
                  "minted_total": 0, "evictions": 0, "superseded": 0 }
}
```

`ca` is exactly `{"present": false}` when none exists (`skip_serializing_if`).
Generate answers `{"ca": {…}, "archived_previous": bool}`; import answers
`{"applied": false, "restart_required": true, "source": "imported"}`.

### Error mapping

| Cause | Response |
| --- | --- |
| `confirm` absent / false / wrongly typed | `400 bad_request`, store untouched |
| Body not JSON in the documented shape | `400 bad_request` |
| Body over 256 KB | `400 bad_request` naming `262144` |
| `validity_days` outside `1..=7300`, `common_name` outside 1–64 chars | `422 validation_failed` |
| `format` not `pem`/`der` (export) | `422 validation_failed` |
| `format: "pfx"`/`"pkcs12"` (import) | `422 unsupported_format:` + the `openssl pkcs12` command |
| `CertError::{Parse, Expired, NotYetValid, KeyMismatch, NotACa}` | `422` with a stable variant-derived prefix (`parse:`, `expired:`, `not_yet_valid:`, `key_mismatch:`, `not_a_ca:`) |
| Export with no CA | `404 not_found` |
| Any other `CertError` (I/O, config) | `500 internal` |
| `CertStore` did not open at boot | `503 unavailable`, **no** `Retry-After` |

No error message echoes any part of the request body — only variant names and
fixed text. Handlers log outcomes only (fingerprint + `archived_previous` on
generate, a bare line on import); nothing logs a body, a certificate or a key.

### Decisions

- **Every blocking `CertStore` call runs under `spawn_blocking`** — `status()`
  (reads the `api-cert.source` marker from disk), `generate_ca`,
  `install_api_pair` and both export readers (they take the CA mutex, which
  `install_ca` holds across disk I/O — p3-01 finding m4). This **overrides the
  plan's** "call it inline first, `spawn_blocking` only if measured slow"; the
  p3-01 review carried it forward as a constraint, and the measurements below
  put generate at ~2 ms on a dev box, i.e. ~18 ms at the documented ~9× RB5009
  factor, on a runtime shared with the DNS path.
- **Import activation is `restart_required: true`, not a live rebind** (plan
  decision 1, unchanged). The pair is on disk; the running acceptor keeps the
  old one; `GET /api/v1/certificates` reports `"imported"` immediately.
- **A `CertStore` that fails to open is not fatal at boot.** p3-01 left this
  open ("decide whether that is fatal at boot or degrades"). `main.rs` logs
  `error!` and passes `None`; DNS keeps resolving and the four routes answer
  `503 unavailable` with no `Retry-After` — the shape API.md already uses for a
  condition only an operator can clear. `AppState.certs` is therefore
  `Option<Arc<CertStore>>`, not the plan's `Arc<CertStore>`.
- **Import targets the API-server pair only.** `install_ca_pair` gets no
  endpoint: the task scope says "API server cert". Importing a real CA stays
  unserved.
- **Oversized body answers `400`, not `413`** — API.md fixes the code set and
  has no `413`; the message names the byte limit.

### Deviations from the plan, and contract gaps (not silently changed)

1. **`leaf_cache` field set.** The plan sketched `{size, capacity, hits,
   misses, minted_total, evictions}`. p3-01's third and fourth fix rounds
   replaced `misses` with `unwarmed_misses` and added `inflight`,
   `prewarm_hits`, `coalesced`, `superseded`. The shipped shape serializes
   `LeafCacheStats` as it actually exists; the plan text predates it.
2. **`archived_previous` is inferred, not reported by `fah-certs`.**
   `CertStore::generate_ca` returns only `CaSummary`; `install_ca` knows
   whether it archived but does not say. The handler reads `has_ca()` inside
   the same `spawn_blocking` closure, immediately before `generate_ca`. That is
   correct for the admin plane (single writer in practice) but is not atomic
   with the archive step, and it reports the **in-memory** CA slot rather than
   what was on disk. Recorded rather than fixed: the fix belongs in
   `fah-certs`' return type.
3. **`rcgen` and `rustls-pemfile` return to `fah-api` as dev-dependencies
   only.** The import tests need a real pair to submit and the export tests
   re-parse what came back over the wire. Release dependencies are unchanged —
   p3-01 removed both from `[dependencies]` and they stay out (ADR-0006).
4. **API.md is not edited.** The task and plan require doc and code in the same
   change; the working agreement requires an explicit yes for any `.md` first.
   The proposed edit is listed below and is held.

### Tests

| Suite | Count | Covers |
| --- | --- | --- |
| `certs::tests` (unit) | 9 | status with/without a CA, generate/import response shapes, every `CertError` → HTTP mapping, PFX refusal, blank-PEM refusal, parameter range checks, pre-epoch timestamp |
| `tests/api.rs` (integration, over HTTPS) | 11 | see below |

Integration coverage: the full lifecycle (status → generate → status → export
both formats → **fingerprint of each export equals the reported one** → import
→ `"imported"` + `restart_required` → regenerate → new fingerprint +
`archived_previous: true`); export purity on both formats and on the store's
own view; a `cert + key` blob pasted into `cert_pem` reaching neither the
response nor `/config/api-cert.pem`; every invalid import class (`expired`,
`not_yet_valid`, `key_mismatch`, unparseable, missing field, `pfx`) as a `422`
with its prefix, and the API pair unchanged after each; a rejected import
echoing no line of the submitted certificate or key; the `confirm` guard in
three forms with the store untouched; parameter range checks; `404` with no CA
and `422` on an unknown export format; the 256 KB limit; `401` without a
credential on all four routes and success with **both** bearer and cookie;
`no-store` on all four; and the `503`-without-`Retry-After` degrade path.

Regression: the whole existing `fah-api` suite is green and unchanged — no
existing route, wire shape or auth behaviour was touched. `request_coverage.rs`
passes with the new `requests/certificates.http`; `layering.rs` passes.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | green — 47 suites, zero failures |
| `cargo test -p fah-api --all-features` | 121 unit + 103 integration |

### Measurements

Dev box (x86_64, Windows, debug profile, `std::time::Instant`, 5 iterations —
diagnostic only, no budget row; p3-06 owns budgets and the ~9× RB5009
conversion).

| Operation | Wall time | Read |
| --- | --- | --- |
| `CertStore::generate_ca` | 22.2 ms first, then 1.8–3.0 ms | first call includes crypto-provider/allocator warm-up; steady state ≈ 2 ms |
| `validate_server_pair` + `install_api_pair` | 0.9–1.9 ms | parse + key match + two file writes |
| `CertStore::status` | 13.1 µs | one file read for the source marker |
| `CertStore::open` | 43.6 µs | boot only |

Each is well above the "never block a worker" threshold at the ~9× factor,
which is why all four handlers use `spawn_blocking`. Hot paths (DNS/HTTP) are
untouched; steady-state RSS gains one `Arc<CertStore>` handle and the
already-allocated 512-entry leaf cache p3-01 sized.

### Files changed

| File | Change |
| --- | --- |
| `crates/fah-api/src/certs.rs` | new — 4 handlers, wire shapes, error mapping, 9 unit tests |
| `crates/fah-api/src/routes.rs` | 4 routes, `bounded_body`, `DefaultBodyLimit` import |
| `crates/fah-api/src/state.rs` | `certs: Option<Arc<CertStore>>` on `AppState` and the builder |
| `crates/fah-api/src/lib.rs` | `mod certs`; re-export `CertStore` |
| `crates/fah-api/Cargo.toml` | dev-only `rcgen`, `rustls-pemfile` |
| `crates/fah-api/tests/api.rs` | harness gains a store + a `certs` option; 11 tests |
| `crates/fastadhunter/src/main.rs` | opens the store off the runtime, degrades on failure |
| `crates/fastadhunter/tests/history_e2e.rs` | harness gains the store |
| `requests/certificates.http` | new |

No file under `crates/fah-certs` was touched.

### Remaining TODOs

- **Doc changes proposed, awaiting approval** (not applied):
  - `API.md` §Certificates — replace the reserved stub with the full spec above
    (routes, shapes, error table, `restart_required` contract, content types,
    `no-store`, body limit, the `503` degrade).
  - `requests/README.md` — add `certificates.http` to the coverage list.
- p3-04/p3-05 wire the leaf cache; `prewarm` still has no endpoint and needs
  none.
- Importing a **CA** pair (`install_ca_pair`) has no endpoint. If the owner
  wants "bring your own root", it is a `target` field on the import route.
- Deviation 2 (`archived_previous` inferred rather than returned) is a
  `fah-certs` contract gap, left for a future change to that crate.

---

## Findings — consolidated 2026-09-03; full history: `git show e35203e:docs/code-review/phase3/p3-02-certificates-api-review.md`

Three independent reviews and four fix rounds. Fixed and withdrawn items are
omitted — git has them. Still open:

| id(s) | Issue | Status | Where |
| --- | --- | --- | --- |
| M1 (boot-abort half) | a corrupt or interrupted API pair aborts boot while `api.tls = true`; the documented recovery (copy back from `api-archive/`, or staged-key completion) has never executed on a device | deferred | p3-06 review §Runbook 7, import-then-restart |
| MEDIUM-A (residual) | the commit-failure rollback test is `#[cfg(windows)]` — the branch has run only on the dev box, never in the container build | deferred | p3-06 review §Runbook 7, import-then-restart is the device-side evidence |
| p3-01 L1 (carried) | `write_private`'s `0600` is `cfg(unix)` and has never compiled or run on the dev box | deferred | p3-06 review §Runbook 7, "0600 on every private key" |
| LOW-A | "never echoes the submitted material … not in the log" — the log half is verified by inspection only; no tracing-subscriber test | deferred | no owner — needs one |
| LOW-C | `POST …/ca/generate` and `POST …/import` from a cookie session carry no reauthentication | won't-fix | documented single-admin model (SECURITY.md §Sessions); parity would be a `current_password` field on both POSTs |
| nitpicks (6) | `install_api_pair` returns `()` not `archived_previous`; `common_name:` prefix on every `Generate` error; `Query<ExportParams>` rejection answers outside the envelope; `instant()` unchecked add; `generate_ca` runs keygen and staging before the archive-cap refusal; a client that disconnects mid-request lets the task finish, so a retry regenerates twice | deferred | no owner — needs one |

**PASS WITH DEFERRED FINDINGS** — 6 open rows (5 deferred, 1 won't-fix). Safe to mark DONE: yes (marked 2026-09-01).
