# p3-01 — Certificate Core — Review

**Task:** `plan/wip/phase3/p3-01-cert-core.md` · **Plan:** `p3-01-cert-core-plan.md`
· **Status:** implementation complete, review not started.

## Implementation Summary

New L2 crate `fah-certs` owns every certificate operation: the API server pair
(moved from `fah-api`), CA generate/load/archive, leaf minting behind a bounded
LRU, PEM import validation, and public-only export. `fah-api` keeps only
`probe_local_address()` and re-exports the moved entry points, so `main.rs` and
both integration harnesses compile unchanged.

| Area | Where |
| --- | --- |
| API server pair (moved, behaviour unchanged) | `crates/fah-certs/src/api.rs` |
| CA params, generation, load, fingerprint | `crates/fah-certs/src/ca.rs` |
| Leaf mint, LRU cache, `MintingResolver` | `crates/fah-certs/src/leaf.rs` |
| On-disk pairs, archive, `CertStore`, status, export | `crates/fah-certs/src/store.rs` |
| PEM import validation | `crates/fah-certs/src/import.rs` |
| One rejection-named error enum | `crates/fah-certs/src/error.rs` |

## Decisions

- **PFX descoped (owner decision, plan §8 Option B).** PEM-only import. No
  PKCS#12 crate enters the tree, so SECURITY.md's fixed crypto set is unchanged.
- **`MintingResolver` holds `Arc<CertStore>`, not `Arc<LeafCache>`** (plan said
  the cache). The store owns both the CA slot and the cache; resolving through
  it makes CA regeneration atomic for the resolver and removes a second copy of
  the CA handle. `fallback: Option<Arc<CertifiedKey>>` is unchanged — `None` is
  p3-04's fail-closed posture, `Some` is p3-05's no-SNI DoT posture.
- **No `export.rs` module.** `ca_public_pem`/`ca_public_der` are `CertStore`
  methods reading `CaHandle`'s certificate fields; they hold no key path and
  cannot reach one. A six-line module added nothing.
- **Key-match check uses rustls, not rcgen.** `any_supported_type()` +
  `public_key()` SPKI compare accepts PKCS#8, SEC1 and PKCS#1 keys, where
  `rcgen::KeyPair::from_pem` would reject formats a user legitimately has. Same
  approved crypto set, wider real-world input.
- **Leaf validity is `now - 24 h` … `now + 7 d`** (not `not_before + 7 d`), so
  the backdate buys skew tolerance without shortening usable life.

## Measurements

Dev box (x86_64, debug-off criterion, single run — diagnostic only, no budget
row lands until p3-06 applies the ~9× RB5009 factor).

| Bench | Result |
| --- | --- |
| `certs_mint` (P-256 keygen + sign + `CertifiedKey`) | 55.560 µs [55.272, 55.862] |
| `certs_cache_hit` (`CertStore::leaf` on a warm entry) | 62.961 ns [62.717, 63.193] |

| Memory | Figure |
| --- | --- |
| Minted leaf DER | 416 B (asserted < 1 KiB by `a_minted_leaf_is_small_enough_for_the_cache_cap`) |
| Cache cap | 512 entries; DER + `SigningKey` + `Arc`/`HashMap` overhead ⇒ well under the ~1.5 MB GAR §5.11 cap. Exact RSS delta deferred to p3-06 |
| Steady-state RSS | unchanged in p3-01 — nothing wires the cache yet |

## Tests

| Suite | Count | Notes |
| --- | --- | --- |
| `api::tests` | 16 | moved intact from `fah-api/src/tls.rs`; p5-02 SAN assertions unmodified |
| `ca::tests` | 3 | CA-ness, validity span, backdating, fingerprint stability, `Debug` redaction |
| `leaf::tests` | 6 | LRU eviction order, expiry re-mint, stats, IP SAN, DER size |
| `import::tests` | 6 | one per named rejection: `Parse`, `Expired`, `NotYetValid`, `KeyMismatch`, `NotACa` |
| `store::tests` | 8 (+1 unix-only) | reopen stability, archive preserves the old key, regeneration purges leaves, `IncompletePair`, tmp recovery, import marker, 0600 |
| `tests/roundtrip.rs` | 4 | acceptance round-trip, no-SNI fallback vs fail-closed, post-regeneration chaining, export purity |

Gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -D
warnings`, `cargo test --all-features --workspace` — all green.
`crates/fastadhunter/tests/layering.rs` green with `fah-certs` at L2.

The 0600 permission test is `#[cfg(unix)]` and therefore **unverified on this
Windows dev box**; it runs in the container build and on any Linux checkout.

## Files changed

| File | Change |
| --- | --- |
| `crates/fah-certs/**` | new crate (7 source files, 1 integration test, 1 bench) |
| `crates/fah-api/src/tls.rs` | reduced to `probe_local_address()` + its test |
| `crates/fah-api/src/lib.rs` | re-exports point at `fah_certs`; `TlsError` → `CertError` |
| `crates/fah-api/Cargo.toml` | `+fah-certs`; `−rcgen`, `−rustls-pemfile`, `−x509-parser` (dev) |
| `crates/fastadhunter/tests/layering.rs` | `fah-certs` assigned L2 |

## Remaining TODOs

- Doc changes: **landed** (owner-approved after the fix rounds). See §Doc changes
  landed below.
- p3-02 owns the endpoints, the activation policy for an imported API pair, and
  serialization of `CaSummary` / `LeafCacheStats`.
- p3-06 owns the PERFORMANCE.md budget rows and the on-device cache-memory
  figure.

---

## Findings

Independent review. Gates re-run locally on this checkout: `cargo fmt --all --
--check` clean; `cargo clippy -p fah-certs -p fah-api --all-targets -- -D
warnings` clean; `cargo test -p fah-certs` = 39 unit + 4 integration, all pass
(Windows, so the one `#[cfg(unix)]` permission test did not run). Findings are
read from source; each states whether it is verified or inferred.

### Critical

**C1 — an imported CA re-exports its own private key; the export path is not
structurally key-free.**
`crates/fah-certs/src/import.rs:41-76` validates only `first_certificate(cert_pem)`
and then stores the **entire caller-supplied string** as `ImportedPair::cert_pem`.
`store.rs:237-238` passes that string to `CaHandle::load`, which keeps it verbatim
(`ca.rs:65-70`), and `CertStore::ca_public_pem` (`store.rs:285-287`) returns it
verbatim. Nothing rejects extra PEM blocks.

Consequence: a user who imports a combined PEM (`cat ca.pem ca.key > ca.pem`, or a
fullchain+key blob — the most common shape people paste) gets that private key
(a) echoed back by `ca_public_pem()`, which is exactly what p3-02's `GET …/ca.pem`
will serve, and (b) written to `/config/ca-cert.pem`, which `write_pair`
(`store.rs:107-128`) leaves at default permissions — only `key_tmp` gets
`restrict_permissions`. `install_api_pair` (`store.rs:293-297`) has the same shape
for `api-cert.pem`.

This defeats SECURITY.md's binding "export is public-certificate-only" and the
task's acceptance criterion "Grep-proof: no private key bytes in any export path".
The plan (§Step 6) asked for functions "structurally unable" to reach a key.
`ca_public_der()` meets that bar (re-encoded from parsed DER); `ca_public_pem()`
does not — it echoes input.

The purity test (`tests/roundtrip.rs:137-154`) passes only because it exercises the
**generated** CA, where rcgen produces the PEM. The imported path has no test.
Verified by reading; not empirically triggered (that would mean adding code, out of
scope for this review).

Direction: re-encode the PEM from `cert_der` instead of storing the input, and/or
reject a `cert_pem` containing any non-`CERTIFICATE` block in `validate()`. Also
restrict permissions on any file that may carry key material.

**Fix before DONE.**

### Major

**M1 — `install_ca` can leave `/config` with no CA while memory still serves the
old one.** `store.rs:237-257`: `archive_existing_ca` moves the live pair away, then
`write_pair` runs. If `write_pair` fails (ENOSPC, EACCES, a full `/config`), `?`
returns early with the CA slot still holding the **old** handle. The store keeps
minting leaves signed by a CA whose files now exist only under `ca-archive/`, and
the next restart finds no CA at all — memory and disk diverge silently, with no
operator signal beyond the returned error. Direction: stage the new pair before
archiving, or restore on failure. Inferred from control flow; untested.
**Fix before DONE.**

