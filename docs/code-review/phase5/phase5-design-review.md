# Phase 5 — Pre-Implementation Design Review

Independent review of [docs/dashboard/](../../dashboard/) and
`plan/open/phase5/*` against the running code, before any implementation.
Nothing in the existing design was assumed correct.

> **Resolved 2026-08-25 by a correction pass.** B1–B9, S1–S19 and D1–D7 are
> closed in `plan/open/phase5/` and `docs/dashboard/`, with six of the
> recommendations amended by the owner — see the note under [Verdict](#verdict).
> **Task numbers in this file are the pre-correction ones.** The phase is now ten
> tasks: `p5-01` static serving · `p5-02` certificate spike · `p5-03` API
> contracts · `p5-04` auth · `p5-05` foundation · `p5-06`–`p5-09` pages ·
> `p5-10` verification. Read `plan/open/phase5/CLAUDE.md` for the current order;
> this file is kept as the record of what was found and why.

## Summary

The design work is strong where it reasons from API.md: the capability matrix's
refusal to invent screens, changed-keys-only config writes, the
`ServeDir`-emits-no-`ETag` correction, budgets-as-markers. What is missing is a
pass against the **running code** rather than against the docs. Nine one-grep
facts invalidate written plans — `.dockerignore`, `request_coverage.rs`,
`GET /config`'s "nothing here is secret" invariant, the certificate's SAN list,
the shared Tokio runtime, and the event socket's lack of a subscription filter.

Two findings (B3/B4, B9) are API-shape decisions that must be settled before
`p5-03` writes a typed client against them.

**Verdict: not ready.** See [Verdict](#verdict).

## Blockers

| # | Affects | Finding | Why it matters |
| - | ------- | ------- | -------------- |
| B1 | `p5-01` §Scope "Multi-stage Dockerfile"; implementation-plan §A.1/E.1 | `.dockerignore` line 7 excludes `/dashboard`. The `frontend` build stage cannot see the frontend source. | The multi-stage build as specified cannot work at all. Also: `Dockerfile` uses `COPY . .` in the Rust builder, so un-ignoring `dashboard/` invalidates the Rust layer cache on every UI edit unless `dashboard/frontend/node_modules` + `dist` stay ignored and the frontend stage copies only its own directory. |
| B2 | `p5-01`, `p5-02` acceptance "Gates green" | `crates/fah-api/tests/request_coverage.rs` scrapes `router()` textually and asserts every route has a request file in `requests/`, with an explicit `UNCOVERED` allowlist. | `p5-02`'s login/logout/logout-all/password routes fail the test on the commit that adds them. Neither task lists `requests/` or `UNCOVERED`. |
| B3 | `p5-02` §Storage; `p5-07` §"All settings" panel | `routes.rs:1237` returns the whole `fah_config::Config`; the doc comment at `:1233` states "Nothing here is secret … so 'secrets redacted' holds by construction". `p5-02` puts `auth.password_hash` in that config. | The Argon2id hash is then returned to every bearer-key holder **and rendered verbatim** by the read-only All-settings panel. `p5-02`'s acceptance covers only the *session secret*. Redact `auth.*` in `GET /config` and state it in `p5-07`. |
| B4 | `p5-02` §Routes | `POST /config` is a deep-merge and would accept `{"auth":{"password_hash":…}}`. | Bypasses the password-change route, its old-password check and global session invalidation. `rules.lists` and `policies` are `422` for exactly this one-writer reason (API.md §POST /config); `auth` needs the same. |
| B5 | `p5-02` §Scope decision 1 | One shared multi-thread runtime serves DNS, HTTP and API (`crates/fastadhunter/src/main.rs:242`). Argon2id at 19 MiB/t=2 is tens of ms on x86, ~9× on the RB5009. | Run on a worker thread it stalls DNS queries on a 4-core box. Must be `spawn_blocking` — the precedent and the comment already exist at `main.rs:757,777`. Nothing in `p5-02` says so, and its criteria pass without it. |
| B6 | `p5-02` §Rate limiting; phase `CLAUDE.md` §Key risks | Peak RSS is driven by **concurrent** verifications, not their rate. 8 simultaneous logins × 19 MiB ≈ 150 MiB against a 128 MB steady-state budget. | The task names memory pressure as the risk and prescribes only a rate limit. A concurrency cap (semaphore, 1–2 permits) is the control that actually bounds it. |
| B7 | `p5-02` §Scope; open-questions.md §1 "Also open" | The CSRF question and an `Origin` check on the WebSocket upgrade did not survive into the task file. | WebSocket upgrades are not subject to the same-origin policy. Any LAN page can open `wss://<box>:8443/api/v1/events`; only `SameSite=Strict` stands between it and a live feed of household DNS traffic. Validate `Origin` on upgrade; record the CSRF decision before the cookie ships. |
| B8 | `p5-08` §HTTPS and the certificate; implementation-plan §E.5; phase `CLAUDE.md` §Definition of done | `crates/fah-api/src/tls.rs:18` — SANs are `fastadhunter`, `localhost`, `127.0.0.1` only. | Opening `https://192.168.88.1:8443/` gives `ERR_CERT_COMMON_NAME_INVALID`, not the "one-time warning for a self-signed certificate" SECURITY.md §TLS promises. The whole `__Host-`/`Secure` cookie design rides on browsers accepting that origin. The fix is a code change (add configured/detected LAN addresses as SANs, allow operator-supplied SANs) that no task owns, and it is deferred to the last task in the phase. |
| B9 | `p5-02`/`p5-03`/`p5-07`; IA §Cross-cutting; phase `CLAUDE.md` §CODE REVIEW "request discipline" | `events.rs::run_socket` sends every `Event::Query` to every subscriber and discards client messages; `EventHub::has_subscribers()` enables per-query publish work as soon as one socket connects. There is no subscription filter. | A phone parked on Settings receives the full per-query firehose (the channel is sized for ~250 QPS). Contradicts "bounded everything on the client" and makes the socket unusable as the always-on transport the IA specifies. The socket already reads and discards inbound frames, so a `{"subscribe":[…]}` message is one match arm — but it is an **API change** that `p5-03`'s socket manager and `p5-07`'s Live Feed both build on. Related: at high rates a phone lags past `CHANNEL_CAPACITY = 256` and is disconnected, producing a permanent reconnect loop exactly when the Live Feed is wanted. Unaddressed in `p5-07` and `p5-08`. |

## Should-fix

| # | Affects | Finding |
| - | ------- | ------- |
| S1 | IA §Dashboard tile row 2; `p5-04` §Scope | Row 1 is `GET /stats`, a rolling 24 h window. Row 2's HTTP tiles are `telemetry.counters.http`, which API.md states are **process-lifetime cumulative and reset on restart**. Two visually identical rows mean different things, and no 24 h HTTP figure exists in the API. Label the row "since restart" or move the tiles. |
| S2 | IA §Dashboard; `p5-04` acceptance "updates from the socket push without polling stats" | `encode_stats` pushes exactly the `/stats` payload. Ruleset card, upstream bars, HTTP tiles, uptime and Cache State come from `/telemetry`, `/health`, `/cache` — **none pushed**. The refresh policy for that half of the page is unspecified. Decide it (one shared telemetry read, slow interval, paused while hidden). |
| S3 | IA §Dashboard; visual-system §Charts; `p5-04`; capability-matrix | "stacked allowed/blocked": `history/summary` items are `queries`, `blocked`, `cache_hits`, `per_type` — there is **no `allowed`**. CONTEXT.md §Verdict defines `allow` as an explicit exception match (~1 200 of 900 k in API.md's own example), not "not blocked". A band computed as `queries − blocked` and labelled "allowed" violates hard rule 6. The matrix already says "Top **permitted**"; IA and visual-system say "allowed". Three docs, two words. |
| S4 | IA §Settings; `p5-07` §Settings | IA claims `GET /config` is the source "of validation metadata". It is not — API.md §GET /config and `routes.rs:1237` return the serialized typed config and nothing else. Types, bounds, enums and mutability classes must all be hand-carried from CONFIGURATION.md. Defensible, but it changes `p5-07`'s cost and drift risk. |
| S5 | IA §Clients; `p5-05` §Clients | `GET /clients` returns ip/name/first_seen/last_seen/queries_24h/blocked_24h — **no policy**. The in-force policy and the direct-vs-inherited distinction come only from `GET /clients/{ip}/policy`, one request per client. Both docs specify the column for every row. Add it to `GET /clients`, or fetch lazily on expansion (which `MobileClients.dc.html` already implies) and say so. |
| S6 | IA §Upstreams; `p5-06` §Upstreams | `p5-06` requires the page to name the strategy in force; API.md is emphatic every health field is meaningless under `fallback`. `telemetry.upstreams[]` carries no strategy field — it is `dns.upstreams.strategy` in `GET /config`, which IA does not list as a source for that page. |
| S7 | IA §Cross-cutting "Empty is not an error"; `p5-04`, `p5-06` | `history.enabled` is runtime-mutable, so Settings can turn it off live; `/history/*` then returns `200` with empty `items` forever. The Dashboard chart and the whole Performance page would read as "no traffic". Read `history.enabled` and say "history is disabled". |
| S8 | `p5-07` §Settings | `[api] tls` (`schema/api.rs:14`, default `true`) is a lock-out key: setting it false removes the only origin a `Secure`/`__Host-` cookie can exist on, and the plan forbids an HTTP fallback. `api.address`/`api.port` move the listener. Exclude `[api]` from the curated form or gate it behind a consequence dialog. |
| S9 | implementation-plan §A.2; `p5-01` acceptance; `p5-03` §Scope | "Vite emits `.gz` and `.br` alongside each asset" — it does not; that needs a plugin or post-build step. `p5-01` tests precompressed selection while its own frontend stage emits only a placeholder `index.html`, and `p5-03` never mentions compression. Nobody owns it. |
| S10 | `p5-01` §Caching and revalidation | `Vary: Accept-Encoding` appears nowhere. Serving a `.br`/`.gz` sibling chosen by `Accept-Encoding` under `public, max-age=31536000, immutable` without `Vary` lets a cache hand a brotli body to a client that did not accept it. Verify what `ServeDir` emits; add the header if absent. |
| S11 | visual-system §Output shape; `p5-03`, `p5-04` | "one JS chunk" is fixed by decision and no doc mentions code splitting, so uPlot ships on the **login page** and to every phone. Route-level `import()` is free in Vite and needs no dependency. The honest gains are cache granularity (one page's change stops invalidating the whole immutable chunk) and parse cost on an old phone — the LAN transfer saving is small and should not be oversold. |
| S12 | `p5-03` §Self-hosted assets; visual-system §Output shape | The web font is the largest avoidable item in the 150 KB budget: a subset WOFF2 is 15–40 KB already-compressed (brotli takes nothing further off it) plus a critical-path request. A system font stack costs 0 KB and looks native on every household device. Drop it unless someone can name what it buys. |
| S13 | visual-system §Stack; phase `CLAUDE.md` §Quality gates; `p5-03` §Size gate | The budget is stated in **gzip** while the handler prefers `.br`. Gate and report both, so the number that gates is the number that travels. |
| S14 | `p5-03` §Socket manager + acceptance "an expired session returns to the login page" | A browser `WebSocket` exposes no status for a failed upgrade — a `401` arrives as an indistinguishable `onerror`. As written this becomes an endless reconnect-with-backoff loop. The manager needs an authenticated REST probe to classify repeated immediate failures. |
| S15 | `p5-02` §Scope decision 5; auth-design-draft §Logout flow | Password change is never required to verify the **current** password. With `SameSite=Strict` and no CSRF token, that check is the remaining defence for an unattended logged-in browser. |
| S16 | `p5-02` §Cookie; auth-design-draft §Security requirements | "explicit expiry" reads as the cookie `Expires` attribute, which is client-controlled. The draft requires expiry enforced server-side — say in the task that the expiry lives **inside the signed token**. |
| S17 | `p5-01` §Auth exemption; `auth.rs:15` `PUBLIC_PATHS` | The exemption is an exact-match one-element list; `p5-01` needs `/`, `/assets/*` and the SPA fallback public. Expressed as "anything not under `/api/v1`", every future root-mounted route is public by default. There is none today (`fah-metrics` has no HTTP surface) — which is why the positive allowlist should be written down now. |
| S18 | CONTEXT.md §Query Log; capability-matrix §Cut; hard rule 6 | CONTEXT.md still defines Query Log as "the bounded, **persisted** record of individual queries and requests … the log's `domain` filter searches either". The matrix cuts exactly that and renames the concept **Live Feed**, which CONTEXT.md does not contain. Vocabulary is binding and changes land in the same change; no p5 task touches it. |
| S19 | IA §Lists; `p5-04` §Lists | `degraded` (format misdetection) and `parse_errors` are absent from the design, and the `rejected` recovery contract — `DELETE` then re-add, because disable/enable does not clear the cached baseline (API.md §GET /lists) — is not surfaced. That is the one page an operator opens when a list is broken. |

## Minor

| # | Affects | Finding |
| - | ------- | ------- |
| M1 | visual-system §Stack | uPlot "~45 KB" vs Chart.js "~200 KB" are minified figures compared against a budget stated in gzip; uPlot is ~16 KB gzipped. |
| M2 | `p5-03` §Shell | No client-side router named, though `p5-01`'s SPA fallback implies history-API routing rather than hash routing. Name it (~1 KB library or 40 lines of `popstate`). |
| M3 | visual-system §Theme; `p5-03` §Theme | Theme needs an inline pre-paint script in `index.html` or every load flashes the wrong theme. |
| M4 | `p5-06` §Upstreams | `upstreams[].family` is `null` for a DoH URL with a domain host (API.md); the task lists `family` per endpoint with no null case. |
| M5 | `p5-07` §All settings panel | `Config.policies` carries `skip_serializing_if = "Vec::is_empty"`, so the panel shows no `policies` key when none are configured — the exact "not exposed here vs not set" ambiguity the panel exists to remove. Harmless (policies are excluded by design) but it shapes the panel's wording. |
| M6 | `p5-06` §Cache | `counters.cache_cleanup.last_duration_micros` is the one last-value gauge among cumulative counters (API.md §telemetry). Deltaing it produces nonsense. |
| M7 | IA §Dashboard | `/history/summary` carries no HTTP series, so "Queries over time" is DNS-only on a dashboard that insists elsewhere the pipelines are never conflated. Label the chart. |
| M8 | `p5-08` §RSS measurement | "steady-state RSS with the dashboard served and a browser connected" measures the event feed as much as static serving, because a connected socket enables per-query publish work. Split the readings. |
| M9 | every task's §Suggested prompt | They reference `plan/wip/phase5/…`; the directory is `plan/open/phase5/` and moving it is the owner's call (root CLAUDE.md §Working agreement). |
| M10 | `p5-01` acceptance "Image and binary size recorded" | `tower-http` is in `Cargo.lock` only via `reqwest`, a **dev**-dependency, so the `fs` feature is a genuine addition to the release binary (`mime_guess` is new; `httpdate` is not). With `strip`+`lto`+`codegen-units=1` and 17 MB of headroom against 30 MB, the answer is near-certainly "keep `ServeDir`". Measure it, do not deliberate. |
| M11 | `p5-01` §Multi-stage Dockerfile | Pin the stage as `FROM --platform=$BUILDPLATFORM node:<pinned> AS frontend`. The output is architecture-independent; without it the arm64 build runs Vite under QEMU emulation. |
| M12 | ROADMAP.md:143,180 | Describes phase 2.5 as in-progress and 2.6 as planned; on disk 2.5 is `closed/` and 2.6 is in `wip/`. |

## Task dependency and order changes

| # | Change | Reason |
| - | ------ | ------ |
| D1 | **Phase 5 cannot start where it sits.** | `plan/wip/` holds `phase2.6-adaptive-stage1`; `plan/open/` holds phase3 and phase4 ahead of phase5. plan/CLAUDE.md runs phases lowest-number-first, one in `wip`. Either the owner explicitly promotes the dashboard ahead of 3 and 4, or phase5 waits — and if it waits, the capability matrix is re-verified after they land. |
| D2 | **Phase 3 and Phase 4 both change what the dashboard shows.** | Phase 3 is certificate machinery (generate CA, import PEM/PFX, export CA, status) plus per-client opt-in HTTPS interception and DoT/DoH listeners — a Certificates page, a per-client control on Clients, and it dissolves the warning problem `p5-08` defers to a deployment finding. Phase 4 activates cosmetic rules (`##`, `#@#`), which today are counted in `rules_inactive` (API.md §GET /lists table) — the Lists three-way partition bar becomes four-way, exactly as `rules_active_url` was split out in p2-03. |
| D3 | **Split a certificate/browser spike out of `p5-08`, run it before `p5-02`.** | The auth design depends on `__Host-`/`Secure` cookies working on the household's real phones against this box's real certificate, whose SANs do not match its LAN address (B8). |
| D4 | **Move the `/events` subscription decision (B9) into `p5-02` or a new `p5-02b`, ahead of `p5-03`.** | The socket manager (`p5-03`) and the Live Feed (`p5-07`) are both built on it. |
| D5 | **`p5-01` has a hidden dependency on `p5-03`.** | Its acceptance criteria require real `.br`/`.gz` siblings and MIME coverage for `.woff2`/`.svg`/`.json`/`.ico`, but the only bundle at that point is a placeholder `index.html`. Either `p5-01` ships a fixture bundle (state it) or those criteria move to `p5-03`. |
| D6 | **Batch the doc-update approvals up front.** | `p5-01` → API.md/ARCHITECTURE.md/SECURITY.md; `p5-02` → API.md/SECURITY.md/CONFIGURATION.md; `p5-03` → root CLAUDE.md; `p5-08` → PERFORMANCE.md/project-state.md; plus ROADMAP.md (D7) and CONTEXT.md (S18). Root CLAUDE.md requires an explicit yes per `.md`, so every task otherwise stops mid-way. |
| D7 | **ROADMAP.md has no Phase 5.** | Line 293 lists the dashboard under "Backlog (no phase committed)", while `plan/open/phase5/CLAUDE.md` opens with "ROADMAP.md's dashboard deliverable". Docs are the source of truth; the phase claims a mandate the roadmap does not grant. |

## Performance and size — what was verified

| Question asked | Answer from the code and docs |
| -------------- | ----------------------------- |
| Bundle gzip **and** brotli | Only gzip is specified (150 KB). Brotli is what ships. See S13. |
| JS/CSS asset count | One JS chunk, one CSS file, fixed by visual-system §Output shape. See S11. |
| First-load request count | `index.html` + JS + CSS + font + SVG sprite ≈ 5, then 3 API reads + 1 WS on the Dashboard. ALPN advertises `h2` (`tls.rs:135`), so extra small requests are cheap on one connection — request count is **not** the pressure point; payload and parse cost are. The WebSocket still opens a second HTTP/1.1 connection (no RFC 8441 support), well inside `MAX_CONNECTIONS = 64`. |
| Critical vs lazy-loaded | Nothing is lazy. See S11. |
| Charts only where needed | No — uPlot is in the single chunk, so it loads on the login page. See S11. |
| Navigation causes full reloads | Unspecified; no router named. See M2. |
| `/events` driving live UI vs polling | Partly. The socket pushes `stats` (full `/stats` payload, ~2 s), `query`, `config_changed`, `list_refreshed`. It pushes **nothing** for `/telemetry`, `/cache`, `/health`, `/clients`, `/lists`, `/policies`, `/config` (S2), and offers no way to opt out of the query firehose (B9). |
| Mobile getting desktop payload | Yes, via the single chunk (S11) and the web font (S12). The responsive-CSS approach itself is correct — a JS branch per viewport would be worse. |
| Dependencies justified | Preact, Vite, uPlot: yes, and the alternatives were weighed. The web font is not justified (S12). `tower-http` is justified (M10). |
| Vite output bloat | Cannot be measured — no code exists. Preconditions to fix in `p5-03`: keep `build.sourcemap` false, set an explicit modern `build.target`, do **not** add `@vitejs/plugin-legacy` (it roughly doubles the bundle), and keep `cssCodeSplit` aligned with whatever S11 decides. |
| Image / memory | Image 13.0 MiB against ≤ 30 MB, RAM 46.6–53.6 MiB against ≤ 128 MB (PERFORMANCE.md:45,49). Static serving is near-free; the real memory item in this phase is Argon2id concurrency (B6), not the bundle. |

## Correction pass — what the owner changed

Six findings were accepted with amendments. The amendments are what shipped.

| # | As written above | As decided |
| - | ---------------- | ---------- |
| B6 | acceptance against the ≤ 128 MB budget | that row is **steady-state**; a login is a transient, so the criterion is written against an explicitly stated transient peak allowance. Semaphore uses `try_acquire` → `503` with `Retry-After`, never a wait queue |
| B7 | `Origin` validation | **no configured allowlist** — a cookie-authenticated upgrade requires `Origin` to equal the request's own effective target origin; a bearer one does not need it at all. An allowlist would break whenever the box is reached by an unconfigured name |
| B9 | subscription protocol | default is **all events**, so no existing client breaks; **no new per-client buffering** — the existing broadcast capacity and lag-disconnect stay, and filtering is what stops a stats-only socket lagging. The engine-side `has_subscribers()` gate moves too, or only the network cost is fixed |
| S3 | rename `allowed` → `permitted` | **not globally.** `history/perf.allowed_delta` is a real allow-verdict counter. `permitted` = the derived `queries − blocked` band; `allow` stays `allow` where the API measured it |
| S5 | prefer lazy `GET /clients/{ip}/policy` | **take the API change** — `GET /clients` gains the policy and assignment source, so the designed column costs one request instead of N |
| S11 | drop the one-chunk rule | drop it, but **cap at 3–4 chunks** and configure module-preload explicitly; Vite's default eagerly refetches the chunks that were split out |

Also decided: new routes go into API.md as `*(Phase 5 — reserved)*` sections
rather than as live surface, following the `## Certificates *(Phase 3 —
reserved)*` precedent; Phase 5 waits for `phase2.6-adaptive-stage1` but is
promoted ahead of `phase3` and `phase4`; documentation approval stays per-change.

## Verdict

**Not ready for implementation** *(as first written — see the correction pass
above).*

Fix B1–B9, settle S1–S8, and get the owner's ruling on D1/D2 before `phase5`
moves to `wip`. That is an editing pass over `p5-01`, `p5-02`,
`capability-matrix.md` and `information-architecture.md` — not a redesign.

The two items that cannot wait for their own task are the API-shape decisions:
`auth` redaction and rejection on `/config` (B3, B4), and the `/events`
subscription filter (B9). Both must be closed before `p5-03` writes a typed
client against the contract.
