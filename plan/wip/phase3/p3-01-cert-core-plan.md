# P3-01 — Certificate Core — Implementation Plan

**Phase:** 3 · **Depends on:** phase2 (closed) · **Task:** `p3-01-cert-core.md`

## TASK START / CONTEXT

Read, in this order, before writing any code:

1. `plan/wip/phase3/p3-01-cert-core.md` — the task file, completely.
2. `plan/wip/phase3/CLAUDE.md` — the phase table; confirm p3-01 is the first
   `WAITING` task.
3. SECURITY.md — **binding, read fully**: fixed crypto set (rustls, rcgen,
   x509-parser, argon2, aws-lc-rs), CA key never leaves `/config`, export is
   public-only, interception never default.
4. ARCHITECTURE.md §Workspace Layout and §Dependency Layering — the layering
   guard (`crates/fastadhunter/tests/layering.rs`) must pass with the new crate.
5. `docs/code-review/phase2/p2-01-review.md` §2 — the admission test this plan
   applies: *does divergence between two siblings produce a silent bug?*
6. `docs/code-review/phase5/p5-02-cert-browser-spike-review.md` — Implementation
   Summary only: `probe_local_address()` / `san_entries()` split, the
   `IncompletePair` guard, the tmp-file write pattern. That machinery is moved,
   not rewritten.
7. `docs/code-review/Global Architecture Review-Reconciled.md` §5 items 7
   (certificate machinery home — shared crate, ADR) and 11 (memory caps for
   every new state owner).
8. `crates/fah-api/src/tls.rs` — the code being moved; read it fully (it is the
   only file this plan moves).

Do not read p3-03/p3-04 task files, unrelated phase-5 reviews, or full review
files beyond the sections above. Do not re-litigate the crate-placement
precedent (p2-01) or the p5-02 SAN design.

## Decisions settled by this plan

1. **New crate `crates/fah-certs`, layer L2** (beside `fah-rules`).
   - Two L3 consumers exist or are scheduled: `fah-api` (p3-02 import/status,
     and the existing self-signed API cert) and `fah-http` (p3-04 leaf minting).
     Siblings may not import each other, so the machinery goes down.
   - Not `fah-common`: ARCHITECTURE.md warns it must not become a dumping
     ground, and p2-01 already spent one admission (`tokio` + `socket2`).
     CA/leaf logic is domain logic, not a small shared utility.
   - Not L1: it is business logic, and L1 is data/config/small-utils by
     definition. L2 keeps the guard honest: `fah-certs` may import L1 only.
   - Recorded as **ADR-0006** (GAR §5.7 demands the ADR). Proposed, not
     written, until the owner approves the `.md` changes (see §Doc changes).
2. **`fah-api/src/tls.rs` generation/load machinery moves into `fah-certs`**
   — the p2-01 pattern: move down, delete the original, move the tests.
   `probe_local_address()` **stays in `fah-api`** (it opens a socket; the
   library stays pure — engineering principle 6). `san_entries()`,
   `generate()`, `load_or_generate()`, `install_crypto_provider()`, the
   tmp-file write pattern and `IncompletePair` move. `fah-api::tls` becomes a
   thin module: the probe plus calls into `fah_certs`.
3. **Key algorithm:** ECDSA P-256 via rcgen's `aws_lc_rs` backend for CA and
   leaves — same provider the workspace already links; small keys, fast mint.
4. **CA storage:** `/config/ca-cert.pem` + `/config/ca-key.pem`, written with
   the existing tmp+rename pattern, key file mode 0600 on unix. Regeneration
   moves the old pair to `/config/ca-archive/<unix-ts>/` (never deletes a
   private key) and logs at `warn!`.
5. **CA parameters:** compiled defaults CN `FastAdHunter CA`, validity 3650
   days; caller-overridable arguments (p3-02 exposes them on
   `POST …/ca/generate`). **No new TOML section in this task** — nothing here
   needs a boot key, and p3-04 owns the interception config surface.
6. **Leaf parameters:** validity 7 days, `not_before` backdated 24 h (clock
   skew between the container and client devices must not make a fresh leaf
   "not yet valid"; the CA gets the same backdate), SAN = the requested host
   (DNS name, or IP SAN when the host parses as an address), signed by the CA.
   Re-mint on expiry is a cache miss.
