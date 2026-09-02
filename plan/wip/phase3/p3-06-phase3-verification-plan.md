# P3-06 — Phase 3 Verification — Implementation Plan

**Phase:** 3 · **Depends on:** p3-02, p3-04, p3-05 · **Task:**
`p3-06-phase3-verification.md`

## TASK START / CONTEXT

1. `plan/wip/phase3/p3-06-phase3-verification.md` — the task file, completely.
2. `plan/wip/phase3/CLAUDE.md` — phase table; p3-01…p3-05 must be `DONE` (or
   the owner has explicitly waved a gap through — record it if so).
3. **Implementation Summaries of every declared dependency:** the
   `docs/code-review/phase3/` review files for p3-02, p3-04, p3-05 — and
   p3-01/p3-03 where a check below names their machinery. Read full findings
   only where a deferred item lands on this task.
4. PERFORMANCE.md — §Budgets (table format, the ~9× dev→RB5009 factor, the
   MB-vs-MiB note) and §Measuring reliably.
5. SECURITY.md — every Phase 3 promise being re-verified: CA key never leaves
   `/config`, public-only export, interception opt-in/never default, the
   fixed crypto set.
6. `docs/measurement-traps.md` — binding for every figure this task produces.
7. `docs/routeros-traps.md` and `docs/deploy-rb5009.md` (§3.2 firewall, §5,
   §5b) — before proposing any router command.
8. `docs/code-review/Global Architecture Review-Reconciled.md` §5 items 7–14
   — the full Phase 3 gate. This task closes the map: **7** cert home /
   ADR-0006 (p3-01), **8** connector redesign — `connect_verified_upstream`,
   hostname-verified upstream TLS (p3-04 decision 4), **9** DoH/DoT placement
   (p3-05 decision 4, incl. the shared-64-permit consequence), **10**
   event/telemetry taxonomy — `EventKind::{HttpsSni, Https}` +
   `ClientTransport` (p3-03/04/05), **11** memory caps per new state owner
   (leaf LRU p3-01, splice buffers p3-03, per-connection bounds p3-04/05),
   **12** 443 steering v4+v6 and **13** on-device TLS measurements
   (discharged here), **14** opt-in bound to stable identity — the p3-04
   owner decision (static-lease precondition), verified per device below.
   Confirm each against the review files; any gap is recorded as a finding,
   not waved through.
9. `docs/project-state.md` — current deployment/container naming before
   proposing anything (the `fah-next` vs `fastadhunter` comment-selector trap).

Root CLAUDE.md working agreement applies with full force here: **the RB5009 is
off limits** — every router step below is *proposed to the owner, who runs it*;
read-only queries are fine. No `.md` edit, commit, or phase move without an
explicit yes.

## Shape of the task

Four workstreams, in this order (each produces evidence the next consumes):

1. Dev-box benches → budget numbers.
2. Security verification suite + offline full-mode E2E (code, gates).
3. On-device work **with the owner**: dst-nat 443, CA install, Private DNS,
   measurements, 24 h soak.
4. Documentation sweep (all proposed, then landed on approval).

## Detailed implementation plan

### Step 1 — bench-backed budget candidates (dev box)

