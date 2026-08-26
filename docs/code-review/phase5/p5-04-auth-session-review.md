# p5-04 — Dashboard Authentication and Sessions

**Task:** [p5-04-auth-session.md](../../../plan/wip/phase5/p5-04-auth-session.md) ·
**Plan:** [p5-04-auth-session-plan.md](../../../plan/wip/phase5/p5-04-auth-session-plan.md) ·
**Branch:** `phase5-04` · **Base:** `9b165ef`

## Implementation Summary

Password login with an Argon2id hash in `/config/auth-hash`, a signed session
cookie keyed by `/data/session-secret`, and cookie acceptance on both the REST
surface and the WebSocket upgrade. The bearer key is untouched. No config-schema
change, no hot-path contact.

| File | Change |
| ---- | ------ |
| `crates/fah-api/src/password.rs` | **new** — `AuthState`, the `/config/auth-hash` store, the Argon2id runner behind a 2-permit `try_acquire` semaphore, and the bounded per-address rate limiter |
| `crates/fah-api/src/session.rs` | **new** — `/data/session-secret`, token mint/verify, cookie build/parse |
| `crates/fah-api/src/error.rs` | `ApiError::RateLimited` / `Unavailable`; per-variant `Cache-Control: no-store` and call-site `Retry-After`; `Unauthorized`'s message no longer names the API key |
| `crates/fah-api/src/auth.rs` | `require_api_key` → `require_auth`: bearer first, then cookie; records `AuthMethod` in the request extensions; `PUBLIC_PATHS` gains `/api/v1/auth/login` |
| `crates/fah-api/src/routes.rs` | four `/auth/*` routes inside the `v1` chain with a `no-store` layer, the `POST /config` `auth` guard, and `Origin` validation ahead of `subscribe_socket()` |
| `crates/fah-api/src/state.rs`, `lib.rs`, `wire.rs` | `AppState.auth`, exports, `LoginRequest` / `PasswordChangeRequest` |
| `crates/fah-api/Cargo.toml` | `argon2` (new), `aws-lc-rs` (manifest edge only), `[features] test-harness`, self dev-dependency |
| `crates/fastadhunter/src/main.rs`, `Cargo.toml` | `AuthState::load_or_create` on `spawn_blocking`, the one-time password log line, and a non-default `test-harness` feature for the measurement harness |
| `crates/fastadhunter/tests/layering.rs` | self-edges skipped, with a new assertion that one may not appear outside `[dev-dependencies]` |
| `crates/fah-api/tests/api.rs`, `crates/fastadhunter/tests/history_e2e.rs` | harness wiring plus 17 new integration cases |
| `requests/auth.http` | login, logout, logout-all and password-change fixtures |

### Design decisions (the six the draft left open)

| # | Decision | Reasoning |
| - | -------- | --------- |
| 1 | Argon2id `m = 19456 KiB, t = 2, p = 1`, `ARGON2_PERMITS = 2` | OWASP baseline, confirmed by measurement below rather than assumed. 45.3 ms median on x86 |
| 2 | 7-day **absolute** expiry, no sliding renewal, no inactivity timeout | Re-authenticating a household phone twice a day is the failure mode of a short absolute expiry. Traded against `HttpOnly` + `Secure` + `SameSite=Strict` on a LAN-only origin; one constant changes it |
| 3 | `hex([ver:u8=1][expiry:u64 BE][nonce:16] ‖ HMAC-SHA256)`, 114 chars | `aws_lc_rs::hmac::verify` is constant-time by contract. Hex, not base64, so no crate is added |
| 4 | First boot generates 16 CSPRNG bytes → 26-char base32, logs once, persists **only** the hash | Plaintext is returned as the second element of a tuple and never enters `AuthState`, so no handler can reach it. Recovery is deleting `/config/auth-hash`, not an open setup page |
| 5 | Password change rotates the secret **before** writing the hash | A crash between the two leaves the old password with zero sessions — recoverable. The reverse order would leave a new password alongside live old cookies |
| 6 | **No** revocation state in `/data` | One account; revocation is global secret rotation. `logout-all` is the only real revocation, `logout` is client-side |

## Measurements

Dev leg only. Linux container (`rust:1.96.0-alpine` build, `alpine:3.21`
runtime), x86-64, `lists = []`. `fah_common::process::resident()` reads
`/proc`, so a Windows-host run would produce no RSS at all.

### Dependency, binary and image delta (§11.1)

Both images built in the same session; no stored figure was reused.

| Figure | Pre (`9b165ef`) | Post | Delta |
| ------ | --------------- | ---- | ----- |
| `/fastadhunter` bytes | 13,330,912 | 13,507,168 | **+176,256 B (+1.32 %)** |
| `docker images` SIZE | 25.4 MB | 25.7 MB | +0.3 MB |
| `docker image inspect .Size` | 6,526,824 | 6,610,609 | +83,785 B |

New packages in `Cargo.lock`, counted as **additions** rather than as edges
gained (the p5-01 attribution trap): **5** — `argon2` 0.5.3, `password-hash`
0.5.0, `base64ct` 1.8.3, `blake2` 0.10.6, `rand_core` 0.6.4.

**`aws-lc-rs` cost zero packages**, confirming plan §4.1: it was already in the
release graph at 1.17.3 through `rcgen`'s `aws_lc_rs` feature, and the manifest
line only makes the name callable.

### Login latency (§11.3) — relaxed limiter, 3 warm-ups then 21 sequential

| Statistic | x86 measured | RB5009 **estimate** (×9) |
| --------- | ------------ | ------------------------ |
| median | **45.31 ms** | ~408 ms |
| min | 45.18 ms | — |
| max | 45.53 ms | — |

The RB5009 column is an estimate and is not evidence; the device leg is
deferred (D3).

### Ratchet (§11.4) — relaxed limiter, `GET /api/v1/debug/memory`

| Point | `process_rss` |
| ----- | ------------- |
| baseline | 48.2 MiB |
| after 200 sequential logins | 50.3 MiB |
| after 10 bursts of 8 concurrent | 120.5 MiB |
| **after 60 s idle** | **45.8 MiB** |

Returns **below** its own baseline. No step survives the idle, so mimalloc is
not retaining the Argon2 arena — which is the claim §11.4 exists to falsify.
`process_peak_rss` high-water for the run: 119.3 MiB.

