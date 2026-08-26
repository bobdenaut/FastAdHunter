# p5-02 — Certificate and Browser Spike — Review

## Implementation Summary

The generated API certificate now covers the address the dashboard is actually
opened at. The SAN set is decided by one pure policy function fed by a
best-effort, zero-dependency address probe; regeneration stays entirely
operator-controlled, and a half-renamed certificate pair can no longer cause a
surviving private key to be overwritten.

| What | Where |
| ---- | ---- |
| `probe_local_address()`, `san_entries()`, SAN-aware `generate()`, `IncompletePair` guard, generation log line | [crates/fah-api/src/tls.rs](../../../crates/fah-api/src/tls.rs) |
| `load_or_generate(config_dir, bind_address)` — one new argument | [crates/fah-api/src/tls.rs](../../../crates/fah-api/src/tls.rs) |
| Call site passes `config.api.address` | [crates/fastadhunter/src/main.rs](../../../crates/fastadhunter/src/main.rs) |
| Harness call sites updated | `crates/fah-api/tests/api.rs`, `crates/fastadhunter/tests/history_e2e.rs` |
| `x509-parser` as a **dev-dependency** (SAN assertion only) | [crates/fah-api/Cargo.toml](../../../crates/fah-api/Cargo.toml) |
| Plan, evidence protocol, migration | [plan/wip/phase5/p5-02-cert-browser-spike-plan.md](../../../plan/wip/phase5/p5-02-cert-browser-spike-plan.md) |

Diff: 6 files, +223 / −30. No route added, no runtime dependency added, no
comment added to any `.rs` file.

### Design decisions

1. **The probe is best-effort discovery of one address, not enumeration.**
   `probe_local_address()` binds a UDP socket and `connect()`s it to
   `192.0.2.1:1` (RFC 5737 TEST-NET-1, never routed), then reads `local_addr()`.
   `connect()` on a UDP socket is a route lookup — **no packet is sent**. It
   returns the single source address the kernel would select for off-link
   traffic; on a multi-homed box it returns one of several, and an operator
   whose reachable address is not the route-selected one pins it via
   `api.address`. Any failure yields `None`, logs at `debug`, and never fails
   boot. Chosen over `if-addrs` because the owner ruled out a runtime dependency
   for this spike and the deployment target has exactly one veth.

2. **`san_entries()` is the deterministic policy layer.** It takes the probe
   result as an argument rather than calling it, which is what makes every SAN
   rule unit-testable with no network. It is the only place SAN membership is
   decided.

3. **The expected baseline is four entries, deliberately not three.**
   `san_entries("0.0.0.0", None)` returns `fastadhunter`, `localhost`,
   `127.0.0.1`, `::1`. `::1` is a deliberate addition to the pre-p5-02 set so
   `https://[::1]:8443/` is a valid local debug origin on a dual-stack host. The
   test asserts the four-entry set; nothing claims byte-identity with the
   pre-change certificate.

4. **`api.address` feeds the SAN set, but only when it is a literal.** The
   `0.0.0.0` production default contributes nothing — which is precisely why
   the discovered address is what makes the dashboard reachable with no
   hand-edited TOML (root CLAUDE.md: every key ships a working compiled-in
   default).

5. **IPv6 is not probed.** The container's global address derives from a
   delegated prefix that rotates with the ISP lease
   ([routeros-traps.md](../routeros-traps.md)), so baking it into a certificate
   produces stale SANs within days. The ULA is stable, but the household URL is
   v4. Phase 3 owns certificate machinery and supersedes this.

6. **Regeneration is operator-controlled; the binary does nothing on its own.**
   No detection of a stale SAN set, no automatic replacement, no implicit config
   migration. An existing `/config` pair is honoured untouched, exactly as
   before. The migration is a documented rename-both-aside procedure the owner
   runs on the RB5009.

7. **Half-pair guard.** Generation previously triggered when *either* file was
   missing and then wrote *both* — so renaming only `api-cert.pem` silently
   overwrote the surviving private key. That is the silent replacement the task
   forbids. `TlsError::IncompletePair` now names both files, refuses to
   generate, and touches nothing.

8. **The SAN set is logged at generation.** A distroless image carries no
   tooling to inspect a certificate in place, and the migration requires
   verifying the new SANs *before* the old pair is discarded. The log line is
   that verification path.

### Migration — proposed commands, owner runs them

1. Rename **both** files in `/config`: `api-cert.pem` to `api-cert.pem.bak`,
   `api-key.pem` to `api-key.pem.bak`. Rename, never delete — the old pair is
   what rollback depends on, and it is preserved until the new certificate has
   been generated and its SANs verified.
2. Restart the container. A new pair is generated and its SANs are logged.
3. Verify from a LAN host before trusting the result:

   ```sh
   openssl s_client -connect 172.17.0.2:8443 </dev/null 2>/dev/null \
     | openssl x509 -noout -subject -dates -ext subjectAltName
   ```

4. **This invalidates every previously accepted browser exception, once.** Every
   household device warns again on its next visit. Tell the household before the
   restart.
5. Rollback: rename the `.bak` pair back over the generated one, restart.

Every step is an RB5009 action. Nothing on the device was changed by this task.

### Tests — as first submitted (superseded by §Approved fixes)

`cargo test -p fah-api --lib tls` — 10 passed, 0 failed. Six are new:

| Test | Asserts |
| ---- | ------- |
| `the_expected_baseline_san_set_is_four_entries` | `san_entries("0.0.0.0", None)` is exactly the four-entry baseline |
| `an_unspecified_bind_address_contributes_no_san` | `0.0.0.0`, `::` and an unparseable string all add nothing |
| `a_literal_bind_address_is_covered` | a pinned literal `api.address` reaches the SAN set |
| `a_discovered_address_is_covered_and_deduplicated` | the discovered address appears exactly once, including when it equals the bind address |
| `a_loopback_or_unspecified_discovery_adds_nothing` | loopback and unspecified discoveries are absorbed by the baseline |
| `the_generated_certificate_carries_the_policy_san_set` | the emitted certificate's parsed SANs equal the policy set (`x509-parser`, dev-only) |
| `a_half_pair_is_reported_and_the_survivor_is_left_untouched` | both half-pair orientations error, and the survivor is byte-identical afterwards |

Pre-existing tests kept unchanged in intent: persistence across restart,
user-supplied pair honoured, malformed pair reported not replaced.

### Gates — as first submitted (superseded by §Approved fixes)

Run 2026-08-26.

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | exit 0, no diff |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, no issues |
| `cargo test -p fah-api --lib tls` | 10 passed, 0 failed |
| `cargo test --all-features --workspace` | 1119 passed, 0 failed, 8 ignored, 44 suites |

`crates/fah-api/tests/request_coverage.rs` ran green; this task adds no route.
No bench: the change is on the boot path, not the hot path. No hot-path
allocation, no runtime memory delta — the probe runs once per generation, which
happens at most once per `/config` lifetime.

`cargo build --release` over the whole workspace fails on a locked
`fah-tui-monitor.exe` when the TUI monitor is running; `-p fastadhunter` builds.
Environmental, not a defect.

## Live verification — generated SAN set

Release binary, empty config directory, `FAH__DNS__LISTEN__PORT=5353`,
`FAH__API__PORT=8443`, dev box `192.168.10.10`. 2026-08-26.

Boot log:

```text
generated self-signed API certificate dns=["fastadhunter", "localhost"] ip=[127.0.0.1, ::1, 192.168.10.10]
```

Certificate on the wire matches the log:

```text
DNS:fastadhunter, DNS:localhost, IP Address:127.0.0.1,
IP Address:0:0:0:0:0:0:0:1, IP Address:192.168.10.10
```

