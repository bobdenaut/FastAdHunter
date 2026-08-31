# P3-02 — Certificates API — Implementation Plan

**Phase:** 3 · **Depends on:** p3-01 · **Task:** `p3-02-certificates-api.md`

## TASK START / CONTEXT

1. `plan/wip/phase3/p3-02-certificates-api.md` — the task file, completely.
2. `plan/wip/phase3/CLAUDE.md` — phase table; p3-01 must be `DONE`.
3. `docs/code-review/phase3/p3-01-cert-core-review.md` — **Implementation
   Summary** (declared dependency): the actual `fah-certs` public surface,
   the PFX decision taken (Option A or B), and any deferred items.
4. API.md §Authentication, §Error format, and §Certificates *(Phase 3 —
   reserved)* (`API.md:1143`) — the namespace and error vocabulary this task
   fills in.
5. SECURITY.md §API access, §TLS for the API, §Data at rest — auth model,
   secret-handling rules, `api.tls` semantics.
6. `crates/fah-api/src/routes.rs` (router construction + auth middleware),
   `src/state.rs` (`AppState`/`AppStateBuilder`), `src/server.rs`
   (`ApiServer::bind`, the acceptor), `src/wire.rs` (wire-shape conventions),
   `crates/fah-api/tests/api.rs` (the HTTPS test harness around line 527).

Do not read p3-03/p3-04 files, and do not re-litigate p3-01's crate placement
or its error vocabulary — this task serializes them.

## Decisions settled by this plan

1. **Import activation is `restart_required: true`, not a live rebind.**
   `ApiServer::bind` builds one `TlsAcceptor` from one `Arc<ServerConfig>`;
   a live swap means threading an `ArcSwap` through the accept loop for an
   operation that happens approximately once per deployment. `[api] tls` is
   already a boot key with the same restart story, and `POST /api/v1/config`
   already answers `restart_required` — reuse that vocabulary. Documented in
   API.md. (If the owner prefers live rebind, it is an `ArcSwap<ServerConfig>`
   read per accepted connection — say so in the review and stop; do not build
   both.)
2. **Auth: the standard middleware, no exemptions.** Bearer key or session
   cookie, exactly like every other `/api/v1` route. The task's "bearer key
   required" predates Phase 5 sessions; SECURITY.md's model (only `/health`
   and `login` exempt) is the binding one.
3. **Request encoding: JSON everywhere.** PEM as JSON strings, PFX DER as
   base64 in JSON — no multipart (nothing else in the API uses it). Explicit
   per-route body limit of 256 KB (a CA bundle is a few KB; the default axum
   limit is needlessly generous for key material).
4. **Export content types:** `pem` → `application/x-pem-file`, `der` →
   `application/pkix-cert` (RFC 2585 — what Android expects for a `.crt`/
   `.cer` download), both with `Content-Disposition: attachment` and a stable
   filename (`fastadhunter-ca.pem` / `.crt`).
5. **`Cache-Control: no-store` on every route in the namespace** — status
   reveals fingerprints (fine) but import/generate carry secrets and nothing
   here should ever sit in a cache.
6. **No new config keys.** CA parameters ride the generate request body.

## Detailed implementation plan

### Step 1 — state

- `AppState`/`AppStateBuilder` (`crates/fah-api/src/state.rs`): add
  `pub certs: Arc<fah_certs::CertStore>`.
- `crates/fastadhunter/src/main.rs`: construct the store (p3-01's
  `CertStore::open(config_dir)`) and pass it in. Both test harnesses gain the
  field, built over a `tempfile` config dir.
- Serialization of concurrent mutations: `CertStore`'s own interior mutex
  (p3-01) already serializes generate/import; no new `AppState` mutex — this
  is not a read-modify-write over `fastadhunter.toml` like `list_mutations`.

### Step 2 — routes (`crates/fah-api/src/routes.rs` + new `src/certs.rs`)

Add under the `v1` router:

- `GET  /certificates` → `certs_status`
- `POST /certificates/ca/generate` → `certs_generate_ca`
- `GET  /certificates/ca/export` → `certs_export_ca`
- `POST /certificates/import` → `certs_import`

Handlers live in a new `crates/fah-api/src/certs.rs` (routes.rs is 69 KB;
follow the existing one-module-per-surface split: `auth.rs`, `password.rs`,
`telemetry.rs`).

### Step 3 — wire shapes (`certs.rs`, serialized structs beside the handlers or in `wire.rs`, matching current convention)

**`GET /api/v1/certificates`** → `200`:

```json
{
  "ca": {
    "present": true,
    "fingerprint_sha256": "AB:CD:…",
    "not_before": "2026-09-01T00:00:00Z",
    "not_after": "2036-08-30T00:00:00Z",
    "subject": "CN=FastAdHunter CA"
  },
  "api_certificate": { "source": "self_signed" },
  "leaf_cache": { "size": 0, "capacity": 512, "hits": 0, "misses": 0,
                  "minted_total": 0, "evictions": 0 }
}
```

