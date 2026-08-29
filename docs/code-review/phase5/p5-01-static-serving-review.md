# p5-01 — Static Serving from fah-api — Review

## Implementation Summary

`fah-api` now serves the baked `/web` directory on the same origin as the API,
and the runtime image carries a fixture bundle so the delivery path is proven
before any frontend source exists.

| What | Where |
| ---- | ----- |
| Static surface (`/`, `/assets/*`, SPA fallback), cache + `Vary` layers, boot check | `crates/fah-api/src/web.rs` (new) |
| Route composition — API 404 moved to the nested router, static merged outside the auth layer | `crates/fah-api/src/routes.rs` |
| Boot check call site | `crates/fah-api/src/server.rs` |
| `tower-http` (`fs`, `set-header`) | `crates/fah-api/Cargo.toml` |
| `frontend` stage, fixture bundle, `COPY --from=frontend /web /web`, scoped Rust copy | `Dockerfile` |
| `/dashboard` unblocked, `**/node_modules` and `dashboard/frontend/dist` excluded | `.dockerignore` |

`ServeDir` serves both mounts with `precompressed_br()` + `precompressed_gzip()`.
Revalidation is `Last-Modified` / `If-Modified-Since` — no `ETag` is generated
and none is asserted, because `ServeDir` does not emit one.

## Decisions

1. **No API route can become public, and the SPA fallback is why the public set
   cannot also be closed.** The static surface is a separate `Router` merged
   *after* `require_api_key`; `auth.rs` and its `PUBLIC_PATHS` are untouched, so
   a future root-mounted route added to the API router keeps the auth layer, and
   an overlapping path makes axum panic at startup rather than fail open. That
   is the risk the task named, and it is closed. What is **not** closed is the
   public set itself: `mounted()`'s fallback answers every unmatched path, so
   the effective rule is "everything outside `/health` and `/api/v1…`" —
   structurally, not textually. An `auth.rs` prefix allowlist was rejected for
   the same reason: the SPA fallback matches arbitrary client-side paths
   (`/lists`, `/settings`, …), so a prefix list could only cover it by writing
   that rule out longhand. (Narrowed by review finding m5; the original wording
   claimed closure the design does not have.)