| Reading | Result |
| ------- | ------ |
| Probe selection on a host with 3 IPv4 addresses (`192.168.10.10` LAN, `172.23.240.1` and `172.23.208.1` virtual adapters) | selected the LAN address; the virtual adapters did not win the route lookup |
| `api.address` contribution | none — the `0.0.0.0` default was in force, so the LAN SAN came **only** from the probe. The "no hand-edited TOML" requirement is demonstrated, not asserted |
| `https://192.168.10.10:8443/health` | `{"status":"degraded","version":"0.2.20","uptime_seconds":60}` — `degraded` is this box having no working upstream, unrelated to TLS |

**The name check, before against after** — `openssl s_client -verify_ip` against
each certificate's own LAN address:

| Certificate | Verify return code |
| ----------- | ------------------ |
| Old — live RB5009, read-only | `64 (IP address mismatch)` |
| New — dev box, this change | `18 (self-signed certificate)` |

Error 64 is the harsher interstitial the task exists to remove; error 18 is the
plain untrusted-issuer case SECURITY.md §TLS already promises. This is the
change's central claim, measured on both sides.

## Evidence status — the load-bearing gap

**Step 0 of the evidence protocol is collected. Steps 1–3 are not, and cannot
be produced from this box.**

Collected 2026-08-26, read-only, against the live RB5009 container — no change
made to the device:

```text
subject=CN=FastAdHunter
notBefore=Jan  1 00:00:00 1975 GMT
notAfter=Jan  1 00:00:00 4096 GMT
X509v3 Subject Alternative Name:
    DNS:fastadhunter, DNS:localhost, IP Address:127.0.0.1
verify error:num=18:self-signed certificate
```

This confirms the premise the task was written on: no SAN covers `172.17.0.2`,
so a household browser opening `https://172.17.0.2:8443/` gets a name-mismatch
interstitial, not the plain untrusted-issuer one SECURITY.md §TLS promises.

**One partial desktop reading, 2026-08-26.** Chrome on the dev box, against the
new certificate at `https://192.168.10.10:8443/health`: page loaded, address bar
**"Not secure"**, panel text *"Your connection to this site is not secure"* — a
generic warning, not a name-mismatch interstitial. Consistent with the error-18
result above. **It does not count as a first-visit reading**: the same panel said
*"You have chosen to turn off security warnings for this site"*, so that profile
already held a stored exception. Interstitial text and click count must be
re-taken from a fresh profile, and the stored exception will mask them until it
is cleared.

