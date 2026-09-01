# P5-02 — Implementation Plan — Certificate and Browser Spike

**Task:** [p5-02-cert-browser-spike.md](p5-02-cert-browser-spike.md) ·
**Phase:** 5 · **Depends on:** p5-01 · **Branch:** `phase5-02`

Approved 2026-08-26 with two wording corrections, both folded in below
(§Probe semantics, §SAN policy — expected baseline).

## Settled inputs

| Question | Answer | Source |
| -------- | ------ | ------ |
| Household URL | `https://172.17.0.2:8443/` — the container's `veth1` address. Not inferred, not substituted | owner, 2026-08-26 |
| Regeneration | Operator-controlled only. No automatic replacement, no implicit migration | owner, 2026-08-26 |
| Discovery mechanism | Zero-dependency. No runtime crate added for this spike | owner, 2026-08-26 |
| Test strategy | Offline, no network in tests | owner, 2026-08-26 |

## Probe semantics — best-effort, one address

`probe_local_address()` is **best-effort discovery of the single source address
the kernel would select for off-link traffic**. It is not interface
enumeration, and it does not claim to see every address the box holds.

- A UDP socket is `connect()`ed to `192.0.2.1:1` (RFC 5737 TEST-NET-1, never
  routed) and its `local_addr()` read. `connect()` on a UDP socket performs a
  route lookup only — **no packet is sent**, no peer is contacted.
- Inside the RouterOS container this selects `172.17.0.2` via the `172.17.0.1`
  default route, which is exactly the household URL.
- A box with several LAN interfaces gets **one** of them — the one the default
  route points through. That is a known and accepted limitation, not a defect:
  the deployment target has one veth. An operator whose reachable address is
  not the route-selected one pins it with `api.address`, which the policy layer
  below also feeds into the SAN set.
- Failure — no route, no permission, sandboxed network — yields `None`. It logs
  at `debug` and **never fails boot**.

`san_entries()` is the deterministic SAN policy layer and the only place SAN
membership is decided. It takes the probe result as an argument rather than
calling the probe, so every rule below is unit-testable with no network at all.

## SAN policy — expected baseline

`san_entries(bind: &str, detected: Option<IpAddr>)` produces a deduplicated,
deterministically ordered set:

| Source | Entries | Condition |
| ------ | ------- | --------- |
| Baseline DNS | `fastadhunter`, `localhost` | always |
| Baseline IP | `127.0.0.1`, `::1` | always |
| Bind address | `api.address` parsed as an `IpAddr` | only when it parses **and** is not unspecified (`0.0.0.0`, `::`) |
| Discovered | the probe result | only when `Some` **and** not loopback, not unspecified |

**The expected baseline is those four entries — `fastadhunter`, `localhost`,
`127.0.0.1`, `::1` — and that is what `san_entries("0.0.0.0", None)` must
return.** This is a **deliberate change from the pre-p5-02 three**
(`fastadhunter`, `localhost`, `127.0.0.1`): `::1` is added so
`https://[::1]:8443/` is a valid local debug origin on a dual-stack host. The
acceptance statement and the corresponding test both assert the four-entry set;
neither claims byte-identity with the pre-change certificate.

The `0.0.0.0` default of `api.address`
([CONFIGURATION.md §api](../../../CONFIGURATION.md)) contributes nothing, which
is why the discovered address is what makes the dashboard reachable with no
hand-edited TOML.

**IPv6 is not probed.** The container's global address derives from a delegated
prefix that rotates with the ISP lease
([routeros-traps.md](../../../docs/routeros-traps.md)), so baking it into a
certificate produces stale SANs within days; the ULA is stable, but the
household URL is v4. Phase 3 owns certificate machinery and supersedes this.

## Code changes

All in [crates/fah-api/src/tls.rs](../../../crates/fah-api/src/tls.rs) unless
noted. **No comments** are added — root CLAUDE.md hard rule 7; rationale lives
in this plan and in the review file.