2. **`/api/v1`'s JSON 404 moved to the nested router's own fallback**; the root
   fallback is now the SPA shell. This is what keeps an unknown API path from
   resolving to HTML, and it is asserted by test, not by inspection.
   (Widened by review finding R2 to the whole `/api` namespace — the nest is now
   two-level, `/api` → `/v1`, with the JSON fallback on `/api`, plus an explicit
   `/api/` route for the trailing-slash form axum's `nest` cannot match.)
3. **`Cache-Control` is emitted only on 2xx and 304.** A `MakeHeaderValue` impl
   (`CacheControl`) gates it, so a missing asset is not handed
   `max-age=31536000, immutable` on a `404`.
4. **The web root cannot be varied by a call site.** `ROOT` is a private const;
   `mounted()` and `check_root()` take no arguments. No config key, no
   parameter, no request-derived path.
5. **The Rust builder copies four entries instead of `COPY . .`** —
   `Cargo.toml Cargo.lock rust-toolchain.toml`, `crates/`, `tui-monitor/`
   (the two `members` globs). Chosen over excluding `dashboard/` in
   `.dockerignore`, because `.dockerignore` has to *stop* excluding
   `dashboard/` for p5-05's real build to see its own source.

## Measurements

Platform `linux/amd64`, musl static, `strip` + `lto` + `codegen-units = 1`.
Baseline built from the pre-change checkout in the same session, not from a
stored figure.

| Figure | Before | After | Delta |
| ------ | ------ | ----- | ----- |
| `/fastadhunter` in the image | 12,970,400 B | 13,310,368 B | **+339,968 B (+2.62%)** |
| Image (`docker images` SIZE) | 24.9 MB | 25.4 MB | +0.5 MB |
| `/web` layer | — | 57.3 kB | — |
| `/web` file bytes | — | 510 B | — |
| Host build (`x86_64-pc-windows-msvc`) | 13,257,216 B | 13,598,208 B | +340,992 B (+2.57%) |

**Attribution** (corrected by review finding m4, then again by R5 — the figure
stood through both, the explanation did not). `git diff Cargo.lock` shows exactly
three **new packages**:

| New package | Why |
| ----------- | --- |
| `mime_guess`, `unicase` | `Content-Type` from the file extension |
| `http-range-header` | `fs` pulls it in; Range requests are now served |

`mime`, `httpdate`, `tokio-util`, `percent-encoding`, `futures-core` and
`http-body-util` gained a `tower-http` edge but were **already** in the release
binary — the task file says as much about `httpdate`, and `mime` is required by
`axum` and `axum-core` (`Cargo.lock:1561` at `59601c0`). So the +340 kB is
`tower-http`'s own `fs`/`set-header` code plus those three crates; `tower-http`
was previously in `Cargo.lock` only through the `reqwest` **dev**-dependency and
without `fs`, so it is a genuine release-binary addition. The bundle contributes
57.3 kB of layer. Against the ≤ 30 MB budget the answer is **keep `ServeDir`** —
a hand-rolled handler was not evaluated further.

Fixture pre-compression (Node zlib, build-time only):

| File | Raw | `.gz` | `.br` |
| ---- | --- | ----- | ----- |
| `index.html` | 157 B | 146 B | **97 B** |
| `assets/app.a1b2c3d4.js` | 33 B | 53 B | 37 B |
| `assets/app.a1b2c3d4.css` | 36 B | 56 B | 40 B |

`index.html` was added to the compressed set by review finding m3, and is the
one input large enough for compression to win. The asset siblings are larger
than their sources — the inputs are ~30 B. Recorded for completeness only; the
bundle gate lands in p5-05. Re-measured after the fixes, from
`docker build --target frontend --build-arg FAH_VERSION=0.2.20`, which also
confirms the build arg now reaches the page (`<h1>FastAdHunter 0.2.20</h1>`,
finding n1).

Live probes against `fastadhunter:p501-after` (`docker run`, real TLS):

| Request | Result |
| ------- | ------ |
| `GET /` | `200`, `text/html`, `cache-control: no-cache`, `vary: accept-encoding` |
| `GET /assets/app.a1b2c3d4.js`, `Accept-Encoding: br` | `200`, `content-encoding: br`, `text/javascript`, `public, max-age=31536000, immutable`, `vary` |
| `GET /api/v1/nope` + key | `404`, `application/json`, `{"error":{"code":"not_found"}}` |
| Image filesystem | 0 entries matching `node`/`npm`; `/web` present with every fixture file (11 at the time of the probe, 13 after m3 added the shell's siblings) |
| UI-only edit (`dashboard/frontend/app.ts`) | every `builder` step incl. `RUN cargo build` reported `CACHED` |
| `--tmpfs /web` (empty root) | `ERROR fah_api::web: web UI missing … path=/web/index.html`; process `running`, `GET /health` `200`, `--healthcheck` exit 0 |

## Tests

| Suite | Count | Notes |
| ----- | ----- | ----- |
| `fah-api` lib | 69 | 16 in `web::tests` — shell, cache split, MIME (7 types), encoding selection, `Vary`, `304` without `ETag`, SPA fallback (plain **and** pre-compressed), asset `404`, traversal split by mount with a readable-escape-target control (6 vectors per mount), missing-shell predicate, `mounted()`, `ROOT`-vs-Dockerfile, multi-chunk read (R3) |
| `fah-api` `tests/api.rs` | 64 | 2 new — `the_static_paths_are_outside_the_api_key_boundary` (renamed by R1) and `every_path_under_api_is_json_never_the_shell` (R2); `an_unknown_route_is_a_json_not_found` extended to assert the API surface is not answered by the static service |
| Workspace | green | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --all-features --workspace`, incl. `request_coverage.rs` and `layering.rs` |

The lib figure originally read 71; the real count before the review fixes was
**64** (53 pre-existing + 11 new), and is **68** after them. Same class of error
as finding m4 — a number written from memory rather than from the runner.

## Files changed

| File | Change |
| ---- | ------ |
| `crates/fah-api/src/web.rs` | new — static router, cache/`Vary` layers, boot check, tests. Review fixes: pre-compressed SPA fallback (m3), `mounted()` and `ROOT`-vs-Dockerfile tests (M2), traversal test split by mount (m1), escape-target control and encoded vectors (m2), `READ_CHUNK` on all three static services (R3) |
| `crates/fah-api/src/routes.rs` | `v1` gains `.fallback(not_found)`; root fallback dropped; `.merge(crate::web::mounted())` after the auth layer. Review fix R2: two-level nest `/api` → `/v1` with the JSON fallback on `/api`, plus `.route("/api/", any(not_found))` |
| `crates/fah-api/tests/request_coverage.rs` | R2: `UNCOVERED` gains `/api/` with its reason |
| `crates/fah-api/src/server.rs` | `crate::web::check_root()` in `ApiServer::bind` |
| `crates/fah-api/src/lib.rs` | `mod web;` |
| `crates/fah-api/Cargo.toml` | `tower-http` `{ fs, set-header }` |
| `crates/fah-api/tests/api.rs` | static-surface composition test; review fix M1 renamed it and replaced the header-only discriminator |
| `Dockerfile` | `frontend` stage (`--platform=$BUILDPLATFORM`, `node:22.21.1-alpine`), fixture bundle, `COPY --from=frontend /web /web`, scoped builder copy. Review fixes: `index.html` pre-compressed (m3), `--build-arg FAH_VERSION=` in the documented builds (n1) |
| `.dockerignore` | `/dashboard` removed; `**/node_modules` (n2) and `dashboard/frontend/dist` added |

## Known limitations / deferred

| Item | Status |
| ---- | ------ |
| **`ServeDir` does not resolve symlinks.** `build_and_validate_path` (tower-http 0.6.11, `src/services/fs/serve_dir/mod.rs:455-497`) rejects `..`, absolute paths and Windows prefixes only. A symlink *inside* `/web` pointing outside it would be followed. | **Not mitigated, by decision.** `/web` is image content copied from the `frontend` stage; the runtime image has no shell and the serving uid (65532) cannot write `/web`, so no writer exists to create one. The acceptance criterion's symlink case is documented here rather than tested. One guard function reverses this if the owner disagrees. |
| **Non-GET methods on an unmatched root path now answer a bodyless `405`.** GET/HEAD get the SPA shell, as intended; every other method is rejected by `ServeDir` before its own fallback runs (`serve_dir/mod.rs:317`, `call_fallback_on_method_not_allowed` defaults false). `POST /api/v2/x` used to answer a JSON error. | Recorded, not changed (finding m6). Nobody chose this shape; changing it means giving `ServeDir` a method-not-allowed fallback, which is scope this task does not have. |
| `request_coverage.rs` unchanged | The scrape reads `.route("…")` literals; the static surface is mounted with `nest_service`/`fallback_service` and is not a REST endpoint. An `UNCOVERED` entry would fail `every_documented_exclusion_still_names_a_real_route`, which requires the entry to name a scraped route. Deliberate, not accidental. |
| `crates/fah-api/src/lib.rs` module-doc diagram | Still shows the pre-change route tree. Hard rule 7 forbids editing Rust comments, so it was left alone — owner's call. |
| DNS-while-`/web`-missing | Process, API and healthcheck verified. Actual name resolution not exercised: the build sandbox has no upstream egress (`all upstreams failed`), so no query can be answered regardless of `/web`. |
| Fixture bundle | Deleted by p5-05, which replaces the `RUN` in the `frontend` stage with `COPY dashboard/frontend/ .` + `npm ci && npm run build`. |
| `favicon.ico` / `.woff2` fixtures | Plain text with the right extensions — they prove MIME mapping, not rendering. |

## Documentation edits proposed — awaiting approval

Root CLAUDE.md §Working agreement 1: none of these were made.

| Document | Proposed change |
| -------- | --------------- |
| API.md | New §Static routes: `GET /`, `GET /assets/*`, SPA fallback; stated **unauthenticated** and outside `/api/v1`; cache-header contract (`immutable` assets, revalidatable shell); `Last-Modified` revalidation, no `ETag`. |
| ARCHITECTURE.md | `fah-api` serves the web UI from the image-baked `/web`; one origin, no second container, no new port; `/web` is never a volume. |
| SECURITY.md | §API access: the static surface is exempt from the API key. State *why* (the bundle holds no secrets; gating it makes the p5-04 login page unreachable) and *how* it is bounded (a separate router merged outside `require_api_key`, not a path rule). |

---

## Findings

Review boundary: working-tree diff against `59601c0` on `phase5-01`, restricted
to `.dockerignore`, `Dockerfile`, `Cargo.lock` and `crates/fah-api/**`. The
`plan/` renames and this file are excluded. P5-01 is not yet committed.

### Verified correct — do not re-litigate

| Claim | Evidence |
| ----- | -------- |
| The `Vary` layer is required, not redundant | `Vary` appears nowhere in tower-http 0.6.11 `src/services/fs/`. The task asked for the check; it was made and it is right. |
| `..%2f` is rejected, not decoded into the path | `serve_dir/mod.rs:461-468` percent-decodes **before** validating components; `ParentDir`/`RootDir`/`Prefix` return `None`. |
| `/assets/*` escapes are a real `404` | The assets `ServeDir` has no `.fallback`, so `invalid_path` yields `404`, not the shell. |
| `merge` cannot fail open | Moving `not_found` into `v1` leaves the API router's fallback `Fallback::Default`, so `merge` takes the static fallback without panicking and the auth layer never wraps it. An overlapping path would panic at startup, not fail open. |
| A missing asset is not cached for a year | `CacheControl::make_header_value` gates on 2xx-or-304; the `404` carries no `Cache-Control`. Asserted by test. |
| The scoped builder `COPY` is complete | Root manifest is virtual (`members = ["crates/*", "tui-monitor"]`); no `.cargo/` exists; no root-level build input is missed. `rustfmt.toml` is not copied and is not a build input. |
| `request_coverage.rs` still scrapes correctly | `.fallback(not_found)` sits inside the `v1` statement before its terminating `;`; neither `.fallback` nor `.merge` matches `.route(`. Both tests pass. |
| Static I/O does not touch the DNS hot path | 64 KiB `buf_chunk_size` per in-flight response (`serve_dir/mod.rs:28`); reads go to Tokio's blocking pool, not the worker threads. Bounded per request. |

Gates re-run in this review: `cargo fmt --all -- --check` clean;
`cargo clippy -p fah-api --all-targets -- -D warnings` clean; `web::tests` 11/11;
`request_coverage` 2/2.

### Major

| # | Finding | Class |
| - | ------- | ----- |
| M1 | **`the_static_surface_is_served_on_the_same_origin_without_a_key` is green with no `/web` at all.** `/web` does not exist on this host (checked) and the test passes: all three paths return `404`, and the test asserts only `status != 401` and that `vary` is present. `vary` comes from the router layer regardless of whether a file was served, so the test proves auth placement and route ordering — genuinely valuable — but not serving, which its name claims. **Impact:** the only integration-level evidence for the feature is blind to a broken static root, an empty bundle or a wrong `ROOT`. What actually proves serving is the `docker run` probe table in §Measurements, and that is not a gate. **Fix before `DONE`** — assert `status == 200` and a body substring under a root the test controls. | **FIXED** |
| M2 | **`mounted()`, `check_root()` and `ROOT` are untested.** Every unit test calls the private `router(root)` / `shell_missing(root)` against a tempdir; the two `pub` functions and the one constant that wire them to production are exercised by nothing. **Impact:** a wrong `ROOT` is caught only by a manual container run. Compounds M1 — together, no gate covers the shipped configuration. **Fix before `DONE`**, cheaply: one assertion that `mounted()` builds and that `ROOT` is what the Dockerfile copies to. | **FIXED** |
| M3 | **No security headers on the new HTML surface.** `nosniff`, `Content-Security-Policy` (`frame-ancestors`) and `Referrer-Policy` are absent from `crates/` and unmentioned in SECURITY.md. Not a P5-01 regression — nothing had them — but P5-01 is what puts a browser-rendered origin on the box, and p5-04 puts a session cookie on that same origin, at which point clickjacking becomes live. The task invests heavily in exact `Content-Type`; without `nosniff` a browser may still sniff past it. **Defer to p5-04**, where the cookie makes it load-bearing, and state the decision in SECURITY.md then. | DEFERRED |

### Minor

| # | Finding | Class |
| - | ------- | ----- |
| m1 | **The traversal test asserts no status.** `assert_ne!(body, SECRET)` only. On the root mount an invalid path reaches `ServeDir`'s fallback (`future.rs:47` `invalid_path` calls `call_fallback`) and returns **`200` + the SPA shell**, so four of the seven attempts are green because the shell is not the secret, not because anything was refused. The test cannot distinguish "rejected" from "resolved to a different file". Add a status assertion. | **FIXED** |
| m2 | **The backslash case tests nothing on the deployed platform.** On Linux `\` is an ordinary filename byte, so `/assets/..\..\secret.txt` is one `Component::Normal` and no traversal logic runs. It exercises rejection only on Windows, which is where the dev box runs it. The acceptance criterion names backslashes; the coverage claim is false for the target. Restated on re-examination: the deeper defect was not the backslash *vector* but that **a `404` was indistinguishable from "nothing was there anyway"** — on Linux every backslash attempt 404s by `ENOENT`, and so would a broken guard. | **FIXED** |
| m3 | **The SPA fallback loses pre-compression.** `ServeFile::new(root.join(SHELL))` has no `.precompressed_br()` / `.precompressed_gzip()`, while the shell reached via `/` does. Today the shell has no siblings so nothing differs. Once p5-05 emits `index.html.br`, `GET /` ships brotli and every deep link and sub-route reload ships the shell uncompressed — the largest text asset on the critical path. One-line fix; **must land before p5-05**. | **FIXED** |
| m4 | **§Measurements attribution is wrong.** `git diff Cargo.lock` shows the only **new packages** are `mime_guess`, `mime`, `unicase` and `http-range-header`. `httpdate`, `tokio-util`, `percent-encoding`, `futures-core` and `http-body-util` were already in the lock — the task file states outright that `httpdate` is not new. The +339,968 B figure stands; the sentence explaining it does not, and `http-range-header` (Range support, now enabled) is unlisted. | **FIXED** |
| m5 | **Decision 1 overstates closure.** "the only public routes are the ones `web::mounted()` serves" is true, but `mounted()`'s fallback serves *every* unmatched path — so the effective public set is "everything not matched by `/health` or `/api/v1…`", which is the negative rule the task forbids, reached structurally instead of textually. The risk the task actually named *is* mitigated: a future route added to the API router keeps the auth layer, and an overlapping path panics at startup. Narrow the claim to that. | **FIXED** |
| m6 | **An unrecorded behaviour change.** Unmatched root paths moved from `401` / JSON `404` to `200 text/html` for GET/HEAD — intended — and to a **bodyless `405`** for every other method (`serve_dir/mod.rs:317`; `call_fallback_on_method_not_allowed` defaults false). `POST /api/v2/x` now answers `405` with no body instead of a JSON error. Low impact, nobody chose it, worth one line. | **FIXED** |
| m7 | **The boot-check log line is untested.** `a_web_root_without_a_shell_is_reported` covers the `shell_missing` predicate, not `report_missing_shell` or `check_root`. The acceptance criterion is "logs the error naming the path"; the evidence is the `--tmpfs /web` probe, not a gate. Acceptable — capturing `tracing` output is not worth a dependency here. | DEFERRED |

### Nitpick

| # | Finding | Class |
| - | ------- | ----- |
| n1 | `ARG FAH_VERSION=dev` is supplied by no documented build command (the Dockerfile's own §Build examples pass no `--build-arg`), so the shipped fixture always reads `FastAdHunter dev` and `meta.json` `{"version":"dev"}`. A dead knob until p5-05 replaces the stage. | **FIXED** |
| n2 | `.dockerignore`'s `dashboard/frontend/node_modules` does not match a nested `node_modules` (npm workspaces, hoist failure). `**/node_modules` is the safe form. Zero cost today — `dashboard/` is empty, so removing the `/dashboard` exclusion adds nothing to the context. | **FIXED** |
| n3 | `//assets/app.<hash>.js` misses the `/assets` nest (matchit does not match the double slash), falls to the root `ServeDir` and serves the same bytes with `no-cache` instead of `immutable`. Fails safe — less caching, never more. | REJECTED |
| n4 | `ServeDir` applies no dotfile or extension filter, so whatever the frontend build leaves in `dist/` is publicly readable: `.map` source maps, `.vite/manifest.json`, bundle-analyser output. Nothing today; a constraint on p5-05's build output. | DEFERRED |

### On the deferred symlink item

The §Known limitations entry is correct on the mechanism — `build_and_validate_path`
does not resolve symlinks, and `/web` is mode-755 root-owned while the serving uid
is 65532 — but note the entrypoint runs as `USER 0:0` and drops privileges, so
"no writer exists" rests on the image having no shell and the binary never
creating one, not on the uid alone. The conclusion is unchanged and the deferral
is accepted. The vector it does not cover is p5-05: a symlink emitted *into*
`dist/` by `npm run build` would be preserved by `COPY --from=frontend`.
Worth one line in p5-05's task rather than a guard function now.

## Verdict — at review time

**PASS WITH DEFERRED FINDINGS.**

No Critical. No correctness, security or performance defect in the shipped code:
route ordering, the auth boundary, filesystem confinement, cache semantics,
`Vary`, encoding selection, the image contents and the build-cache scoping were
each checked against the implementation and against tower-http 0.6.11's source,
and each holds. The +340 kB binary delta is immaterial against 17 MB of headroom
and `ServeDir` stays.

The findings that matter are about **what the gates prove**, not about what the
code does. M1 and M2 mean the entire static surface is green on this host with no
`/web` directory in existence; the feature is real, but the evidence for it is a
manual `docker run` recorded in prose, not a test that will fail when someone
breaks it. Recommend fixing **M1, M2 and m1 before `DONE`** — all three are
assertions, not redesign — and **m3 before p5-05**. Everything else is deferred
or rejected as recorded above.

## Fixes applied — review follow-up

Approved for fix: M1, M2, m1, m3, m4, m5, m6, n1, n2. Held deferred by the
owner: M3 (security headers → p5-04) and the symlink item (→ p5-05, when a real
frontend build exists). No redesign, no future-task functionality pulled in.

| # | What changed | Verified by |
| - | ------------ | ----------- |
| M1 | `tests/api.rs`: renamed to `the_static_surface_answers_on_the_same_origin_without_a_key` (renamed again by R1 to `the_static_paths_are_outside_the_api_key_boundary`) — it proves auth placement and route ordering, which is what it can prove — and the discriminator is no longer the `vary` header alone. It now also asserts the response is **not** `application/json` and its body carries no `"error"` key, so an API answer fails it positively rather than by a missing header. | `cargo test -p fah-api --test api`, 63 green |
| M2 | `web.rs`: two tests narrow the gap M1 cannot close. `the_public_mount_answers_from_the_fixed_root` (renamed by R1 to `the_public_mount_builds_and_carries_the_vary_layer`, which is all it proves) exercises `mounted()`, and `the_fixed_root_is_the_directory_the_image_ships` asserts the Dockerfile contains `COPY --from=frontend {ROOT} {ROOT}` on a non-comment line (R8) — the same scrape-the-source pattern `request_coverage.rs` already uses. A `ROOT` that drifts from the image now fails a gate instead of a manual container run. | `cargo test -p fah-api --lib web::`, 15 green |
| m1 | `path_traversal_cannot_escape_the_root` split by mount, because the two mounts answer differently and one assertion could not cover both. `traversal_under_assets_is_refused_with_a_404` asserts `404` (that mount has no fallback). `traversal_under_the_root_resolves_to_the_shell_never_the_target` asserts `200` **and** that the body is the shell — `ServeDir` hands an invalid path to its own fallback (`future.rs:47`), so the shell is the correct answer and is now asserted, not assumed. | as above |
| m3 | `web.rs`: the SPA fallback is `ServeFile::new(…).precompressed_br().precompressed_gzip()`. Dockerfile: `index.html` joined the compressed set so the shipped fixture exercises it. | **Falsified before accepting** — reverting the `ServeFile` change alone fails `the_spa_fallback_serves_the_precompressed_shell` with `left: "" / right: "br"` on `/lists/oisd-basic`. The fix is load-bearing, not decorative. |
| m4 | §Measurements attribution rewritten against `git diff Cargo.lock`: four new packages (`mime_guess`, `mime`, `unicase`, `http-range-header`), and `httpdate`/`tokio-util`/`percent-encoding`/`futures-core`/`http-body-util` correctly named as pre-existing. Range support, previously unmentioned, is now stated. The +339,968 B figure is unchanged. | `git diff Cargo.lock` |
| m5 | §Decisions 1 rewritten. It now claims only what holds — no API route can become public, a future API route keeps the auth layer, an overlapping path panics at startup — and states plainly that the public *set* is not closed, because the SPA fallback answers every unmatched path. | — |
| m6 | Recorded in §Known limitations: non-GET methods on an unmatched root path now answer a bodyless `405`, because `ServeDir` rejects them before its own fallback runs. Behaviour left as-is; changing it is scope this task does not have. | `serve_dir/mod.rs:317` |
| n1 | Dockerfile header: both documented build commands now pass `--build-arg FAH_VERSION=`, and the header says what the arg is for. | `docker build --target frontend --build-arg FAH_VERSION=0.2.20` emits `<h1>FastAdHunter 0.2.20</h1>` — previously always `dev` |
| n2 | `.dockerignore`: `dashboard/frontend/node_modules` → `**/node_modules`, so a nested or hoist-failed `node_modules` cannot enter the context either. `dashboard/frontend/dist` stays exact — there is no other `dist` to catch, and a blanket `**/dist` would be a wider rule than the problem. | frontend stage still builds |

### m2 — resolved in a second pass

The first pass left m2 open. Re-examined, the finding was aimed slightly wrong,
and naming the real defect made the fix small enough to need no platform
machinery at all.

**What the finding got right.** On Linux `\` is an ordinary filename byte, so
`/assets/..\..\secret.txt` is a single `Component::Normal` and tower-http's
`ParentDir` rejection never runs. The case exercises the guard only on Windows.

**What it got wrong.** It called the case vacuous. It is not: if anyone ever
added `\`→`/` normalization — the plausible "make it work on Windows too"
change — that path would resolve to the fixture's out-of-root `secret.txt` on
Linux and the assertion would fail. The vector has real falsifying power.

**The actual defect, which applied to *every* attempt on *both* platforms.**
`assert_eq!(status, 404)` cannot tell "the guard refused this" from "nothing was
there anyway". On Linux every backslash attempt 404s by `ENOENT`; so would a
completely absent guard, and so would a fixture that silently stopped writing
the escape target.

**The fix**, entirely inside the existing tests:

| Change | Why |
| ------ | --- |
| `readable_escape_target()` asserts the out-of-root file exists and still holds `SECRET`, before any attempt runs | Turns every subsequent `404` into evidence. A `404` now means "reachable file, refused", not "no such file". |
| Added `%2e%2e%2f%2e%2e%2f` (fully-encoded `../../`) to both mounts | The existing `%2F` attempts target `/secret.txt` at *filesystem* root, which no test can create — they could never be decisive. These land on the fixture's real target. |
| Added `..%5c..%5c` (percent-encoded backslash) to both mounts | Same bytes as the raw case after decoding, so a normalizer introduced at either the decode or the component stage is caught, not just one. |
| Added `/..\secret.txt` and `/..%5csecret.txt` to the root mount | The root mount had no backslash coverage at all. |

No `cfg(windows)`, no second fixture, no platform abstraction. Six attempts per
mount, all decisive on both platforms.

**Falsified before accepting.** Writing an empty `secret.txt` in the fixture
makes both traversal tests fail with `the escape target must exist and be
readable, or a 404 proves nothing` — so the control is load-bearing rather than
decorative. Restored and re-run green.

### Gates after the fixes

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | green, 0 failures |
| `fah-api` lib / `tests/api.rs` / `request_coverage.rs` | 68 / 63 / 2 |
| `crates/fastadhunter/tests/layering.rs` | green |
| `docker build --target frontend` | builds; `/web` holds 13 files, no Node in the output |

Not re-run: the full image build and the live TLS probe table above. Nothing in
these fixes touches the Rust dependency graph or the runtime stage, so the
binary and image figures stand; the frontend stage was rebuilt and re-measured.

## Verdict after fixes

**PASS WITH DEFERRED FINDINGS.**

Ten of fourteen findings fixed and verified, two deferred by owner decision
(M3 → p5-04, symlink → p5-05), two deferred on their own merits (m7, n4), one
rejected (n3). **No finding is left OPEN.**

The substantive change is that the static surface is no longer proven only by a
`docker run` recorded in prose. `ROOT`, the pre-compressed SPA fallback and both
traversal mounts now fail a gate when they break. M1's fix is honest about its
limit: the integration test still cannot serve real files, because the task fixes
the root at `/web` and forbids a config knob — so M2's Dockerfile assertion is
what covers the wiring instead of a test that could not exist without breaking
that rule.

(Corrected by R1 in the second review: `mounted()` was named in this list and
does not belong there — no gate fails if that mount serves nothing. `ROOT` is
covered, `mounted()` is not.)

Nothing found in this review was a correctness, security or performance defect
in the shipped code, and that is unchanged.

---

# Second full review — independent pass

**Boundary.** Working-tree diff against `59601c0` on `phase5-01`, restricted to
`.dockerignore`, `Dockerfile`, `Cargo.lock`, `crates/fah-api/**`. P5-01 is still
uncommitted; `HEAD` is `59601c0` and carries nothing of this task. The `plan/`
`open` → `wip` renames and this review file are outside the boundary and were
not reviewed. Nothing else on the branch belongs to another task.

## Re-verified independently — prior findings

Each prior finding was re-checked against the code as it stands, not against the
fix note.

| # | Prior class | This pass | Evidence |
| - | ----------- | --------- | -------- |
| M1 | FIXED | **FIXED, with a residual** — see R1 | `tests/api.rs:648-671`; the added discriminators are real but inert on a `404` |
| M2 | FIXED | **PARTIALLY FIXED** — `ROOT`-vs-Dockerfile holds, `mounted()` does not — see R1 | `web.rs:337-359` |
| M3 | DEFERRED | **DEFERRED** (owner: p5-04) | unchanged |
| m1 | FIXED | **FIXED** | `web.rs:277-322`, both mounts assert status; the root mount also asserts the shell body |
| m2 | FIXED | **FIXED** | `readable_escape_target` (`web.rs:267-275`) makes each `404` decisive; 6 vectors per mount |
| m3 | FIXED | **FIXED** | `web.rs:34-38` — `ServeFile` carries both `precompressed_*`; `the_spa_fallback_serves_the_precompressed_shell` green |
| m4 | FIXED | **STILL WRONG** — see R5 | the lockfile diff yields three new packages, not four |
| m5 | FIXED | **FIXED** | §Decisions 1 now claims only structural closure |
| m6 | FIXED | **FIXED** (recorded) | `serve_dir/mod.rs:317` confirmed |
| m7 | DEFERRED | **DEFERRED** | unchanged |
| n1 | FIXED | **FIXED** | Dockerfile header passes the build arg in both documented builds |
| n2 | FIXED | **FIXED** | `.dockerignore` `**/node_modules` |
| n3 | REJECTED | **REJECTED** — concur, fails safe (`no-cache` instead of `immutable`) | — |
| n4 | DEFERRED | **DEFERRED** — extended by R4 | — |
| symlink | DEFERRED | **DEFERRED** — concur | `build_and_validate_path` does not resolve symlinks; no writer exists in the runtime image |

## Verified correct in this pass — do not re-litigate

| Claim | Evidence |
| ----- | -------- |
| Encoding negotiation honours q-values and `q=0` | `tower-http-0.6.11/src/content_encoding.rs:114-119` filters `qvalue.0 > 0`, then `max_by_key`. `br;q=0` cannot select `.br`. |
| The auth boundary cannot fail open | `routes.rs:81-95` — the auth layer closes over `/health` + `/api/v1`; `.merge(web::mounted())` runs after `.with_state`. `v1` owns `.fallback(not_found)`, so the root fallback slot is `Fallback::Default` and `merge` takes the static one. An overlapping path panics at startup. `auth.rs:15` `PUBLIC_PATHS` untouched. |
| The scoped builder `COPY` is complete | No `build.rs` anywhere in the workspace. Every `include_str!` / `include_bytes!` / `CARGO_MANIFEST_DIR` use resolves inside `crates/` or `tui-monitor/`. Root manifest is virtual, `members = ["crates/*", "tui-monitor"]`. |
| `dashboard/` costs nothing in build context today | It holds only `.gitkeep`, so dropping the `/dashboard` exclusion adds one file. |
| `/web` layer ordering is right | `COPY --from=frontend /web /web` is the **last** `COPY` in the runtime stage, so a UI-only change invalidates no earlier layer. |
| Static I/O is off the DNS hot path | `ServeDir` reads go to Tokio's blocking pool; `mounted()` builds one router at startup; `CacheControl` uses `HeaderValue::from_static`, so this crate's own per-response code allocates nothing. |
| Gates green at review time | `fmt --check` clean; `clippy --workspace --all-targets -D warnings` clean; `cargo test --all-features --workspace` green — `fah-api` lib 68, `tests/api.rs` 63, `request_coverage` 2, `layering` green. |

Not re-run in this pass: the full image build and the live TLS probe table. The
binary and image figures are carried forward from §Measurements, not re-measured.

## New findings

### Minor

| # | Finding | Class |
| - | ------- | ----- |
| R1 | **M2's fix reproduces the defect M1 named.** `the_public_mount_answers_from_the_fixed_root` (`web.rs:337-345`) calls `mounted()` and asserts **only** `vary == accept-encoding`. `/web` does not exist on this host (checked, both as `/web` and as `E:/web`) and the test is green: the request 404s through `ServeDir` then `ServeFile`, and the router's `Vary` layer sets the header whether or not a file was served. It proves the layer is wired into `mounted()`, and nothing about the mount answering *from the root*. The same holds for M1's own fix — on a `404` with an empty body, `content-type != application/json` and `!body.contains("\"error\"")` are both vacuously true, so the effective discriminator is still `vary` plus `!= 401`. **Impact:** §Verdict after fixes claims "`ROOT`, `mounted()` … now fail a gate when they break". Half of that holds — `the_fixed_root_is_the_directory_the_image_ships` genuinely pins `ROOT` to the Dockerfile — but `mounted()` serving nothing at all stays green. The gap M1 opened is narrower than before, not closed. **Fix:** rename both tests to what they prove (layer wiring, auth placement, route ordering), or delete `the_public_mount_answers_from_the_fixed_root` — the `ROOT`-vs-Dockerfile test is the one carrying the weight. Not a code defect; a claim defect. | **OPEN** |
| R2 | **Any `/api` path outside `/api/v1` now answers `200 text/html`.** The nest is `/api/v1` exactly, so `GET /api/v2/stats`, `GET /api/` and `GET /apiv1/stats` fall to the root fallback and receive the SPA shell. A client on a future or mistyped API version gets a `200` with an HTML body instead of a JSON error — the parse failure the phase file's §Key risks names under "Route ordering", displaced one path segment up. m6 recorded the non-GET half of this (`405`); the GET half is unrecorded. **Impact:** low today, since no `/api/v2` exists, but it is a latent trap for p5-03, which adds API surface. **Fix (one line, deferrable):** nest `/api` with its own JSON-404 fallback and put `v1` inside it, so everything under `/api` stays JSON regardless of version. The task's §Scope only required `/api/v1/*`, so this is beyond what was asked. | **OPEN** |
| R3 | **The 64 KiB-per-response read buffer is now reachable without a key, and the number of concurrent responses is not bounded.** `ServeDir`'s `buf_chunk_size` defaults to 64 KiB (`serve_dir/mod.rs:55,75`) and `ReaderStream::with_capacity` reserves it on the body's first poll, regardless of file size. Connections are capped at 64 (`server.rs:33`), but `tls.rs:135` advertises `h2` in ALPN and `hyper_util::server::conn::auto::Builder` is constructed with defaults, so `max_concurrent_streams` is never set and h2 advertises no limit. A client that opens streams and stalls its flow-control window holds ~64 KiB per stalled stream; ~1,600 streams across 64 connections reach 100 MB against the 128 MB steady-state budget. Before P5-01 an unauthenticated request could only earn a `401`, which allocates no such buffer. **Impact:** a LAN-local memory-pressure vector on a 1 GB box shared with RouterOS, requiring a deliberately malicious client — not reachable from a browser. **Fix if wanted:** `.with_buf_chunk_size(16 * 1024)` on both `ServeDir`s (`serve_dir/mod.rs:121`), and/or a `max_concurrent_streams` on the hyper builder — the latter is `fah-api` server scope, not static-serving scope. Recorded rather than proposed: the first pass's "Bounded per request" is true, and silent on the count. | **OPEN** |
| R4 | **`immutable` is keyed on the directory, not on a content hash.** `cached(IMMUTABLE, assets)` (`web.rs:41`) applies `max-age=31536000, immutable` to **everything** served under `/assets/`, hashed or not. The fixture hashes every filename and Vite's `assets/` output is hashed by default, so nothing is wrong today. A single unhashed file emitted into `assets/` by p5-05's build — a copied static file, a plugin's fixed-name output, a manifest — becomes uncacheable-away for a year with no URL to change. Extends n4, which covers *what* the build leaves in `dist/`, with *how it is named*. **Fix:** a stated constraint on p5-05 — `/assets/` holds content-hashed filenames only — rather than code here. | **OPEN** |

### Nitpick

| # | Finding | Class |
| - | ------- | ----- |
| R5 | **m4's correction is itself wrong, in m4's own class.** The lockfile diff against `59601c0` adds exactly three packages: `http-range-header`, `mime_guess`, `unicase`. `mime` is **not** new — it sits at `Cargo.lock:1561` in `59601c0`, required by `axum` and `axum-core` (pre-change lock lines 155 and 183), so it was already in the release binary. §Measurements' corrected attribution table lists four new packages and names `mime` among them. The +339,968 B figure is unaffected. | **OPEN** |
| R6 | **A now-false comment in `tests/api.rs`.** `// Auth wraps the whole router, so it answers before routing does.`, above the `anonymous` assertion in `an_unknown_route_is_a_json_not_found`, describes the pre-P5-01 composition; auth now wraps the API router only. It is also a Rust comment, which root CLAUDE.md hard rule 7 forbids outright — deleting it satisfies both. Pre-existing text, made wrong by this change. | **OPEN** |
| R7 | **`health_is_public_and_every_other_route_is_not` no longer describes the router.** Its body enumerates only `/api/v1/*`, so it still passes for the right reason, but the name asserts a closed public set that P5-01 deliberately opened — the same overstatement m5 corrected in §Decisions 1, left standing in a test name. | **OPEN** |
| R8 | **The Dockerfile scrape matches a comment.** `the_fixed_root_is_the_directory_the_image_ships` asserts the literal `COPY --from=frontend /web /web` appears anywhere in the file. Commenting the `COPY` out — the plausible way it breaks — leaves the string present and the test green. Anchoring the match to a line that does not start with `#` costs one line. | **OPEN** |
| R9 | **No `charset` on `text/html` or `text/css`.** `mime_guess` returns a bare `text/html` and `ServeDir` appends nothing. The fixture carries `<meta charset="utf-8">`, CSS inherits the referencing document's encoding, and ES modules are UTF-8 by specification regardless of `Content-Type`, so nothing misdecodes today. Worth one line in p5-05 alongside M3's security headers rather than a layer here. | DEFERRED |
| R10 | **The fixture emits `.gz` / `.br` siblings larger than their sources, and `ServeDir` serves them blindly.** §Measurements records `app.js` 33 B, `.gz` 53 B, `.br` 37 B; a client advertising `gzip` receives 53 bytes where 33 would do. Harmless at fixture scale, and `ServeDir` has no size comparison — selection is purely `Accept-Encoding` plus the sibling's existence. The implied rule is written down nowhere: **p5-05's compression step must emit a sibling only when it is smaller than its source.** | DEFERRED |
| R11 | `FROM --platform=$BUILDPLATFORM` expands to empty under the legacy non-BuildKit builder, failing the stage. **Rejected:** BuildKit is the default in every supported Docker version, the Dockerfile header documents `docker buildx build`, and the flag is required by the task. | REJECTED |

## Verdict — second review

**PASS WITH DEFERRED FINDINGS.**

No Critical, no Major. The security boundary, filesystem confinement, cache
split, `Vary`, encoding negotiation, route ordering, image contents and
build-cache scoping were each re-derived from the code and from tower-http
0.6.11's source in this pass, and each holds independently of the first review's
account of them. `q=0` handling and the completeness of the scoped builder `COPY`
were checked directly rather than inferred.

Eleven findings are new: four Minor, seven Nitpick. Nine are **OPEN**, two
DEFERRED on their merits, one REJECTED. None is a correctness, security or
performance defect in the shipped code:

- **R1, R5, R6, R7, R8** are claim and evidence defects — a test whose name
  outruns its assertions, an attribution table with one wrong row, two stale
  descriptions. They cost nothing to fix, and they are why the first review's own
  §Verdict overstates its coverage.
- **R2, R3, R4, R9, R10** are latent traps aimed at later tasks (p5-03, p5-05) or
  at `fah-api` server scope, not at P5-01's deliverable.

Recommendation: fix **R5, R6, R7 and R8** before `DONE` — four edits, no
redesign — and take **R1** with them, since it is a rename or a deletion.
**R2, R3 and R4** are the owner's call on scope; each has a one-line form
recorded above.

## Fixes applied — second-review follow-up

Approved for fix: R1, R5, R6, R7, R8. Held deferred: R4, R9, R10 and the symlink
item (→ p5-05), M3 (→ p5-04). R2 and R3 re-reviewed below rather than fixed.

| # | What changed | Verified by |
| - | ------------ | ----------- |
| R1 | Two renames, no assertion changes — both tests now claim only what they prove. `web.rs`: `the_public_mount_answers_from_the_fixed_root` → **`the_public_mount_builds_and_carries_the_vary_layer`**. `tests/api.rs`: `the_static_surface_answers_on_the_same_origin_without_a_key` → **`the_static_paths_are_outside_the_api_key_boundary`**. §Verdict after fixes corrected: `mounted()` was listed among what "now fail a gate when they break" and does not belong there — `ROOT` is covered by the Dockerfile scrape, `mounted()` is not, and no test on this host can cover it while `ROOT` is a fixed const. | `cargo test -p fah-api`, 68 / 63 / 2 green |
| R5 | §Measurements attribution corrected a second time: **three** new packages (`mime_guess`, `unicase`, `http-range-header`), not four. `mime` moved to the pre-existing list with its reason — `axum` and `axum-core` require it (`Cargo.lock:1561` at `59601c0`, referenced from lines 155 and 183). The +339,968 B figure is unchanged for the third time. | `git diff 59601c0 -- Cargo.lock \| grep '^[+-]name = '` → exactly three `+` lines |
| R6 | `tests/api.rs`: deleted `// Auth wraps the whole router, so it answers before routing does.` — false since the auth layer stopped wrapping the root fallback, and forbidden outright by hard rule 7. | gates green; the assertion it sat above is unchanged |
| R7 | `tests/api.rs`: `health_is_public_and_every_other_route_is_not` → **`health_is_public_and_every_api_route_is_not`**. The body only ever enumerated `/api/v1/*`; the name is now true of the router P5-01 ships. | `cargo test -p fah-api --test api`, 63 green |
| R8 | `web.rs`: the Dockerfile scrape now requires the `COPY` on a line whose first non-space character is not `#`, instead of anywhere in the file. | **Falsified before accepting** — commenting out `COPY --from=frontend /web /web` fails `the_fixed_root_is_the_directory_the_image_ships` at `web.rs:356`. The pre-fix assertion passed with the line commented out. Dockerfile restored; `git diff --stat Dockerfile` back to `50 insertions(+), 3 deletions(-)`. |

### Gates after the second-review fixes

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | green, 0 failures |
| `fah-api` lib / `tests/api.rs` / `request_coverage.rs` | 68 / 63 / 2 |
| `crates/fastadhunter/tests/layering.rs` | green |

Not re-run: the image build and the live TLS probe table. Nothing in these five
fixes touches the dependency graph, the runtime stage or any served byte — four
are renames or text, one tightens a source-scraping assertion.

## R2 — owner-scope decision: keep `/api/*` inside the JSON world, or defer to p5-03

**The behaviour.** `routes.rs` nests `/api/v1` exactly. `GET /api/v2/stats`,
`GET /api/` and `GET /api` therefore miss every API route, fall to the merged
static fallback and return `200 text/html` — the SPA shell. `GET /apiv1/stats`
does too, and always should: it is not an API path.

**What the task requires.** §Scope: "An unknown path under `/api/v1/` must still
produce the API's own JSON error." §Acceptance: `GET /api/v1/nonexistent` returns
JSON `404`. Both are met and asserted (`an_unknown_route_is_a_json_not_found`).
The finding is about the version segment one level up, which no criterion names.

**The change, if taken.** One statement in `routes.rs`:

```text
.nest("/api", Router::new().nest("/v1", v1).fallback(not_found))
```

replacing `.nest("/api/v1", v1)`. The two-level nest is required — adding a
second `.nest("/api", …)` alongside `.nest("/api/v1", …)` registers conflicting
matchit wildcards and panics at startup.

| Axis | Assessment |
| ---- | ---------- |
| `request_coverage.rs` | **Survives.** The scrape splits on `let v1 = Router::new()` up to its first `;` and prefixes that block with a hardcoded `/api/v1`; the statement is untouched. The inserted `Router::new()` carries no `.route(` literal, so the root block gains nothing. Verified by reading `request_coverage.rs:60-82`, not by inference. |
| Auth | Unchanged. The layer wraps the whole API router either way, so the new fallback is behind the key exactly as `v1`'s is. |
| Blast radius | `/api`, `/api/`, `/api/v2/**` and every future version prefix answer JSON `404` instead of HTML `200`. Nothing else moves. |
| Cost | One statement, one test. |
| Risk of not taking it | Latent, not live. p5-03 adds surface under `/api/v1` only, so it does not trigger this. The trap fires the day an `/api/v2` client, a proxy rewrite or a typo'd base URL hits the box and gets HTML where it expects JSON. |
| Risk of taking it | Beyond the acceptance criteria; a route-composition change in the one file the task calls its highest-risk detail, at a point where the surrounding tests are green. |

**Recommendation: take it in p5-01.** The task's own framing — route ordering is
"the single highest-risk detail" — is better served by a rule that holds for the
whole `/api` namespace than by one that holds for `v1` because `v2` does not
exist yet. It is two lines, the coverage gate is checked and survives, and p5-03
is the task that starts adding API surface. Deferring is defensible on scope
grounds; it is not defensible on cost.

**Status: OPEN — owner's call.** Not implemented; the review phase is analysis
only and this is outside the acceptance criteria.

## R3 — owner-scope decision: anonymous HTTP/2 static-serving memory exposure

### Correction to R3 as first written

R3 claimed `max_concurrent_streams` "is never set and h2 advertises no limit".
**That is wrong.** hyper 1.11.0 defaults it to `Some(200)`
(`src/proto/h2/server.rs:69`), which `hyper_util`'s `auto::Builder` inherits. The
exposure is bounded, not unbounded. The corrected number is worse, not better.

### The measured ceiling

| Factor | Value | Source |
| ------ | ----- | ------ |
| Concurrent connections | 64 | `server.rs:33` `MAX_CONNECTIONS` |
| Concurrent h2 streams per connection | 200 | hyper 1.11.0 `proto/h2/server.rs:69` |
| Read buffer per in-flight static response | 64 KiB | `tower-http` `serve_dir/mod.rs:55,75` → `ReaderStream::with_capacity`, reserved on the body's first poll regardless of file size |
| **Ceiling** | **64 × 200 × 64 KiB = 800 MiB** | product of the above |
| Budget | ≤ 128 MB steady state; 46.6–53.6 MiB today | phase `CLAUDE.md` §Performance requirements |
| Box | 1 GB, shared with RouterOS | root `CLAUDE.md` §Environment notes |

`h2` itself advertises nothing by default (`Settings::default()` leaves
`max_concurrent_streams: None`); the 200 comes from hyper's own config, and
`h2` 0.4.15 applies it at `proto/h2/server.rs:143-144`. ALPN offers `h2`
(`tls.rs:135`), so a browser or any TLS client negotiates it.

**How it is reached.** A `404` opens no body, so the attacker must request files
that exist — the bundle's own assets. Stalling each stream's flow-control window
keeps hyper from polling the body again, and the `BytesMut` is retained between
polls. A burst without stalling reaches the same peak transiently.

**What P5-01 changed.** The category is **pre-existing**: TCP accept, the TLS
handshake and the h2 preface all complete before `require_api_key` runs, so 64
unauthenticated connections × 200 streams were already reachable on 8443. What
P5-01 changes is the per-stream constant — from an anonymous request that could
only earn a `401` (small, inference: single-KiB order, not measured) to one that
can hold 64 KiB. Roughly a 16–64× rise in the ceiling of an exposure that
already existed.

**Why it matters here specifically.** The container runs `memory-high=unlimited`
(phase `CLAUDE.md` §Standing constraints 6), so nothing throttles; the RB5009's
1 GB is shared with RouterOS, and the process that dies is the household's DNS
resolver. Hard rule 4 — "memory must not grow with traffic" — is the rule in
question, and 800 MiB against a 128 MB budget fails it.

### Levers

| Lever | Where | Ceiling | Scope |
| ----- | ----- | ------- | ----- |
| none (today) | — | 800 MiB | — |
| `with_buf_chunk_size(8 * 1024)` on both `ServeDir`s and the `ServeFile` (all three exist in 0.6.11: `serve_dir/mod.rs:121`, `serve_file.rs:104`) | `web.rs` | 100 MiB | **P5-01 local** |
| `max_concurrent_streams(16)` on the hyper builder | `server.rs` | 64 MiB | server-wide, affects every API response |
| both | both | 8 MiB | — |

**The binding lever is the stream count, and it is not P5-01's.** Changing it
alters every API and WebSocket connection on the box, including `/events`, whose
cadence and slot accounting the phase file discusses at length. That belongs to
server hardening, with its own measurement.

**Recommendation: take the local half in p5-01, defer the binding half.**
`with_buf_chunk_size(8 * 1024)` at the three `web.rs` call sites is three tokens
per call site, changes no served byte, cannot regress correctness, and drops the
static surface's contribution 8×. It does **not** close the finding — 100 MiB is
still at the budget — and this review will not claim it does. It removes P5-01's
own multiplier and leaves a single, honestly-named server-hardening item:
*cap `max_concurrent_streams` on the API listener.*

Cost of the smaller chunk: ~5 reads instead of 1 for a 40 KB asset, on a surface
that is not the DNS hot path and is served from Tokio's blocking pool. No
measurement was taken, and none is proposed — this is a bound, not an
optimization (principle 8 is about the latter).

**Status: OPEN — owner's call**, split into a P5-01-local part and a
server-hardening part. Not implemented.

## Final finding table — every finding, both reviews

| # | Severity | Finding | Class |
| - | -------- | ------- | ----- |
| M1 | Major | Integration test green with no `/web` | FIXED (narrowed; residual named in R1) |
| M2 | Major | `mounted()`, `check_root()`, `ROOT` untested | FIXED for `ROOT`; `mounted()` residual named in R1 |
| M3 | Major | No security headers on the HTML surface | DEFERRED → p5-04 |
| m1 | Minor | Traversal test asserted no status | FIXED |
| m2 | Minor | A `404` could not be told from "nothing was there" | FIXED |
| m3 | Minor | SPA fallback lost pre-compression | FIXED |
| m4 | Minor | §Measurements attribution wrong | FIXED, then corrected again by R5 |
| m5 | Minor | Decision 1 overstated closure | FIXED |
| m6 | Minor | Unrecorded `405` on non-GET unmatched root paths | FIXED (recorded) |
| m7 | Minor | Boot-check log line untested | DEFERRED |
| n1 | Nitpick | `FAH_VERSION` supplied by no documented build | FIXED |
| n2 | Nitpick | `node_modules` exclusion not nested-safe | FIXED |
| n3 | Nitpick | `//assets/…` misses the nest | REJECTED (fails safe) |
| n4 | Nitpick | No dotfile/extension filter on `dist/` output | DEFERRED → p5-05 |
| — | Minor | `ServeDir` does not resolve symlinks | DEFERRED → p5-05 |
| R1 | Minor | M2's fix reproduced M1's defect; two test names outran their assertions | **FIXED** |
| R2 | Minor | `/api/*` outside `/api/v1` answers `200 text/html` | **OPEN — owner's call** (recommend fix in p5-01) |
| R3 | Minor | 800 MiB anonymous h2 ceiling on the static surface | **OPEN — owner's call** (recommend local half in p5-01, stream cap to server hardening) |
| R4 | Minor | `immutable` keyed on the directory, not a content hash | DEFERRED → p5-05 |
| R5 | Nitpick | m4's correction listed `mime` as new | **FIXED** |
| R6 | Nitpick | False comment in `tests/api.rs` | **FIXED** |
| R7 | Nitpick | `health_is_public_and_every_other_route_is_not` name false | **FIXED** |
| R8 | Nitpick | Dockerfile scrape matched a commented-out line | **FIXED** |
| R9 | Nitpick | No `charset` on `text/html` / `text/css` | DEFERRED → p5-05 |
| R10 | Nitpick | Compression siblings emitted even when larger | DEFERRED → p5-05 |
| R11 | Nitpick | `$BUILDPLATFORM` empty on the legacy builder | REJECTED |

**Totals:** 25 findings — 13 FIXED, 8 DEFERRED (p5-04: 1; p5-05: 5; on merit: 2),
2 REJECTED, **2 OPEN, both owner-scope decisions, neither a local defect.**

## Verdict after the second-review fixes

**PASS WITH DEFERRED FINDINGS.**

No avoidable local OPEN finding remains. Every finding that could be closed
inside P5-01's own scope with an assertion, a rename or a corrected sentence has
been closed and verified, and R8's fix was falsified before it was accepted.

The two that remain OPEN are deliberately not local:

- **R2** is a route-composition improvement one segment above the acceptance
  criteria. Recommended for p5-01 — two lines, coverage gate checked and
  surviving — but it is a scope decision, not a defect.
- **R3** is a resource bound whose binding lever (`max_concurrent_streams`) is
  server-wide and affects every API and WebSocket connection. The P5-01-local
  part (`with_buf_chunk_size`) is recommended and would cut the static surface's
  contribution 8×, but it does **not** close the finding, and the review does not
  pretend otherwise. The residue is one named server-hardening item.

Still true, and unchanged by either review: nothing found in the shipped code is
a correctness or security defect. R3 is the one finding that touches a hard rule
(bounded memory), and it names a pre-existing exposure that P5-01 multiplies
rather than creates.

Documentation edits for API.md, ARCHITECTURE.md and SECURITY.md remain proposed
and unapproved (§Documentation edits proposed). P5-01 is not committed.

## Scope decisions taken — R2 and R3

Owner decision, this session. R2 fixed in full; R3 fixed only in the part that is
local to P5-01, with the remainder deferred and named. R4, R9, R10 and the
symlink item stay deferred to p5-05; M3 stays deferred to p5-04.

### R2 — `/api/*` stays inside the JSON world · FIXED

The rule the router now enforces:

```text
/api/**   → API router → JSON 404 when no route matches
/**       → static / SPA shell
```

| Change | Where |
| ------ | ----- |
| `let api = Router::new().nest("/v1", v1).fallback(not_found);` and `.nest("/api", api)` replacing `.nest("/api/v1", v1)` | `routes.rs` |
| `.route("/api/", any(not_found))` on the root router | `routes.rs` |
| `UNCOVERED` gains `/api/` with its reason | `tests/request_coverage.rs` |
| `every_path_under_api_is_json_never_the_shell` — `/api`, `/api/`, `/api/v2/stats`, `/api/v1/nope`, each asserted `404` + `not_found` + no `vary`, and `401` without the key | `tests/api.rs` |

**Discovered while implementing, not predicted by the review: `/api/` needed its
own route.** axum's `nest("/api", …)` registers `/api` and
`/api/{*tail}`, and a matchit catch-all requires at least one character — so
`/api/` matched neither and still fell to the SPA shell after the two-level nest
was in place. The first run of the new test failed on exactly that path. The
explicit `any(not_found)` route closes it; `any` rather than `get` so a `POST /api/`
gets the same JSON error as `POST /api/v2/x` does through the nest fallback.

That route is a `.route("…")` literal, so `request_coverage.rs` scrapes it and
demands a request file. An `UNCOVERED` entry is the intended escape hatch — the
test's own doc calls adding one "a decision" — and
`every_documented_exclusion_still_names_a_real_route` verifies the entry keeps
naming a real route.

**Falsified before accepting**, both halves:

| Reverted | Result |
| -------- | ------ |
| `.route("/api/", any(not_found))` removed | `every_path_under_api_is_json_never_the_shell` fails: `/api/ was answered by the static service` |
| two-level nest reverted to `.nest("/api/v1", v1)` | same test fails: `/api was answered by the static service` |

Both reverts restored and re-run green. `an_unknown_route_is_a_json_not_found`
is unaffected. Auth is unchanged — the layer wraps the whole API router, so
`/api/v2/x` and `/api/` are `401` without a key and JSON `404` with one.

### R3 — anonymous h2 static memory · P5-01 local part FIXED, server-wide part DEFERRED

**Do not read this as a closed finding.** The split:

| Half | Status |
| ---- | ------ |
| P5-01's own multiplier — the 64 KiB read buffer the static surface introduced | **FIXED** |
| The binding bound — `max_concurrent_streams` on the API listener | **DEFERRED → server hardening** |

`READ_CHUNK = 8 * 1024` is applied at all three static call sites in `web.rs`:
the `/assets` `ServeDir`, the root `ServeDir`, and the SPA-fallback `ServeFile`
(`with_buf_chunk_size`, `serve_dir/mod.rs:121` and `serve_file.rs:104`).

| Ceiling | Value |
| ------- | ----- |
| Before | 64 conns × 200 streams × 64 KiB = **800 MiB** |
| After | 64 conns × 200 streams × 8 KiB = **100 MiB** |
| Budget | ≤ 128 MB steady state, on a 1 GB box shared with RouterOS |

100 MiB is still at the budget. The static surface no longer contributes the
multiplier it introduced, and nothing more than that is claimed.

**What the new test does and does not prove.**
`an_asset_larger_than_the_read_chunk_arrives_whole` writes a `READ_CHUNK * 3 + 17`
byte asset and asserts the body arrives byte-identical. That guards the real
regression risk of a smaller chunk — a file spanning several reads arriving
truncated or reordered — and it is green. It does **not** prove `READ_CHUNK` is
wired in: the chunk size is not observable from an HTTP response, so the same
test passes at any value. The three call sites are verified by reading them.
Recorded here rather than left for a later reviewer to find, because it is the
same class as R1.

**Residue, for the server-hardening task.** hyper 1.11.0 defaults
`max_concurrent_streams` to `Some(200)` (`proto/h2/server.rs:69`) and
`server.rs:33` caps connections at 64. Capping streams is what makes the product
fit the budget — `max_concurrent_streams(16)` with the 8 KiB chunk gives 8 MiB —
but it applies to every API and WebSocket connection, including `/events`, whose
cadence and slot accounting the phase file discusses at length. It needs its own
measurement and its own task. The exposure it addresses is **pre-existing**: TCP
accept, the TLS handshake and the h2 preface all complete before
`require_api_key` runs.

### Gates after the scope fixes

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --all-features --workspace` | green, 44 suites, 0 failures |
| `fah-api` lib / `tests/api.rs` / `request_coverage.rs` | **69 / 64 / 2** |
| `crates/fastadhunter/tests/layering.rs` | green |

Not re-run: the image build and the live TLS probe table. R2 changes route
composition only; R3 changes a read-buffer constant. Neither touches the
dependency graph, the runtime stage or any served byte, so the binary and image
figures in §Measurements stand.

## Final finding table — all three passes

| # | Severity | Finding | Class |
| - | -------- | ------- | ----- |
| M1 | Major | Integration test green with no `/web` | FIXED (narrowed; residual named in R1) |
| M2 | Major | `mounted()`, `check_root()`, `ROOT` untested | FIXED for `ROOT`; `mounted()` residual named in R1 |
| M3 | Major | No security headers on the HTML surface | DEFERRED → p5-04 |
| m1 | Minor | Traversal test asserted no status | FIXED |
| m2 | Minor | A `404` could not be told from "nothing was there" | FIXED |
| m3 | Minor | SPA fallback lost pre-compression | FIXED |
| m4 | Minor | §Measurements attribution wrong | FIXED, then corrected again by R5 |
| m5 | Minor | Decision 1 overstated closure | FIXED |
| m6 | Minor | Unrecorded `405` on non-GET unmatched root paths | FIXED (recorded) |
| m7 | Minor | Boot-check log line untested | DEFERRED |
| n1 | Nitpick | `FAH_VERSION` supplied by no documented build | FIXED |
| n2 | Nitpick | `node_modules` exclusion not nested-safe | FIXED |
| n3 | Nitpick | `//assets/…` misses the nest | REJECTED (fails safe) |
| n4 | Nitpick | No dotfile/extension filter on `dist/` output | DEFERRED → p5-05 |
| — | Minor | `ServeDir` does not resolve symlinks | DEFERRED → p5-05 |
| R1 | Minor | Two test names outran their assertions | FIXED |
| R2 | Minor | `/api/*` outside `/api/v1` answered `200 text/html` | **FIXED** (incl. the `/api/` case the review had not predicted) |
| R3 | Minor | Anonymous h2 memory ceiling on the static surface | **PARTIALLY FIXED / DEFERRED** — P5-01 local multiplier FIXED (800 MiB → 100 MiB); server-wide stream bound DEFERRED to server hardening |
| R4 | Minor | `immutable` keyed on the directory, not a content hash | DEFERRED → p5-05 |
| R5 | Nitpick | m4's correction listed `mime` as new | FIXED |
| R6 | Nitpick | False comment in `tests/api.rs` | FIXED |
| R7 | Nitpick | `health_is_public_and_every_other_route_is_not` name false | FIXED |
| R8 | Nitpick | Dockerfile scrape matched a commented-out line | FIXED |
| R9 | Nitpick | No `charset` on `text/html` / `text/css` | DEFERRED → p5-05 |
| R10 | Nitpick | Compression siblings emitted even when larger | DEFERRED → p5-05 |
| R11 | Nitpick | `$BUILDPLATFORM` empty on the legacy builder | REJECTED |

**Totals:** 25 findings — 14 FIXED, 1 PARTIALLY FIXED with a named deferral,
8 DEFERRED (p5-04: 1; p5-05: 5; on merit: 2), 2 REJECTED. **0 OPEN.**

### Carried out of P5-01

| Item | To |
| ---- | -- |
| Security headers on the HTML origin (M3) | p5-04, where the session cookie makes them load-bearing |
| Cap `max_concurrent_streams` on the API listener (R3 residue) | server hardening, with its own measurement |
| `dist/` output constraints — no dotfiles or source maps (n4), `/assets/` content-hashed only (R4), compression siblings only when smaller (R10), `charset` on text types (R9), no symlinks emitted into `dist/` | p5-05 |
| Boot-check log line untested (m7) | accepted as-is; capturing `tracing` is not worth a dependency |

## Verdict — final

**PASS WITH DEFERRED FINDINGS.**

**No OPEN finding remains.** Every finding is FIXED, explicitly deferred to a
named later task, or rejected with a reason. The two scope decisions the owner
took closed the last two:

- **R2 is fully fixed**, and the fix found a case the review had missed —
  `/api/` needed an explicit route because axum's nest catch-all cannot match an
  empty tail. Both halves were falsified before acceptance.
- **R3 is deliberately not marked fixed.** The half that P5-01 introduced is
  closed and measured (800 MiB → 100 MiB); the half that binds the result is
  server-wide, pre-existing, and now recorded as a single named item rather than
  as a vague concern.

Nothing found across the three passes is a correctness or security defect in the
shipped code. The static surface's route ordering, auth boundary, filesystem
confinement, cache split, `Vary`, encoding negotiation, image contents and
build-cache scoping were each verified against the implementation and against
tower-http 0.6.11's source, and each holds.

Documentation edits for API.md, ARCHITECTURE.md and SECURITY.md remain proposed
and unapproved (§Documentation edits proposed) — API.md's static-routes section
now also needs to state the `/api/**` → JSON rule that R2 introduced. P5-01 is
not committed.