`ca` is `{"present": false}` with the other keys absent when no CA exists.
`api_certificate.source` is `"self_signed" | "imported"` (p3-01's marker).

**`POST /api/v1/certificates/ca/generate`** — body
`{"confirm": true, "common_name"?: "…", "validity_days"?: 3650}`.
- `confirm` missing/false → `400 bad_request` ("confirm: true required —
  regeneration invalidates the previous CA"); nothing happens.
- Success → `200` with the new `ca` status block plus
  `"archived_previous": true|false`.
- `validity_days` outside `1..=7300` → `422 validation_failed`.

**`GET /api/v1/certificates/ca/export?format=pem|der`** — default `pem`;
unknown format → `422 validation_failed`. No CA → `404 not_found`. Body is the
raw certificate, content types per decision 4. **The handler calls only
`ca_public_pem`/`ca_public_der`** — the p3-01 functions that cannot read a key.

**`POST /api/v1/certificates/import`** — body either
`{"format": "pem", "cert_pem": "…", "key_pem": "…"}` or
`{"format": "pfx", "pfx_base64": "…", "passphrase": "…"}` (the second only if
p3-01 took PFX Option A; otherwise `422` with a message naming the deferral).
- Validation errors map 1:1 from `CertError` variants to
  `422 validation_failed` with a stable, variant-derived `message` prefix
  (`expired`, `not_yet_valid`, `key_mismatch`, `parse`, `bad_passphrase`) so
  clients can distinguish causes without new error codes.
- Success → `200 {"applied": false, "restart_required": true,
  "source": "imported"}`; the pair is on disk, the running acceptor is
  unchanged, and `GET /api/v1/certificates` reflects `"imported"` immediately.

### Step 4 — secrets hygiene

- Handlers never log request bodies; the only tracing they emit is
  outcome-level (`info!` on generate/import success with fingerprint,
  `warn!` on CA regeneration — already in p3-01).
- The deserialized import structs get no derived `Debug` (or a redacted manual
  impl), so an error path cannot print key material or the passphrase.
- Error messages echo nothing from the body — variant names only.
- `no_store` wrapper (already used for `auth/login` at `routes.rs:91`) applied
  to all four routes.

### Step 5 — API.md

Write the full §Certificates spec (routes, shapes above, error mapping, the
`restart_required` contract, content types) replacing the reserved stub —
**proposed as a doc edit and held for owner approval; code and doc land in the
same change once approved** (the task requires same-change; the working
agreement requires the explicit yes first).

## Data flow / concurrency summary

Route handler → `Arc<CertStore>` (sync, admin-plane mutex inside) →
filesystem under `/config`. Generate/import are `spawn_blocking` candidates
only if measured slow; CA generation is sub-second and rare — call it inline
first, matching how other admin routes do filesystem work, and note the
`spawn_blocking` fallback in the review if a handler ever holds the runtime
> 100 ms (measure, don't assume).

## Performance contract

| Metric | Class | Value |
| ------ | ----- | ----- |
| Status route | target | reads in-memory summary + one atomic stats snapshot; no disk I/O per call |
| Generate/import | diagnostic | admin-plane, no budget; TBD — record wall time once in the review |
| Hot paths (DNS/HTTP) | hard gate | untouched |
| Steady-state RSS | hard gate | unchanged (no new resident state beyond the store handle) |

## Tests

### Unit

- Wire-shape serialization: status with/without CA, generate response, import
  error mapping for every `CertError` variant (variant → HTTP code + message
  prefix).
- `confirm` guard: absent, `false`, wrong type → `400`, store untouched.

### Integration (`crates/fah-api/tests/api.rs` harness, over HTTPS)

- **Lifecycle (the acceptance test):** `GET` status (no CA) → `POST
  ca/generate` → `GET` status (present, fingerprint F) → `GET ca/export` in
  both formats → parse each export and assert its SHA-256 fingerprint equals F
  → `POST import` with a valid pair → status shows `"imported"` +
  `restart_required` was returned → regenerate with `confirm` → status shows a
  new fingerprint and `archived_previous: true`.
- Auth: all four routes → `401 unauthorized` without credentials; work with
  the bearer key; work with a session cookie.
- Import of each invalid input class over the wire → `422` with the named
  prefix.
- Oversized body → rejected by the 256 KB limit.

### Regression

- Full existing `fah-api` suite green — no existing route, wire shape, or
  auth behaviour changes.
- `GET /api/v1/config` still omits every secret (no new leakage vector).

### Security

- Export responses contain no `PRIVATE` material (asserted on both formats,
  over the wire).
- A failed import's response body and the captured tracing output contain
  neither key bytes nor the passphrase (assert with a tracing test subscriber,
  the pattern existing auth tests use if present; otherwise assert on the
  response only and note it).
- `no_store` present on all four routes.

### Performance

- None beyond the diagnostic timing note — admin plane.

## Verification / Gates

- **Mandatory:** `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --all-features --workspace`.
- **Mandatory:** lifecycle integration test green over HTTPS; API.md spec and
  implementation shipped in the same approved change (the golden-file check is
  the wire-shape tests pinning the documented JSON).
- **Recommended:** run the dashboard against the branch once if the dashboard
  grows a certificates view later — out of scope now.

## Doc changes (proposed — owner approval required)

- API.md: replace the reserved §Certificates stub with the full spec.
- No other document changes; SECURITY.md/ARCHITECTURE.md were p3-01's.

## Non-goals

- Interception toggles and per-client opt-in config (p3-04).
- Client CA-install walkthrough (p3-06).
- Live acceptor rebind (decision 1).
- Dashboard UI.

## Acceptance criteria (from the task file)

- API.md §Certificates fully specified and matching the implementation
  (wire-shape tests pin it).
- Export responses never contain private keys (asserted).
- Gates green.