**M2 — `ImportedPair` does not record which validation it passed, so a non-CA can
be installed as the CA.** `import.rs:33-39` returns the same type from
`validate_server_pair` and `validate_ca_pair`; `CertStore::install_ca_pair`
(`store.rs:233-235`) accepts any `ImportedPair`, and `CaHandle::load` does not
re-check `is_ca`. A p3-02 handler that calls `validate_server_pair` then
`install_ca_pair` installs a leaf as the authority; every later mint produces a
chain no client accepts, and the real CA has already been archived (M1's path). The
confusion is live in-tree already: `store.rs:436-437` validates with
`validate_ca_pair` and installs the result as the **API** pair. Direction: a marker
on `ImportedPair` (or two types) checked by `install_ca`. **Fix before DONE** —
p3-02 will lean on this guard.

**M3 — the CA archive can delete a private key.** `store.rs:259-274` names the
archive directory `ca-archive/<unix seconds>`; `create_dir_all` is idempotent and
`fs::rename` replaces an existing destination on both Unix and Windows. Two
regenerations inside the same second (a double-submitted `POST …/ca/generate`,
trivially reachable from a dashboard button) overwrite the first archive's
`ca-key.pem`. That contradicts plan §4 ("never deletes a private key") and the name
of the test meant to cover it —
`regeneration_archives_the_old_pair_and_never_deletes_a_private_key`
(`store.rs:367`) archives once, so the collision is untested. Direction: sub-second
stamp, uniquifying suffix, or refuse to overwrite. Inferred from `fs::rename`
semantics; untested. **Fix before DONE** (cheap).

**M4 — p3-05's declared fallback cannot be built from the surface p3-01 ships.**
`p3-05-dot-doh-listeners-plan.md:194-196` specifies `MintingResolver` with
"fallback = the API pair's `CertifiedKey` so a no-SNI hello still handshakes".
`fah-certs` exposes only `load_or_generate -> Arc<ServerConfig>` (`api.rs:25`), and
`rustls::ServerConfig` offers no way back to the `CertifiedKey`. No accessor exists
(`lib.rs:8-13`). p3-05 would have to re-read and re-parse `api-cert.pem` /
`api-key.pem` inside an L3 sibling — precisely the duplication this crate exists to
prevent (the p2-01 admission test the task file cites). The round-trip test papers
over the gap by using `store.leaf("127.0.0.1")` as the fallback
(`tests/roundtrip.rs:96`), which is not p3-05's case. Direction: return the
`CertifiedKey` (or an `ApiPair` handle) beside the `ServerConfig`, or add
`CertStore::api_certified_key()`. Verified against the p3-05 plan text.
**Fix before DONE, or explicitly re-scope p3-05's fallback.**

**M5 — the shipped seam mints synchronously on a tokio worker, with no pre-warm
entry point.** `MintingResolver::resolve` (`leaf.rs:202-215`) runs inside rustls'
ClientHello processing, i.e. inside `tokio_rustls::accept` on a runtime worker. A
miss runs `mint` (`leaf.rs:151-188`): P-256 keygen plus signature, measured
55.560 µs on this x86 box, so roughly 0.5 ms on the RB5009 at the documented ~9×
factor — blocking that worker. On a 4-core RB5009 sharing a runtime with the DNS
path, a burst of first-sight hosts is a tail-latency hazard for queries, not only
for handshakes. Compounding it: minting deliberately happens outside the cache lock
(plan §7), so N concurrent hellos for the same new host each pay a full mint. The
plan assigns pre-warming to p3-04, but `CertStore` exposes only `leaf()` — there is
no non-blocking or mint-into-cache call for p3-04 to pre-warm with. Direction:
either add a pre-warm entry point here, or record that p3-04 must wrap `leaf()` in
`spawn_blocking` and single-flight it. The µs figure is this task's own bench; the
RB5009 number is the documented conversion, not a device measurement. **Deferring
to p3-04 is acceptable, but the constraint must be written down** — p3-04's plan
currently assumes the seam handles it.

### Minor

**m1 — the key-match check is opt-in and silently skipped when `public_key()`
returns `None`.** `import.rs:66-70` wraps the SPKI comparison in `if let Some(spki)`.
`rustls::crypto::signer::SigningKey::public_key` has a trait default of `None`
("Opt-out by default", rustls 0.23.42 `src/crypto/signer.rs:67`). Verified that all
three aws-lc-rs implementations (`RsaSigningKey`, `EcdsaSigningKey`,
`Ed25519SigningKey`) return `Some`, so this is **not reachable today** — but a
security gate that fails open on an unknown key type is the wrong default.
Direction: `else { return Err(KeyMismatch) }`. Deferral acceptable; the fix is one
line.

**m2 — remote traffic can drive unbounded log volume.** `leaf.rs:208` emits `warn!`
per failed resolve. A listener wired to a store with no CA logs `NoCa` once per
connection, attacker-pacable. Direction: rate-limit, or log once per error class per
interval. Defer to the task that wires a listener, but record it there.

**m3 — no host normalization or length guard at `CertStore::leaf`.** rustls
lowercases SNI and caps DNS names at 253 bytes (verified: `server/hs.rs:726`, plus
`DnsName` validation), so the resolver path is safe. But `leaf()` is public, and
p3-03/p3-04 may call it with a `Host` header or their own parsed SNI: mixed case
then produces duplicate cache entries, and per-entry key size is bounded only by
what the caller passes. The "≤ ~1.5 MB at cap" claim is bounded by *count* and rests
on caller discipline rather than on the type. Direction: `to_ascii_lowercase` plus a
length guard at the `leaf()` boundary.

**m4 — the CA mutex is held across filesystem I/O.** `install_ca` takes `lock_ca()`
at `store.rs:240` and holds it through archive plus `write_pair`; `leaf()` takes the
same mutex per handshake (`store.rs:277`). Every in-flight handshake stalls on disk
for the duration of a CA regeneration. Rare admin path, so impact is low — but p3-02
must also keep `generate_ca` off the async runtime (`spawn_blocking`), which its
plan does not currently say.

**m5 — nothing checks the CA's own validity when minting.** An expired or
not-yet-valid CA still mints (`store.rs:276-279`), and a leaf's `not_after`
(now + 7 d) can outrun the CA's. Clients then fail with an opaque error while status
reports a healthy cache. Direction: refuse to mint outside the CA's window, or
surface it in `CaSummary`.

**m6 — `validate_ca_pair` checks `is_ca` but not `keyCertSign`** (`import.rs:59-61`).
A CA certificate lacking that key usage imports cleanly and then mints leaves strict
validators reject.

**m7 — private key material has a wider surface than it needs.**
`ImportedPair::key_pem()` is `pub` (`import.rs:28`), so any L3 consumer can read the
key; only `CertStore` needs it, and `pub(crate)` suffices. Neither `ImportedPair`
nor `CaHandle` zeroizes on drop, so key bytes linger in freed heap. `Debug` is
correctly redacted on both (verified by the tests at `import.rs:120` and `ca.rs:179`).

**m8 — `write_pair` does not fsync** before or after the renames
(`store.rs:107-128`). Pre-existing behaviour moved verbatim from `fah-api`, so not a
regression; on a router that loses power it can still leave a zero-length or missing
key. Note only.

**m9 — the `certs_mint` bench measures more than the summary claims.**
`benches/certs.rs:14-24` includes the `format!` for the host, the `HashMap` insert,
and — past 512 iterations — the O(512) `evict_one` scan on every later iteration.
The Measurements table calls 55.560 µs "P-256 keygen + sign + `CertifiedKey`". Still
a fine diagnostic, but p3-06 must not treat it as pure mint cost.

**m10 — `LeafCache::with_capacity(0)`** degenerates to a one-entry cache rather than
a disabled one (`leaf.rs:115`: `evict_one` no-ops on an empty map, then the insert
runs). Unreachable today (`LEAF_CACHE_CAPACITY` is a const 512); relevant only if
capacity becomes configurable.

### Nitpick

- `mint` parses `host` as `IpAddr` twice (`leaf.rs:152,157`).
- Two `Generated` structs (`api.rs:20`, `ca.rs:36`) and two `CLOCK_SKEW_HOURS`
  constants with different values (`api.rs:11` = 1, `ca.rs:11` = 24). Both are
  intentional — the API pair keeps its Phase-1 behaviour — but the shared name in
  sibling modules invites a wrong edit.
- Leaf `keyUsage` includes `keyEncipherment` (`leaf.rs:171`), meaningless for an
  ECDSA key; TLS 1.2 ECDHE and TLS 1.3 both need only `digitalSignature`.
- CA uses `BasicConstraints::Unconstrained` (`ca.rs:98`). `Constrained(0)` is free
  hardening: a leaked CA key could then not mint sub-CAs.
- `Debug for LeafCache` / `CertStore` call `stats()`, which locks. Formatting either
  from inside a held lock would deadlock (`std::sync::Mutex` is not reentrant). Not
  reachable today.
- `unix_now()`'s `unwrap_or_default()` (`store.rs:326-331`) yields 0 on a pre-epoch
  clock, which would make every cached leaf look valid forever.

### Plan compliance

| Item | Verdict |
| --- | --- |
| New L2 crate `fah-certs`, no fah-* deps, guard updated | Met — `layering.rs:12` assigns L2; the crate imports no workspace crate at all |
| `fah-api/src/tls.rs` machinery moved, original deleted, tests moved | Met — 15 of 16 moved tests are identical by name; `probe_local_address` and its test stayed |
| ECDSA P-256 via rcgen `aws_lc_rs` | Met |
| CA storage, 0600 key, tmp+rename, archive on regenerate, `warn!` | Met — but see C1 (cert file unrestricted) and M3 (archive collision) |
| CA defaults CN / 3650 d, caller-overridable, no TOML key | Met |
| Leaf 7 d, backdated 24 h, DNS/IP SAN | Met |
| Leaf cache LRU 512, mint outside the lock, stats | Met; see m3/m10 on the bound |
| `generate_ca` purges the cache | Met and tested (`store.rs:385`) |
| `MintingResolver` at L2, fallback slot, no wiring | Met; see M4 on whether the fallback is constructible |
| PFX Option B (descoped) | Met — no PKCS#12 crate entered the tree; SECURITY.md's fixed set unchanged |
| Import rejections, one variant each | Met, all six tested |
| Export public-only | **Not met for the imported path — C1** |
| No new config keys, no endpoints, no comments in Rust | Met (verified: zero `//` or `/*` in `crates/fah-certs`) |
| Benches added and run | Met; see m9 |

**Undocumented deviations**, beyond the four the Decisions section already records
(all four of which are sound):

1. Plan §Step 2 called for a `fah-api::tls::load_or_generate(config_dir,
   bind_address)` wrapper running the probe internally. The implementation instead
   re-exports `fah_certs::load_or_generate` and leaves the probe to the caller
   (`lib.rs:45`, `main.rs:465-468`). Behaviourally identical — the pre-existing
   signature already took `detected: Option<IpAddr>` — and it keeps the library
   pure, so it is an improvement; it is simply not what the plan described.
2. `CertStore` exposes no `LeafCache` handle, while
   `p3-04-tls-interception-plan.md:327` states p3-01 delivers `Arc<LeafCache>`.
   `MintingResolver` covers p3-04's real need, so this wants a correction in p3-04's
   plan text rather than in the code.

**No scope creep and no unnecessary dependencies.** `time`, `tempfile`, `criterion`,
`tokio`, `tokio-rustls` are dev or plan-authorized; `aws-lc-rs`, `rcgen`, `rustls`,
`rustls-pemfile`, `x509-parser`, `thiserror`, `tracing` are the SECURITY.md set plus
two workspace staples. `fah-api` correctly dropped `rcgen`, `rustls-pemfile` and the
`x509-parser` dev-dep; its remaining `aws-lc-rs` edge is still real (`session.rs:7`,
HMAC). One note: `fah-certs` calls `rustls::crypto::aws_lc_rs::…` without declaring
the `aws_lc_rs` feature on its own `rustls` dependency, relying on the workspace
default. That would be a compile error rather than a silent one if it ever changed,
but declaring it is free.

### Tests

Proven: the acceptance round-trip (a client trusting **only** the exported DER
handshakes against a minted leaf; a client trusting nothing fails), the no-SNI
matrix in both postures, post-regeneration chaining, LRU eviction *order* (not just
size), expiry re-mint, and one named rejection per import failure mode. That is
meaningfully more than happy-path coverage.

Gaps, ordered by how much they matter:

1. **Export purity is tested only on the generated CA** (`roundtrip.rs:137`). The
   imported path is untested — which is why C1 survived.
2. **No test installs a non-CA through `install_ca_pair`** (M2).
3. **No test for a failed `write_pair` during regeneration** (M1); the
   archived-CA / empty-`/config` state is unexercised.
4. **No test for two regenerations inside one second** (M3); the test claiming
   "never deletes a private key" archives once.
5. **`MintingResolver`'s mint-failure branch is untested.** The plan asked for "on
   no-SNI **or mint failure** return `fallback`"; only the no-SNI half is covered.
6. **No concurrency test.** The deliberate mint-outside-the-lock race (plan §7, "a
   lost race re-inserts an identical leaf, harmless") is asserted, not demonstrated.
7. `a_leaf_minted_after_regeneration_chains_to_the_new_authority_only`
   (`roundtrip.rs:111`) never mints before regenerating, so despite its name it does
   not exercise the purge — only that a fresh mint uses the current slot. The purge
   is covered by the store unit test, so coverage is not lost, but the integration
   test overclaims.
8. Server-side handshake errors are swallowed (`roundtrip.rs:43`, `let _ =`), so a
   client-side `expect_err` cannot distinguish "resolver returned `None`" from a
   transport failure. Adequate here, fragile if reused.

Nothing looks flaky: no sleeps, no wall-clock races beyond `now()` values compared
with day-scale margins, ephemeral ports throughout.

### Windows-vs-Unix classification

Correct as far as it goes: the 0600 test is `#[cfg(unix)]` and the summary says
plainly it did not run here. Two things it does not say:

- `restrict_permissions` is a **no-op** on non-unix (`store.rs:172-175`), so on this
  dev box key files carry inherited ACLs. Fine for a dev box — but it means the
  whole permission story, including C1's unrestricted cert file, has **zero** local
  coverage, not just one skipped assertion.
- M1's failure path and M3's archive collision are platform-independent and untested
  everywhere; they are not Windows gaps.

`fs::rename` replaces an existing destination on both platforms, so the tmp+rename
pattern itself is portable. No finding there.

### Verdict (initial review)

**BLOCKED.**

C1 defeats an explicit acceptance criterion ("no private key bytes in any export
path") and a binding SECURITY.md promise, on the exact code path p3-02 is about to
expose over HTTP. M2 and M4 are contract defects that p3-02 and p3-05 would build
on. M1 and M3 are cheap fixes to state-integrity bugs in the admin path.

Everything else — layering, crate placement, the mechanical move, the LRU, the
import rejection taxonomy, the round-trip proof, the PFX descope — is sound and
needs no rework.

---

## Fixes applied

Owner-approved scope: C1, M1, M2, M3, M4 only. Minor and nitpick findings were
left open except where a fix required touching them (m7, noted below).

| Finding | Status |
| --- | --- |
| C1 imported CA re-exports its private key | Fixed |
| M1 install_ca can leave `/config` without a CA | Fixed |
| M2 `ImportedPair` conflates CA and server validation | Fixed |
| M3 archive collision destroys an archived key | Fixed |
| M4 API `CertifiedKey` not reachable for p3-05 | Fixed |
| m1–m10, nitpicks | Open, unchanged |

### C1 — exports are rebuilt from parsed DER, never echoed

`CaHandle::load` now derives its PEM from `cert_der` via a new
`store::certificate_pem` (`store.rs`), so `ca_public_pem()` returns re-encoded
certificate blocks whatever the input was — including a CA already on disk from a
pre-fix import. `import::validate` parses the input with a new
`store::all_certificates`, validates the first certificate, and rebuilds
`cert_pem` from the parsed DER: a `CA` import keeps only the root, a server import
keeps the whole chain, and any `PRIVATE KEY` block in the certificate field is
dropped before anything is written or exported. `install_ca` writes
`handle.cert_pem()`, so `/config/ca-cert.pem` is certificate-only too.

New dependency `pem = "3"` (encoding only, not crypto — already compiled through
rcgen's `pem` feature; SECURITY.md's fixed crypto set is untouched). Output is
pinned to `LineEnding::LF` so it does not vary by build platform the way rcgen's
does.

Regression tests: `ca::a_loaded_authority_re_encodes_its_certificate_instead_of_echoing_the_input`,
`import::private_material_pasted_into_the_certificate_field_is_dropped`,
`import::a_server_chain_keeps_every_certificate_but_a_ca_keeps_only_the_root`,
`store::an_imported_authority_never_writes_or_exports_private_material`,
`store::an_api_pair_carrying_pasted_key_material_is_written_certificate_only`,
`roundtrip::an_imported_authority_exports_no_private_material`. All feed a
`cert || key` blob through the real import path and assert on both the export and
the on-disk file.

### M1 — nothing destructive happens before the replacement is staged

`write_pair` split into `stage_pair` (write both tmp files, 0600 on the key) and
`commit_pair` (the two renames); `write_pair` is now their composition, so the
`fah-api` pair path is byte-for-byte the old behaviour. `install_ca` reordered to
**stage → archive → commit**, and the archive became a **copy** rather than a
rename, so the live pair survives an archive failure as well. A `commit_pair`
failure calls the new `restore_ca`, which copies the archived pair back.

Consequence: whatever fails, the live pair on disk and the in-memory CA slot stay
in agreement — the slot is still only assigned after a successful commit.

Regression test: `store::a_failed_regeneration_leaves_the_live_authority_untouched`
puts a directory where `ca-cert.pem.tmp` must be written, so staging fails; it then
asserts the summary, both live files, the absence of any archive directory, the
cleanup of the staged key, and that a reopen returns the original CA. This fails on
the pre-fix code, which archived before writing.

### M2 — CA and server pairs are now distinct types

`ImportedPair` is gone from the public API. `validate_server_pair` returns
`ValidatedServerPair`, `validate_ca_pair` returns `ValidatedCaPair`; both wrap one
private `Material` struct so the validator itself is not duplicated.
`install_ca_pair` takes `&ValidatedCaPair` and `install_api_pair` takes
`&ValidatedServerPair`, so passing a server-validated pair to the CA installer is a
compile error.

Defence in depth for pairs that do not come through the validator (a hand-edited
`/config`): `ca::summarize` now rejects a non-CA certificate, so `CaHandle::load` —
and therefore `CertStore::open` and every install path — returns `NotACa`.

The store test that previously validated a pair as a CA and installed it as the API
pair now uses `validate_server_pair`, which is what it always meant.

Regression tests: `ca::a_certificate_that_is_not_an_authority_cannot_be_loaded_as_one`,
`store::an_authority_on_disk_that_is_not_a_ca_is_refused_at_open`. The type-level
half is enforced by the compiler.

While rewriting these types, `key_pem()` on both became `pub(crate)` — no consumer
outside the store needs private key material, and leaving it `pub` would have
undercut C1. That is finding m7's first half, fixed because the fix required
redesigning the type anyway; the missing zeroization half stays open.

### M3 — archive directories are claimed atomically

`archive_existing_ca` now calls `unused_archive_dir`, which walks
`ca-archive/<stamp>`, `<stamp>-1`, `<stamp>-2`, … using `fs::create_dir` (which
fails with `AlreadyExists` rather than silently reusing, unlike `create_dir_all`)
and returns the first it creates, bounded at 1024 attempts per second. The archived
key is explicitly re-restricted to 0600 rather than relying on `fs::copy`'s
permission propagation.

Regression test: `store::repeated_regenerations_within_one_second_each_get_their_own_archive`
regenerates four times in a tight loop, then asserts three archive directories
holding three **distinct** keys, each matching a replaced live key. On the pre-fix
code all four land on the same second and collapse into one directory.

### M4 — the API `CertifiedKey` is reachable without leaving `fah-certs`

`api::parse_pair` factored out of `server_config`, and a new
`api::certified_key(config_dir)` builds an `Arc<CertifiedKey>` from the API pair on
disk, exposed as `CertStore::api_certified_key()`. p3-05 can now construct its
declared no-SNI fallback with one call and no PEM parsing of its own. Missing pair
returns `CertError::Empty`, not a panic.

`load_or_generate`'s signature is unchanged, so `main.rs` and both integration
harnesses are untouched.

Regression tests: `store::the_api_certified_key_is_the_pair_on_disk` (including the
no-pair error) and
`roundtrip::the_api_pair_serves_a_no_sni_hello_as_the_resolver_fallback`, which
builds a `MintingResolver` with that key as fallback and completes a real no-SNI
handshake against it — the exact shape p3-05 declared.

### Verification

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | green, every crate |
| `cargo test -p fah-certs` | 49 unit (was 39) + 6 integration (was 4) |

One flake seen and cleared: the first workspace run failed
`fah-dns upstream::tests::adaptive::every_endpoint_penalized_still_sends_a_packet_and_claims_no_probe`.
`fah-dns` has no `fah-certs` edge (`fah-common`, `fah-config`, `fah-model`,
`fah-rules` only), and four subsequent runs of that suite passed 200/200. Pre-existing
flake, unrelated to this change; worth its own look, not tracked here.

Benches re-run (mint and cache-hit paths were not touched): `certs_mint`
53.416 µs, `certs_cache_hit` 63.909 ns. Criterion reports −3.5% and +1.3% against
its stored baseline; per `docs/measurement-traps.md` that comparison is not a valid
A/B, and both figures are dev-box run-to-run noise. The Measurements table above
keeps the original numbers as the recorded diagnostic.

### Files changed by the fixes

| File | Change |
| --- | --- |
| `crates/fah-certs/Cargo.toml` | `+pem` (encoding only) |
| `crates/fah-certs/src/store.rs` | `all_certificates` / `certificate_pem`; `stage_pair` + `commit_pair` split; `copy`; copy-based collision-proof archive; `restore_ca`; `api_certified_key`; validated-pair signatures |
| `crates/fah-certs/src/ca.rs` | PEM re-encoded from DER in `CaHandle::load`; `summarize` rejects a non-CA |
| `crates/fah-certs/src/import.rs` | `ValidatedServerPair` / `ValidatedCaPair` replace `ImportedPair`; certificate-only reconstruction; `key_pem()` `pub(crate)` |
| `crates/fah-certs/src/api.rs` | `parse_pair` factored out; `certified_key` added |
| `crates/fah-certs/src/lib.rs` | exports the two validated pair types |
| `crates/fah-certs/src/leaf.rs` | test helper follows `CaHandle::load(&str, …)` |
| `crates/fah-certs/tests/roundtrip.rs` | two new tests (M4 fallback, C1 imported export) |

No file outside `crates/fah-certs` changed. `fah-api`, `fastadhunter` and both
integration harnesses compile untouched.

### Verdict after the first fix round

**PASS WITH DEFERRED FINDINGS.**

C1, M1, M2, M3 and M4 are fixed and each carries a regression test that fails on the
pre-fix code. Deferred at that point: m1–m10 and the nitpicks.

---

## Second fix round — minor findings

Owner-approved scope: m1, m3, m5, m6, m9, n4. Taken now because all six live inside
`fah-certs` while it still has no consumers; m3 in particular changes a contract
that p3-03/p3-04/p3-05 would otherwise have to be corrected against.

| Finding | Status |
| --- | --- |
| m1 key-match check fails open on `public_key() == None` | Fixed |
| m3 no host normalization or length guard at `leaf()` | Fixed |
| m5 no CA-validity check on mint; leaf can outlive the CA | Fixed |
| m6 `validate_ca_pair` ignores `keyCertSign` | Fixed |
| m9 `certs_mint` bench mislabelled | Fixed |
| n4 CA `BasicConstraints::Unconstrained` | Fixed |
| m2, m4, m7, m8, m10, remaining nitpicks | Open, unchanged |

### m1 — the key-match check can no longer be skipped

`import::validate` now uses `let Some(spki) = signing_key.public_key() else { return
Err(KeyMismatch) }`. A key type that declines to publish its SPKI is rejected rather
than waved through.

Regression tests: `import::the_key_match_check_runs_for_every_key_algorithm_the_validator_accepts`
(P-256, P-384, Ed25519) and `import::an_rsa_pair_is_key_matched_rather_than_waved_through`
(RSA-2048). Each asserts both halves — a matching pair is accepted, a mismatched one
is `KeyMismatch` — so a future rustls or aws-lc-rs change that stops exposing an SPKI
for any accepted algorithm fails the suite instead of silently disabling the check.

### m3 — hosts are normalized and bounded at the `CertStore::leaf` boundary

New `CertError::InvalidHost`. `leaf()` rejects an empty host and anything over 253
bytes (the DNS name limit rustls already enforces on SNI), then lowercases via
`Cow`: borrowed when the host is already lowercase — the common case, since rustls
lowercases SNI itself — owned only when it is not. The cache is therefore keyed
consistently no matter which consumer calls it, and per-entry key size is now
bounded by the type rather than by caller discipline.

Regression test: `store::a_host_is_matched_case_insensitively_and_bounded_in_length`
asserts `"A.Example"` and `"a.example"` return the same `Arc` with one mint and one
cache entry, that empty and 254-byte hosts are `InvalidHost`, and that exactly 253
bytes is accepted.

### m5 — an out-of-window CA mints nothing, and leaves never outlive it

`CertStore::leaf` compares the wall clock against the loaded CA's validity window
and returns `NotYetValid` / `Expired` before touching the cache. `leaf::mint` clamps
`not_after` to `min(now + 7 d, ca.not_after)`.

Regression test: `store::an_expired_authority_mints_nothing_and_a_short_lived_one_clamps_its_leaves`
uses a CA generated with `validity_days: 0` (expired on arrival, since the CA is
backdated 24 h) and asserts `Expired` with `minted_total == 0`, then a
`validity_days: 2` CA and asserts the minted leaf's `not_after` equals the CA's
exactly.

### m6 — a CA that may not sign certificates is refused

`validate_ca_pair` now goes through `signs_certificates`: `is_ca` **and** — when a
`keyUsage` extension is present — `keyCertSign`. An absent extension is accepted,
which is what RFC 5280 §4.2.1.3 means by an unrestricted key; only an extension that
is present and withholds `keyCertSign` is rejected, as `NotACa`.

Regression test: `import::an_authority_that_may_not_sign_certificates_is_rejected`
covers both directions — `CrlSign`-only is rejected, no `keyUsage` at all is
accepted.

### m9 — the mint bench no longer times string formatting

`certs_mint` pre-generates 4096 host strings outside the timed loop. What it
measures is the **cold `CertStore::leaf` path** — keygen, signing, `CertifiedKey`
construction, cache insert and, at capacity, the eviction scan — which is the cost
p3-04 actually pays per first-sight host. p3-06 should label the budget row that
way rather than as raw mint cost. No test; the deliverable is the corrected bench
and this correction to the Measurements section above.

### n4 — the CA cannot issue intermediate authorities

`BasicConstraints::Unconstrained` → `Constrained(0)`. A leaked CA key can still
forge leaves (unavoidable for a MITM root) but cannot mint a sub-CA.

Regression test: `store::the_generated_authority_may_not_issue_intermediate_authorities`
parses the exported DER and asserts `ca == true` with `path_len_constraint == Some(0)`.

### Verification (second round)

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | green, every crate, no flake this run |
| `cargo test -p fah-certs` | 55 unit (was 49) + 6 integration |

Benches, with the honest caveat that criterion's stored baseline is not a valid A/B
(`docs/measurement-traps.md`):

| Bench | Before | After | Note |
| --- | --- | --- | --- |
| `certs_mint` | 53.4 µs | 55.5 µs | mint gained one `from_unix_timestamp` + `min`; also no longer times `format!` |
| `certs_cache_hit` | 63.9 ns | 65.2 ns, then 66.6 ns | see below |

The cache-hit path gained a bounded ASCII scan over the host (≤ 253 bytes, no
allocation when already lowercase) and two integer comparisons against the CA
window. A second bench run of the **same unchanged binary** moved 65.2 → 66.6 ns
(+2.6%), i.e. the observed deltas sit inside this box's run-to-run drift. **No
attributable regression was measured**; the added work is a handful of nanoseconds
by inspection, and this is the per-handshake path, not the DNS hot path. A real A/B
against a pre-change checkout belongs to p3-06 if the number ever matters.

### Files changed (second round)

| File | Change |
| --- | --- |
| `crates/fah-certs/src/error.rs` | `+InvalidHost` |
| `crates/fah-certs/src/import.rs` | `let … else` on the SPKI check; `signs_certificates`; four new tests and their key-generation helpers |
| `crates/fah-certs/src/store.rs` | host guard + `Cow` lowercase and CA-window check in `leaf()`; `MAX_HOST_LEN`; three new tests |
| `crates/fah-certs/src/ca.rs` | `BasicConstraints::Constrained(0)` |
| `crates/fah-certs/src/leaf.rs` | leaf `not_after` clamped to the CA's |
| `crates/fah-certs/benches/certs.rs` | hosts pre-generated outside the timed loop |

Still no file outside `crates/fah-certs`. Public surface additions:
`CertError::InvalidHost` only.

## Doc changes landed

Owner approved after the second fix round. The task's acceptance criterion
("SECURITY.md + ARCHITECTURE.md updated for crate placement") is now met.

| File | Change |
| --- | --- |
| `docs/decisions/0006-certificate-machinery-home.md` | **New.** Placement (L2 `fah-certs`, the p2-01 admission test, why not `fah-common` or L1), the PEM-only/PFX-descope decision and its reasoning, the structural export guarantee, revisit criteria for both decisions |
| `ARCHITECTURE.md` | `fah-certs` in §Workspace Layout; added to L2 in §Dependency Layering with a paragraph on why it is L2 and what it owns, pointing at ADR-0006 |
| `SECURITY.md` | §TLS for the API: user-supplied certificate is **PEM**, PFX explicitly not accepted with the `openssl pkcs12` conversion command; new bullet on import re-encoding from parsed DER. §Later phases: export is re-encoded from certificate DER, machinery lives in `fah-certs`. Guiding rule: notes `pem` is base64 framing, not a widening of the fixed crypto set |
| `API.md` | §Certificates: dropped "import PFX" from the reserved namespace, added the descope note and conversion command, pointed the endpoint spec at p3-02 |
| `plan/wip/phase3/p3-01-cert-core.md` | §Scope: PFX line replaced with the settled Option B decision |
| `CLAUDE.md` | crate count 11 → 12; new reading-protocol row `fah-certs` → SECURITY.md + ADR-0006 |

`CLAUDE.md` was not on the plan's proposed list. It was edited because this task
made two of its statements stale — the crate count, and a reading-protocol table
that would otherwise send an agent working on `fah-certs` to no document at all.
Both are one-line factual corrections; revert if you disagree.

Gates after the doc edits: `cargo fmt --all -- --check` clean, `cargo clippy
--workspace --all-targets -- -D warnings` clean, `cargo test --all-features
--workspace` green (47 suites, zero failures).

## Third fix round — M5 (pre-warm split)

Owner-approved spec, implemented as proposed. `resolve()` never mints; there is no
inline-mint escape hatch; no separate in-flight cap.

### What changed

`rustls::server::ResolvesServerCert::resolve` is synchronous, so any mint inside it
blocks a tokio worker — roughly 0.5 ms on the RB5009 per first-sight host. The fix
is to take minting off the handshake path entirely rather than to make it faster.

| Entry point | Blocking? | Mints? |
| --- | --- | --- |
| `CertStore::prewarm(host)` | yes — call from `spawn_blocking` | yes, single-flighted |
| `CertStore::cached_leaf(host)` | no | never |

`CertStore::leaf` is deleted. `MintingResolver::resolve` is now
`cached_leaf(host).or_else(|| fallback.clone())`, so `fallback: None` still means
fail-closed and `Some` still serves the no-SNI DoT case — both semantics unchanged.

Single-flight uses `std::sync::Condvar` only, so `fah-certs` stays tokio-free and
ADR-0006's purity claim holds. A leader inserts the host into an `inflight` set,
drops the lock, mints, inserts the entry, then releases the slot and
`notify_all`s. Waiters re-check under the lock and take the leader's leaf.
`notify_all` rather than `notify_one` because waiters for different hosts share one
condvar — waking one could wake the wrong host and strand the right one.

Two things fell out for free:

- **m2 is closed.** The per-handshake `warn!` on a failed resolve is gone; an
  unwarmed miss is now a counter (`unwarmed_misses`), not a remote-pacable log line.
- **m4's blast radius shrank.** `cached_leaf` does not touch the CA mutex, so
  handshakes no longer contend with `generate_ca` at all — only `prewarm` does.

### Deviations from the spec

Four, all reported rather than silently absorbed:

1. **`InflightGuard` added** (not specified). A panic inside `mint` while holding an
   in-flight slot would strand every waiter for that host forever. A `Drop` guard
   releases the slot and notifies on any exit path — success, `?`, or unwind.
2. **`certs_prewarm_coalesced` bench replaced by `certs_prewarm_warm`.** Benchmarking
   the waiter path under criterion measures thread scheduling, not this code.
   `certs_prewarm_warm` measures the repeat-`prewarm` path, which is what p3-04
   actually pays on every connection after the first to a known host.
3. **Spec test 10 is deterministic instead of concurrent.** "Force a mint failure
   under contention" has no reliable trigger — `NoCa`, `Expired` and `InvalidHost`
   are all rejected before the cache is touched. The test instead uses a host rcgen
   rejects (`ü.example`: passes the length guard, fails `Ia5String`), asserts
   `inflight == 0` after the failure, and asserts a retry fails again rather than
   blocking on a stale marker — which is the invariant that mattered.
4. **`misses` removed rather than kept alongside `unwarmed_misses`.** Under the split
   they would have been the same number. The counter set is now `hits`,
   `unwarmed_misses` (both from `cached_leaf`), `prewarm_hits`, `coalesced`,
   `minted_total`, `evictions`, plus `size` / `capacity` / `inflight` gauges.

### Tests

63 → 64 unit, 6 → 8 integration in `fah-certs`. New coverage:

| Proves | Test |
| --- | --- |
| pre-warm then read; a read alone mints nothing | `leaf::a_prewarmed_host_is_then_served_without_minting`, `store::a_cache_read_never_mints_and_only_a_prewarm_does` |
| single-flight collapses concurrent mints | `leaf::concurrent_prewarms_of_one_host_mint_exactly_once` (16 threads on a `Barrier` ⇒ `minted_total == 1`) |
| distinct hosts do not serialize or deadlock | `leaf::concurrent_prewarms_of_distinct_hosts_all_complete` |
| mixed prewarm/read concurrency | `store::concurrent_prewarm_and_cache_reads_stay_consistent` |
| a warm host is not re-minted | `leaf::a_second_prewarm_of_a_warm_host_does_not_mint_again` |
| failure releases the in-flight slot | `store::a_failed_mint_releases_the_host_instead_of_stranding_waiters` |
| resolve never mints, fail-closed | `roundtrip::an_unwarmed_host_is_refused_rather_than_minted_during_the_handshake` (`minted_total == 0` after an aborted handshake) |
| resolve never mints, fallback posture | `roundtrip::an_unwarmed_host_falls_back_when_a_fallback_is_set` |
| 512 cap holds | `store::the_cache_cap_holds_across_many_prewarms` (600 hosts ⇒ size 512, evictions 88) |
| expired entry is neither served nor reused | `leaf::an_expired_entry_is_neither_served_nor_reused` |
| regeneration purge, via the cache read | `store::a_prewarmed_entry_is_dropped_when_the_authority_is_replaced` |
| case-insensitive on both entry points | `store::a_host_is_matched_case_insensitively_and_bounded_in_length` |

No sleeps and no timing thresholds anywhere — the concurrency tests assert on
counters and `Barrier` ordering only.

`roundtrip::a_leaf_minted_after_regeneration_chains_to_the_new_authority_only` now
pre-warms and connects **before** regenerating, so it finally exercises the purge it
is named for — closing nitpick n6 from the initial review.

### Verification (third round)

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | green — 47 suites, zero failures, no flake this run |
| `cargo test -p fah-certs` | 64 unit + 8 integration |

| Bench | Before | After | Read |
| --- | --- | --- | --- |
| `certs_cache_hit` (`cached_leaf`) | 65.2 ns | **48.6 ns** (−27.6%) | attributable: the per-handshake path no longer clones the CA `Arc` under its mutex. Well outside the ±3% run-to-run drift measured in round two |
| `certs_mint` (`prewarm`, cold) | 54.9 µs | 54.9 µs (p = 0.42) | unchanged |
| `certs_prewarm_warm` | — | 69.0 ns | new: repeat pre-warm of a known host |

The headline is architectural, not the nanoseconds: **zero keygen or signing work
now runs on a tokio worker**, and a burst of N connections to one new host costs one
mint instead of N.

### Plan notes updated

Only the approved plan files were touched; no other `.md`.

| File | Change |
| --- | --- |
| `p3-04-tls-interception-plan.md` | §6 rewritten: `resolve()` never mints; the `peek → spawn_blocking(prewarm) → TlsAcceptor` sequence; single-flight is p3-01's, so p3-04 adds no minting/caching/coalescing of its own; `unwarmed_misses` makes a forgotten pre-warm observable; `inflight` is bounded by p3-04's connection cap, which is why `fah-certs` has none. Step 6 corrected — `LeafCache` is private, `Arc<CertStore>` is the handle |
| `p3-05-dot-doh-listeners-plan.md` | fallback sourced from `CertStore::api_certified_key()`; new bullet requiring a startup `spawn_blocking(prewarm(dot_hostname))` and naming `unwarmed_misses` as the signal if it is skipped |

### Verdict after all three fix rounds

**PASS WITH DEFERRED FINDINGS.**

Fixed: C1, M1, M2, M3, M4, M5, m1, m2, m3, m5, m6, m9, n4 and n6 — each with a
regression test that fails on the pre-fix code, except m9 (a bench label) and M2's
type-level half (which the compiler enforces).

Still open, all deliberately:

| # | Issue | Why it stays |
| --- | --- | --- |
| m4 | `generate_ca` holds the CA mutex across disk I/O | p3-02 calls it from `spawn_blocking`; no `fah-certs` change. Blast radius already reduced — handshakes no longer take this mutex |
| m7 | no zeroization of key material on drop | needs a dependency outside SECURITY.md's fixed set — owner decision |
| m8 | `write_pair` does not fsync | pre-existing, also touches the API pair path; wants its own change and test |
| m10 | `LeafCache::with_capacity(0)` degenerates to one entry | unreachable — capacity is a const 512 |
| nitpicks | double `IpAddr` parse; duplicate `Generated` / `CLOCK_SKEW_HOURS`; leaf `keyEncipherment` on an EC key; `Debug`-calls-`stats()` deadlock shape; `unix_now()` → 0 pre-epoch | none affects behaviour today |

No open finding now constrains another p3 task. Doc changes landed (§Doc changes
landed), so every acceptance criterion in the task file is met.

---

## Final independent review

Fresh review of the current checkout; previous verdicts and the implementation
report were not taken on trust. Gates re-run here: `cargo fmt --all -- --check`
clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`cargo test --all-features --workspace` green (47 suites, 0 failures);
`fah-certs` = 64 unit + 8 integration. Windows box, so the one `#[cfg(unix)]`
permission test did not run.

Re-verified as genuinely fixed, by reading the code and not the report: C1
(both `ca_public_pem` and `install_ca` go through `certificate_pem`, rebuilt
from parsed DER), M1 (stage → archive-by-copy → commit → `restore_ca`), M2
(`ValidatedCaPair`/`ValidatedServerPair` are distinct types; `ca::summarize`
rejects a non-CA on every load path), M3 (`fs::create_dir` claim loop), M4
(`api_certified_key`), M5 (`resolve()` is `cached_leaf().or_else(fallback)` —
no crypto, no CA mutex, no disk; single-flight via `Condvar` with a `Drop`
guard, correct re-check-under-lock, no lost wakeup, mint outside the lock).

### Findings

#### H1 — a CA regeneration racing an in-flight pre-warm re-poisons the cache

`store.rs::install_ca` / `leaf.rs::LeafCache::prewarm`.

`CertStore::prewarm` clones `Arc<CaHandle>` under `lock_ca()`, releases the
lock, then mints. `install_ca` assigns the new slot, drops the lock, and calls
`leaves.clear()`. A pre-warm that captured the **old** handle before
`install_ca` took the lock inserts its leaf *after* `clear()` — nothing between
mint and insert re-checks which authority is current, and `clear()` does not
touch `inflight`.

Consequence: a leaf signed by the archived CA is served for up to 7 days (or
until eviction/restart) to clients that already trust the new root — exactly
the failure the purge exists to prevent, and which
`store::a_prewarmed_entry_is_dropped_when_the_authority_is_replaced` claims to
cover. It does not: that test is sequential.

Window is one mint (~55 µs dev box, ~0.5 ms RB5009) and needs a concurrent
regeneration, so probability is low — but p3-02 puts `generate_ca` behind a
dashboard button and p3-04 pre-warms per connection.

Verified by construction from source; not empirically triggered (a deterministic
test needs an injection point that does not exist).

Fix: an epoch on `Inner`, bumped by `clear()`; `prewarm` captures it when it
becomes leader and skips the insert if it changed. `cached_leaf` then misses and
p3-04 fails closed / re-warms.

#### H2 — nothing checks that the CA on disk matches its private key

`ca.rs::CaHandle::load`.

`rcgen::Issuer::from_ca_cert_der` does **no** key/certificate consistency check
(verified in rcgen 0.14 source: *"It will not check for the presence of the
BasicConstraints extension, or perform any other validation"*). `CaHandle::load`
adds none. The import path does compare SPKI (`import.rs::validate`), but
`CertStore::open` does not go through it.

Consequence: a `/config` holding `ca-cert.pem` + a non-matching `ca-key.pem`
opens clean, `status()` reports a healthy CA with a valid fingerprint and
window, and **every minted leaf carries a signature no client can verify**.
Total, silent interception failure with no operator signal. The API pair is
protected here — `with_single_cert` runs rustls' `keys_match` — so the CA path
is the only unguarded one.

Reachable without hand-editing: `commit_pair` renames cert then key. A crash
between the two during a *regeneration* leaves cert = new, key = old (both
exist, so `load_pair`'s recovery branch is skipped and the new `key_tmp` is
**discarded**). The new private key is destroyed and the store silently loads a
mismatched pair.

Fix: reuse the SPKI comparison from `import::validate` inside `CaHandle::load`
(→ `KeyMismatch`). Optionally commit the key rename first so the existing
"cert present, key missing, key_tmp present" recovery covers the replace case.

#### M1 — `api_certified_key` skips the consistency check `server_config` performs

`api.rs::certified_key` builds `CertifiedKey::new(certs, signing_key)`;
`api.rs::server_config` builds through `with_single_cert`, which calls
`CertifiedKey::from_der` → `keys_match`. The p3-05 fallback seam therefore
accepts an on-disk API pair that the API listener refuses to start with. p3-05's
DoT clients would fail every no-SNI handshake at signature verification.

Fix: `CertifiedKey::from_der(certs, key, provider)` in `certified_key` — same
crate set, one call, and it drops the manual `any_supported_type`.

#### M2 — `p3-04-tls-interception-plan.md` still contradicts the shipped API

The third fix round corrected Step 6 (line 351) but left four other references
to a type that is private and a method that no longer exists:

| Line | Stale text |
| --- | --- |
| 17 | `LeafCache::get_or_mint(host) -> Arc<CertifiedKey>` named as the consumed seam |
| 314 | "The leaf comes from `fah_certs::LeafCache`" |
| 319 | `TlsProxy` field `Arc<LeafCache>` |
| 489 | main.rs "build `LeafCache`/configs/`ExclusionSet`" |

p3-04 reads §TASK START before Step 6, so it hits the wrong contract first.
Fix: replace all four with `Arc<CertStore>` + `prewarm`/`cached_leaf`.
`p3-05-dot-doh-listeners-plan.md` was checked line by line and **is** consistent
with the shipped surface.

#### M3 — p3-05's single startup pre-warm expires after 7 days

`p3-05-dot-doh-listeners-plan.md:203` warms the DoT hostname once at startup.
`LEAF_VALIDITY_DAYS = 7` and `cached_leaf` treats an expired entry as a miss, so
after a week of uptime the DoT listener silently degrades to the API-pair
fallback — which is precisely what Android Private DNS hostname mode rejects
(p3-05 decision 3). `fah-certs` exposes no refresh hook and no expiry signal
beyond `unwarmed_misses`.

Fix (p3-05, not p3-01): a periodic `spawn_blocking(prewarm(hostname))` well
inside 7 days, or re-warm on `unwarmed_misses` increase.

#### L1 — the private key file is briefly world-readable

`store.rs::stage_pair` writes `key_tmp` with `fs::write` (umask-dependent, 0644
under the container's default umask), then chmods 0600. Use
`OpenOptions::new().mode(0o600)` on unix so the file never exists unrestricted.
Pre-existing pattern moved from `fah-api`, not a regression.

#### L2 — a half-failed `commit_pair` can block boot on the API pair path

If the cert rename succeeds and the key rename fails, `commit_pair` deletes the
just-committed cert; the old cert is already gone. Result: cert missing, key
present → `IncompletePair` at next open. The CA path recovers via `restore_ca`
when an archive exists; `install_api_pair` and `load_or_generate` have no
restore. Same-directory rename failure is very unlikely; noted, not urgent.

#### L3 — an invalid host is invisible in the stats

`CertStore::cached_leaf` returns `None` on `normalize` failure without touching
any counter, so a systematically malformed SNI reads as zero traffic rather than
as `unwarmed_misses`.

#### L4 — expired entries occupy capacity and inflate `stats().size`

`take_fresh` leaves a stale entry in the map; only a re-`prewarm` or eviction
removes it. Bound is still 512, but `size` overstates live occupancy in the
status p3-02 serializes.

#### L5 — `fah-certs` uses `rustls::crypto::aws_lc_rs` without declaring the feature

Carried unchanged from the initial review. Relies on the workspace default.
Free to declare.

#### L6 — no negative caching for a host rcgen rejects

A host that passes the 253-byte guard but fails `Ia5String` re-attempts on every
connection. `CertificateParams::new` fails **before** keygen, so the cost is
small — but it is per-connection work driven by remote input.

#### Still open from earlier rounds

m4 (CA mutex held across disk I/O), m7 (no zeroization), m8 (no fsync), m10
(`with_capacity(0)`), and the nitpicks (double `IpAddr` parse; duplicate
`Generated` / `CLOCK_SKEW_HOURS`; leaf `keyEncipherment` on an EC key;
`Debug`-calls-`stats()` deadlock shape; `unix_now()` → 0 pre-epoch). All
re-checked; none newly reachable.

### Tests — do they prove the claims?

| Claim | Proven? |
| --- | --- |
| Client trusting only the exported CA verifies a minted leaf; trusting nothing fails | Yes — `roundtrip.rs:63` |
| `resolve()` never mints | Yes — `minted_total == 0` after an aborted handshake, `roundtrip.rs:154` |
| Single-flight collapses concurrent mints | Yes — 16 threads on a `Barrier`, `minted_total == 1`, all `Arc::ptr_eq` |
| Failure releases the in-flight slot | Yes — deterministic, asserts `inflight == 0` and that a retry re-fails |
| Export is key-free on generated **and** imported paths | Yes — both covered since C1 |
| Type-level CA/server pair separation | Yes — compiler-enforced |
| Cap holds (600 hosts ⇒ 512 + 88 evictions) | Yes |
| **Regeneration purge under concurrency** | **No — H1**; the purge test is sequential |
| **CA key⇄cert consistency** | **No — H2**; no test, no check |
| API `CertifiedKey` consistency | No — M1 |
| 0600 permissions | Unrun on this box (`#[cfg(unix)]`); `restrict_permissions` is a no-op on Windows, so the permission story has zero local coverage |

No sleeps, no timing thresholds, no wall-clock races. Nothing looks flaky.

### Plan compliance

Every row from the earlier compliance table re-verified as still met, plus the
items the fix rounds added. Layering confirmed independently: `fah-certs`
declares no `fah-*` dependency and `layering.rs:12` assigns L2. Zero Rust
comments in the crate. No scope creep; `pem` is the only added edge and is
encoding, not crypto. Docs landed (ADR-0006, ARCHITECTURE.md, SECURITY.md,
API.md) and their text matches the code — checked, not assumed.

Undocumented deviations: none beyond the two already recorded. The `fah-api`
wrapper deviation and the `LeafCache`-vs-`CertStore` handle deviation both
stand; the second is what M2 asks to correct in p3-04's plan text.

### Carried into later tasks

| To | Item |
| --- | --- |
| p3-02 | H1 becomes reachable the moment `POST …/ca/generate` ships — fix it first or gate the endpoint |
| p3-02 | `generate_ca`, `install_ca_pair`, `install_api_pair`, `api_certified_key` and `status()` (it reads `api-cert.source` from disk) all block — every one needs `spawn_blocking` |
| p3-02 | `CertStore::open` **fails** on a corrupt or non-CA `ca-cert.pem`; decide whether that is fatal at boot or degrades to "no CA" |
| p3-04 | M2 — correct the four stale `LeafCache` references before implementing |
| p3-04 | `prewarm` is a remote-input-driven CPU amplifier (~0.5 ms RB5009 per first-sight host); the connection cap is its only bound |
| p3-05 | M3 — re-warm the DoT hostname inside the 7-day leaf lifetime |
| p3-06 | `certs_mint` measures the cold `prewarm` path, not raw mint; leaf-cache RSS still needs an on-device figure |

### Safe to mark DONE?

**Not yet.** H1 and H2 are each roughly ten lines, both live entirely inside
`fah-certs`, and the crate still has no consumers — the same argument that
justified the second fix round. Both are silent-failure classes: H2 produces a
CA that reports healthy and issues unverifiable leaves; H1 re-introduces the one
invariant the purge was added to guarantee. M1 is a two-line follow-on.

If the owner prefers to ship as-is, H1 and H2 must be named blockers on p3-02
rather than generic deferrals — p3-02 is the task that makes H1 reachable.
Nothing else in the crate blocks.

### Verdict

**PASS WITH DEFERRED FINDINGS** — conditional on H1, H2 and M1 being fixed
before p3-02 starts, and M2/M3 being applied to the p3-04/p3-05 plan text.

---

## Fourth fix round — H1, H2, M1, M2

Owner-approved scope: H1, H2, M1 and the p3-04 plan text (M2). No unrelated
low/minor finding was touched.

| Finding | Status |
| --- | --- |
| H1 stale leaf reinserted after a CA regeneration | Fixed |
| H2 CA cert/key match never verified | Fixed |
| M1 `api_certified_key` skips the consistency check | Fixed |
| M2 stale `LeafCache` references in p3-04's plan | Fixed |
| M3 (p3-05 re-warm), L1–L6, m4/m7/m8/m10, nitpicks | Open, unchanged |

### H1 — an authority epoch gates the insert

`LeafCache::Inner` gained `epoch: u64`; `clear()` bumps it. `prewarm` split into
two lock sections so the epoch can be carried across the mint:

| Step | Function | Under the lock |
| --- | --- | --- |
| claim or coalesce | `lease(host, now) -> Lease` | hit, or wait, or insert into `inflight` and capture `epoch` |
| mint | `mint(ca, host)` | no |
| publish | `store_minted(guard, epoch, key, not_after)` | insert **only** if `inner.epoch == epoch` |

A pre-warm that captured the old `Arc<CaHandle>` before `install_ca` took the CA
mutex now finds a bumped epoch at publish time and drops its leaf instead of
repopulating a cache that was just purged. `CertStore::prewarm` still returns
`Ok`; nothing is cached, so `cached_leaf` misses, p3-04 fails closed and the next
connection re-warms under the new authority. Single-flight, coalescing, the
`InflightGuard` and every counter behave exactly as before.

New stat `LeafCacheStats::superseded` counts the dropped inserts — a public
field p3-02 serializes, and the only signal that a regeneration raced a
pre-warm.

Regression tests (`leaf.rs`):
`a_mint_started_under_a_replaced_authority_never_reaches_the_cache` drives
`lease` → `mint` → `clear` → `store_minted` in that exact order, so the race is
deterministic rather than thread-timed, and asserts `size == 0`,
`inflight == 0`, `superseded == 1`. Verified to **fail** on the unguarded insert
(`if inner.epoch == epoch || true` ⇒ panic at the `cached()` assertion).
`a_mint_that_wins_the_race_against_no_regeneration_is_cached` pins the other
direction so the guard cannot degenerate into "never cache".

### H2 — the CA pair is key-matched at every load

`import::validate`'s SPKI comparison extracted to
`import::ensure_key_matches(key_pem, subject_pki) -> Result<(), CertError>`;
`validate` calls it and so does `ca::summarize`, which now takes `key_pem` and
runs the check on the same parse that already reads `is_ca` and the validity
window — one parse, one implementation, no new dependency.

Every CA load path goes through `CaHandle::load` → `summarize`, so
`CertStore::open`, `generate_ca` and `install_ca_pair` all reject a mismatched
pair with `CertError::KeyMismatch` instead of loading a CA that reports healthy
and mints leaves no client can verify.

Interrupted-regeneration half: `load_pair` no longer discards the staged tmp
files when both live files exist. The discard moved to `store::discard_staged`,
called by `CertStore::open` and `api::load_or_generate` **after** the pair has
loaded successfully. A crash between `commit_pair`'s two renames therefore
leaves the live pair mismatched (now a loud `KeyMismatch` at open) **and** the
staged key intact for recovery, instead of deleting the only copy of the new
private key on the way to a silent failure.

Regression tests:
`ca::an_authority_whose_key_does_not_match_its_certificate_cannot_be_loaded`,
`store::an_authority_whose_key_does_not_match_is_refused_at_open`, and
`store::an_interrupted_regeneration_is_refused_loudly_and_keeps_the_staged_key`
(writes the replacement cert over the live one and the replacement key to
`ca-key.pem.tmp`, then asserts `KeyMismatch`, that the staged key survives, and
that the old live key is untouched). All three pass only with the fix.

### M1 — one consistency rule for both API-pair readers

`api::certified_key` now builds through
`CertifiedKey::from_der(certs, key, &aws_lc_rs::default_provider())`, which runs
rustls' `keys_match` — the same check `with_single_cert` already performed in
`server_config`. Failure maps to the existing `CertError::Config` variant, so the
p3-05 fallback seam and the API listener now agree on what an acceptable pair is.

Regression test:
`store::the_api_certified_key_refuses_a_pair_whose_key_does_not_match` asserts
both readers reject the same overwritten key.

### M2 — p3-04's plan matches the shipped API

`p3-04-tls-interception-plan.md`, five references corrected: §TASK START item 3
now names `Arc<CertStore>` + `prewarm`/`cached_leaf` and states that `LeafCache`
is private; Step 3, Step 4's `TlsProxy` field list, Step 6's hand-off and the
touched-files list all say `CertStore`. No other `.md` touched.

### Verification (fourth round)

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | green — 47 suites, zero failures |
| `cargo test -p fah-certs` | 70 unit (was 64) + 8 integration |

### Files changed (fourth round)

| File | Change |
| --- | --- |
| `crates/fah-certs/src/leaf.rs` | `Inner::epoch`; `clear()` bumps it; `prewarm` split into `lease` + `store_minted` with an epoch-gated insert; `LeafCacheStats::superseded`; two tests |
| `crates/fah-certs/src/ca.rs` | `summarize(der, key_pem)` key-matches; one test |
| `crates/fah-certs/src/import.rs` | `ensure_key_matches` extracted `pub(crate)`; `validate` calls it |
| `crates/fah-certs/src/api.rs` | `certified_key` via `CertifiedKey::from_der`; `load_or_generate` discards staged files after success |
| `crates/fah-certs/src/store.rs` | `load_pair` no longer discards tmps; `discard_staged`; `open` discards after a successful load; three tests |
| `plan/wip/phase3/p3-04-tls-interception-plan.md` | five `LeafCache` references corrected |

Public surface change: `LeafCacheStats` gained `superseded`. No file outside
`crates/fah-certs` and the p3-04 plan changed.

### Verdict after the fourth fix round

**PASS.**

No blocker remains. H1, H2 and M1 each carry a regression test that fails on the
pre-fix code. Still open and all deliberate: M3 (p3-05 must re-warm the DoT
hostname inside the 7-day leaf lifetime — a p3-05 change, not a p3-01 one),
L1–L6, m4, m7, m8, m10 and the nitpicks. None of them blocks p3-02, p3-04 or
p3-05; the §Carried into later tasks table above still applies, minus the H1/H2
rows.