> **Reading correction.** `process_peak_rss` is not a usable bound on a
> transient burst peak in this codebase — see
> [the retraction](#retraction--f4s-second-point-was-wrong). It is descriptive
> high-water telemetry only; every burst delta in this file is derived from
> sampled `process_rss`.

### Concurrency bound (§11.5) — 8 simultaneous logins, relaxed limiter

| Outcome | Count |
| ------- | ----- |
| `204` (in flight, = `ARGON2_PERMITS`) | **2** |
| `503 unavailable` + `Retry-After: 1` | **6** |
| `429` | **0** |

Reproduced identically across the 10-burst run (80 attempts, only `204`/`503`).

### Transient allowance (§11.7) — one settled 8-way burst

> **SUPERSEDED.** This leg's baseline was taken before the process reached a
> plateau, so the delta below is not the quantity the allowance governs. The
> authoritative figure is the single-process re-measurement in
> [§F4](#f4--re-measurement-one-process-settled-baseline): **39.60 MiB against
> a ≤ 40 MiB allowance, within bound**. The table is kept as the record of what
> was measured, not as a result.

| Point | `process_rss` |
| ----- | ------------- |
| settled steady state | 29.9 MiB |
| peak during the burst | 75.1 MiB |
| delta | 45.3 MiB |
| after 60 s idle | 35.1 MiB |

### DNS-unmoved (§11.5)

Sampled against a **locally blocked** name (`||blocked.invalid^` installed via
`PUT /api/v1/rules/user`), so the answer never leaves the engine. A first
attempt sampled a forwarded name and measured 1604 ms on both sides — the
container has no route to the upstreams, so that run measured the upstream
timeout and proved nothing; it was discarded.

| Workload | n | median | max |
| -------- | - | ------ | --- |
| control, no login load | 60 | **0 ms** | 4 ms |
| during a sustained 2-worker login burst | 60 | **0 ms** | 4 ms |

Statistically unmoved, same session, same process.

### Burst rejection (§11.6) — **shipped** binary, production limiter

| Attempt | Status | `Retry-After` |
| ------- | ------ | ------------- |
| 1–5 | `401` | — |
| **6** | **`429 rate_limited`** | **59** |
| 7 | `429` | 59 |
| correct password, same window | `429` | 59 |

The same leg against the `test-harness` build answers `401` seven times and then
`204` — no `429`. That contrast is **runtime** evidence that the feature gate
actually separates the two builds, and that the shipped artefact carries the
production constants.

## Acceptance reconciliation

Verdicts are against measured evidence, not test-name presence.

| # | Criterion | Measured input | Derived value | Expected bound | Verdict |
| - | --------- | -------------- | ------------- | -------------- | ------- |
| 1 | Right/wrong password indistinguishable | Two `401` responses byte-compared (wrong password vs valid password after hash replacement) | Identical status, body and header set | Byte-identical | **SATISFIED** |
| 2 | Cookie authenticates `/api/v1/*` | Real TLS integration | `200` with cookie, `401` bare | `200` | **SATISFIED** |
| 3 | Expiry enforced from the token | Expired token in `Cookie` with `Expires: 2999` | `401` `unauthorized` | `401` | **SATISFIED** |
| 4 | Unknown token version rejected | Version byte `2`, valid MAC | `401` `unauthorized` | `401` | **SATISFIED** |
| 5 | Socket upgrades on the cookie alone | Matching `Origin` | `101` | `101` | **SATISFIED** |
| 6 | Foreign `Origin` rejected, matching accepted | Foreign and absent `Origin` | handshake refused; subscriber count stayed 0 | reject / accept | **SATISFIED** |
| 7 | Bearer upgrade with no `Origin` succeeds | Header form and `?token=` form | `101` both | `101` | **SATISFIED** |
| 8 | Bearer access unchanged | 71 pre-existing `tests/api.rs` cases and the `auth.rs` unit tests, **assertions unedited** | all pass | pass with no assertion diff | **SATISFIED** — see D2 |
| 9 | Password change requires the current password | Wrong current password | `401`, secret file byte-identical | `401`, nothing written | **SATISFIED** |
| 10 | Password change kills every session | Old cookie after change; `rotate_secret` crash simulation; login-vs-rotation race | `401`; old password survives with zero sessions; raced token never verifies | all three | **SATISFIED** |
| 11 | `GET /config` carries no auth material | Response body | no `auth` key, no `$argon2` | absent | **SATISFIED** |
| 12 | `POST /config` with `auth.password_hash` | Explicit guard | `422` naming `/api/v1/auth/password` | `422` | **SATISFIED** |
| 13 | Secret never in `/config`, logs or errors | Redacted `Debug`/`Display` + five response bodies scanned | no occurrence | absent | **SATISFIED** |
| 13b | All four store states defined, none boot-fatal | One tempdir case each | both present → nothing written; hash absent → full reset; both absent → first boot; hash present + secret absent → secret alone regenerated, hash byte-identical, old token dead, no plaintext | as stated | **SATISFIED** |
| 13c | `no-store` on every auth response | `204`, `400`, both `401` paths, `422`, `429`, both `503` causes | header present on all seven | present | **SATISFIED** |
| 14 | `api.tls = false` rejects login | Integration | `503` `unavailable`, **no** `Retry-After`, no `Set-Cookie`, message names TLS | as stated | **SATISFIED** |
| 14b | The two `503` causes are distinguishable | Saturation vs TLS-off | `Retry-After: 1` vs header absent, same `unavailable` slug | distinguishable | **SATISFIED** |
| 14c | Only `login` is gated on TLS | Bearer against the other three | `logout` `204`; `logout-all` `204` + secret file changed; `password` `401` then `204` | as stated | **SATISFIED** |
| 14d | Relaxed limiter absent from the shipped build | §11.6 shipped binary vs test-harness build | `429` on attempt 6 / no `429` at all | production constants ship | **SATISFIED** — now runtime evidence, not only structural |
| 15 | Argon2id runs on `spawn_blocking` | DNS median under sustained login burst vs same-session control | 0 ms vs 0 ms, max 4 ms both | unmoved | **SATISFIED** |
| 16 | Semaphore bounds concurrency; saturation answers `503` + `Retry-After` | 8 simultaneous logins | 2 × `204`, 6 × `503` `Retry-After: 1`, 0 × `429` | exactly 2 in flight | **SATISFIED** |
| 17 | Transient allowance stated separately and respected | Superseded by the single-process re-measurement (§F4): E = 40.09 MiB settled, F = 79.69 MiB sampled during the burst, G = 39.69 MiB after 60 s | **39.60 MiB** transient delta, returns to the plateau | ≤ 40 MiB | **transient bound PROVEN · long-run ratchet half DEFERRED to the device leg** |
| 18 | Rate limiting bounded in memory | 10 000 distinct addresses, production limits | map ≤ 128 at every step | ≤ 128 | **SATISFIED** |
| 19 | Rate limiting rejects a burst | §11.6, production limits | 6th attempt → `429` + `Retry-After: 59` | `429` | **SATISFIED** |
| 20 | Parameters recorded with device, workload and measured peak RSS | Dev leg above | recorded | device leg required | **DEFERRED** (D3) — never satisfied by conversion |
| 21 | Gates green, `request_coverage.rs` included | §Gates below | all green | green | **SATISFIED** |

**25 satisfied · 1 deferred (20) · criterion 17's long-run ratchet half deferred
alongside it.** The original tally read "24 satisfied · 1 not met (17)"; row 17
is corrected above against the §F4 re-measurement.

## Findings from measurement

**M1 — WITHDRAWN.** M1 claimed the ≤ 40 MiB transient allowance was falsified
at 45.3 MiB. That figure was derived against an under-warmed baseline and does
not stand; the allowance is **not** falsified. The authoritative result is the
single-process re-measurement in
[§F4](#f4--re-measurement-one-process-settled-baseline): **39.60 MiB against a
≤ 40 MiB allowance, within bound, returning to the plateau within 60 s.**

Nothing was retuned on the strength of either run. `ARGON2_PERMITS` stays at 2,
`m` stays at 19,456 KiB, and the allowance stays at ≤ 40 MiB. What survives
from M1 is a risk observation, not a failure: the measured margin is **0.40 MiB**
over a `ARGON2_PERMITS × m` arena floor of 38 MiB. Whether to widen the
allowance or drop `ARGON2_PERMITS` to 1 remains a plan-side decision for the
owner; the arithmetic is fixed and no further measurement changes it.

M1's superseded claim that §11.4's `process_rss` / `process_peak_rss` pair
proved two different runs is retracted separately — see
[the retraction](#retraction--f4s-second-point-was-wrong).

## Deviations from the plan

| # | Deviation | Why |
| - | --------- | --- |
| D1 | **No Rust doc comments anywhere in the new code**, and `get_config`'s doc comment could **not** be rewritten as §1/§9 require | `.claude/hooks/no-rust-comments.sh` rejects every `//`, `///` and `//!`, enforcing root CLAUDE.md hard rule 7. The hook overrides the plan. Nothing the existing comment asserts became false — auth material genuinely is not in `Config` — and the invariant is now enforced by the `POST /config` guard and asserted against the response body |
| D2 | The shared `start_with` harness in `tests/api.rs` gained an `auth` field | Unavoidable: `AppStateBuilder` has a new required field. **No bearer test's assertions were touched**, which is what criterion 8 actually pins |
| D3 | `AuthState::for_tests` takes `config_dir`/`data_dir` rather than being purely in-memory (§10) | `logout-all` and password change must exercise the real staged-write and rotation path, and both integration harnesses already own tempdirs. A purely in-memory store would have made those two routes untestable |
| D4 | A non-default `test-harness` feature was added to the **`fastadhunter`** crate too, forwarding to `fah-api/test-harness` and swapping the limiter in `main.rs` | §11.2 requires the measurement harness to run with the relaxed limiter, and §11.4's claim is about **mimalloc**, which only the binary links. A test-target harness would have measured musl's allocator instead. Non-default; `Dockerfile:86` carries no `--all-features` |
| D5 | `crates/fastadhunter/tests/layering.rs` narrowed | The plan's own §6.1 self dev-dependency reads as `fah-api (L3) → fah-api (L3)`. Self-edges are now skipped, **and a new assertion forbids one outside `[dev-dependencies]`**, so the guard is not merely loosened |
| D6 | An unreadable or non-hex `session-secret` is treated as **absent** and regenerated | §2.1 defines empty-as-absent only for the hash. Same fail-safe direction, never boot-fatal, logged at `warn!` |
| D7 | Criterion 1's plan wording ("identical status, body, header set") is unachievable | Success is `204` and failure `401`. Implemented as §5.1's actual sentence: the two **`401` causes** are byte-identical. §17 row 1 contradicts §5.1; §5.1 was followed |
| D8 | The plan file on disk is **revision 3**; the prompt named revision 4 | No revision-4 file exists in the tree. Implemented against revision 3 |

## Gates

```text
cargo fmt --all -- --check                                          GREEN
cargo clippy --workspace --all-targets -- -D warnings               GREEN (0 errors)
cargo clippy --workspace --all-targets --all-features -- -D warnings GREEN (0 errors)
cargo test --all-features --workspace                               GREEN (0 failures)
```

`crates/fah-api/tests/request_coverage.rs` passes with the four new routes
covered by `requests/auth.http`; no `UNCOVERED` entry was needed, because
nothing in that file is ever issued by the suite. No bench: no hot path is
touched.

New test counts: `fah-api` lib 126 (was 100), `tests/api.rs` 88 (was 71).

## Remaining TODOs

1. **RB5009 leg (deferred, D3).** Nothing in this task touched the router; the
   p2.6 L.3 soak is live on `soak-p2.6-11` (`1c430aa`, 0.2.20). Carried forward
   as a live acceptance item, never satisfiable by conversion. Required
   evidence: device and workload, measured login latency, measured peak RSS and
   high-water behaviour, DNS-unmoved result.
2. **Long-run RSS ratchet (criterion 17, second half).** Not answerable on a
   dev box: the §F4 control run moves RSS 10.44 MiB with zero logins, so
   warm-up dominates any burst-driven step over a run of that length. Fold it
   into the device soak.
3. **Recorded risk, not a failure: the transient margin is 0.40 MiB.** Measured
   39.60 MiB against a ≤ 40 MiB allowance whose `ARGON2_PERMITS × m` floor is
   38 MiB. Widening the allowance or dropping `ARGON2_PERMITS` to 1 stays the
   owner's decision. **M1's "falsified at 45.3 MiB" is withdrawn.**
4. **Documentation (§14) — WRITTEN.** Approved and applied; see
   [§Documentation applied](#documentation-14--applied).

## Findings

Independent review, 2026-08-26. Every criterion was re-derived from the code,
the tests and the recorded figures; the author's acceptance matrix above was
not taken as input. Gates were re-run rather than trusted.

### Re-run gates (this review, not the author's)

```text
cargo fmt --all -- --check                                            exit 0
cargo clippy --workspace --all-targets --all-features -- -D warnings  exit 0
cargo test --all-features --workspace                                 exit 0
cargo test --all-features -p fah-api --lib                            126 passed
cargo test --all-features -p fah-api --test api                        88 passed
cargo test --all-features -p fah-api --test request_coverage            2 passed
```

`git diff 9b165ef -- Cargo.lock` adds exactly five packages — `argon2`,
`password-hash`, `base64ct`, `blake2`, `rand_core` — and **not** `aws-lc-rs`,
so §11.1's manifest-edge claim is confirmed independently.
`cargo tree -p fastadhunter -e normal --depth 1` prints `fah-api v0.2.20` with
**no feature list**, confirming structurally — not only by the §11.6 runtime
contrast — that `test-harness` is off in the shipped graph.

### Findings, severity-ranked

**F1 — A cancelled login releases its Argon2 permit while the verification is
still running. Major · implementation defect · BLOCKING.**

`routes.rs:1470-1477` (`auth_login`) and `:1509-1522` (`auth_password`) hold the
`Argon2Permit` in the request future. `password.rs:322-336` runs the actual
Argon2 work inside `tokio::task::spawn_blocking`. A blocking task cannot be
cancelled once started — dropping its `JoinHandle` detaches it — but dropping
the request future **does** drop the permit. A client that sends a login and
then closes the connection therefore frees a permit while its 19 MiB arena is
still live.

Verified, not inferred. A temporary probe against the real `AuthState` (added,
run, removed — the tree is unchanged) took both permits inside spawned tasks,
confirmed `try_argon2_permit()` returned `None` while the two verifications were
in flight, aborted both futures, and then acquired **two** permits immediately,
while the Argon2 work was still running:

```text
test an_aborted_login_releases_its_permit_immediately ... ok
```

Consequence: the semaphore does not bound concurrent Argon2 arenas under
aborted requests, which is the exact property the task text asks it to
guarantee ("Peak RSS is driven by *concurrent* verifications … A small
semaphore bounds it"). The remaining bound is the rate limiter — 30 attempts
per 60 s globally — so a burst of 30 abandoned logins can put ~30 × 19 MiB
≈ 570 MiB of transient arena in flight at once. On a 1 GB RB5009 shared with
RouterOS that is an OOM-of-the-live-resolver risk, not a headroom question.

The permit's lifetime has to be tied to the blocking work, not to the request
future — i.e. moved into the `spawn_blocking` closure so it is released when
the closure returns. Not changed here; it is a defect, not a measurement gap.

**F2 — `api.tls` is read per request from the mutable config snapshot, so a
boot-key patch takes effect immediately for auth while the listener does not
change. Major · implementation defect · BLOCKING.**

`routes.rs:1456` (`auth_login`) and `routes.rs:1548` (`events_socket`) read
`state.config.current().api.tls`. Plan §8 asserts this is safe because
"`api.tls` is a boot key, so it cannot drift under a live connection". That
premise is false: `config_store.rs:130-138` classifies a boot key only for
`restart_required`, then stores the patched `Config` into the live `ArcSwap`
unconditionally (`self.current.store(Arc::new(candidate))`). `api.tls` is in
`BOOT_KEYS` (`config_store.rs:24-26`) but its *value* is published live.

So on a box serving HTTPS, `POST /api/v1/config {"api":{"tls":false}}` — which
`p5-09`'s settings page will make reachable — immediately makes
`POST /auth/login` answer `503 unavailable` ("requires TLS") and makes
`same_origin` compare an `http` scheme against every browser's `https` `Origin`,
so **every cookie-authenticated WebSocket upgrade starts failing `401`**, on a
listener that is still TLS. The mirror case (patching `tls = true` on a
plaintext box) lets login mint a `Secure` cookie the browser discards.

The listener's real TLS state is known at `ApiServer::bind` (the
`Option<TlsConfig>` argument) and is what these two call sites should be
reading. Recorded as a defect, not redesigned here.

**F3 — The startup `warn!` D2 requires was not added. Minor · implementation
defect · undocumented deviation.**

Plan §0.2 D2 and §5.1 both require a boot-time `warn!` stating that session
authentication is unavailable while `api.tls = false` and that bearer
authentication is unaffected. `main.rs:471-473` carries only the pre-existing
p2-era line — "api.tls is disabled — the API key travels in plaintext; see
SECURITY.md" — which names neither half. Not a comment, so hard rule 7 did not
block it; it is simply missing, and it is not in the §Deviations table.

**F4 — RESOLVED by the §F4 re-measurement; sub-point 2 WITHDRAWN. Kept as the
record of why the re-measurement was ordered.** M1's 45.3 MiB is derived
against a baseline the run itself contradicts,
and §11.4 and §11.7 cannot come from one coherent process. Major ·
measurement-methodology defect · supersedes the author's M1.**

Three independent problems, each fatal to the number as written:

1. **The baseline is not steady state.** §11.7 uses `settled steady state =
   29.9 MiB`, yet the same table records `after 60 s idle = 35.1 MiB` — the
   process never returns to 29.9. It is also 17–24 MiB below the documented
   46.6–53.6 MiB steady state that §11.7's own allowance is written against.
   Taking the run's own settled figure, the delta is `75.1 − 35.1 = 40.0 MiB`
   — **at** the ≤ 40 MiB bound, not 5.3 MiB over it. The bound is therefore
   neither shown met nor shown falsified.
2. ~~**§11.4's two figures are mutually impossible.**~~ **WITHDRAWN** — the
   inversion is a sampling artefact, not evidence of mixed runs. See
   [the retraction](#retraction--f4s-second-point-was-wrong), which also
   records the consequence that survives: `process_peak_rss` must not be used
   to bound a transient burst peak in this codebase.
3. **The "+70.2 MiB pile-up" measures the wrong quantity.** Ten bursts of eight
   admit two verifications each — twenty verifications total, never more than
   two concurrent. 120.5 MiB is therefore mimalloc holding freed arenas across
   sequential verifications, not concurrent arena footprint. The allowance is
   written about memory "while verifications are in flight"; allocator
   retention that fully purges within 60 s is a different claim, and it is the
   one §11.4 exists to test — which it passes (45.8 MiB < 48.2 MiB baseline).

Separately, and independent of any measurement: **the plan's ≤ 40 MiB allowance
was set too tight at plan time.** `ARGON2_PERMITS × m` is `2 × 19,456 KiB` =
38 MiB of arena floor, leaving 2 MiB for eight concurrent TLS sessions, their
HTTP state and one not-yet-purged arena. No implementation could have met it.
This is a plan defect, not an implementation one.

Correct disposition at the time this was written: criterion 17 was **not proven
either way**, and the action was a re-measurement with a settled baseline, not a
change to `ARGON2_PERMITS` and not a restatement of the allowance.

**Resolved.** That re-measurement was run —
[§F4](#f4--re-measurement-one-process-settled-baseline). Criterion 17's
transient half is **PROVEN at 39.60 MiB against a ≤ 40 MiB allowance**, from a
verified plateau in one process, with no constant retuned. Its long-run
no-residual-ratchet half is **DEFERRED to the device/soak leg**, because the
control run shows warm-up moves RSS more than the workload does over a dev-box
run of this length. Both the author's "NOT MET at the stated bound" and this
finding's "not proven either way" are superseded by that result.

**F5 — No compile-time guard stops `test-harness` from reaching a release
build. Minor · acceptable deferred.**

D4 states "Production `AuthState` always passes the production values".
`main.rs:441-453` makes the production wiring itself conditional, so a release
built with `--all-features` ships a binary whose login rate limits are
`u32::MAX` — effectively no online-guessing control — announced only by a
`warn!` at boot. The current defences (non-default feature, `resolver = "2"`,
`Dockerfile:86` without `--all-features`) all hold today and I verified them;
the residual is that nothing *mechanically* fails the build. D4's deviation is
justified by §11.4's mimalloc requirement and is recorded as D4; the missing
guard is not. A `compile_error!` on `all(feature = "test-harness",
not(debug_assertions))` would close it. Owner's call, not required to close
this task.

**F6 — The full-map fallback rejects unconditionally rather than consulting the
global bucket. Informational.**

Plan §6 says a full map "falls back to the global bucket, which fails closed".
`password.rs:107-115` increments the global bucket and returns `Limited`
regardless of whether the global cap was actually reached. Stricter than the
plan, in the safe direction, and the unit test
`a_full_map_falls_back_to_the_global_bucket_and_fails_closed` encodes the
stricter behaviour. §6.2 already accepts the lockout trade. Noted so the
wording and the code are not later assumed to agree.

**F7 — Criterion 13's "never in logs" half is not tested. Informational.**

`session.rs:27-37` redacts `Debug` and `Display`, and
`the_secret_never_prints_itself` pins it; five response bodies are scanned. No
test inspects emitted log output. The structural argument is strong — the
secret is never formatted anywhere outside those two impls — but the criterion's
"in logs" clause rests on that argument, not on an assertion.

**F8 — `get_config`'s doc comment still names only the API key and the TLS
private key. Informational.**

`routes.rs:1322-1325`. §9 asked for it to name `auth-hash` and `session-secret`
too. D1 is accepted — `.claude/hooks/no-rust-comments.sh` rejects any edit whose
`new_string` contains a comment marker, which includes touching a region that
already holds one. The invariant the comment states remains **true**: auth
material is not in `Config` at all, so "secrets redacted holds by construction"
is still accurate, merely less complete than §9 wanted.

**F9 — `ApiError::Unauthorized`'s doc comment still says "Missing or invalid API
key" while its message no longer does. Informational.** `error.rs:12` versus
`error.rs:40-43`. Same hook constraint as F8.

**F10 — Password change shares login's rate-limit buckets. Informational.**
`auth_password` calls the same `spend_argon2` (`routes.rs:1509`), so five
password-change attempts from one address inside 60 s lock that address out of
login as well. Harmless and arguably correct — the resource being protected is
Argon2 CPU — but unstated in the plan.

**F11 — `docs/code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md` was
edited on this branch (+48 lines, an interim soak reading). Informational.**
Unrelated to p5-04 and outside this task's diff. Flagged for scope, not for
content.

### Agreed with the author

- **D7 is correct.** Task criterion 1's "identical status, body, header set" is
  unachievable against a `204`/`401` split; §5.1's actual sentence — the two
  `401` causes are byte-identical — is the falsifiable one, and
  `a_wrong_password_is_byte_identical_to_a_password_whose_hash_was_replaced`
  compares status, body and every header but `date`.
- **D2 is correct and criterion 8 genuinely holds.** `git diff -U0` over
  `tests/api.rs` deletes nine lines: three import lines and six
  `_data_dir` → `data_dir` renames. No bearer assertion was touched.
- **D5 is correct.** The self-edge skip is paired with a new assertion that a
  self-dependency may not appear outside `[dev-dependencies]`
  (`layering.rs:155-159`), so the guard is narrowed, not loosened.
- **The DNS-unmoved workload choice is right, and the author's own hedging
  undersells it.** A locally blocked name keeps the query entirely on the Tokio
  worker path — recv, parse, match, send — which is precisely where a
  verification running on a runtime worker would show up. Adding an upstream
  round trip would have buried a 45 ms stall under network latency, which is
  what the discarded 1604 ms run demonstrates. Discarding it was correct.

### Where the author's evidence does not carry the claim

- **Criterion 15's measurement is non-probative** (though the criterion itself
  is satisfied — see the matrix). Two permits against a many-core dev box means
  worker starvation was never possible in that configuration; no positive
  control was run to show the probe *can* see a stall; and a 1 ms-resolution
  sampler reporting `median 0 ms / max 4 ms` on both legs cannot discriminate.
  What actually proves criterion 15 is the code: `password.rs:313` and `:322`
  both enter `tokio::task::spawn_blocking`, and `main.rs:440` wraps the
  boot-time hash the same way. That is direct and complete.
- **Criterion 17's evidence is self-contradictory** — F4.
- **Criterion 16 is proven only for the non-cancelled path** — F1.

### Independent acceptance derivation

Criterion → what was measured or executed → derived value → bound → verdict.
"PROVEN" means I re-derived it from code or from a test I re-ran; "ASSERTED"
means the claim rests on argument rather than on an executed check.

| # | Criterion | Inputs I checked | Derived | Bound | Verdict |
| - | --------- | ---------------- | ------- | ----- | ------- |
| 1 | Right/wrong password indistinguishable | `api.rs:3070-3110`; `comparable()` drops only `date` | status, body and full header set equal across both `401` causes | byte-identical | **PROVEN** |
| 2 | Cookie authenticates `/api/v1/*` | `api.rs:3115` over real TLS | `200` with cookie, `401` bare | `200` | **PROVEN** |
| 3 | Expiry enforced from the token | `session.rs:72-90` compares the payload expiry; `api.rs:3132` sends a past-expiry token | `401` `unauthorized` | `401` | **PROVEN**. The server only ever receives `Cookie: name=value`, so no client-side attribute is reachable; the test's `expires` request header is decorative, not the proof |
| 4 | Unknown token version rejected | `session.rs:80` before MAC; `session.rs:221`, `api.rs:3155` | `401`, version `2` with a valid MAC | `401` | **PROVEN** |
| 5 | Socket upgrades on the cookie alone | `api.rs:3563` | `101` | `101` | **PROVEN** |
| 6 | Foreign `Origin` rejected, matching accepted | `api.rs:3575`; check at `routes.rs:1547-1552`, **before** `subscribe_socket()` at `:1554` | both refused, subscriber count 0 | reject / accept | **PROVEN** |
| 7 | Bearer upgrade with no `Origin` succeeds | `api.rs:3604`, header and `?token=` forms | `101` both | `101` | **PROVEN** |
| 8 | Bearer access unchanged | `git diff -U0` over `tests/api.rs`: 9 deletions, all imports/renames | zero assertion edits | zero diff to assertions | **PROVEN** |
| 9 | Password change requires the current password | `api.rs:3230-3252` | `401`, secret file byte-identical | `401`, nothing written | **PROVEN** |
| 10 | Password change kills every session | `api.rs:3285-3296`; `password.rs:697` crash case; `password.rs:764` race case | old cookie `401`; crash leaves old password + zero sessions; raced token dead | all three | **PROVEN** |
| 11 | `GET /config` carries no auth material | `api.rs:3335-3342`, response body | no `auth` key, no `$argon2` | absent | **PROVEN** |
| 12 | `POST /config` with `auth.password_hash` | guard `routes.rs:1371-1379`; `api.rs:3345` | `422`, message names `/api/v1/auth/password` | `422` | **PROVEN** |
| 13 | Secret never in `/config`, logs or errors | `session.rs:27-37` + `api.rs:3300` scanning five bodies | no occurrence | absent | **PROVEN for responses · ASSERTED for logs** (F7) |
| 13b | Four store states, none boot-fatal | four tempdir cases, `password.rs:601-690` | matches §2.1's table exactly | as stated | **PROVEN** |
| 13c | `no-store` on every auth response | assertions across `api.rs:3047`, `:3070`, `:3132`, `:3230`, `:3364`, `:3389`, `:3404`, `:3417`, `:3453` | present on all seven rows | present | **PROVEN** |
| 14 | `api.tls = false` rejects login | `routes.rs:1456`; `api.rs:3453` | `503` `unavailable`, no `Retry-After`, no `Set-Cookie`, message names TLS | as stated | **PROVEN at the handler** · defective at the source of truth (F2), and F3 missing |
| 14b | The two `503` causes are distinguishable | `api.rs:3364` vs `:3453` | `Retry-After: 1` vs header absent, same slug | distinguishable | **PROVEN** |
| 14c | Only `login` is gated on TLS | `api.rs:3477-3517`, bearer against the other three | `204` / `204` + secret changed / `401` then `204` | as stated | **PROVEN** |
| 14d | Relaxed limiter absent from the shipped build | `cargo tree -p fastadhunter -e normal` (no features on `fah-api`), `Dockerfile:86`, `Cargo.toml:2`, plus §11.6's runtime contrast | production constants ship | absent | **PROVEN** · residual F5 |
| 15 | Argon2id runs on `spawn_blocking` | `password.rs:313`, `:322`; `main.rs:440` | every Argon2 call site enters the blocking pool | never on a runtime worker | **PROVEN structurally.** The §11.5 latency leg is non-probative and should not be cited as the proof |
| 16 | Semaphore bounds concurrency; saturation answers `503` + `Retry-After` | `api.rs:3364` (`held.len() == 2`), `password.rs:800`, §11.5's 2/6/0 | exactly 2 in flight, 6 × `503` `Retry-After: 1`, 0 × `429` | 2 in flight | **PROVEN for completed requests · FALSIFIED for cancelled ones (F1)** |
| 17 | Transient allowance stated separately and respected | Re-measured in §F4: E = 40.09 MiB settled (plateau verified), F = 79.69 MiB sampled during the burst, G = 39.69 MiB after 60 s | **39.60 MiB** transient delta | ≤ 40 MiB | **transient bound PROVEN · long-run ratchet half DEFERRED to the device leg.** Was "not proven either way" before the §F4 re-measurement |
| 18 | Rate limiting bounded in memory | `password.rs:483`, 10 000 addresses at production limits | map ≤ 128 at every step | ≤ 128 | **PROVEN** |
| 19 | Rate limiting rejects a burst | `api.rs:3417` at `production_limits()`; §11.6 on the shipped binary | 6th attempt `429` + `Retry-After` in 1..=60 | `429` | **PROVEN** |
| 20 | Parameters recorded with device, workload, measured peak RSS | dev leg only | recorded for x86 container | device leg required | **DEFERRED (D3)** — correctly, and never satisfiable by the ×9 conversion |
| 21 | Gates green, `request_coverage.rs` included | re-run above; `requests/auth.http` carries all four routes; scraper at `request_coverage.rs:65-90` reads the `v1` chain the routes sit in | all green, 216 `fah-api` tests | green | **PROVEN** |

**Totals as first derived (before the fix round): 20 proven · 1 proven for
responses and asserted for logs (13) · 1 proven structurally with a
non-probative measurement leg (15) · 1 proven only for the non-cancelled path
(16) · 1 not proven either way (17) · 1 correctly deferred (20).**

The fix round moves three of those. **Final standing is in the
[re-derived table](#re-derived-criteria) and the
[status matrix](#final-statusfinding-matrix):** 24 proven, criterion 13's
"in logs" half asserted, criterion 17's long-run ratchet half and criterion 20
deferred to the device leg.

### Classification of every issue raised

| Finding | Class | Blocks close? |
| ------- | ----- | ------------- |
| F1 permit released on cancellation | implementation defect | **yes** |
| F2 `api.tls` read from the mutable snapshot | implementation defect | **yes** |
| F3 missing startup `warn!` | implementation defect | no — fix with F1/F2 |
| F4 §11.4/§11.7 baseline and high-water contradiction | measurement-methodology defect | no — re-measure |
| F5 no compile guard on `test-harness` | acceptable deferred | no |
| F6 full-map fallback stricter than §6 | acceptable deferred | no |
| F7 "never in logs" untested | test gap | no |
| F8, F9 stale doc comments | acceptable deferred (hook-constrained) | no |
| F10 shared rate-limit buckets | acceptable deferred | no |
| F11 unrelated p2.6 file edited | scope note | no |
| RB5009 leg | acceptable deferred under D3 | no |
| Author's M1 as written | superseded by F4 | no |

Nothing here asks for a runtime change on the strength of thin evidence. F1 and
F2 are defects in shipped code, established from the code itself and, for F1,
from an executed probe. F4 asks for a re-measurement, not a re-design — and in
particular `ARGON2_PERMITS` was **not** moved. The re-measurement was run and
the allowance holds; the final disposition of every row above is in the
[status matrix](#final-statusfinding-matrix).

### Architecture, performance and memory — no findings

- §1's shape is exactly what shipped: `password.rs`, `session.rs`, extensions to
  `auth.rs` / `error.rs` / `routes.rs` / `state.rs`, one new runtime crate, no
  config-schema change. No `fah-model` or `fah-config` edit, so hard rules 1
  and 2 are untouched, and `layering.rs` still passes with a *narrower* guard.
- **No hot-path contact.** `git diff --stat 9b165ef` touches no `fah-dns`,
  `fah-rules` or `fah-model` file. The `RwLock`, the limiter `Mutex` and the
  semaphore are admin-plane only, so hard rule 3 holds and no bench is owed.
- New retained state is bounded: one hash `String`, one 32-byte HMAC key, a
  `Semaphore` and a `HashMap` hard-capped at 128 buckets (~8 KiB). Hard rule 4
  holds — with F1 as the one exception, which is transient rather than retained.
- Rotation ordering is right and matches §3.1/§5.1: `replace_password`
  (`password.rs:294-301`) rotates the secret then writes the hash, both inside
  the write guard, file before memory. Every failure mode lands on
  old-password-with-zero-sessions, which is the recoverable direction.
- `verify_and_mint` (`password.rs:277-289`) holds the read guard across
  verification *and* mint, so no login can straddle a rotation — E2's actual
  requirement, and `no_login_racing_a_rotation_yields_a_surviving_session`
  pins it.

## Fix round — F1, F2, F3, F5 applied; F4 re-measured

Requested after the review above. Scope was fixed by the owner: apply F1–F3,
close the cheap non-behavioural findings, re-measure F4, and re-derive
criterion 17 from the new numbers. No second architectural pass.

### F1 — the permit now belongs to the blocking task

`Argon2Permit` is moved **into** the `spawn_blocking` closure instead of being
borrowed by the request future:

| Before | After |
| ------ | ----- |
| `verify_and_mint(&Argon2Permit, String)` | `verify_and_mint(Argon2Permit, String)` |
| `verify_only(&Argon2Permit, String) -> Result<bool, _>` | `verify_only(Argon2Permit, String) -> Result<(bool, Argon2Permit), _>` |
| `hash_password_off_runtime(&Argon2Permit, String)` | `hash_password_off_runtime(Argon2Permit, String)` |
| `verify_password(hash, candidate) -> bool` | `verify_password(permit, hash, candidate) -> (bool, Argon2Permit)` |

The permit crosses into the closure, is returned out of it where the caller
still needs it (`verify_only`, so E4's "one permit spans the whole password
change" survives), and is dropped when the blocking work returns. The handlers
no longer call `drop(permit)` — there is nothing left in the request future to
drop. `auth_password` still holds one permit across verification **and**
hashing, run sequentially, so the `ARGON2_PERMITS × m` ceiling is unchanged.

The two handler-side windows where the permit sits in the future
(`verify_only` returning → `hash_password_off_runtime` being called) carry no
`.await` between them, so no cancellation point exists while an arena is live.

**Regression test**, `password.rs`
`an_abandoned_verification_keeps_its_permit_until_the_argon2_work_ends`:
takes both permits inside spawned tasks, confirms saturation, aborts both
futures, and then asserts `try_argon2_permit()` is still `None` — the assertion
the old code failed — before waiting for the eventual release. The "still
running" half rests on a margin, not on an observation: the abort-to-assert gap
is microseconds against a verification of tens of milliseconds in release and
seconds in the dev profile the gates use.

**Trade, stated rather than discovered.** The fix converts a memory-exhaustion
vector into a permit-occupancy one: abandoned logins now hold their permit for
the full verification, so a burst of abandoned requests answers `503`
`unavailable` on login instead of growing RSS. That is the direction §6's
`try_acquire` design already chose, and memory stays bounded by
`ARGON2_PERMITS × m` under every arrival pattern.

### F2 — the listener's own TLS state is the source of truth

`ApiServer::bind` already computed `tls_config.is_some()` one line below where
it built the state (`server.rs:71-72`). The two lines are swapped and the value
is passed into `AppStateBuilder::build(events, tls)`, which stores it as
`AppState.tls`. `bind`'s signature does not change (§10 holds), the binary is
untouched, and `AppStateBuilder` gains no field — the flag is derived where the
listener is actually configured, so there is exactly one source of truth and it
cannot drift.

`auth_login` and `events_socket` now read `state.tls`. `grep` over
`crates/fah-api/src/` confirms no `api.tls` read remains outside
`config_store.rs`'s boot-key list, which is untouched: boot-key semantics,
`restart_required`, and the live publication of patched values are all
unchanged. The defect was auth reading a value it had no business reading, not
the config store's behaviour.

**Regression test**, `tests/api.rs`
`patching_the_api_tls_boot_key_does_not_move_the_auth_decision`: patches
`api.tls = false` over bearer, asserts `restart_required: true` **and** that
`GET /config` now reports `false` — pinning that the patched value really is
published live — then asserts login still answers `204` with a `Set-Cookie`,
and that a cookie-authenticated WebSocket upgrade with an `https` `Origin` still
completes. Both would have failed before the fix.

### F3 — the startup warning names both halves

`main.rs`'s `api.tls`-disabled `warn!` now states that dashboard session login
is unavailable because the cookie requires a `Secure` `__Host-` prefix, that
bearer-key authentication is unaffected, and that the other three
`/api/v1/auth` routes stay usable. Confirmed in the measurement container's own
boot log.

### F5 — the harness feature cannot reach a release build

`crates/fah-api/src/lib.rs` gains

```rust
#[cfg(all(feature = "test-harness", not(debug_assertions)))]
compile_error!("…must never be compiled into a release build…");
```

Verified both ways:

```text
cargo check --release --all-features -p fah-api
  error: the test-harness feature carries relaxed authentication limits …
cargo check --release -p fastadhunter                              exit 0
```

`fastadhunter`'s own `test-harness` feature forwards to this one, so a single
guard covers both crates.

**Two consequences, both accepted and recorded.**

1. `cargo test --release --all-features` no longer compiles. The gates at §13
   run the dev profile, and nothing in the repository builds tests in release,
   so no gate moves.
2. §11.2's measurement harness has to be built with debug assertions on:
   `RUSTFLAGS="-C debug-assertions=yes" cargo build --release --locked -p
   fastadhunter --features test-harness`. That keeps optimisation and mimalloc
   — which is what §11.4's claim is actually about — while satisfying the
   guard. The `argon2` crate uses explicit `wrapping_add`, so the added
   overflow checks do not land inside the Argon2 inner loop; the RSS legs are
   unaffected either way, and no latency leg was re-run here.

### F6 — the full-map fallback, worded as implemented

When `per_address` is at `max_tracked` and eviction of expired buckets does not
free a slot, the request is **rejected outright** with `429` and the global
bucket's `retry_after`. It is not admitted-if-the-global-cap-allows: the global
bucket is incremented, and `Limited` is returned unconditionally
(`password.rs:109-119`). §6's phrase "falls back to the global bucket, which
fails closed" describes the intent; the code is the stricter reading of it, and
`a_full_map_falls_back_to_the_global_bucket_and_fails_closed` pins the strict
behaviour with `global: u32::MAX`, where a threshold-consulting implementation
would have returned `Allowed`. §6.2's accepted trade — a spoofed-source flood
locks the operator out of the admin plane until the window rolls, while DNS
answering continues — applies exactly as written.

### F10 — login and password change share one protection budget

`auth_password` spends the same `spend_argon2` gate as `auth_login`
(`routes.rs`), so both routes draw on the same per-address bucket (5 / 60 s),
the same global bucket (30 / 60 s) and the same `ARGON2_PERMITS` semaphore.
This is deliberate: the resource being protected is Argon2 CPU and RAM, not
credential-guess count, and a password change costs two Argon2 operations
rather than one. The visible consequence is that five password-change attempts
from one address inside 60 s also lock that address out of login until the
window rolls.

### F7 — not done, condition not met

`fah-api` carries no `tracing-subscriber` dependency and no test writer, in
`src/` or in `tests/`. Capturing emitted log lines would mean introducing a
logging harness for one assertion, which the fix round explicitly excludes. The
"never in logs" half of criterion 13 therefore remains an argument from
`session.rs`'s redacted `Debug`/`Display` plus the absence of any other
formatting site, not an executed check.

### F8, F9 — deferred

`.claude/hooks/no-rust-comments.sh` rejects any edit whose replacement text
contains a comment marker, which includes an edit that merely re-emits the
existing doc comment around the line being changed. Both are stale wording, not
false statements. Unchanged.

### Out of the fix round

`docs/code-review/phase2.6/p2.6-11-optin-deploy-soak-review.md` is left exactly
as it was found in the working tree (F11). Nothing was committed.

## F4 — re-measurement, one process, settled baseline

Everything below is a **single container and a single process**,
`fastadhunter:p504-harness`, built from the repository `Dockerfile` with only
the build line changed as §F5 records. Runtime `distroless/static` on x86-64,
mimalloc linked, `lists = []` so `ruleset_bytes = 68`. `process_rss` is
`/proc/self/status` `VmRSS`; `committed` is mimalloc's
`allocator_committed_bytes`. Relaxed limiter, so the rate limiter is not a
confounder — the same split §11.2 requires.

The correction the old figures needed was the baseline: the reading is taken
**after** the warm-up load and a settled plateau, and the plateau is verified by
a second read rather than assumed.

### Cycle 1 — §11.4 ratchet and §11.7 allowance

| Point | `process_rss` | `process_peak_rss` | committed |
| ----- | ------------- | ------------------ | --------- |
| A settled baseline (warm-up, then 120 s idle) | **32.02 MiB** | 71.07 | 71.31 |
| A' re-read 30 s later (plateau check) | **32.02 MiB** | 71.07 | 71.31 |
| B after 200 sequential logins | 51.62 MiB | 71.07 | 71.31 |
| C after 10 bursts of 8 concurrent | 78.10 MiB | 77.46 | 113.50 |
| D after 60 s idle | 40.09 MiB | 77.46 | 113.50 |
| D' after 120 s idle | 40.09 MiB | 77.46 | 113.50 |
| E settled steady state (further 60 s) | **40.09 MiB** | 77.46 | 113.50 |
| F max `process_rss` sampled during one 8-way burst | **79.69 MiB** | — | — |
| G after 60 s idle | 39.69 MiB | 79.02 | 113.50 |

**Criterion 17, derived:**

| Quantity | Value |
| -------- | ----- |
| settled steady state before the measured burst (E) | 40.09 MiB |
| peak while verifications were in flight (F) | 79.69 MiB |
| **transient delta** | **39.60 MiB** |
| stated allowance | ≤ 40 MiB |
| return within 60 s of idle (G vs E) | 39.69 vs 40.09 MiB, −0.40 |

**Within the allowance, and it returns.** `m` did not move, so §11.7's
recompute-and-re-measure clause does not fire; `ARGON2_PERMITS` stays at 2 and
the allowance stays at ≤ 40 MiB, both unchanged as instructed. The margin is
thin — 0.40 MiB against a `ARGON2_PERMITS × m` floor of 38 MiB — which is the
plan-side observation the earlier review already recorded and which no
measurement can change.

### Cycle 2 — a second identical load cycle on the same process

| Point | `process_rss` |
| ----- | ------------- |
| H floor before cycle 2 | 39.54 MiB |
| I after 200 sequential logins | 59.58 MiB |
| J after 10 bursts of 8 | 79.62 MiB |
| K after 120 s idle | 42.77 MiB |
| L after 240 s idle | 46.24 MiB |

The settled floor did not come back to A's 32.02 MiB, and cycle 2's floor sits
above cycle 1's. Read on its own that looks like a ratchet — which is exactly
what §11.4 exists to test, so it was tested rather than argued.

### Control — the same image, same config, zero logins

Same image, same configuration, same host, **zero logins** — only a
`GET /api/v1/debug/memory` every 30 s:

| t | `process_rss` |
| - | ------------- |
| 0 s | 23.17 MiB |
| 120 s | 25.32 MiB |
| 240 s | 27.25 MiB |
| 480 s | 30.05 MiB |
| 720 s | 33.73 MiB |
| 960 s | 33.61 MiB |
| 1170 s | 33.61 MiB |

**+10.44 MiB over the first twelve minutes with no Argon2 executed at all**,
then flat from t = 720 s. That is the process's own warm-up — the 300 s stats
snapshot cycle, the history writers, the cache machinery — and it is the same
curve the p2.6 L.3 soak records at a longer timescale, where RSS warms through
hour 8 before the drift window opens
([p2.6-11](../phase2.6/p2.6-11-optin-deploy-soak-review.md) interim reading:
4 h means 50.68 → 57.38 MiB across hours 0–24).

**So the floor movement in cycles 1 and 2 is not attributable to Argon2.** A
20-minute run cannot separate a login-driven ratchet from ordinary warm-up,
because warm-up is the larger term over that window. §11.4's ratchet question
is not answerable at this timescale in either direction, and the instrument
that answers it is the device soak, not a dev-box burst run.

What §11.4 *can* say, and does: every reading returns to a plateau within 60 s
of the load stopping (D = D' = E = 40.09 MiB; G returns to 39.69 MiB after the
§11.7 burst), and no reading stays anywhere near the 78–80 MiB burst level. No
step proportional to the number of verifications survives. mimalloc's
`allocator_committed_bytes` does **not** come back down — 71.31 → 113.50 MiB,
held for the rest of the run — which is worth recording because it is committed
address space, not resident memory, and the ≤ 128 MB budget is written against
RSS.

### Retraction — F4's second point was wrong

The earlier review argued that `process_rss` 120.5 MiB alongside
`process_peak_rss` 119.3 MiB proved §11.4 and §11.7 came from different runs,
because `ru_maxrss` is a monotone high-water. **That inference is withdrawn.**
The same inversion reproduced twice in this run, which is a single container and
a single process by construction:

| Point | `process_rss` | `process_peak_rss` |
| ----- | ------------- | ------------------ |
| C | 78.10 MiB | 77.46 MiB |
| J | 79.62 MiB | 79.02 MiB |

The mechanism is in the sampling, not in the runs. `TelemetrySnapshot::collect`
(`telemetry.rs:69`) reads `VmRSS` from `/proc/self/status` **first** and
`state.telemetry.process()` — `getrusage(RUSAGE_SELF).ru_maxrss` — **second**.
Linux refreshes `mm->hiwater_rss` only at unmap and exit points, and
`getrusage` returns `max(stored hiwater, current RSS)`. If RSS spikes and then
falls between the two reads, the stored hiwater can be stale and the current
RSS already lower, so `ru_maxrss` legitimately comes back **below** a `VmRSS`
sample taken microseconds earlier at the top of the spike.

The author's §11.4 pair is therefore not evidence of anything, and neither is
mine. What survives, and is worth carrying forward:

**`process_peak_rss` must not be used to bound a transient burst peak on this
codebase.** It under-reports spikes that are freed before the next unmap. The
instrument for a burst peak is a sampled `process_rss`, which is what both runs
actually used for the delta — so no derived figure moves.

F4's first point stands and is confirmed: 29.9 MiB was an under-warmed
baseline, exactly as this run's 32.02 MiB was under-warmed against the same
process's later 40.09 MiB plateau. F4's third point stands unchanged.

### Re-derived criteria

| # | Criterion | Inputs | Derived | Bound | Verdict |
| - | --------- | ------ | ------- | ----- | ------- |
| 14 | `api.tls = false` rejects login | `state.tls` from `bind`; `with_tls_off_only_login_is_refused_and_it_advertises_no_retry` **and** `patching_the_api_tls_boot_key_does_not_move_the_auth_decision` | `503` `unavailable`, no `Retry-After`, no `Set-Cookie`; a live boot-key patch moves neither login nor the `Origin` scheme | as stated | **PROVEN** — F2 closed, and the config-patch case is now pinned by a test |
| 14d | Relaxed limiter absent from the shipped build | `compile_error!` guard, verified failing under `--release --all-features` and passing under `--release`; `cargo tree -p fastadhunter -e normal`; `Dockerfile:86` | mechanically impossible, not merely absent | absent | **PROVEN** — F5 closed |
| 15 | Argon2id runs on `spawn_blocking` | both call sites still enter the blocking pool, now owning the permit | never on a runtime worker | unmoved | **PROVEN structurally**, unchanged |
| 16 | Semaphore bounds concurrency; saturation answers `503` + `Retry-After` | `a_saturated_verifier_answers_503_with_a_retry_after_of_one`; `the_semaphore_bounds_concurrent_verifications`; `an_abandoned_verification_keeps_its_permit_until_the_argon2_work_ends` | exactly 2 permits, `503` + `Retry-After: 1`, and an aborted request no longer frees one early | 2 in flight under every arrival pattern | **PROVEN** — F1 closed; the cancellation hole is now covered by a test that fails against the old code |
| 17 | Transient allowance stated separately and respected | E = 40.09 MiB settled, plateau verified by a second read; F = 79.69 MiB sampled during an 8-way burst; G = 39.69 MiB after 60 s | **39.60 MiB** transient delta; returns to the plateau within 60 s | ≤ 40 MiB above steady state | **PROVEN for the transient half.** The no-residual-step half is **not answerable at this timescale** — the control run moves 10.44 MiB with zero logins, so warm-up dominates. Carried into the device leg |

`ARGON2_PERMITS` stays at 2. The allowance stays at ≤ 40 MiB. `m` stays at
19,456 KiB. Nothing was retuned to make a number fit.

**One caveat on this run's concurrency tally, stated rather than buried.** The
8-way burst driver spawns eight separate `curl` processes from a Windows shell,
and process spawn is slower than one release-build verification, so arrivals
stagger: the bursts answered 5 × `204` / 3 × `503` rather than the 2 / 6 the
author's §11.5 recorded. That does not weaken the peak figure — the semaphore
caps concurrent verifications at 2 regardless of arrival pattern, and F is an
empirical maximum over the whole burst — but this run neither reproduces nor
contradicts §11.5's 2 / 6 / 0 tally. The semaphore bound itself is proven by
the two tests named in row 16, not by either burst run.

### Gates, re-run after the fixes

```text
cargo fmt --all -- --check                                            exit 0
cargo clippy --workspace --all-targets -- -D warnings                 exit 0
cargo clippy --workspace --all-targets --all-features -- -D warnings  exit 0
cargo test --all-features --workspace                    1195 passed, 8 ignored
cargo test --all-features -p fah-api --lib                            127 passed
cargo test --all-features -p fah-api --test api                        89 passed
cargo test --all-features -p fah-api --test request_coverage            2 passed
cargo check --release -p fastadhunter                                 exit 0
cargo check --release --all-features -p fah-api          compile_error, as designed
```

Two tests added over the reviewed state: the F1 abort regression in
`password.rs` and the F2 boot-key regression in `tests/api.rs`.

## Final status/finding matrix

Every finding raised anywhere in this file, with its closing disposition. This
table supersedes the intermediate classifications above.

| # | Finding | Class | Disposition | Evidence |
| - | ------- | ----- | ----------- | -------- |
| F1 | Argon2 permit released when the request future is cancelled | implementation defect, Major | **FIXED** | Permit moved into the `spawn_blocking` closure; `an_abandoned_verification_keeps_its_permit_until_the_argon2_work_ends` fails against the old code |
| F2 | `api.tls` read per request from the mutable config snapshot | implementation defect, Major | **FIXED** | `AppState.tls` derived once in `ApiServer::bind`; `patching_the_api_tls_boot_key_does_not_move_the_auth_decision` |
| F3 | Startup `warn!` did not name the session-login consequence | implementation defect, Minor | **FIXED** | `main.rs` warning; confirmed in the measurement container's boot log |
| F4 | §11.4/§11.7 measurement methodology | measurement-methodology defect | **RESOLVED by re-measurement**; its high-water sub-claim **WITHDRAWN** | Single-process run, verified plateau, 39.60 MiB ≤ 40 MiB; control run isolates warm-up |
| F5 | Nothing mechanically stopped `test-harness` reaching a release build | Minor | **FIXED** | `compile_error!` guard, verified failing under `--release --all-features` and passing under `--release` |
| F6 | Full-map rate-limit fallback stricter than §6's wording | documentation | **DOCUMENTED** as implemented | §F6 above; `a_full_map_falls_back_to_the_global_bucket_and_fails_closed` |
| F7 | Criterion 13's "never in logs" half untested | test gap | **DEFERRED** — precondition not met | No `tracing-subscriber` and no test writer in `fah-api`; a logging harness is out of scope |
| F8 | `get_config` doc comment does not name the two new files | informational | **DEFERRED** — hook-blocked | `.claude/hooks/no-rust-comments.sh`; the invariant it states is still true |
| F9 | `ApiError::Unauthorized` doc comment says "API key" | informational | **DEFERRED** — hook-blocked | Same constraint |
| F10 | Login and password change share one Argon2 protection budget | documentation | **DOCUMENTED** | §F10 above |
| F11 | Unrelated `p2.6-11` working-tree edit | scope note | **OUT OF SCOPE** — left untouched, must not enter the p5-04 commit | `git status` |
| M1 | "The ≤ 40 MiB allowance is falsified at 45.3 MiB" | superseded | **WITHDRAWN** | Under-warmed baseline; superseded by §F4's 39.60 MiB |

### Acceptance criteria, final standing

| Standing | Criteria |
| -------- | -------- |
| **PROVEN** (re-derived from code or from a re-run test) | 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13b, 13c, 14, 14b, 14c, 14d, 15, 16, 18, 19, 21 |
| **PROVEN in part** | 13 — proven for responses and errors; the "in logs" half rests on `session.rs`'s redacted `Debug`/`Display` plus the absence of any other formatting site (F7) |
| **PROVEN in part · rest DEFERRED** | 17 — transient-memory bound **PROVEN at 39.60 MiB ≤ 40 MiB** from a verified plateau; long-run no-residual-ratchet half **DEFERRED** to the device/soak leg |
| **DEFERRED under D3** | 20 — RB5009 device leg; never satisfiable by the ×9 conversion |

**24 of 26 criteria fully proven · 1 proven in part (13) · 1 proven in part with
its remainder deferred (17) · 1 deferred (20).**

### Deferred acceptance items carried forward

1. **RB5009 device leg (criterion 20 / D3).** Device and workload, measured
   login latency, measured peak RSS and high-water behaviour, DNS-unmoved
   result. Nothing in this task touched the router; the p2.6 L.3 soak stays
   undisturbed.
2. **Long-run RSS ratchet (criterion 17, second half).** Answerable only
   against a settled, hours-old process. The dev-box control run moves RSS
   10.44 MiB with zero logins, so warm-up dominates any burst-driven step at
   that timescale. Fold into the device soak.
3. **Transient margin of 0.40 MiB — recorded risk observation, not a failure.**
   Measured 39.60 MiB against a ≤ 40 MiB allowance whose `ARGON2_PERMITS × m`
   floor is 38 MiB. `ARGON2_PERMITS`, the Argon2 parameters and the allowance
   are all unchanged; whether to widen the allowance or drop `ARGON2_PERMITS`
   to 1 stays the owner's decision.
4. **Documentation (§14) — WRITTEN**, no longer carried forward. See below.

## Documentation (§14) — applied

Approved by the owner and written in this round. Plan §14's table, with what
actually landed:

| Document | Change |
| -------- | ------ |
| [API.md](../../../API.md) | §Session authentication promoted from *reserved* to live: the four routes with their bodies and every outcome, the token layout, the 7-day absolute lifetime, first-run and recovery, the middleware rule, `Origin` on the upgrade, the `/config` omission, `no-store`. §Authentication now names both exemptions and the cookie-or-bearer rule. §Error format's code set expanded to eight and given the three-row `Retry-After` discriminator table. `GET /config` says *omits*, not redacts; `POST /config` documents the `auth` `422`. |
| [SECURITY.md](../../../SECURITY.md) | §API access: "No users, roles or sessions in Phase 1" replaced. New §Dashboard sessions (storage, cookie, lifetime trade, the logout vs `logout-all` distinction, rate limiting and its admin-plane lockout trade, `Origin` and the `Host`-derivation assumption), §Password recovery (including that a reset rotates the secret, and the mirror `/data/session-secret` deletion), §`api.tls = false` removes session login with the localhost collateral. §Data at rest gains both files and why a hash is still secret. The crypto rule gains `argon2` and `aws-lc-rs`. |
| [CONFIGURATION.md](../../../CONFIGURATION.md) | §First boot gains the generated password and the session secret; §Volumes gains both files and what regenerating the secret costs. **States explicitly that there is no `[auth]` section and no mutability class** — decision 1, which supersedes the task's original sentence. The `[api]` comment block names both exemptions and the `tls = false` consequence. |
| [root CLAUDE.md](../../../CLAUDE.md) | Hard rule 5's crypto list gains `argon2` and `aws-lc-rs`, each with what it is for. |
| [CONTRIBUTING.md](../../../CONTRIBUTING.md) | New §`test-harness` is a dev-profile-only feature: what it carries, the `compile_error!` guard, that `cargo test --release --all-features` deliberately does not compile, the `RUSTFLAGS` incantation for the measurement harness, and never `--all-features` in a release build. The gate line is corrected to `cargo test --all-features --workspace`. §Tests gains the `request_coverage.rs` fixture rule. |
| CONTEXT.md | Untouched — `permitted` belongs to `p5-09`. |

Plan §14's note stands: the task's sentence *"CONFIGURATION.md gains the
`[auth]` section and its mutability class"* is void under decision 1 and is
corrected in CONFIGURATION.md rather than silently skipped.

## Verdict

**PASS WITH DEFERRED FINDINGS.**

The two blocking defects are closed and each is pinned by a test that fails
against the code as reviewed:

- **F1** — the Argon2 permit now belongs to the blocking task, so an abandoned
  request cannot free it while its arena is live. The transient ceiling is
  `ARGON2_PERMITS × m` under every arrival pattern, not only under well-behaved
  clients.
- **F2** — authentication reads the listener's own TLS state, derived once in
  `ApiServer::bind`. Config-store boot-key semantics are untouched and there is
  no second mutable source.

**F3** and **F5** landed in the same round; **F6** and **F10** are documented
above as implemented rather than as the plan worded them; **F7** was skipped
because its stated precondition — existing deterministic log capture — does not
hold; **F8** and **F9** stay deferred behind the no-Rust-comments hook.

Criterion 17's transient half is now measured properly and **met at 39.60 MiB
against a ≤ 40 MiB allowance**, from a settled baseline in one coherent
process, with no constant retuned. Its no-residual-step half, and criterion 20
in full, carry forward to the RB5009 leg under D3 — the control run shows why:
on a dev box over twenty minutes, warm-up moves RSS more than the workload
does, and only the device soak can separate them.

Carried forward as live acceptance items:

1. **RB5009 leg (criterion 20, D3)** — device and workload, measured login
   latency, measured peak RSS and high-water behaviour, DNS-unmoved result.
   Never satisfied by the ×9 conversion. Nothing in this task touched the
   router.
2. **Ratchet over a long run (criterion 17, second half)** — answerable only
   against a settled, hours-old process; fold it into the device soak rather
   than repeating a dev-box burst.
3. **The ≤ 40 MiB allowance is 2 MiB above the `ARGON2_PERMITS × m` floor.**
   Measured margin: 0.40 MiB. Whether to widen the allowance or drop
   `ARGON2_PERMITS` to 1 remains the owner's decision — it is a plan-side
   choice, not an implementation detail, and no measurement will change the
   arithmetic.
Documentation is **no longer deferred**: the §14 set was approved and written in
this round — API.md, SECURITY.md, CONFIGURATION.md, root CLAUDE.md and
CONTRIBUTING.md. See [§Documentation applied](#documentation-14--applied).