7. **Leaf cache:** LRU, compiled capacity **512 entries**, keyed by host
   (`Arc<str>`), value `Arc<rustls::sign::CertifiedKey>`. ~2–3 KB per entry ⇒
   ≤ ~1.5 MB at cap — the GAR §5.11 memory cap for this state owner, bounded
   by count. No reusable LRU exists at L1/L2 and none is imported: a
   `HashMap<Arc<str>, (u64, Arc<CertifiedKey>)>` with a monotonic use counter,
   evict-min on insert at capacity (O(capacity) scan on the mint path only,
   O(1) on hit) — 512 entries make the scan trivial and the mint already
   dwarfs it. `std::sync::Mutex` around the map; **minting happens outside the
   lock** (lock → miss → drop lock → mint → lock → insert; a lost race
   re-inserts an identical leaf, harmless). This is not the DNS hot path;
   it is the per-TLS-handshake path, and p3-04 decides pre-warming.
8. **PFX/PKCS#12 import — owner decision required before implementation.**
   SECURITY.md's fixed set contains no PKCS#12 parser, and hand-rolling one is
   forbidden. Real-world PFX files are **encrypted** (PBES1/PBES2 + 3DES or
   AES), so a decode-only ASN.1 parser is not enough — import needs parsing
   *and* decryption. The honest dependency surface is `p12-keystore` (or
   RustCrypto `pkcs12` plus its PBKDF/cipher crates), i.e. several new crypto
   crates, not one parser.
   - **Option A:** add `p12-keystore` (decode+decrypt), amend SECURITY.md's
     fixed set in the same change, record the widened surface in ADR-0006.
   - **Option B (recommended):** descope PFX from p3-01/p3-02 to PEM-only
     import and note the deferral in the task and API.md. Every OS/browser
     exports PEM (or `openssl pkcs12` converts), and hard rule 5 favors the
     smallest crypto surface.
   Present both to the owner at task start; implement whichever is chosen. All
   other steps are independent of this decision.

## Detailed implementation plan

### Step 1 — crate skeleton and layering

- `crates/fah-certs/`: `Cargo.toml` (deps: `rcgen` {aws_lc_rs, pem},
  `rustls`, `rustls-pemfile`, `x509-parser`, `aws-lc-rs` (direct — the
  fingerprint digest in Step 3 calls its API, transitive linkage is not
  enough), `thiserror`, `tracing`; no tokio),
  `src/lib.rs` with modules `store`, `ca`, `leaf`, `import`, `export`.
- Workspace member added; `crates/fastadhunter/tests/layering.rs` gains the L2
  assignment for `fah-certs`. `fah-api` adds the dependency (L3 → L2, legal).
- Error type: one `CertError` enum (thiserror), variants named for every
  rejection the task demands: `Expired`, `NotYetValid`, `KeyMismatch`,
  `NotACa`, `Parse`, `Io {path}`, `IncompletePair {present, missing}`,
  `Generate`. Import rejections must be distinguishable by variant, not by
  message text.

### Step 2 — move the API-cert machinery (mechanical)

- Move from `crates/fah-api/src/tls.rs` into `fah_certs`: `TlsError` folds
  into `CertError`; `install_crypto_provider()`, `san_entries()`,
  `generate()`, `load_or_generate()`, `write_pair`/tmp constants, the
  interrupted-generation recovery and `IncompletePair` guard. Unit tests move
  with them; the p5-02 SAN assertions must pass unchanged.
- `fah-api::tls` retains `probe_local_address()` and a
  `load_or_generate(config_dir, bind_address)` wrapper that runs the probe and
  delegates, so `crates/fastadhunter/src/main.rs` and both test harnesses
  (`crates/fah-api/tests/api.rs`, `crates/fastadhunter/tests/history_e2e.rs`)
  keep compiling with at most an import change.
- `x509-parser` stops being dev-only where import validation needs it at
  runtime (in `fah-certs`); `fah-api` may drop its dev-dependency if its
  remaining tests no longer use it.

### Step 3 — CA generation, load, archive (`ca.rs`, `store.rs`)

- `CertStore::open(config_dir: &Path) -> Result<CertStore, CertError>` —
  loads an existing CA pair if present (tmp-recovery + `IncompletePair`
  exactly as the API pair does today); a store with no CA is valid
  (`ca(): Option<&CaHandle>`). No I/O after `open` except explicit operations.
- `CertStore::generate_ca(&mut self, params: CaParams) -> Result<CaSummary>` —
  if a CA exists, archive first (`/config/ca-archive/<unix-ts>/`, `rename`,
  key permissions preserved), then generate (rcgen, `IsCa::Ca`,
  `BasicConstraints`), write with tmp+rename, key 0600, `warn!` on
  regeneration. Returns fingerprint + validity.
- Fingerprint: SHA-256 over the DER certificate via `aws-lc-rs` digest (already
  in-tree; no new crypto dependency), rendered as colon-separated hex.