1. `probe_local_address() -> Option<IpAddr>` — as specified above.
2. `san_entries(bind: &str, detected: Option<IpAddr>) -> (Vec<String>, Vec<IpAddr>)`
   — pure policy, no I/O.
3. `generate(bind: &str)` — calls the probe once, passes the result to
   `san_entries`, builds `rcgen::CertificateParams` from both lists.
4. `load_or_generate(config_dir: &Path, bind_address: &str)` — signature grows
   one argument. Call sites: [main.rs](../../../crates/fastadhunter/src/main.rs)
   (`config.api.address` is already in scope at the call), the
   [lib.rs](../../../crates/fah-api/src/lib.rs) re-export, and two test
   harnesses (`crates/fah-api/tests/api.rs`,
   `crates/fastadhunter/tests/history_e2e.rs`).
5. **Half-pair guard.** Today generation triggers when *either* file is missing
   and then writes *both* — so an operator who renames only `api-cert.pem` has
   their surviving private key silently overwritten. That is the silent
   replacement this task forbids. A new `TlsError` variant reports which file is
   missing, refuses to generate, and touches nothing.
6. **SAN visibility.** `tracing::info!` on generation lists the SAN set, so the
   operator can verify the new certificate from the container log — a distroless
   image carries no tooling to inspect it in place.

Explicitly **not** built: runtime certificate parsing, any CA machinery, any
auth change, any UI, any HTTP fallback.

## Regeneration migration — operator-run, no code

The binary detects nothing, renames nothing and regenerates nothing on its own.
An existing `/config` pair is honoured untouched, exactly as today.

1. Rename **both** files in `/config` — `api-cert.pem` to `api-cert.pem.bak`
   and `api-key.pem` to `api-key.pem.bak`. **Rename, never delete**: the old
   pair is what rollback depends on, and it is preserved until the new
   certificate has been generated and its SANs verified.
2. Restart the container. Both files are absent, so a new pair is generated and
   its SANs are logged.
3. Verify from a LAN host, before trusting the result:

   ```sh
   openssl s_client -connect 172.17.0.2:8443 </dev/null 2>/dev/null \
     | openssl x509 -noout -subject -dates -ext subjectAltName
   ```

4. **This invalidates every previously accepted browser exception, once.** Every
   household device warns again on its next visit and must accept once more.
   Tell the household before the restart, not after.
5. Rollback: rename the `.bak` pair back over the generated one, restart.

Every step is an RB5009 action. The commands are proposed here; the owner runs
them (root CLAUDE.md §Working agreement — the router is off limits).

## Evidence protocol

Run in full **before** the code change (baseline) and **again after**
(verification). Same table both times.

**Where.** The bulk runs against a **LAN dev box** serving the branch binary —
same rcgen path, same "IP literal, self-signed, no matching SAN" condition, and
the certificate can be regenerated freely. The RB5009 contributes one read-only
confirmation against the real `https://172.17.0.2:8443/` origin.

### Address forms

| Form | URL | Note |
| ---- | --- | ---- |
| IP literal | `https://172.17.0.2:8443/health` | load-bearing — the household URL |
| SAN hostname | `https://fastadhunter:8443/health` | likely fails to resolve; the failure is itself a reading |
| mDNS / DNS name | — | none known; record one only if the household has one |

`/health` is unauthenticated and root-mounted
([routes.rs](../../../crates/fah-api/src/routes.rs)), so it needs no API key and
no deployed frontend — the evidence does not depend on p5-01 being on the box.

### Step 0 — anchor the served certificate

Run the `openssl` command from the migration section above and record the SAN
list verbatim. This is the fact the interstitial is explained by.

### Step 1 — desktop

Record browser name and full version.

1. Open each address form in a **fresh browser profile** — not a private
   window, which discards exceptions by design and would fake a negative.
