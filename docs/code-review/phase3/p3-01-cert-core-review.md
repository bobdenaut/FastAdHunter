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

## Findings — consolidated 2026-09-03; full history: `git show e35203e:docs/code-review/phase3/p3-01-cert-core-review.md`

Five review rounds and five fix rounds. Fixed and withdrawn items are omitted —
git has them. Still open:

| id(s) | Issue | Status | Where |
| --- | --- | --- | --- |
| F3 | CA path has no interrupted-regeneration completion; the API path does (`complete_interrupted_replacement`) | deferred | no owner — needs one |
| F6 | imported chain intermediates are decoded but never validated; only `chain[0]` is window-checked | deferred | no owner — needs one |
| F7 | the SEC1/PKCS#1 key acceptance claim in §Decisions has no fixture | deferred | no owner — needs one |
| F8, M3 | no renewal margin on leaves — an entry is served until `not_after` passes | won't-fix | handshake-time mint moots it (p3-05 review §Deviations: an expired entry is re-minted on the next hello, so no re-warm margin exists to tune) |
| m7 | private key material has a wider surface than it needs; no zeroization | deferred | no owner — needs one (owner decision) |
| m8 | `write_pair` does not fsync before or after the renames | deferred | no owner — needs one |
| m10 | `LeafCache::with_capacity(0)` degenerates to a one-entry cache, not a disabled one | deferred | no owner — needs one |
| L3 | an invalid host is invisible in the stats | deferred | no owner — needs one |
| L4 | expired entries occupy capacity and inflate `stats().size` (lazy `take_fresh` removal only) | deferred | no owner — needs one |
| L6 | no negative caching for a host rcgen rejects | deferred | no owner — needs one |
| nitpicks (unnumbered, 5) | duplicate `Generated` / `CLOCK_SKEW_HOURS` in sibling modules; `Debug` calls `stats()` (lock shape); leaf `not_before` not clamped to the CA's; `parse_pair` maps PEM errors to `Io`; wall clock read inside the library | deferred | no owner — needs one |

**PASS WITH DEFERRED FINDINGS** — 11 open rows (10 deferred, 1 won't-fix).

---

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

