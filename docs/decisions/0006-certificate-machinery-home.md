# Where certificate machinery lives, and which import formats it accepts

Phase 3 needs a certificate authority, per-host leaf minting, PEM import and a
public-only export path. Phase 1 already put `rcgen` and `load_or_generate` in
`fah-api` (L3) for the self-signed API server certificate. Phase 3 needs the
same primitives in `fah-http` (L3) for interception leaves and in `fah-dns` (L3)
for the DoT listener's certificate. Those three are siblings and may not import
each other, so the options are duplicate `rcgen` usage in three crates, or push
the shared machinery down.

p2-01 hit this exact shape with dual-stack socket binding — `fah-dns` and
`fah-http` both needed it — and resolved it by moving the behaviour to
`fah_common::listen` (L1) and deleting the original. The admission test recorded
in [that review](../code-review/phase2/p2-01-review.md) §2 is: *does divergence
between the two siblings produce a silent bug?*

For certificates it plainly does. Two crates disagreeing about SAN
construction, validity windows, key-file permissions or no-SNI handling is a
security defect, not a style difference. The machinery goes down.

**Not `fah-common`.** ARCHITECTURE.md rule 4 says it is for genuinely shared
small utilities and not a dumping ground, and p2-01 already spent one admission
there (`tokio` + `socket2`). CA and leaf logic is domain logic, and adding
`rcgen` + `x509-parser` + `aws-lc-rs` would be a third extension of a crate that
was explicitly warned against growing.

**Not L1.** L1 is data, config and small utilities by definition. A CA that
generates keys, writes them with restrictive permissions, archives its
predecessor and mints certificates is business logic.

## Decision

**A new crate `crates/fah-certs` at L2, beside `fah-rules`.**

L2 keeps the layering guard honest: `fah-certs` may import L1 only, and in
practice imports no workspace crate at all. It stays pure — no tokio, no
listeners, no async, no I/O beyond the explicit `/config` operations its API
names. `crates/fastadhunter/tests/layering.rs` assigns it L2 and fails the
suite if any edge points sideways or up.

`fah-api::tls` kept only `probe_local_address()`, which opens a socket; the
library stays free of ambient I/O (engineering principle 6). Everything else —
`san_entries`, generation, load, the tmp+rename write pattern, the
interrupted-generation recovery and the `IncompletePair` guard — moved with its
tests, and `fah-api` dropped `rcgen`, `rustls-pemfile` and its `x509-parser`
dev-dependency.

The `ResolvesServerCert` implementation (`MintingResolver`) ships here too, with
an optional fallback slot: `None` is the fail-closed posture for interception,
`Some` lets the DoT listener serve the API pair to a client that sends no SNI.
One type, both postures — two implementations diverging on expiry re-mint or
no-SNI handling is exactly the silent bug the admission test asks about. Wiring
it into a `ServerConfig` belongs to the consumers.

## PKCS#12 (PFX) import is descoped

SECURITY.md fixes the crypto set to rustls, rcgen, x509-parser, argon2 and
aws-lc-rs, and hard rule 5 forbids hand-rolled crypto. That set contains no
PKCS#12 parser. Real-world `.pfx` files are encrypted (PBES1/PBES2 with 3DES or
AES), so a decode-only ASN.1 parser would not be enough — import would need
parsing *and* decryption, i.e. `p12-keystore` or RustCrypto's `pkcs12` plus its
PBKDF and cipher crates. That is several new crypto crates, not one parser.

**Decision: PEM-only import.** Every OS and browser can export PEM, and
`openssl pkcs12 -in cert.pfx -out cert.pem -nodes` converts anything that
cannot. The smallest crypto surface wins over the convenience of one import
format. SECURITY.md's fixed set is therefore unchanged by Phase 3.

One consequence worth naming: because there is no passphrase to handle, no type
in `fah-certs` holds a secret that a `Debug` derive could leak. Private key
material is still redacted manually in every `Debug` implementation, and the
accessors that expose it are crate-private.

## Export is public-certificate-only, structurally

SECURITY.md promises the CA private key never leaves `/config` and that export
is public-certificate-only. The export functions read the parsed certificate
DER and re-encode it; they hold no key path and cannot reach one. Import
likewise rebuilds the certificate PEM from the parsed DER rather than echoing
what the caller submitted, so a combined `cert + key` blob pasted into the
certificate field — the most common shape people paste — cannot reach either the
export path or the on-disk certificate file.

## Revisit criteria

Reopen the placement if a fourth consumer needs certificates from L1 or L2
(which would argue for splitting the pure types lower), or if `fah-certs` starts
accumulating responsibilities that are not certificate operations.

Reopen the PFX decision if a PKCS#12 crate lands in a Rust ecosystem position
comparable to rustls' — audited, widely depended on, and narrow enough to add
to SECURITY.md's fixed set — or if user reports show the `openssl` conversion
step is a real barrier rather than a theoretical one.