2. Record the interstitial: error code (`ERR_CERT_COMMON_NAME_INVALID` versus
   `ERR_CERT_AUTHORITY_INVALID`, or the browser's equivalent), the headline
   text, and the number of clicks needed to proceed.
3. Proceed, and confirm the `/health` JSON renders.
4. Run the cookie probe (Step 3) in the devtools console.
5. Quit the browser completely, reopen, revisit. Record whether the interstitial
   returns, then re-run the read-back probe.

### Step 2 — phone

Record device, OS version, browser and version. Console access, best first:

- **Android Chrome** — desktop Chrome `chrome://inspect`, USB debugging.
- **iOS Safari with a Mac** — Settings, Safari, Advanced, Web Inspector; then
  the Mac's Develop menu.
- **No desktop tooling** — bookmarklet: save any bookmark, edit its URL to the
  `javascript:` line in Step 3, then tap it while on the `/health` page.

Same five sub-steps as desktop, plus: record whether the exception survives a
**device reboot**, not only a browser restart. Phones are the case that fails.

### Step 3 — the `__Host-` cookie probe

Three cookies, so a failure is attributable rather than merely observed.

**Set** — once, on the `/health` page:

```text
javascript:(function(){var d=document;d.cookie="__Host-fahprobe=1; Path=/; Secure; SameSite=Lax; Max-Age=86400";d.cookie="fahprobe-secure=1; Path=/; Secure; SameSite=Lax; Max-Age=86400";d.cookie="fahprobe-plain=1; Path=/; Max-Age=86400";alert(d.cookie||"(empty)")})()
```

**Read back** — after the browser restart, and again after the device reboot:

```text
javascript:alert(document.cookie||"(empty)")
```

| Present after restart | Meaning |
| --------------------- | ------- |
| all three | `p5-04`'s cookie design holds as written |
| `fahprobe-secure` and `fahprobe-plain`, no `__Host-` | the prefix is rejected on this origin — `p5-04` drops the prefix, keeps `Secure` |
| `fahprobe-plain` only | `Secure` is not honoured; the origin is not treated as secure. **`p5-04` is blocked**, and no HTTP fallback is on the table |
| none | the exception or the storage was discarded. Record which — a lost exception and a lost cookie are different failures |

The probe tests `SameSite=Lax`. If `p5-04` settles on `Strict`, that is one
extra reading, not a re-run.

### Recording template

One row per device, browser, address form and before/after:

`device | browser+version | URL | interstitial code and text | clicks to proceed | exception survives browser restart | exception survives reboot | __Host- | Secure | plain`

## Tests — offline, no network

1. `san_entries("0.0.0.0", None)` returns exactly the four-entry expected
   baseline — `fastadhunter`, `localhost`, `127.0.0.1`, `::1`.
2. A literal `bind` is included; `0.0.0.0` and `::` are excluded.
3. A `detected` address is included, and is deduplicated against both the
   baseline loopbacks and the bind address.
4. A loopback or unspecified `detected` address adds nothing.
5. The generated certificate carries the expected SANs — parsed with
   `x509-parser` as a **dev-dependency** only, already present in `Cargo.lock`,
   so the runtime binary is unchanged.
6. An existing pair is left untouched (exists today; kept).
7. A half-pair returns an error, and the surviving file is byte-identical on
   disk afterwards.
8. A malformed pair is reported, not replaced (exists today; kept).

## Gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --all-features --workspace
```

`crates/fah-api/tests/request_coverage.rs` is included and is trivially green —
this task adds no route. No bench: the change is on the boot path, not the hot
path.

## Documents this task proposes to change

**Listing is not permission.** Each is proposed to the owner and waits for a yes
(root CLAUDE.md §Working agreement 1).

| Document | Proposed change |
| -------- | --------------- |
| SECURITY.md §TLS | what a browser actually shows on an IP-literal origin; the accept-once / install-the-certificate / trusted-name recommendation with its measured cost; the operator-run migration and its one-time exception invalidation |
| CONFIGURATION.md §api | one line: `address`, when a literal, is included in the generated certificate's SANs |

## Out of scope

Phase 3's CA machinery — generate CA, import PEM/PFX, export CA, certificate
status endpoints. Runtime certificate parsing. Any UI. Any change to the `p5-04`
auth design. Any HTTP fallback. Any runtime dependency added for this spike.