- `CaSummary { fingerprint_sha256, not_before, not_after, subject }` — the
  status DTO p3-02 serializes; timestamps read back with x509-parser so status
  reports what is on disk, not what generation intended.
- Ownership: `CertStore` owns paths and the loaded CA key
  (`rcgen::KeyPair` + issuer cert); it is wrapped in `Arc` by consumers.
  Interior mutability only around `generate_ca`/import (an `std::sync::Mutex`
  over the mutable CA slot) — admin-plane rare operations, never per-query.

### Step 4 — leaf minting and cache (`leaf.rs`)

- `LeafCache::get_or_mint(&self, host: &str) -> Result<Arc<CertifiedKey>>` —
  LRU as decided above; expired entry = miss. Minting builds an rcgen leaf
  signed by the CA, converts to `rustls::sign::CertifiedKey` once, and shares
  it by `Arc` — no PEM round-trip on the mint path.
- **`generate_ca` (and any CA-replacing import) clears the leaf cache** —
  p3-02's generate route needs no restart, so without the purge, leaves
  signed by the archived CA would be served for up to 7 days to clients that
  already trust the new one.
- `LeafCacheStats { size, capacity, hits, misses, minted_total, evictions }` —
  relaxed atomics beside the mutex, read by p3-02 status.
- **`MintingResolver`** (`struct MintingResolver { cache: Arc<LeafCache>,
  fallback: Option<Arc<CertifiedKey>> }` implementing
  `rustls::server::ResolvesServerCert`: read `hello.server_name()`,
  `cache.get_or_mint(name)`; on no-SNI or mint failure return `fallback`
  (`None` when unset, aborting the handshake)) lives **here**, not in a
  consumer. The `fallback` slot is what lets p3-05's DoT listener serve the
  API pair to a no-SNI client while p3-04 passes `None` and stays
  fail-closed — one type, both postures. Two L3 consumers need it —
  p3-04 (interception `ServerConfig`) and p3-05 (DoT with a CA present) — and
  they are siblings; two implementations diverging on expiry re-mint or
  no-SNI handling is a silent bug (the p2-01 admission test). It is a pure
  type over the cache — no I/O, no async — so it belongs at L2. **Wiring it
  into any `ServerConfig` stays p3-04's / p3-05's work**; p3-01 delivers the
  seam and proves it with the round-trip test (which may use it directly).

### Step 5 — import (`import.rs`)

- `validate_server_pair(cert_pem: &[u8], key_pem: &[u8]) -> Result<ImportedPair>`
  — x509-parser on the cert: reject `Expired` / `NotYetValid` / `Parse`;
  key-match check by comparing the certificate's SPKI against the public key
  derived from the private key (rcgen `KeyPair::from_pem` →
  `public_key_der()`); `KeyMismatch` when they differ.
- `validate_ca_pair(…)` — same, plus `NotACa` when BasicConstraints/`CA:TRUE`
  is absent (used when a user imports their own CA; the API-server pair path
  must **not** require CA-ness).
- `CertStore::install_api_pair(pair)` — writes to the existing
  `api-cert.pem`/`api-key.pem` paths with tmp+rename, replacing the
  self-signed pair. Activation policy (restart vs rebind) is p3-02's decision;
  this function only persists.