**Still outstanding, and owner-run by nature:** the desktop and phone readings —
interstitial text and code per address form, cost to proceed, whether the
exception survives a browser restart and a device reboot, and above all whether
a `Secure` `__Host-`-prefixed cookie sets **and persists** on that origin. The
full protocol, including the three-cookie probe and its attribution table, is in
[the plan](../../../plan/wip/phase5/p5-02-cert-browser-spike-plan.md#evidence-protocol).

**These are acceptance criteria of p5-02 and they are not met.** The code half
is complete and green; the evidence half needs real devices on the LAN.

## Known limitations and deferred items

1. **Multi-homed boxes get one address.** By design, per §Design decisions 1.
   `api.address` is the escape hatch. Not a defect on the deployment target.

2. **A pre-existing certificate is never re-evaluated.** A box that already
   holds a pair keeps its old SANs until the operator runs the migration. There
   is no boot-time warning that the loaded certificate fails to cover the bind
   address, because that requires runtime certificate parsing — a runtime
   dependency the owner ruled out. Deferred to Phase 3, which owns certificate
   status.

3. ~~**The generated certificate's validity window is 1975 → 4096.**~~
   **Resolved** — see finding F2 and §Approved fixes. The window is now
   `now − 1 h` to `now + 397 days`. The Step 0 reading above still shows
   1975 → 4096 because it was taken against the certificate the RB5009 already
   holds, which this task does not touch.

4. **No SECURITY.md or CONFIGURATION.md edit was made.** Both are proposed and
   waiting for the owner's yes (root CLAUDE.md §Working agreement 1), and the
   SECURITY.md recommendation cannot be written honestly until the desktop and
   phone readings exist — its whole content is "what a household should do, with
   the measured cost of each option".

5. **The phase directory is in `plan/wip/phase5/`, while that file's §Parallel
   track says it stays in `plan/open/phase5`.** File and disk disagree. Not
   touched by this task; the owner performs phase moves.

## Findings

Reviewed: working-tree diff only (6 files, +223 / −30). The `p5-01` and `p2.6`
work already on `phase5-02` was excluded. Gates re-run independently for this
review: `cargo fmt --all -- --check` exit 0; `cargo clippy -p fah-api
-p fastadhunter --all-targets` no warnings; `cargo test -p fah-api --lib tls`
10 passed.

**F1, F2 and F3 were subsequently approved and fixed — see §Approved fixes
below for what changed and the re-run gates. The finding text is left as
written.**

### Major

**F1 — A non-atomic pair write plus the new guard can wedge boot permanently.**

`generate()` writes the certificate and then the key with two plain `fs::write`
calls ([tls.rs:86-88](../../../crates/fah-api/src/tls.rs#L86-L88)). If the key
write fails (ENOSPC, EACCES, a signal) or the process dies between the two,
`/config` is left holding a certificate and no key.

- *Before* p5-02 the next boot regenerated both and self-healed.
- *After* p5-02 the next boot returns `IncompletePair`, and
  [main.rs:437](../../../crates/fastadhunter/src/main.rs#L437) propagates it
  with `?` out of `Engine` construction — the whole binary refuses to start,
  **DNS included**, until a human intervenes on the device.

The guard itself is right; the write is what makes it unrecoverable. Fix: write
`api-cert.pem.tmp` / `api-key.pem.tmp` and rename both once both succeed, or
remove the just-written certificate when the key write fails. **Fix before
`DONE`** — a new failure mode on a live household resolver, and cheap to close.
Inferred from the code path, not observed.

**F2 — The 1975 → 4096 validity window should be decided in this task, not
deferred past the migration.**

§Known limitations 3 is right that the window is rcgen's default and
pre-existing. The reason to settle it here is sequencing, not blame:

1. Apple's 398-day cap applies to TLS **server** certificates; the documented
   exemption is for user- or admin-added **roots**, not for a per-site "visit
   anyway" override. The iOS leg is therefore at real risk of failing for a
   reason this task can remove with two lines.
2. The new SAN set already forces exactly one regeneration. Setting
   `not_before` / `not_after` in the same change is free.
3. Fixed later, it costs a **second** invalidation of every household browser
   exception — the outcome the task text calls "worse than the mismatch it
   fixes".

**Fix before `DONE`**, or record an explicit owner decision to accept a possible
second migration. The evidence run should not be spent measuring a certificate
already known to be replaceable at zero cost.

### Minor

| # | Finding | Impact | Disposition |
| - | ------- | ------ | ----------- |
| F3 | `the_generated_certificate_carries_the_policy_san_set` ([tls.rs:329-341](../../../crates/fah-api/src/tls.rs#L329-L341)) re-invokes `probe_local_address()` to build its own expectation, because `generate()` calls the probe internally. The plan created the `detected`-as-argument seam for exactly this and stopped one level short | Two live route lookups compared against each other: a VPN coming up or an interface flapping between them fails the test for a reason unrelated to the code. Also contradicts plan §Settled inputs "offline, no network in tests" — as do `api.rs` and `history_e2e.rs`, whose harnesses now perform a route lookup and bake the dev box's real LAN address into throwaway certificates | Fix before `DONE`: `generate(bind, detected)`, probe at the single `load_or_generate` call site |
| F4 | Nothing automated proves the **name check** passes. The x509 test proves SAN content; "error 64 → error 18" rests on one manual `openssl` run. Both harnesses use `danger_accept_invalid_certs(true)` ([api.rs:518](../../../crates/fah-api/tests/api.rs#L518)) | A regression emitting an IP SAN in a form rustls/webpki rejects would pass every test | Defer. A harness variant pinning the generated certificate as a root against `https://127.0.0.1:<port>` closes it |
| F5 | The proposed doc-edit set omits the boot refusal. SECURITY.md §TLS says "Users can replace it with their own certificate (PEM/PFX) in `/config`" — precisely the operator action that now hard-fails when done one file at a time | An operator following the shipped doc gets a container that will not start | Add to the same approval round — one line, same section |
| F6 | `probe_local_address` binds `Ipv4Addr::UNSPECIFIED` ([tls.rs:102](../../../crates/fah-api/src/tls.rs#L102)); a box with no IPv4 default route yields `None` and falls back to the four-entry baseline | The "reachable with no hand-edited TOML" property silently does not hold there. Accepted by the plan (the household URL is v4), but §Known limitations 1 covers multi-homed only, not v6-only | Defer; add to §Known limitations |
| F7 | `load_or_generate`'s doc comment ([tls.rs:58-60](../../../crates/fah-api/src/tls.rs#L58-L60)) still describes only first-boot generation and the honoured user pair; the half-pair refusal — the newest and most operator-visible branch — is absent | The function's stated contract is now incomplete. Rule 7 forbids *adding* comments; this is an existing one whose contract drifted | Defer |
| F8 | The RouterOS premise is unverified. The plan asserts the probe selects `172.17.0.2` inside the container; the live verification measures the **dev box** | If the container's route lookup picks another address, the migration burns every household exception for nothing | Defer — contained: the migration renames rather than deletes, logs the SAN set, and verifies with `openssl` before the old pair is discarded. Worth stating that migration step 3 is a gate, not a formality |

### Nitpick

- `parsed_sans` sorts both sides before comparing, so the test cannot catch a
  change in SAN emission order — which the plan calls "deterministically
  ordered". Order is stable in `san_entries`; assert it or drop the sorts.
- `dns_names.clone()` ([tls.rs:134](../../../crates/fah-api/src/tls.rs#L134))
  exists only so the vector survives the move into `CertificateParams` for the
  log line. Boot path, two short strings — the cost is irrelevant; emitting the
  log before the move would remove it.

### Checked and clear

| Area | Result |
| ---- | ------ |
| Layering / dependencies | No new crate edge. `x509-parser` is dev-only, one version (0.18.1), already in `Cargo.lock` — the lock gains a single dep-edge line. Runtime binary unchanged |
| Hot path | Untouched. Probe and generation run once per `/config` lifetime, after `drop_to_service_user`, against a `const SocketAddr` — no name resolution, so no blocking-in-async hazard on the shared runtime |
| Memory | No retained state; two short `Vec`s dropped at the end of `generate` |
| Concurrency / cancellation / shutdown | No async, no task, no lock, no shared state added |
| Panics | No new `unwrap` / `expect` outside `#[cfg(test)]` |
| Call sites | All three updated; `lib.rs:42` is a rename re-export with no signature to change |
| SAN policy vs plan table | Matches. Bind and discovered entries are filtered on unspecified and deduplicated; loopback discoveries are absorbed by the baseline rather than filtered, which is equivalent because both loopbacks are in `SAN_BASELINE_IPS` |
| Unparseable `api.address` | Unreachable in production — validated as an `IpAddr` at config load ([fah-config/src/lib.rs:120](../../../crates/fah-config/src/lib.rs#L120)). The branch is defensive, and the test asserting it is fine |
| Image | `/config` is seeded **empty** (Dockerfile:93-98), so SANs are per-deployment and never baked at build time |
| Scope | No route, no runtime dependency, no CA machinery, no UI, no auth change, no HTTP fallback, no comment added to any `.rs` file |

### Acceptance criteria

| Criterion | Status |
| --------- | ------ |
| Desktop and phone behaviour recorded per address form | **Not met** — one partial desktop reading, from a profile that already held an exception |
| `Secure` `__Host-` cookie sets **and persists** on the phone | **Not met** — not attempted |
| SAN mechanism implemented, with a test asserting the generated names | Met |
| Regeneration defined and tested; existing pair not silently replaced | Met — see F1 for the write that undermines the guard |
| SECURITY.md recommendation drafted | **Not met** — correctly blocked on the evidence |
| No HTTP fallback introduced or proposed | Met |
| Gates green, `request_coverage.rs` included | Met |

## Approved fixes — F1, F2, F3

Applied 2026-08-26 on the owner's explicit approval of the three blocking
findings only. F4, F6, F7 and F8 are untouched and stay deferred. No browser or
phone evidence was run.

| Finding | Status |
| ------- | ------ |
| F1 — half-pair write can wedge boot | **Fixed** |
| F2 — 1975 → 4096 validity window | **Fixed** |
| F3 — live route lookup inside the test expectation | **Fixed** |
| F4, F6, F7, F8 | Deferred, unchanged |
| Nitpicks | The sort in `parsed_sans` is gone as a side effect of F3 — the test now compares SAN order directly. The `dns_names.clone()` stands |

### F1 — staged write, then rename

`generate()` no longer writes into the final paths. `write_pair()` stages
`api-key.pem.tmp` (permissions restricted while still staged) and
`api-cert.pem.tmp`, then renames the certificate into place and the key second.

| Interruption point | Next boot |
| ------------------ | --------- |
| During either staged write | Neither final file exists; both temps are removed on the error path; a normal first-boot generation runs |
| Between the two renames — the SIGKILL / power-cut case | `api-cert.pem` exists, `api-key.pem` does not, `api-key.pem.tmp` does: the pair is completed by renaming the staged key, logged at `warn`. **This is the case that used to wedge the boot** |
| Rename of the key returns an error | The just-renamed certificate and the staged key are both removed, so the next boot regenerates rather than half-loading |

The `IncompletePair` guard is intact and still fires for the case it was written
for — an operator moving one file aside, with no staged survivor to recover
from. The recovery is deliberately one-directional (certificate present, key
staged), which is the only order `write_pair` can produce.

A complete pair also discards any leftover temp file on load, so a stale staged
key can never be applied to a certificate it does not belong to.

### F2 — bounded validity, 397 days

`not_before = now − 1 h`, `not_after = not_before + 397 days`
([tls.rs](../../../crates/fah-api/src/tls.rs)). 397 sits inside Apple's 398-day
cap with a day of margin; the hour of backdating absorbs clock skew between the
box and the visiting device.

`not_after` joins the SAN set in the generation log line, so the operator reads
the expiry from the container log without tooling — the same reason the SAN set
is logged.

**Dependency impact.** No new crate. `time` was already a direct runtime
dependency of `fah-api` (`formatting`, `parsing`); `now_utc()` needs its `std`
feature, which is the single flag added. `alloc` was already enabled through
`formatting`, so `std` is the only newly-compiled surface. `Cargo.lock` is
unchanged by this fix. The release-binary delta was **not measured** — it is
bounded to `time`'s std-gated surface (`now_utc`, `SystemTime` conversions,
`std::error::Error` impls); p5-10 owns the image budget and can measure it there
if it matters.

### F3 — detected address is an explicit input

`generate(bind_address, detected)` now takes the probe result. The single live
call is `probe_local_address()` at the one generation site inside
`load_or_generate`, so the boot cost is unchanged: one route lookup, once per
`/config` lifetime.

`the_generated_certificate_carries_the_policy_san_set` calls
`generate("192.168.88.1", Some(172.17.0.2))` directly and parses the returned
PEM. No socket, no filesystem, no second route lookup, and the SAN order is now
asserted rather than sorted away.

### Tests — 14 in `tls`, four new

| Test | Asserts |
| ---- | ------- |
| `the_generated_certificate_is_bounded_to_397_days` | parsed `not_after − not_before` is exactly 397 days, and the window contains now |
| `an_interrupted_generation_is_completed_on_the_next_boot` | certificate + staged key recovers to a complete pair; the certificate is byte-identical and the temp is gone |
| `a_stale_temp_file_is_discarded_once_the_pair_is_complete` | leftover temps are removed on a normal load and the live key is untouched |
| `a_half_pair_without_a_staged_survivor_is_still_reported` | a temp staged in the direction `write_pair` cannot produce does not defeat the `IncompletePair` guard |

`the_generated_certificate_carries_the_policy_san_set` was rewritten (F3). The
other nine are unchanged.

### Gates — re-run after the fixes, 2026-08-26

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | exit 0, no diff |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, no issues |
| `cargo test -p fah-api --lib tls` | 14 passed, 0 failed |
| `cargo test --all-features --workspace` | 1123 passed, 0 failed, 8 ignored, 44 suites |

`request_coverage.rs` green; no route added. No bench — boot path, not hot path.
Diff after the fixes: 6 files, +354 / −31.

### New deferred items introduced by these fixes

| # | Item | Why it is deferred |
| - | ---- | ------------------ |
| F9 | **The certificate now expires.** 397 days after first boot the dashboard starts failing with a date error, and nothing renews it — `load_or_generate` never re-evaluates a pair it finds. The operator must re-run the migration roughly annually, and every household device accepts a new exception each time | Detecting expiry needs runtime certificate parsing, which the plan rules out (`x509-parser` is dev-only). Phase 3 owns certificate machinery and status; this is the concrete requirement it inherits. Until then the expiry is visible in the generation log and via `openssl x509 -dates` |
| F10 | **A wrong clock at first generation bakes a wrong window.** The RB5009 has no battery-backed RTC; if the container generates before the time is synced, the 397-day window is anchored to whatever the clock said | Not introduced by the fix so much as exposed by it — an unbounded window had no such exposure. Cheap check for the migration: confirm `/system/clock` before the restart that regenerates |

Both are recorded so §Known limitations stays the single list; neither blocks
this task.

## Status

**BLOCKED — on evidence only.**

The code half is closed. F1, F2 and F3 are fixed and covered by tests; F4, F6,
F7, F8, F9 and F10 are deferred and none of them blocks the task. Gates are
green across the workspace.

What remains is the acceptance criteria the plan's evidence protocol exists to
satisfy, and they are unchanged:

| Criterion | Status |
| --------- | ------ |
| Desktop and phone behaviour recorded per address form | Not met |
| `Secure` `__Host-` cookie sets **and persists** on the phone | Not met |
| SECURITY.md recommendation drafted | Not met — blocked on the two above |

The evidence must be taken against a certificate carrying **these** fixes: the
397-day window is exactly the property the iOS reading is most likely to turn
on, and a reading against the old unbounded certificate would have to be
re-taken. `p5-04` does not start until the cookie result is in.

## Second review pass — post-fix, 2026-08-26

Scope: the p5-02 working-tree diff only (6 files, +354 / −31). `p5-01` and
`p2.6` work already on `phase5-02` excluded. Gates re-run independently for this
pass: `cargo fmt --all -- --check` exit 0; `cargo clippy -p fah-api
-p fastadhunter --all-targets` clean; `cargo test -p fah-api --lib tls`
14 passed, 0 failed.

### Plan compliance — units 1–6

| Unit | Status |
| ---- | ------ |
| 1 `probe_local_address()` | [tls.rs:118-123](../../../crates/fah-api/src/tls.rs#L118-L123). UDP `connect()` to `PROBE_TARGET`, loopback and unspecified filtered, every failure yields `None`. Never fails boot |
| 2 `san_entries()` | [tls.rs:125-145](../../../crates/fah-api/src/tls.rs#L125-L145). Pure, no I/O, matches the plan's four-row policy table exactly |
| 3 `generate()` | Signature is `generate(bind, detected)` after F3 — one level deeper than the plan wrote, and correct |
| 4 `load_or_generate(config_dir, bind_address)` | All three call sites updated ([main.rs:437](../../../crates/fastadhunter/src/main.rs#L437), `api.rs:491`, `history_e2e.rs:431`); `lib.rs:42` is a rename re-export |
| 5 Half-pair guard | `IncompletePair` in both orientations, generation refused, nothing written |
| 6 SAN visibility | `tracing::info!` carries `dns`, `ip` and `not_after` |
| Migration | Doc-only. The binary detects, renames and regenerates nothing on its own — verified against the code, not only the prose |
| Out of scope | Held. No route, no CA machinery, no runtime certificate parsing, no UI, no auth change, no HTTP fallback, no runtime crate |
| Rule 7 — no comments | Held. The diff adds no `//`, `///` or `//!` line to any `.rs` file |

### Findings — this pass

#### Minor

| # | Finding | Impact | Disposition |
| - | ------- | ------ | ----------- |
| F11 | **F3 is fixed for the unit test only.** `load_or_generate` still calls `probe_local_address()` internally ([tls.rs:104](../../../crates/fah-api/src/tls.rs#L104)), so `api.rs:491` and `history_e2e.rs:431` still perform a live route lookup per harness start and still bake the dev box's real LAN address into a throwaway certificate. F3's finding text named both harnesses; §Approved fixes records F3 as "Fixed" without that qualification | No failure risk — a sandboxed probe returns `None` and the harness proceeds. It is the plan's §Settled inputs "offline, no network in tests" that is still not literally true | Defer, but correct the §Approved fixes claim to "unit test only". Closing it properly means threading `detected` through `load_or_generate`, which the harnesses would pass as `None` |
| F12 | **The interrupted-generation recovery is content-blind.** [tls.rs:77-83](../../../crates/fah-api/src/tls.rs#L77-L83) renames `api-key.pem.tmp` into place on the sole evidence that a certificate exists and a key does not. It never checks that the key belongs to that certificate | Reachable during the documented migration: generation is interrupted between the two renames, the operator restores **only** `api-cert.pem.bak`, and the staged new key is then married to the old certificate. Both files now exist, so `IncompletePair` cannot fire and generation never re-runs — every boot fails with `TlsError::Config`, permanently, with a message that names neither file | Defer, with two cheap mitigations: add "restore both or neither" to the migration text, and give `TlsError::Config` the two paths so the operator is told what to move aside |
| F13 | **Fixed temp names are not concurrency-safe.** `api-key.pem.tmp` and `api-cert.pem.tmp` are constants. Two instances booting against the same `/config` can interleave into a durably committed certificate-from-A / key-from-B pair. [`fah-config::write_atomic`](../../../crates/fah-config/src/lib.rs#L105-L107) suffixes its temp with the PID for exactly this reason | Low probability — one container, one `/config` — and the pre-change code had the same class of hazard transiently. What is new is that the mismatch is now committed by a rename rather than overwritten on the next boot | Defer. The deterministic name is *required* by the F12 recovery path, so this is a deliberate trade; record it as one |
| F14 | **Nothing proves the `bind_address` argument reaches the certificate.** Every `load_or_generate` test passes `"0.0.0.0"`, which by policy contributes no SAN; the only SAN-content test calls `generate` directly ([tls.rs:386-397](../../../crates/fah-api/src/tls.rs#L386-L397)) | A regression that dropped the argument inside `load_or_generate` — hardcoding `"0.0.0.0"`, or passing the wrong string — passes all 14 tests. This is the one seam between the call site and the policy that no test crosses | Defer; one test closes it: `load_or_generate(dir, "192.168.88.1")`, then parse the on-disk PEM and assert the literal is present |

#### Nitpick

- **The staged writes are not fsynced** — neither the temp files nor the
  directory. `rename` is atomic but not durable, so the "SIGKILL / power-cut"
  row in §Approved fixes F1 is stronger than the code guarantees: a power cut can
  leave a renamed but empty file. Repo-wide, not a p5-02 regression —
  `fah-config::write_atomic` does not sync either, and nothing under `crates/`
  calls `sync_all`. Worth narrowing that row to "SIGKILL" and leaving power-cut
  durability as a separate, repo-wide question.
- **`discard()` swallows its error** ([tls.rs:249-251](../../../crates/fah-api/src/tls.rs#L249-L251)).
  On the key-rename failure path, if removing the just-renamed certificate fails,
  the next boot sees a certificate without a key and no staged survivor — the
  `IncompletePair` wedge. Vanishingly rare; recorded for completeness.
- **Not claimed, and worth claiming: the key permission window closed.** The
  pre-change code wrote `api-key.pem` and *then* chmod'd it, so the final key
  existed as 0644 for a moment. `write_pair` restricts the file while it is still
  `api-key.pem.tmp`, so the final path never exists world-readable. A security
  improvement §Approved fixes does not mention.

### Re-checked and clear

| Area | Result |
| ---- | ------ |
| SAN policy vs plan table | Exact match, including that loopback discoveries are absorbed by `SAN_BASELINE_IPS` rather than filtered |
| Validity window | `not_before = now − 1 h`, `+ 397 d` — effective 396 d 23 h from generation, inside Apple's 398-day cap. The test asserts the span exactly and that it contains now; not timing-fragile |
| Half-pair guard | Intact after the F1 rework. Both orientations error, the survivor is byte-identical, the missing file is not created |
| Boot-path cost | One `socket` / `connect` / `getsockname` on generation only; two `unlink` attempts per boot on the complete-pair path. `connect()` on a UDP socket sends nothing. `Engine::start` is `async`, so these are blocking syscalls on a worker thread — microseconds, before any listener binds, and the pre-change code already read two files there |
| Memory | Two short `Vec`s, dropped at the end of `generate`. No retained state, no hot-path allocation |
| Dependency impact | `Cargo.lock` +1 line — the `x509-parser` edge under `fah-api`; the crate was already in the lock. `time` gains `std` on an existing direct runtime dependency. No new crate compiled into the release binary |
| Dev-dependency placement | `x509-parser = "0.18"` sits under `[dev-dependencies]` — verified in the file, not inferred from the diff hunk |
| Error propagation and panics | No new `unwrap` or `expect` outside `#[cfg(test)]`. `IncompletePair` names both paths and states the remedy |
| Concurrency and cancellation | No async, task, lock or shared state added |
| Migration semantics | Rename both aside, verify, then discard. Step 3 is a gate, not a formality — F8 stands |

### Verdict

**BLOCKED — on evidence only. Unchanged.**

The code half stands up to a second pass. F11–F14 are all deferred; none blocks
the task, and none changes the acceptance-criteria table in §Status. What remains
is still the desktop and phone readings, the `__Host-` cookie persistence result,
and the SECURITY.md recommendation that depends on both. No browser or phone
evidence was taken in this pass, by instruction.

## Live evidence — local Docker, 2026-08-26

Run against the current p5-02 tree. **The RB5009 was not touched** — no build, no
deploy, no configuration change. Read-only device figures elsewhere in this file
are unchanged.

### Setup

| What | Value |
| ---- | ----- |
| Image | `fastadhunter:p5-02`, `docker buildx build --platform linux/amd64 --build-arg FAH_VERSION=0.2.20`, 25.4 MB, built from the working tree including every p5-02 fix |
| Host | Windows 11, dev box, LAN `192.168.10.10`; Docker Desktop 29.7.2, `linux/amd64` |
| Docker bridge | `172.17.0.0/16`, gateway `172.17.0.1` — the same subnet as the RouterOS container |
| Container A | empty `/config`, no published port — measures what the probe selects inside a container |
| Container B | `/config` seeded with a pair generated by this same code on the host, `-p 8443:8443` — the browser and phone target |
| Certificate under test | `notBefore Aug 26 05:34:10 2026 GMT`, `notAfter Sep 27 05:34:10 2027 GMT` (397 days), SANs `DNS:fastadhunter, DNS:localhost, IP:127.0.0.1, IP:::1, IP:192.168.10.10` |

**Why two containers.** Container A answers the RouterOS premise; container B
answers the browser questions. They have to be separate because Docker Desktop
publishes a port on the host rather than routing to the container — a
container-generated certificate covers `172.17.0.2`, which no phone on the LAN
can reach. See F16.

### Container A — the probe inside a container

```text
bridge=172.17.0.2
generated self-signed API certificate dns=["fastadhunter", "localhost"]
  ip=[127.0.0.1, ::1, 172.17.0.2] not_after=2027-09-27 5:37:25 +00:00:00
```

The container was assigned `172.17.0.2` and the probe selected `172.17.0.2` — the
plan's RouterOS premise, reproduced on the same subnet with the same container
runtime shape. **F8 is now supported by measurement on a proxy**, not by
assertion; it is still not measured on the RB5009 itself.

### Container B — the served certificate, and the name check four ways

Container B logged no generation line: the seeded pair was honoured untouched,
which is the pre-existing-pair path exercised end to end in a real container.

`openssl s_client` against `192.168.10.10:8443`, served by the container:

| Condition | Verify return code |
| --------- | ------------------ |
| Untrusted issuer, address **in** the SAN set | `18 (self-signed certificate)` |
| Untrusted issuer, address **not** in the SAN set (`172.23.240.1`, same listener) | `64 (IP address mismatch)` |
| Certificate installed as a trusted root, address **in** the SAN set | **`0 (ok)`** |
| Certificate installed as a trusted root, address **not** in the SAN set | `64 (IP address mismatch)` |

Rows 3 and 4 are the load-bearing pair. Installing the box certificate on a
device produces a **clean, warning-free connection only because p5-02 put the
address in the SAN set**; before p5-02 the same install still failed the name
check. That is the measured justification for the SECURITY.md recommendation
below.

### Step 1 — desktop

| Field | Value |
| ----- | ----- |
| Browser | Google Chrome 151.0.7922.174, Windows 11 x64 |
| Profile | fresh on-disk profile (`ms-playwright-mcp\mcp-chrome-bed4e76`), not a private window — the interstitial appeared, confirming no stored exception for this host |
| URL | `https://192.168.10.10:8443/health` |
| Error code | `net::ERR_CERT_AUTHORITY_INVALID` |
| Headline | "Your connection is not private" |
| Body | "Attackers might be trying to steal your information from **192.168.10.10** (for example, passwords, messages, or credit cards)." |
| Advanced text | "This server could not prove that it is **192.168.10.10**; its security certificate is not trusted by your computer's operating system. This may be caused by a misconfiguration or an attacker intercepting your connection." |
| Actions to proceed | **2** — "Advanced", then "Proceed to 192.168.10.10 (unsafe)" |
| `/health` | `{"status":"degraded","version":"0.2.20","uptime_seconds":60}` — `degraded` is this box having no working upstream, unrelated to TLS |
| `window.isSecureContext` | `true` after the exception |

### Step 1 control — an address the SAN set does not cover

Same listener, same certificate, `https://172.23.240.1:8443/health`, same fresh
profile: error code, headline, body and Advanced text are **character-identical**
except for the address. Chrome's `MapCertStatusToNetError` ranks
`AUTHORITY_INVALID` above `COMMON_NAME_INVALID`, so a self-signed certificate
always reports the authority error and the name mismatch is never surfaced. See
F15 — this corrects the task's stated premise for Chrome.

### Step 3 — the three-cookie probe, desktop

Set on `/health` after accepting the exception, then read back after a **real
browser restart** (the Chrome process was killed and relaunched against the same
on-disk profile — verified by a new PID, not by closing a tab).

| Cookie | Set | After browser restart |
| ------ | --- | --------------------- |
| `__Host-fahprobe` (`Path=/; Secure; SameSite=Lax; Max-Age=86400`) | ✅ | ✅ |
| `fahprobe-secure` (`Secure`) | ✅ | ✅ |
| `fahprobe-plain` | ✅ | ✅ |

The certificate exception also survived the restart — the revisit rendered
`/health` with no interstitial.

Against the plan's attribution table this is the **"all three"** row: on a
desktop Chrome, at an IP-literal origin with an accepted self-signed
certificate, `p5-04`'s cookie design holds as written. `isSecureContext` is
`true`, so `Secure` is honoured and the `__Host-` prefix is accepted with no
host requirement beyond `Path=/` and `Secure`.

### Step 2 — phone: NOT RUN — **superseded, the leg was run; see §Phone leg — Android/Brave**

I cannot drive a physical device. This leg is owner-run and is the one the task
calls load-bearing; a desktop result does not substitute for it, because the
phone is the case that is expected to fail.

**Container B is left running** for it: `https://192.168.10.10:8443/health`,
certificate as above. If the phone cannot reach it, the Windows firewall is the
first suspect — inbound is allowed only through two "Docker Desktop Backend"
rules and the LAN profile is enabled.

Steps, unchanged from the plan's protocol:

1. Record device, OS version, browser and version.
2. Open `https://192.168.10.10:8443/health` in a **fresh, non-private** tab.
   Record the interstitial's exact text, its error code, and the number of taps
   needed to proceed.
3. Confirm the `/health` JSON renders.
4. Run the set probe once, via console or the bookmarklet:

   ```text
   javascript:(function(){var d=document;d.cookie="__Host-fahprobe=1; Path=/; Secure; SameSite=Lax; Max-Age=86400";d.cookie="fahprobe-secure=1; Path=/; Secure; SameSite=Lax; Max-Age=86400";d.cookie="fahprobe-plain=1; Path=/; Max-Age=86400";alert(d.cookie||"(empty)")})()
   ```

5. Quit the browser completely, reopen, revisit, and read back:

   ```text
   javascript:alert(document.cookie||"(empty)")
   ```

6. Reboot the device, revisit, read back again. Record whether the **exception**
   and the **cookies** survive separately — the plan's table distinguishes them.

Recording row:

`device | browser+version | URL | interstitial code and text | taps to proceed | exception survives browser restart | exception survives reboot | __Host- | Secure | plain`

### Cleanup

`docker rm -f fah502B` and `docker volume rm fah502-cfgA fah502-dataA
fah502-cfgB fah502-dataB` when the phone leg is done. Container A is already
removed.

## Findings — evidence pass

| # | Finding | Impact | Disposition |
| - | ------- | ------ | ----------- |
| F15 | **The task's premise does not hold for Chrome.** The task text says a browser opening an IP-literal origin "gets a **name-mismatch** error instead, which is a different and less forgiving interstitial". Measured: Chrome shows `ERR_CERT_AUTHORITY_INVALID` with identical wording whether or not the address is in the SAN set, because authority-invalid outranks name-mismatch in `MapCertStatusToNetError` | The SAN change buys **nothing** on a Chrome first visit — same code, same text, same two clicks. What it does buy is measured and real: the trusted-root path returns `0 (ok)` instead of `64`, so "install the box certificate once" becomes a working recommendation rather than a broken one. The justification for the change moves from "a gentler interstitial" to "the install path works at all" | Not a code defect — a premise correction. Record it in SECURITY.md's wording so the recommendation is not sold on an effect Chrome does not show. Safari and iOS are untested and may differ; the phone leg settles that |
| F16 | **The SAN policy assumes clients reach the container's own address.** Measured in container A: the probe selects the bridge address (`172.17.0.2`). Under a published port (`-p 8443:8443`, Docker Desktop, or any NAT'd host) clients connect to the *host's* address, which the certificate never covers and `api.address` cannot supply either — it has to stay bindable inside the container | RouterOS is unaffected and was confirmed by construction: the container's assigned address and the probe's selection were both `172.17.0.2`, which is the household URL. Any port-publishing deployment gets a name mismatch that no configuration in this task can fix | Defer, and document the constraint. The escape hatch in §Known limitations 1 (`api.address`) does **not** cover this case, which is a real gap in that note |

### Step 1 — the other address forms, same browser and profile

| Form | URL | Result |
| ---- | --- | ------ |
| IP literal | `https://192.168.10.10:8443/health` | interstitial, `ERR_CERT_AUTHORITY_INVALID`, 2 actions, page renders — the full reading above |
| SAN hostname | `https://fastadhunter:8443/health` | `net::ERR_NAME_NOT_RESOLVED` — no interstitial, no connection. Nothing resolves the name; the SAN entry is unreachable without DNS or a hosts entry. The plan predicted this and treated the failure as a reading |
| Loopback | `https://localhost:8443/health` | interstitial, `ERR_CERT_AUTHORITY_INVALID`, despite `localhost` being in the SAN set — confirming the certificate is untrusted, not misnamed. **Not usable as cookie evidence**: browsers treat `localhost` as a potentially-trustworthy origin regardless of TLS, so `Secure` and `__Host-` would succeed there even over a broken certificate |
| mDNS / DNS name | — | none exists on this LAN; nothing to record |

## Acceptance criteria — after the evidence pass

| Criterion | Status |
| --------- | ------ |
| Desktop behaviour recorded per address form, with device, browser and version | **Met** — Chrome 151.0.7922.174 on Windows 11, fresh profile, four address forms |
| Phone behaviour recorded per address form | **Not met** — owner-run; container B is left running for it |
| `Secure` `__Host-` cookie sets **and persists** — desktop | **Met** — all three cookies set and survived a real browser restart, alongside the certificate exception |
| `Secure` `__Host-` cookie sets **and persists** — phone | **Not met** — this is the load-bearing reading and it is the one still missing |
| SAN mechanism implemented, with a test asserting the generated names | Met |
| Regeneration defined and tested; existing pair not silently replaced | Met — and now exercised in a real container: container B logged no generation line and served the seeded pair |
| SECURITY.md recommendation drafted | **Drafted below**, from measured results, with the one phone-dependent line marked |
| No HTTP fallback introduced or proposed | Met |
| Gates green, `request_coverage.rs` included | Met |

## SECURITY.md §TLS — proposed wording

**Proposed, not applied.** Root CLAUDE.md §Working agreement 1 — this waits for
an explicit yes, and the phone-dependent sentence is marked so it is not
approved on evidence that does not exist yet.

> **What a browser shows.** The API certificate is self-signed, so every browser
> warns on the first visit. Chrome reports `ERR_CERT_AUTHORITY_INVALID` —
> "Your connection is not private" — and proceeding takes two actions:
> **Advanced**, then **Proceed to \<address\> (unsafe)**. The exception, and any
> cookie set afterwards, survive a browser restart. *(Measured: Chrome
> 151.0.7922.174, Windows 11, fresh profile, IP-literal origin.)*
>
> The certificate covers the box's own LAN address, discovered at generation
> time, alongside `fastadhunter`, `localhost`, `127.0.0.1` and `::1`. It is
> valid for 397 days from first boot and does **not** renew itself; regenerating
> it is an operator action (see the migration below) and invalidates every
> accepted browser exception once.
>
> **Three options for a household, cheapest first.**
>
> | Option | Cost | What it gets |
> | ------ | ---- | ------------ |
> | Accept the warning once, per device and per browser | 2 actions per device; repeated after any certificate regeneration | A working dashboard. The address bar keeps saying "Not secure" |
> | Install the box certificate as a trusted root on each device | One install per device, plus a repeat after each regeneration | A clean connection with no warning — verified: `openssl` returns `0 (ok)` once the certificate is trusted **and** the address is in its SAN set. Trusting a certificate whose SAN set misses the address still fails with `64 (IP address mismatch)`, which is why the SAN set matters more than the warning does |
> | A real name with a publicly trusted certificate | A domain, DNS, and a renewal mechanism this project does not ship | No warning anywhere, no per-device work. Out of scope until Phase 3 |
>
> **`fastadhunter` is in the certificate but resolves nowhere.** Reaching the
> dashboard by name needs a DNS entry or a hosts file; without one the browser
> fails with `ERR_NAME_NOT_RESOLVED` before TLS is ever attempted.
>
> **Do not use `https://localhost:8443/` to judge whether the certificate
> works.** Browsers treat `localhost` as a secure origin regardless of TLS, so
> it hides exactly the failure worth finding.
>
> *(Pending the phone reading:)* — one sentence on whether a phone persists the
> session cookie across a device reboot, and what a household should do if it
> does not.

**Note for the same edit.** The task text says an IP-literal origin produces a
name-mismatch interstitial. On Chrome it does not — see F15 — so the section
should not promise a gentler warning. The honest claim is that the SAN set makes
the *install-the-certificate* path work, and that the warning itself is
unchanged.

## CONFIGURATION.md §api — proposed wording

> `address` — when set to a literal IP rather than `0.0.0.0` or `::`, it is
> included in the SANs of the generated API certificate. It does not need to be
> set: the box's own LAN address is discovered at generation time. Pin it only
> when the reachable address is not the one the default route selects.

## Status — after the evidence pass

**BLOCKED — on the phone leg only.**

Everything a dev box can produce is produced: the code is complete and green,
the container path is exercised end to end, the RouterOS premise is supported by
a same-subnet proxy measurement, and the desktop cookie result is the plan's
best-case "all three" row. `p5-04`'s cookie design holds on desktop Chrome.

What is missing is the reading the task called load-bearing — a `Secure`,
`__Host-`-prefixed cookie setting and persisting on a real phone across a
browser restart and a device reboot. Container B is running and waiting for it.
`p5-04` does not start until that result is in.

## Phone leg — Android / Brave, 2026-08-26

Owner-run against container B, `https://192.168.10.10:8443/`. Supersedes the
"Step 2 — phone: NOT RUN" section above.

**How the probe was delivered.** The plan's `javascript:` bookmarklet is
impractical on Android — Chromium strips the `javascript:` scheme on paste into
a bookmark URL. Instead a static probe page (`probe.html`) was **bind-mounted
into the running container's `/web`** and served by the API itself, so the probe
executes on the exact origin under test. Three buttons — set, read back, reset —
and a table reading `document.cookie` plus `window.isSecureContext` live on every
render. No code, no image and no repository file changed; the page lives in the
session scratchpad and the mount is dropped with the container. `/web` remains
image content in every shipped path (Phase 5 standing constraint 2) — this was a
test-harness mount on a throwaway container, not a deployment.

| Field | Value |
| ----- | ----- |
| Device | Android phone on the same LAN, Android 16 |
| Browser | Brave 1.93.138 |
| URL | `https://192.168.10.10:8443/probe.html` (and `/health`) |
| Interstitial, first visit | "Your connection is not private" / `net::ERR_CERT_AUTHORITY_INVALID` — identical to desktop Chrome, as expected from a shared Chromium |
| Actions to proceed | **Not recorded** — the owner proceeded before the count was asked for. Desktop was 2 |
| `/health` | JSON rendered |
| `window.isSecureContext` | `true` |

### The three-cookie probe — the load-bearing reading

| Cookie | Set | After a full Brave restart (force-stopped) | After a device reboot |
| ------ | --- | ----------------------------------------- | --------------------- |
| `__Host-fahprobe` (`Path=/; Secure; SameSite=Lax; Max-Age=86400`) | ✅ | ✅ | ✅ |
| `fahprobe-secure` (`Secure`) | ✅ | ✅ | ✅ |
| `fahprobe-plain` | ✅ | ✅ | ✅ |
| `window.isSecureContext` | `true` | `true` | `true` |

The certificate exception also survived both: after the browser restart and
after the reboot the page loaded with **no interstitial**. Brave shows a red ✗
beside the URL — the origin stays marked untrusted — but that indicator does not
affect secure-context status, and `Secure` and `__Host-` are honoured underneath
it.

**This is the plan's "all three" row, on the device the plan expected to fail.**

### What it settles

| Question | Answer |
| -------- | ------ |
| Does a phone treat this origin as secure? | Yes — `isSecureContext` is `true` on an IP-literal origin with an accepted self-signed certificate |
| Does a `Secure` cookie persist on a phone? | Yes, across a force-stop and across a reboot |
| Is the `__Host-` prefix accepted on an IP-literal origin? | Yes — it needs `Secure` and `Path=/` and no `Domain`, none of which requires a hostname |
| Is `p5-04` unblocked? | **Yes.** The cookie design holds as written: `__Host-` prefix, `Secure`, `SameSite`. No HTTP fallback is needed and none is on the table |
| Does the exception survive a reboot? | Yes, on this device and browser |

Scope: one Android device, Brave 1.93.138, one origin, one LAN. iOS Safari is
**untested** — it is the remaining unknown, and Phase 3 or `p5-04` should re-run
this page against it before assuming parity.

## Acceptance criteria — final

| Criterion | Status |
| --------- | ------ |
| Desktop behaviour recorded per address form, with device, browser and version | **Met** — Chrome 151.0.7922.174, Windows 11, fresh profile, four address forms |
| Phone behaviour recorded, with device, browser and version | **Met** — Android 16, Brave 1.93.138. One gap: the tap count to proceed was not recorded |
| `Secure` `__Host-` cookie sets **and persists** on the phone | **Met** — set, survived a force-stop, survived a device reboot |
| SAN mechanism implemented, with a test asserting the generated names | Met |
| Regeneration defined and tested; existing pair not silently replaced | Met — and exercised in a real container: the seeded pair was served with no generation line |
| SECURITY.md recommendation drafted | Met — drafted above and below; **awaiting the owner's yes**, not applied |
| No HTTP fallback introduced or proposed | Met |
| Gates green, `request_coverage.rs` included | Met |

## SECURITY.md §TLS — the phone sentence, now measured

Replaces the "*(Pending the phone reading:)*" placeholder in the draft above:

> **Phones work.** Measured on Android 16 with Brave 1.93.138 against an
> IP-literal origin: after the one-time warning is accepted, the browser treats
> the origin as secure, and a `Secure`, `__Host-`-prefixed session cookie
> survives both a full browser restart and a device reboot. The address bar keeps
> a "not secure" marker; it does not affect the session. iOS Safari has not been
> measured.

## Status — final

**PASS WITH DEFERRED FINDINGS.**

Every acceptance criterion is met. The code is complete, gates are green across
the workspace, and the evidence the task was written to produce now exists on
both a desktop and a phone.

| Open item | Owner action |
| --------- | ------------ |
| SECURITY.md §TLS and CONFIGURATION.md §api | Drafted here, waiting for an explicit yes before either file is touched |
| F4, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16 | Deferred. F15 (Chrome masks the name mismatch) changes how the SECURITY.md text should be worded and is folded into the draft; F16 (port-publishing deployments) is a documented constraint |
| Test-harness teardown | `docker rm -f fah502B` and `docker volume rm fah502-cfgA fah502-dataA fah502-cfgB fah502-dataB`. Nothing to revert in the repository |

Nothing was committed. The RB5009 was not touched at any point.

## Approved fixes — F11, F14, F12-half, F4, and the documentation edits

Applied 2026-08-26 on explicit approval. `sync_all`, F7 and F13 were excluded by
name and stay deferred exactly as recorded. No evidence was re-run; the
certificates the evidence was taken against are unaffected by these changes.

| Finding | Status |
| ------- | ------ |
| F11 — harnesses still performed a live route lookup | **Fixed** |
| F14 — no test proved `bind_address` reaches the certificate | **Fixed** |
| F12 (half) — `TlsError::Config` named neither file | **Fixed** |
| F4 — nothing automated proved the name check | **Fixed** |
| F6, F9, F10, F16 | Recorded as limitations below; no code change, as approved |
| SECURITY.md §TLS, CONFIGURATION.md §api | **Applied** |
| `sync_all` nitpick, F7, F13 | Deferred, untouched |
| F5, F8, `dns_names.clone()` nitpick | Deferred, untouched |

### F11 — the probe moved to the production call site

`load_or_generate(config_dir, bind_address, detected)` now takes the discovered
address as an argument; it performs no I/O of its own beyond the certificate
files. `probe_local_address` is `pub` and re-exported from `fah-api`, and the
**only** live call in the workspace is
[main.rs:440](../../../crates/fastadhunter/src/main.rs#L440) — verified by grep,
which returns exactly three hits: the definition, the re-export, and that call.

The binary now owns this I/O, which is where root CLAUDE.md principle 6 puts it.
Both harnesses (`api.rs:492`, `history_e2e.rs:433`) pass `None`, so
`cargo test --workspace` opens no socket and bakes no real address into a
throwaway certificate. The plan's §Settled inputs "offline, no network in tests"
is now literally true.

Signature deviation from the plan (`load_or_generate(config_dir, bind_address)`)
is deliberate and approved.

### F14 — a literal bind address is proved end to end

`a_literal_bind_address_reaches_the_generated_certificate` calls
`load_or_generate(dir, "192.168.88.1", None)`, reads the PEM **from disk**, and
parses its SANs. It asserts the literal is present and that the full IP set
equals the policy set. A regression that dropped or hardcoded the argument
inside `load_or_generate` now fails a test rather than passing all of them.

### F12-half — the mismatch error names both files

`TlsError::Config` carries `cert` and `key`:

```text
building TLS config from {cert} and {key}: {source}
```

An operator who lands in the F12 state — a staged key married to an unrelated
certificate — is told which two paths to move aside. The recovery itself is
unchanged and F12 stays deferred.

**One incidental change.** Adding two `PathBuf`s pushed `TlsError` over clippy's
`result_large_err` threshold and eleven call sites failed the gate. `source` is
now `Box<rustls::Error>`, which shrinks the variant below the limit. It is part
of this fix, not a separate decision: `rustls::Error` was always the large
member, and boxing it costs one allocation on a path that is already fatal.

### F4 — the name check is now asserted, not inferred

`the_generated_certificate_passes_the_name_check_for_its_sans` performs a real
rustls handshake against a loopback listener built from the generated
`ServerConfig`, with the generated certificate installed as the client's **only
trust root**:

| Requested name | Expected | Asserted |
| -------------- | -------- | -------- |
| `127.0.0.1` — in the SAN set | handshake succeeds | `expect(...)` |
| `not-fastadhunter` — outside the SAN set | rejected on the name check | `expect_err(...)`, and the message must name `NotValidForName` |

This is what F15 made necessary: no browser reports a name mismatch on a
self-signed certificate, so browser error text can never validate the SAN set. A
test can. It is deterministic, offline, and uses no new dependency — `tokio` and
`tokio-rustls` are already `fah-api` runtime dependencies.

### Documentation applied

| Document | Change |
| -------- | ------ |
| [SECURITY.md](../../../SECURITY.md) §TLS for the API | Two new subsections, +37 lines: what a browser actually shows (Chrome and Android/Brave, both measured), the 397-day window and that it does not renew, the three household options with their measured cost, `fastadhunter` resolving nowhere, and the warning against judging the certificate from `localhost`. Wording follows F15: it does not promise a gentler interstitial |
| [CONFIGURATION.md](../../../CONFIGURATION.md) §api | +4 lines in the `[api]` block: `address`, when a literal, joins the certificate's SANs; it does not need setting because the LAN address is discovered; pin it only when the reachable address is not the route-selected one |

F5's line about the half-pair boot refusal was **not** added — it was not in the
approved set and stays deferred.

## Known limitations — consolidated

The single list, as recorded. Items 1–5 are unchanged from §Known limitations
above; 6–9 are the deferred findings the owner approved recording here.

| # | Limitation | Source |
| - | ---------- | ------ |
| 6 | **A box with no IPv4 default route gets no discovered SAN.** The probe binds `Ipv4Addr::UNSPECIFIED`; on a v6-only host it yields `None` and the certificate falls back to the four-entry baseline, so "reachable with no hand-edited TOML" quietly does not hold. `api.address` is the escape hatch | F6 |
| 7 | **The certificate expires after 397 days and nothing renews it.** `load_or_generate` never re-evaluates a pair it finds. The operator re-runs the migration roughly annually and every household device accepts a new exception each time. Detecting expiry needs runtime certificate parsing, which this task rules out; Phase 3 owns it. Until then the expiry is in the generation log and in `openssl x509 -dates` | F9 |
| 8 | **A wrong clock at first generation bakes a wrong window.** The RB5009 has no battery-backed RTC. If the container generates before time is synced, the 397-day window is anchored to whatever the clock said. Cheap check: confirm `/system/clock` before the restart that regenerates | F10 |
| 9 | **The SAN policy assumes clients reach the container's own address.** Measured: inside a Docker bridge container the probe selects the bridge address (`172.17.0.2`). Under a published port (`-p 8443:8443`, Docker Desktop, or any NAT'd host) clients connect to the *host's* address, which the certificate never covers and which `api.address` cannot supply either — it has to stay bindable inside the container. RouterOS is unaffected: the container's address **is** the household URL | F16 |

## Gates — re-run after these fixes, 2026-08-26

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | exit 0, no diff |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, no issues |
| `cargo test -p fah-api --lib tls` | 16 passed, 0 failed |
| `cargo test --all-features --workspace` | 1125 passed, 0 failed, 0 suites failed |

`request_coverage.rs` green; no route added. No bench — boot path, not hot path.
Diff: 8 files, +482 / −35 (code and documentation).

## Focused confirmation review — F11, F14, F12-half, F4, documentation

Scope limited by instruction to the five items above. No general re-review was
performed and no new findings were sought.

| Item | Verified | Result |
| ---- | -------- | ------ |
| F11 | `grep -rn probe_local_address crates --include=*.rs` returns three hits: definition, re-export, and the single `main.rs` call. Both harnesses pass `None` | Confirmed. Tests perform no route lookup |
| F11 — layering | `probe_local_address` is `pub` on an L3 crate and called from L4. No new crate edge, no sibling import | Confirmed |
| F11 — boot cost | Still exactly one route lookup per boot, and only when `api.tls` is on; it now happens before `load_or_generate` rather than inside it | Unchanged |
| F14 | Test reads the PEM from disk, not the in-memory return value, so it crosses the file boundary as well as the argument boundary | Confirmed |
| F12-half | Error string is `building TLS config from {cert} and {key}: {source}`; both paths are the ones `server_config` was already given | Confirmed |
| F12-half — `Box` | `TlsError` is back under clippy's `result_large_err` threshold; the workspace clippy gate passes with `-D warnings` | Confirmed |
| F4 | Positive and negative branch both assert; the negative requires the message to name `NotValidForName`, so it cannot pass on an unrelated handshake failure | Confirmed |
| F4 — determinism | Loopback listener on port 0, no external network, no timing assertion, no sleep. The spawned accept loop ends with the test's runtime | Confirmed |
| Documentation | Applied as drafted, F15's wording constraint respected. F5 not added, as it was not approved | Confirmed |
| Untouched by instruction | `sync_all`, F7, F13 — no change in the diff. F5, F8 and the `dns_names.clone()` nitpick likewise | Confirmed |

**Status: PASS WITH DEFERRED FINDINGS** — unchanged. Nothing was committed; the
RB5009 was not touched.