Benches to add or re-run, each A/B against a real pre-phase-3 checkout (never
criterion's stored baseline — measurement-traps rule):

| Figure | Where measured | Feeds |
| ------ | -------------- | ----- |
| SNI verdict + splice added latency **and splice throughput** | `fah-http` `benches/proxy.rs` `https_sni_splice` (p3-03) — see the p3-03 carry-over below | budget row |
| Interception handshake overhead (terminate + re-originate vs splice) | new `fah-http` bench | budget row |
| Minted-leaf cache hit rate under a browsing-like host distribution | `fah-certs` bench + a workload replay (host list from a real browsing session, corpus recorded) | budget row (hit-rate target) |
| Leaf mint cost | p3-01's `certs_mint` bench, re-run | diagnostic beside the cache row |
| DoT/DoH added latency vs UDP (in-engine) | p3-05's harness measurement, promoted to a repeatable bench | budget row |
| RAM with all engines loaded (`dns+http+https`, rules compiled, caches warm) | dev-box RSS reading + on-device soak (step 3) | re-affirmed RAM row |

**p3-03 carry-over (review findings M4 + n5, deferred to this task):**

- **M4 — splice throughput.** On the dev box the splice arm ran ~6× under
  direct-to-origin (148 vs 907 MiB/s, loopback, 1 MiB per connection, 16 KiB
  buffers). If any of that survives on-device the RB5009 ceiling sits near
  25 MiB/s, under gigabit LAN. Before writing the budget row: A/B
  `SPLICE_BUF` 16 KiB vs 64 KiB **on the probe container** against a real
  pre-change checkout, and add a steady-state arm (one connection, N MiB) beside
  the per-connection one so connect + ClientHello + teardown are not in the
  throughput figure. Memory cost of the larger buffer is
  `2 × SPLICE_BUF × https.max_connections` — report both axes.
- **n5 — bench fidelity.** `splice_in_front_of` in `benches/proxy.rs` runs
  its own accept loop: no permit and no `set_nodelay` on the accepted client
  socket, which production's `accept_loop` sets. Rebuild the harness on
  `TlsServer::bind/serve` first, so the measured relay is the shipped one; a
  figure taken on the old harness is diagnostic only.

**p3-04 carry-over (review S1 decision + N8, deferred to this task):**

- **S1 — h2 limits on the terminate leg were set, not measured.** Shipped:
  64 concurrent streams × 64 KiB send buffer **per stream**, 256 KiB
  connection window, 64 KiB stream window, on both the client and the origin
  side (`intercept.rs` `H2_*` consts); CONFIGURATION.md `[https]
  max_connections` states the ≈ 5.5 MiB per-session worst case. Measure on the
  probe container: intercepted h2 throughput with these limits, and per-session
  RSS under a 64-stream stall (slow client, fast origin). The named alternative
  is 32 streams × 32 KiB (≈ 1.5 MiB). Report both axes; lowering the constants
  is an owner decision on the measured trade, never a pre-emptive edit.
- **N8 — `spawn_blocking(prewarm)` runs on every intercepted connection, cache
  hit or not.** One cross-thread hop (and a thread spawn on a cold blocking
  pool) per connection. Profile the terminate-leg handshake on-device; if the
  hop is material against the two TLS handshakes it sits between, the named
  fix is a non-counting cache peek in `fah-certs` (`cached_leaf` cannot serve
  as the peek — it counts `unwarmed_misses`). Measure before touching.

Every number recorded with corpus, workload and device
(`docs/code-review/phase3/p3-06-phase3-verification-review.md` §Measurements;
root docs get a pointer, never the narrative — root CLAUDE.md rule 19).
Dev-box latency figures convert with the measured ~9× factor only for
CPU-bound in-engine work; **TLS and HTTP-path figures do not convert** —
p2-08 measured 4.5–10× spread for HTTP work — so every TLS/splice/handshake
budget row rests on the step-4 on-device measurements, with the dev-box run
as the A/B sanity check. No clock readings. Budget values are **derived from these measurements plus headroom,
proposed to the owner in the review file** — this plan invents none; each
PERFORMANCE.md row is `TBD — must be measured during verification` until then.

### Step 2 — security verification suite (`tests/` or `crates/fastadhunter/tests/security_phase3.rs`)

Adversarial checks, not unit re-runs — each asserts an externally observable
property of the full binary or full API surface:

1. **CA key unreachable via every API route.** Walk the live route table
   (boot the binary/harness, enumerate every documented route from API.md,
   including `/dns-query`, static dashboard paths and `/debug/*`), request
   each with authenticated GET/POST probes, and assert no response body ever
   contains a private-key block or the raw key bytes (read
   `/config/ca-key.pem` first, search responses for its base64 payload —
   stronger than grepping for `PRIVATE KEY`). Path-traversal probes against
   the static file server (`web.rs`) aimed at `/config` included.
2. **A non-listed client cannot be intercepted.** Two clients, one opted in
   (p3-04's mechanism); the non-listed one's TLS connection is spliced
   byte-identically (sampled comparison of client-observed certificate chain
   — it must be the origin's, never a minted leaf).
3. **A bad upstream certificate is never masked.** With interception active,
   an upstream presenting an invalid cert (self-signed test origin) must
   produce a client-visible failure, not a re-signed success.
4. **Exported artifacts contain no private material** — over the wire, both
   formats (extends p3-02's test to the full-binary harness).
5. **Interception disabled ⇒ byte-identical splice** — sampled payload
   comparison through the SNI path vs a direct connection.
6. **DoT never falls back to plaintext; `/dns-query` is the only
   unauthenticated addition** (re-asserted at the full-binary level).

Any failure here is a finding for the review file and blocks `DONE` — these
are SECURITY.md promises, not targets.

**p3-04 carry-over — terminate-leg unhappy paths (review M4 rows 5–9 and
N10, deferred to this task by the accepted p3-04 review; integration tests in
`crates/fah-http/tests/interception.rs`, in that harness):**

| Path | Proof |
| --- | --- |
| concurrent h2 requests from one listed client | 8 parallel `/page` on one h2 session ⇒ all 200, `origin.connections == 1` |
| streaming request body | POST 4 MiB ⇒ origin sees 4 MiB; proxy RSS delta bounded by the S1 limits — a body is never held |
| client disconnect mid-response, upstream disconnect mid-response | session ends and the permit is released: `max_connections = 1`, a second connect succeeds. Rename or extend `an_idle_intercepted_session_is_closed_and_its_permit_returned`, which today asserts nothing about the permit (N10) |
| shutdown with live intercepted sessions | record the semantics: `Engine::shutdown` aborts the accept loop only; live sessions end with the runtime — pre-existing, shared with :80 and the splice leg (p3-04 L4) |
| IPv6 listed client end-to-end | `[::1]` listed, connect over v6 ⇒ intercepted (today only `intercepts()` is unit-tested for v6) |

These are the p3-04 harness's own tests, not new unit coverage; the "Unit:
none new" line below stands.

**p3-05 carry-over (review N3, deferred to this task):** a certificate
failure at boot (no store, an unloadable API pair, a `ServerConfig` build
error) closes 853 with one `error!` line; `Server::dot_addr()` reads `None`
after `serve`, but no API surface reports it — `/health` and
`GET /api/v1/certificates` do not know DoT is closed, so a default-on
listener can silently disappear. Propose (API.md edit, owner approval) a
`dot` listener state — `listening` / `closed` plus the reason — on
`GET /api/v1/certificates` or in `/health` `checks`, and extend suite item 6
with the closed posture: an unloadable API pair ⇒ 853 refuses, :53 answers,
never plaintext. Until it lands, the Step 4 Private DNS runbook starts by
reading the container log for that line.

### Step 3 — offline full-mode E2E (one scripted scenario)

Extend `crates/fastadhunter/tests/e2e.rs` (or a sibling `e2e_https.rs`
sharing `tests/common/`): boot `engine.mode = "dns+http+https"` with ephemeral
ports, one rule set, one client; assert in sequence — DNS block (UDP), HTTP
URL block (8080 path), SNI block, a no-SNI ClientHello handled per
`[https.sni] no_sni` (closed and classified — p3-03 owns the full matrix, this
just proves the full-mode path does not hang or crash on it), intercepted HTTPS
URL block (client trusting the test CA, opted in), DoT query answered, DoH query
answered. One test, seven assertions, so the mode's definition of done is a
single green line. Windows WSAEACCES trap noted; the test must skip-with-message, not
fail, when the ephemeral bind is refused (matching existing e2e handling).

### Step 4 — on-device work (WITH the owner — propose, never run)

Prepared as a numbered runbook in the review file; each step: the exact
command, what it does, when it takes effect, and the rollback.

1. **dst-nat 443 (v4) + the v6 story (GAR §5.12).** Read the owner's live
   firewall/NAT chains first (read-only `print`), then propose rule text with
   explicit placement — never a bare `add` (it appends behind any final
   drop). Target is the container's `[https.listen]` port — default **8444**
   (8443 is the API's; p3-03 rejects the collision at startup). The rule is
   **`protocol=tcp` only** — UDP/443 (QUIC) stays unsteered so browsers fall
   back to TCP instead of black-holing HTTP/3 (the container listens on no
   UDP 443; QUIC is a documented p3-03 non-goal). Rollback =
   remove the one rule. The v6 half: propose either the
   equivalent v6 steering or an explicit, recorded owner decision that v6/443
   stays unsteered this phase (record which traffic that leaves uncovered).
   **Separately — a distinct concern from v6 steering — warn the owner that
   steering all :443 closes no-SNI/ECH connections.** The container cannot
   recover their destination: measured on-device 2026-08-31,
   `getsockopt(SO_ORIGINAL_DST)` returns `ENOENT` on a dst-nat'd flow (the NAT
   conntrack lives in the router's netns, `docs/routeros-traps.md`), so such a
   connection is closed, not forwarded, and the DNS layer is its only backstop.
   Confirm the deployed lists cover the domains the owner cares about before
   enabling full mode.
2. **CA install walkthrough** on one Android and/or Windows test device:
   export DER via the API, install, screenshots into `docs/images/`
   (image files are not `.md` — still list them for the owner since they land
   in the repo). Then browse; record what the device shows.
   **`GET /api/v1/certificates/ca/export` is authenticated** (p3-02 decision,
   SECURITY.md's two-exemption rule stands — the public root is not secret,
   but a third exemption widens the unauthenticated surface for one download
   per device). The walkthrough is therefore: open the dashboard on the
   device, log in, download `fastadhunter-ca.crt` (the session cookie carries
   the request), install from Downloads. Not a bare URL. If the device's
   browser drops cookies on download, fall back to `curl -H "Authorization:
   Bearer …" -o fastadhunter-ca.crt` from another machine and transfer the
   file; record which path the walkthrough used.
   **Sequencing (p3-04 L2, owner decision: a listed client with no CA is
   closed, not spliced):** list the test device in `[https.interception]
   clients` only **after** the CA is installed on it, then restart
   (boot-class). Listing it earlier turns every HTTPS connection from that
   device into a closed socket and a `status 0` `https` event until the
   install lands. **ECH (p3-04 L7):** from the listed device, browse one
   ECH-enabled origin; expect the browser to retry without ECH and the retry
   to be filtered under the real name; record the extra upstream handshake and
   `status 0` event per ECH origin, and whether the retry was visible to the
   user. Unlisted devices are unaffected.
3. **Private DNS**: p3-05 decision 3 walked end-to-end — pick the hostname,
   propose the local answer for it (a `$dnsrewrite` rule mapping it to the
   container address — the bootstrap: the phone resolves the Private DNS
   hostname over plain DNS while validating), set Settings → Network →
   Private DNS → hostname, confirm the SNI-minted leaf validates. **Record
   the explicitly-tested assumption either way:** whether this device's
   Private DNS validation consults the user CA store (vendor behaviour
   varies; if it refuses, the imported-real-cert route is the remaining path
   and the walkthrough documents that outcome).
4. **Pinned-app spot check**: one banking app on the test device with
   interception active for it excluded/not opted in — must work unchanged.
   **Before this check** the owner trims or extends
   `fah_http::BASELINE_EXCLUSIONS` (p3-04 TODO — the shipped list is a first
   cut: Apple/Google/Microsoft update, push and store hosts, WhatsApp, Signal,
   PayPal, Revolut, Wise, N26, eight Romanian banks); it is a code change
   with its own gates, and the final list is recorded in this task's review
   file together with the CONFIGURATION.md `exclude_domains` text that names
   it. The banking app used must be covered by the baseline or by
   `exclude_domains`, else the check proves nothing about exclusions.
   For every intercepted client, **verify the static-lease precondition**
   (p3-04's GAR §5.14 owner decision): confirm on the router (read-only) that
   the listed IP is a static lease/address before calling §5.14 closed.
5. **Measurements on-device (GAR §5.13):** TLS handshake cost, splice
   throughput, intercepted-session RSS under a 64-stream stall (P3), DoT/DoH
   latency vs UDP — via the probe-container procedure (`docs/routeros-traps.md`; the
   *(2026-09-03: "interception CPU under browsing" withdrawn — per-leg CPU is
   not separable on the RB5009; the review's P8 full-mode CPU diagnostic
   replaces it, review §Pre-declaration declaration changes)*
   `fah-probe` harness facts in `docs/project-state.md`), not the production
   container. Pre-declare each measurement's workload and sample size before
   running it — the phase-2.6 lesson: a declaration that can be quietly
   edited is not a declaration.
6. **24 h soak in full mode** on the production container — this is a deploy
   and needs its own owner approval; numbers vs the budget rows; RAM ≤ 128 MB
   steady (budget in decimal MB, readings in MiB — compare like with like).
   Watch item from p3-05 decision 4: peak concurrent DoH sessions against the
   shared 64-permit API ceiling — the recorded number decides whether the
   named escape hatch (const bump / separate semaphore) is ever built.
   Second watch item (p3-03 m8, split in p3-04): `non_tls` vs
   `hello_timeouts` over the window — the ratio says whether silent browser
   preconnects dominate the port, which decides if the `hello_timeout_ms`
   default (10 s of permit per silent socket) needs revisiting.
   **Prerequisite for that watch item (p3-04 L5 + TODO, required before the
   soak starts):** `non_tls`, `hello_timeouts` and `upstream_cert_failures`
   are counted in `fah_http::ProxyCounters` but published nowhere —
   `/telemetry` carries only the refused sum (`main.rs` telemetry poll). Settle
   the p3-04 decision first: a per-listener block on `GET /api/v1/telemetry`
   (API.md edit, owner approval — API.md §telemetry already names the three
   as unpublished) or another agreed read path. The same decision must fix
   L5 before any consumer exists: on the terminate leg `requests` is per
   connection while `blocked`/`refused_claim` are per request, so
   `blocked > requests` is possible on one listener.
   **Third watch item (p3-04 N4 detector, shared with p3-05):**
   `GET /api/v1/certificates` `leaf_cache.unwarmed_misses` reads 0 after the
   browsing workload and at the end of the soak — **with a CA installed**.
   p3-05 mints at the handshake (there is no re-warm ticker), so DoT moves the
   counter only when the resolver misses without a preceding mint: no CA
   (every SNI hello counts — expected, not a finding), an invalid SNI, or a
   mint failure. A non-zero value with a CA is p3-04's prewarm-then-evict
   window (more than 512 first-sight hosts inside one handshake) or a DoT
   mint failure — attribute it before filing.
   **Mint-rate watch (p3-05 review N8):** with a CA installed, **every** LAN
   client can drive one mint per DoT handshake for any SNI it names — 853 is
   default-on, whereas p3-04 confined SNI-driven minting to listed clients.
   Record `leaf_cache.minted_total` growth per hour over the soak, the peak
   `inflight`, and `superseded`/`evictions`; a hostile or misbehaving client
   shows as a mint rate far above the number of DoT hostnames in use and as
   p3-04's leaves churning (re-mints on the terminate leg). Today's bound is
   64 connections × one P-256 mint; measure the on-device mint cost here
   (the ≈ 0.5 ms figure is the dev-box number through the documented factor,
   not a reading). A per-client mint rate limit is the named escape hatch,
   built only if the soak shows the need.
   **Fourth watch item (p3-04 L4):** an intercepted h2 session cut by the
   idle watchdog leaves its in-flight stream tasks and the upstream
   connection task alive until the origin answers or `hello_timeout` fires,
   outside `max_connections`. Over the soak, RSS must not trend with the
   number of idle-cut sessions; record the reading as the L4 evidence. The
   shutdown half of L4 is in Step 2's carry-over table.
7. **Certificate-store checks that only the device can give** (deferred by
   the p3-01 and p3-02 reviews to this task — **all mandatory**, on the probe
   container, propose-only for anything on the production one):
   - **Import-then-restart** over the API: `POST …/import` with a real pair,
     restart, confirm the acceptor serves the imported certificate and status
     reports `"imported"`. This is the acceptance path for p3-02 M1's
     boot-abort half and for the MEDIUM-2 recovery API.md documents (copy
     back from `api-archive/`, or the staged-key completion).
   - **`0600` on every private key** after each write path: first-boot
     `api-key.pem`, imported `api-key.pem`, `ca-key.pem` after generate and
     after import, every archived key under `ca-archive/` and `api-archive/`,
     and the staged `*.pem.tmp` while it exists. `write_private`'s
     `OpenOptionsExt::mode(0o600)` compiles only on unix, so this is its first
     execution anywhere — zero coverage on the Windows dev box.
   - **Archive bound**: regenerate past `fah_certs::MAX_ARCHIVES` (8) and
     confirm the ninth answers the documented `ArchiveFull` error with the
     live pair intact; record the retention story the operator needs
     (p3-02 LOW-2 — pruning is an owner decision, verification only records
     what happens at the cap).
   - **Generate / import wall time** on the device (p3-02 plan §Performance
     contract left them `TBD`), for the PERFORMANCE.md rows below.

### Step 5 — documentation sweep (all proposed, landed on approval)

- PERFORMANCE.md §Budgets: the new rows (SNI+splice, interception handshake,
  leaf-cache hit rate, DoT/DoH added latency, **cold `prewarm` per first-sight
  host** — p3-01's `certs_mint` bench measures the whole cold path including
  the eviction scan, not raw keygen, so label the row that way — and
  **CA generate / pair import wall time** from Step 4 item 7), each with its
  measured column and a pointer to the review file.
- SECURITY.md: no new promises — verify wording matches what shipped
  (present-tense sweep of §Later phases).
- `docs/deploy-rb5009.md`: new §HTTPS (dst-nat 443, CA install, Private DNS,
  rollback) mirroring the existing §5b structure — including the operator
  warning that steering all :443 closes no-SNI/ECH connections (measured
  `ENOENT`, not forwardable; DNS-layer backstop), so the operator knows what
  full mode does to that slice of traffic before enabling it.
- **Dashboard re-review (ROADMAP.md: Phase 3 triggers one).** p3-03 left
  `https-sni` out of `dashboard/frontend/src/pages/live-feed/filters.ts`
  (`KINDS = ['dns', 'http']`), and `detail.tsx` gates the HTTP detail block on
  `kind === 'http'`, so an SNI row renders through the DNS-shaped branch with
  empty method/path; p3-04 adds a third kind (`https`). Add both kinds to
  `KINDS` and a detail branch for each, or record the owner's decision to
  defer to a dashboard task — an unfiltered kind is a finding, not a note.
- README operating-modes wording drift check (the task's doc sweep).
- CONFIGURATION.md/API.md: only if p3-02…p3-05 left an approved edit pending.
- ROADMAP.md: Phase 3 deliverables (SNI filtering, per-client interception,
  DoT/DoH listeners) to delivered wording at phase close — the p3-04 plan's
  §Doc changes carried this line and the p3-04 approved doc list did not
  include it, so it lands here.

## Performance contract

This task *sets* the contract rather than consuming one. Classes:

- **Hard gates:** RAM ≤ 128 MB steady on-device in full mode; security suite
  green; verdict parity and splice-byte-identity properties.
- **Targets (numbers TBD from step 1, proposed to the owner):** SNI added
  latency, interception handshake overhead, leaf-cache hit rate, DoT/DoH
  added latency.
- **Diagnostic:** mint cost, handshake CPU profile, per-transport soak
  distribution, startup delta with the cert store present.

Rule: every figure published in PERFORMANCE.md carries measured status; a
target that could not be measured on-device ships as
`TBD — must be measured during verification` or is withheld, never invented.
The >10 % hot-path regression rule applies to the phase's whole diff against
the pre-phase-3 checkout on the existing DNS/HTTP benches.

## Tests

### Unit

None new — this task verifies; unit coverage belongs to p3-01…p3-05.

### Integration

The security suite (step 2) — six scenarios above, exact test names
`ca_key_unreachable_via_every_route`, `non_listed_client_is_never_minted_a_leaf`,
`bad_upstream_cert_is_not_masked`, `exports_contain_no_private_material`,
`splice_is_byte_identical_when_interception_is_off`,
`dns_query_is_the_only_new_unauthenticated_route`.

### E2E

`full_mode_blocks_at_every_layer` (step 3) — the one scripted scenario.

### Regression

- Full workspace suite green at the phase tip.
- Existing DNS/HTTP benches vs pre-phase-3 checkout: within the 10 % rule or
  justified.
- Phase 2 HTTP e2e (`http_e2e.rs`) unchanged — SNI/interception must not have
  disturbed the plaintext path.

### Security

The suite *is* the security deliverable (step 2); on-device: pinned-app
check, CA-install verification, WAN-exposure re-check (nothing new listens
WAN-side — read-only router query).

### Performance

Step 1 benches + step 4.5 on-device measurements + the soak.

## Verification / Gates

- **Mandatory:** fmt / clippy / test workspace; security suite green; E2E
  scenario green; bench A/B recorded; review file complete with §Measurements
  (corpus/workload/device on every row).
- **Mandatory but owner-executed:** dst-nat 443 applied, CA installed on the
  test device, Private DNS working, 24 h soak numbers recorded. If the owner
  defers the soak, the task goes `AWAITING SOAK` in the phase table with the
  flip condition named — the phase is not finished while it stands
  (plan/CLAUDE.md vocabulary).
- **Recommended:** re-run the p2.6-13 harness sanity checks before trusting
  probe-container numbers.
- **Diagnostic:** per-transport traffic split over the soak window.

## Non-goals

- Phase 4 HTML rewriting; performance tuning beyond meeting budgets (file
  follow-ups); ECH workarounds (measured transport limitation — no-SNI/ECH has
  no recoverable destination, the DNS layer catches those domains); iOS
  walkthrough (Android/Windows only, per the task).

## Acceptance criteria (from the task file)

- Every new budget row bench-backed and met on dev hardware; soak numbers on
  device; RAM ≤ 128 MB steady in full mode.
- Security suite green; deploy guide reproducible; pinned-app spot check
  unaffected.
- Gates green.