- PFX (per the owner's Option A/B decision): `pfx_to_pem(der: &[u8],
  passphrase: &str) -> Result<(cert_pem, key_pem)>`, then the same PEM
  validation path — one validator, two entry formats (principle 4). The
  passphrase is never logged and never stored; no `Debug` derive on any type
  holding it or a private key (manual redacted impls, matching the
  session-secret precedent in SECURITY.md §Data at rest).

### Step 6 — export and status (`export.rs`)

- `ca_public_pem(&self) -> Option<String>` and `ca_public_der(&self) ->
  Option<Vec<u8>>` — read/derive from the **certificate** only. The functions
  take no key path and are structurally unable to read one; the security test
  asserts output contains no `PRIVATE KEY` block and that DER parses as a
  certificate (x509-parser) and not as a key.
- Status = `CaSummary` + `LeafCacheStats` + which API pair is active
  (`self_signed | imported` — persisted as a one-line marker file
  `/config/api-cert.source` written by `install_api_pair`, absent = self_signed;
  cheaper and more honest than re-parsing issuers on every status call).

### Step 7 — wiring

- `crates/fastadhunter/src/main.rs`: build `Arc<CertStore>` from
  `/config` at startup (before privilege drop is irrelevant here — `/config`
  is writable post-drop), pass to `AppStateBuilder` **in p3-02**; in p3-01 the
  binary change is only whatever the moved `load_or_generate` requires.
  Nothing new runs per query; startup cost is one directory read.

## Performance contract

| Metric | Class | Value |
| ------ | ----- | ----- |
| Leaf cache memory at cap (512 entries) | hard gate (bounded-by-count) | ≤ ~1.5 MB; byte figure TBD — must be measured during verification (p3-06) |
| Leaf mint cost (dev box) | diagnostic | TBD — must be measured during verification; criterion bench added now, budget row set in p3-06 with the ~9× factor |
| Cache hit (`get_or_mint` hot case) | target | lock + hash + `Arc` clone; no allocation beyond the clone |
| CA generation | diagnostic | cold admin path; TBD — measure, no budget |
| DNS/HTTP hot paths | hard gate | untouched — nothing from this crate is on a per-query path in p3-01 |
| Steady-state RSS | hard gate | unchanged in p3-01 (cache is empty until p3-04 wires it) |

No number above is invented as a gate; budgets land in PERFORMANCE.md via
p3-06 from measured data.

## Tests

### Unit (`fah-certs`)

- `san_entries` suite moves intact from `fah-api` (p5-02 assertions unchanged).
- Fingerprint is stable across load/generate for the same DER.
- LRU: eviction at capacity, expired-leaf re-mint, stats counters.
- `MintingResolver`: returns a leaf for a hello carrying SNI; on a hello
  without SNI returns `None` when no fallback is set and the fallback key
  when one is; expired cached leaf is re-minted, not served.
- CA regeneration purges the cache: mint, `generate_ca`, next `get_or_mint`
  chains to the new CA (verified against the new root, rejected by the old).
- Import rejections, one test per named variant: garbage → `Parse`, expired
  cert → `Expired`, mismatched key → `KeyMismatch`, leaf-where-CA-expected →
  `NotACa`. (PFX: wrong passphrase → named error, if Option A.)
- `IncompletePair` and tmp-recovery behaviour for the CA pair (mirror the
  existing api-pair tests).

### Integration

- **Round-trip (the acceptance test):** generate CA → mint leaf for
  `test.example` → rustls server using the `CertifiedKey`, rustls client whose
  root store holds **only** `ca_public_der()` → handshake succeeds and
  hostname-validates. Second client trusting nothing → handshake fails.
  Over `tokio::io::duplex` or a loopback listener in `fah-certs` tests
  (tokio + tokio-rustls as dev-dependencies, matching `fah-dns`'s precedent).

### Regression

- Existing `fah-api` TLS tests (first-boot generation, honored user pair,
  incomplete-pair refusal, SAN set) pass after the move.
- `crates/fastadhunter/tests/layering.rs` green with the new L2 crate.
- Full workspace suite unchanged otherwise.

### Security

- Export purity: PEM export contains exactly one `CERTIFICATE` block and no
  `PRIVATE` substring; DER export parses as a certificate.
- Key file mode 0600 after generate and after import (unix; skipped on the
  Windows dev box with an explicit `#[cfg]`).
- Archive preserves the old key file and its permissions; nothing deletes a
  private key.
- Redaction: `format!("{:?}", …)` over every store/import type contains no key
  bytes and no passphrase.

### Performance

- Criterion bench `certs_mint` (mint cost) and `certs_cache_hit` — diagnostic
  only in this task.

## Verification / Gates

- **Mandatory:** `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --all-features --workspace`.
- **Mandatory:** layering test green; round-trip and export-purity tests green.
- **Recommended:** run the two new benches once and record numbers in the
  review file (they seed p3-06's budget rows).
- **Diagnostic:** none beyond the benches.

## Doc changes (proposed — owner approval required before editing any `.md`)

- ARCHITECTURE.md: `fah-certs` in §Workspace Layout and §Dependency Layering
  (L2).
- New `docs/decisions/0007-certificate-machinery-home.md` (placement + PFX
  decision).
- SECURITY.md: only if PFX Option A (set amendment).
- Finish the code first, then list these edits and wait, per the working
  agreement. The task's own review file
  `docs/code-review/phase3/p3-01-cert-core-review.md` needs no approval.

## Non-goals

- No API endpoints (p3-02), no SNI path (p3-03), no interception or listener
  wiring (p3-04/p3-05 — the `MintingResolver` *type* ships here, its
  `ServerConfig` wiring does not), no DoT/DoH (p3-05), no PERFORMANCE.md
  budget rows (p3-06), no config keys, no comments in Rust code (hard rule 7).

## Acceptance criteria (from the task file)

- Round-trip test green (client trusting only exported CA ↔ minted leaf).
- Export paths provably free of private material (asserted, not grepped by
  hand).
- SECURITY.md + ARCHITECTURE.md updates **proposed** and, once approved,
  landed in the same change; ADR-0006 records the placement.
- Gates green.
