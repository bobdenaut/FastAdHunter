# P3-06 — Phase 3 Verification — Implementation Plan (campaign 2)

**Phase:** 3 · **Depends on:** p3-02, p3-04, p3-05 · **Task:**
`p3-06-phase3-verification.md`

Rewritten 2026-09-08 for the post-merge tip. Merge `e0c6071` brought `main`
`857865d` (0.3.3) into `phase3-06` and re-homed the HTTPS listener onto the
HTTP allocation domains. Campaign 1's on-device figures were taken at
`a2d0802`, before that merge, and are superseded in full — see
`p3-06-testing-plan.md` §What changed under the campaign. The code side of this
task (security suite, full-mode e2e, runbook text) survived the merge and is
green at the tip; the measurement side starts over.

## TASK START / CONTEXT

1. `plan/wip/phase3/p3-06-phase3-verification.md` — the task file, completely.
2. `plan/wip/phase3/CLAUDE.md` — phase table; p3-01…p3-05 are `DONE`, p3-06 is
   `AWAITING SOAK`.
3. **Implementation Summaries of every declared dependency:** the
   `docs/code-review/phase3/` review files for p3-02, p3-04, p3-05 — and
   p3-01/p3-03 where a check below names their machinery. Read full findings
   only where a deferred item lands on this task.
4. `docs/code-review/phase3/main-phase3-integration-audit.md` — **read first**
   among the review files. It establishes what the merge did and did not
   change, and its F1–F6 are this task's inbox (§Merge inbox).
5. `docs/code-review/phase2.6/alloc-domains-n-sweep.md` — the N sweep for
   `dns+http`. Its rig is reused by P10 and its measured LAN ceiling
   invalidates one of campaign 1's budget rows.
6. `docs/project-state.md` — deployment before proposing anything: production
   is 0.3.4 on `veth1` with N=2 pinned on `fah-env`. ~~the 0.3.3 soak runs
   to **2026-09-16**~~ — **stopped on day 5, 2026-09-16, no soak is running**
   (`ad795b0`). The probe (`fah-probe` on `veth3`, envlist `fah-env`) is
   recorded in `docs/code-review/phase3/p3-06-testing-results.md` §Session
   state and `docs/routeros-traps.md`, not there.
7. PERFORMANCE.md — §Budgets (table format, the ~9× dev→RB5009 factor, the
   MB-vs-MiB note) and §Measuring reliably.
8. SECURITY.md — every Phase 3 promise being re-verified: CA key never leaves
   `/config`, public-only export, interception opt-in/never default, the fixed
   crypto set.
9. `docs/measurement-traps.md` — binding for every figure this task produces.
10. `docs/routeros-traps.md` and `docs/deploy-rb5009.md` (§3.2 firewall, §5,
    §5b) — before proposing any router command.
11. `docs/code-review/Global Architecture Review-Reconciled.md` §5 items 7–14
    — the full Phase 3 gate. This task closes the map: **7** cert home /
    **ADR-0007** `0007-certificate-machinery-home.md` (p3-01; renamed from
    `0006-certificate-machinery-home.md` at `5c61891`, after the merge,
    because `main` had taken 0006 for the allocation-domain ADR. Root docs
    now mean the allocation-domain ADR by "ADR-0006"; the p3-01 / p3-02
    review and plan files, `phase3-audit.md` and older rows of this task's
    review still write "ADR-0006" for the certificate ADR — read 0007 there),
    **8** connector redesign — `connect_verified_upstream`, hostname-verified
    upstream TLS (p3-04 decision 4), **9** DoH/DoT placement (p3-05 decision 4,
    incl. the shared-64-permit consequence), **10** event/telemetry taxonomy —
    `EventKind::{HttpsSni, Https}` + `ClientTransport` (p3-03/04/05), **11**
    memory caps per new state owner (leaf LRU p3-01, splice buffers p3-03,
    per-connection bounds p3-04/05), **12** 443 steering v4+v6 and **13**
    on-device TLS measurements (discharged here), **14** opt-in bound to
    stable identity — the p3-04 owner decision (static-lease precondition),
    verified per device below. Confirm each against the review files; any gap
    is recorded as a finding, not waved through.

Root CLAUDE.md working agreement applies with full force: **the RB5009 is off
limits** — every router step below is *proposed to the owner, who runs it*;
read-only queries are fine. No `.md` edit, commit, or phase move without an
explicit yes.

## Shape of the task

Four workstreams, in this order (each produces evidence the next consumes):

1. Dev-box benches and code gates at the tip → budget candidates.
2. Security verification suite + offline full-mode E2E — re-run at the tip and
   close the merge's coverage gaps.
3. On-device work **with the owner**: probe preconditions, the N sweep, the
   measurement campaign, dst-nat 443, CA install, Private DNS, 24 h soak.
4. Documentation sweep (all proposed, then landed on approval).

## Merge inbox

The integration audit left six items on this task. They are closed here, not
deferred again.

| # | Item | Closed by |
| --- | --- | --- |
| F1 | `main`'s IP-literal fix covers `Proxy` only; on the HTTPS path an allowed IP-literal SNI is handed to the resolver and fails 100 % of the time, while CONFIGURATION.md says the switch governs both | **owner decision in step 5**: port the fix (literal → `policy.check` → connect) or state HTTP-only in CONFIGURATION.md and in the `tls_server` test that pins today's outcome. Not left open past this task |
| F2 | splice on a domain has no crate-level test; only `e2e_https` covers it, through the compiled-in N, on a box with ≥ 2 cores | step 2: one `domains: 1` splice test in `fah-http/tests/sni.rs` |
| F3 | the per-domain `JoinSet` bound is `http.max_connections + https.max_connections`, and `HANDOFF_QUEUE` (32/domain) is shared by both lanes, so a head-of-line stall blocks both acceptors | step 5 doc line; measured incidentally by P10's mixed arm |
| F4 | the 5 s drain is held by an idle spliced session (`idle_timeout` 60 s) or an idle intercepted keep-alive | recorded in step 4's shutdown arm; the fix rides alloc 11b, owner decision before the full-mode soak |
| F5 | `https` is in `BOOT_KEYS` but no `https.*` key is in the classification test's boot list | step 2: one line in that test |
| F6 | the review file's "Runbook 6 cannot start before the 0.3.1 soak ends 2026-09-08" is stale — 0.3.1 was stopped on day 6, ~~0.3.3 runs to 2026-09-16~~. **Amended 2026-09-16: 0.3.3 became 0.3.4 and that soak was stopped on day 5 too (`ad795b0`). No soak has yet run to term on this device, so no soak sequencing gates anything today** | step 4's soak sequencing, and the review file's next approved edit |

## Detailed implementation plan

### Step 1 — bench-backed budget candidates (dev box)

**The A/B baseline changed, and improved.** Campaign 1 had to A/B against a
pre-phase-3 checkout that also predated the allocation domains, so the delta
mixed two changes. `main` at `857865d` now carries the domains, so
`phase3-06` tip vs `main` `857865d` isolates Phase 3 on one execution model.
Never criterion's stored baseline — measurement-traps rule.

| Figure | Where measured | Feeds |
| ------ | -------------- | ----- |
| SNI verdict + splice added latency, and splice throughput | `fah-http` `benches/proxy.rs` `https_sni_splice`, harness rebuilt on `TlsServer::bind/serve` so the measured relay is the shipped one | budget row |
| Interception handshake overhead (terminate + re-originate vs splice) | `fah-http` bench | budget row |
| Minted-leaf cache hit rate under a browsing-like host distribution | `fah-certs` bench + a workload replay (host list from a real browsing session, corpus recorded) | budget row (hit-rate target) |
| Leaf mint cost | p3-01's `certs_mint` bench, re-run; D11-on-device is its device twin | diagnostic beside the cache row |
| DoT/DoH added latency vs UDP (in-engine) | the `encrypted_latency` harness, promoted to a repeatable bench | budget row |
| RAM with all engines loaded (`dns+http+https`, rules compiled, caches warm) | dev-box RSS reading + the on-device soak | re-affirmed RAM row |
| Existing DNS/HTTP benches | unchanged set, A/B tip vs `857865d` | the >10 % hot-path regression rule |

Dev-box figures convert with the measured ~9× factor only for CPU-bound
in-engine work; **TLS and HTTP-path figures do not convert** — p2-08 measured a
4.5–10× spread for HTTP work — so every TLS/splice/handshake budget row rests
on the step-4 on-device measurements, with the dev-box run as the A/B sanity
check. No clock readings. Budget values are derived from measurement plus
headroom and **proposed to the owner in the review file**; this plan invents
none, and each PERFORMANCE.md row is `TBD — must be measured during
verification` until then.

Every number carries corpus, workload, device **and N**
(`docs/code-review/phase3/p3-06-testing-results-2.md`; the review file
§Measurements links to it; root docs get a pointer, never the narrative — root
CLAUDE.md rule 19).

### Step 2 — security suite and coverage gaps

The suite exists and is green at the tip: `security_phase3` 7/7,
`fah-http` `interception` 33/33, `sni` 9/9 (audit §Measurements). This step
re-runs it at the phase tip and closes what the merge exposed.

Re-run, unchanged in intent — each asserts an externally observable property of
the full binary or full API surface:

1. `ca_key_unreachable_via_every_route` — walk the live route table, request
   each documented route with authenticated probes, assert no response body
   contains the CA key's base64 payload (read `/config/ca-key.pem` first;
   stronger than grepping for `PRIVATE KEY`). Path-traversal probes against the
   static file server included.
2. `non_listed_client_is_never_minted_a_leaf` — the non-listed client's TLS
   connection is spliced, and the client-observed chain is the origin's.
3. `bad_upstream_cert_is_not_masked` — an invalid upstream cert produces a
   client-visible failure, never a re-signed success.
4. `exports_contain_no_private_material` — both formats, over the wire.
5. `splice_is_byte_identical_when_interception_is_off`.
6. `dns_query_is_the_only_new_unauthenticated_route`, including the closed
   posture: an unloadable API pair ⇒ 853 refuses, :53 answers, never plaintext.

New in campaign 2:

- **F2 — splice on a domain.** `fah-http/tests/sni.rs` gains one test with
  `domains: 1`, asserting the splice verdict and that the session ran on a
  `fah-http-<i>` thread. Today the harness defaults to `domains: 0` and the
  domain splice path is covered only by `e2e_https`, on a box with ≥ 2 cores.
- **F5 — boot-key classification.** One `https.*` key added to the
  `fah-api` `config_store` boot list in
  `boot_key_classification_matches_what_actually_applies_the_key`.
- **Hand-off saturation (F3), integration level.** With `domains: 1` and a
  `HANDOFF_QUEUE` full of one lane's connections, the other lane's acceptor is
  still bounded and no permit leaks. Property, not a figure.

Any failure here is a finding for the review file and blocks `DONE` — these are
SECURITY.md promises, not targets.

### Step 3 — offline full-mode E2E

`crates/fastadhunter/tests/e2e_https.rs` exists and passes 2/2. One change:

- **A4 — the domain lane is exercised by accident.** The test writes no
  `[runtime]` key, so N comes from `available_parallelism` on whatever box
  runs it; on a 1–2 core CI-less dev box it would silently take the shared
  path. Pin `runtime.http_runtimes` in the fixture and assert the thread name,
  so "full mode on the domain lane" is what the green line means.

The scenario stands: boot `engine.mode = "dns+http+https"` with ephemeral
ports, one rule set, one client; assert in sequence — DNS block (UDP), HTTP URL
block, SNI block, a no-SNI ClientHello handled per `[https.sni] no_sni`,
intercepted HTTPS URL block (client trusting the test CA, opted in), DoT query
answered, DoH query answered. The Windows WSAEACCES trap stands: skip with a
message, never fail, when the ephemeral bind is refused.

### Step 4 — on-device work (WITH the owner — propose, never run)

Prepared as a numbered runbook in the review file; each step: the exact
command, what it does, when it takes effect, and the rollback. The measurement
arms and their declarations live in `p3-06-testing-plan.md`; this step is the
sequencing and the owner-executed half.

**0. Probe preconditions — blocking, before any HTTPS arm.**

- The probe config carries `strategy = "fallback"`, removed at `fa9451a`. The
  probe **will not boot** on the tip build until it is dropped or set to
  `adaptive`.
- The probe is attached to `envlists fah-env`, which pins production's
  `FAH__RUNTIME__HTTP_RUNTIMES=2`; env beats file and API, so N cannot be moved
  from the config API. Propose `fahprobe-env` (mimalloc keys + the N var,
  modelled on phase 2.6's `h1buf-env`) and `/container/set` to attach it.
- New images at the tip hash for every container: `fah-probe`,
  `fah-splicebench`, `fah-p4`, `fah-certs`. Campaign 1's `a2d0802` images are
  retired.
- The second LAN endpoint is the **Mac** — wired, Node, ssh, AC power, a second
  IPv4 alias for P2, `sudo` for the `:443` origins. Preconditions and the
  Darwin costs are in `p3-06-testing-plan.md` §The Mac endpoint.
- **Still outstanding:** P3 needs a publicly trusted certificate under a public
  name on the Mac origin (Let's Encrypt DNS-01). The release probe verifies
  upstreams against `webpki-roots` only. Until that exists, P3 does not run.
- **Closed, not owed:** campaign 1's P3 BLOCKED state — one h2 stream of 64
  answered through the terminate leg (smoke F7: control 64/64, stall 5/64) —
  is **p3-04 S2**: filed, root-caused (256 KiB h2 connection window), fixed
  (`H2_CONNECTION_WINDOW` = 4 MiB, both legs), pinned by the `interception.rs`
  stall tests and reviewed in the review file §Post-review work E
  (2026-09-03). Campaign 2's P3 stall arm confirms the fix on the device; a
  barrier not met is a regression finding. The fix also moved the P3 memory
  ceiling from 5.5 MiB to ≈ 8 MiB (`p3-06-testing-plan.md` §The P3 ceiling).

**1. P10 — the N sweep with TLS (ADR-0006 revisit trigger).** Runs before the
other on-device arms: it fixes the N every later figure is taken at, and its
result is what the `phase3-06` deploy uses. `http_runtimes ∈ {0, 1, 2, 4}`, one
owner-run restart per arm, the phase-2.6 rig plus TLS arms
(`p3-06-testing-plan.md` §P10 rig). Output: a proposed N and the measured trade
— connection rate, p95, DNS under TLS load, cores, ΔRSS — for the owner to
decide on.

**2. The measurement campaign** — SNI, P1 (loopback sweep, LAN, control), P2,
P3, P4, P4-LAN, P5, D11, P6, P8-probe, P9-probe, Runbook 7. All declared in
`p3-06-testing-plan.md`; all at the N item 1 settles unless the arm sweeps it.

**3. dst-nat 443 (v4) + the v6 story (GAR §5.12).** Read the owner's live
firewall/NAT chains first (read-only `print`), then propose rule text with
explicit placement — never a bare `add` (it appends behind any final drop).
Target is the container's `[https.listen]` port — default **8444** (8443 is the
API's; p3-03 rejects the collision at startup). The rule is **`protocol=tcp`
only** — UDP/443 (QUIC) stays unsteered so browsers fall back to TCP instead of
black-holing HTTP/3. Rollback = remove the one rule. The v6 half: propose
either equivalent v6 steering or an explicit, recorded owner decision that
v6/443 stays unsteered this phase, naming the traffic that leaves uncovered.
**Separately — a distinct concern from v6 steering — warn the owner that
steering all :443 closes no-SNI/ECH connections.** The container cannot recover
their destination: measured on-device 2026-08-31, `getsockopt(SO_ORIGINAL_DST)`
returns `ENOENT` on a dst-nat'd flow (the NAT conntrack lives in the router's
netns), so such a connection is closed, not forwarded, and the DNS layer is its
only backstop. Confirm the deployed lists cover the domains the owner cares
about before enabling full mode.

**4. CA install walkthrough** on one Android and/or Windows test device: export
DER via the API, install, screenshots into `docs/images/` (image files are not
`.md` — still list them for the owner since they land in the repo). Then
browse; record what the device shows.
`GET /api/v1/certificates/ca/export` is **authenticated** (p3-02 decision), so
the walkthrough is: open the dashboard on the device, log in, download
`fastadhunter-ca.crt`, install from Downloads. Not a bare URL. If the device's
browser drops cookies on download, fall back to `curl -H "Authorization:
Bearer …" -o fastadhunter-ca.crt` from another machine and transfer the file;
record which path was used.
**Sequencing (p3-04 L2, owner decision: a listed client with no CA is closed,
not spliced):** list the test device in `[https.interception] clients` only
**after** the CA is installed on it, then restart (boot-class). Listing it
earlier turns every HTTPS connection from that device into a closed socket and
a `status 0` `https` event until the install lands.
**ECH (p3-04 L7):** from the listed device, browse one ECH-enabled origin;
expect the browser to retry without ECH and the retry to be filtered under the
real name; record the extra upstream handshake and `status 0` event per ECH
origin, and whether the retry was visible to the user.

**5. Private DNS** — p3-05 decision 3 walked end-to-end: pick the hostname,
propose the local answer for it (a `$dnsrewrite` rule mapping it to the
container address — the bootstrap: the phone resolves the Private DNS hostname
over plain DNS while validating), set Settings → Network → Private DNS →
hostname, confirm the SNI-minted leaf validates. **Record the explicitly-tested
assumption either way:** whether this device's Private DNS validation consults
the user CA store (vendor behaviour varies; if it refuses, the imported-real-
cert route is the remaining path and the walkthrough documents that outcome).
The runbook starts by reading the container log for a certificate failure at
boot — a `DotListener::Closed` posture is surfaced but easy to miss.

**6. Pinned-app spot check** — one banking app on the test device with
interception active but that app excluded or not opted in; must work unchanged.
**Before this check** the owner trims or extends
`fah_http::BASELINE_EXCLUSIONS` (the shipped list is a first cut); it is a code
change with its own gates, and the final list is recorded in this task's review
file together with the CONFIGURATION.md `exclude_domains` text that names it.
The app used must be covered by the baseline or by `exclude_domains`, else the
check proves nothing. For every intercepted client, **verify the static-lease
precondition** (GAR §5.14): confirm on the router (read-only) that the listed
IP is a static lease before calling §5.14 closed.

**7. Certificate-store checks that only the device can give** — all mandatory,
on the probe container, propose-only for anything on the production one:

- **Import-then-restart** over the API: `POST …/import` with a real pair,
  restart, confirm the acceptor serves the imported certificate and status
  reports `"imported"`.
- **`0600` on every private key** after each write path: first-boot
  `api-key.pem`, imported `api-key.pem`, `ca-key.pem` after generate and after
  import, every archived key under `ca-archive/` and `api-archive/`, and the
  staged `*.pem.tmp` while it exists. `write_private`'s
  `OpenOptionsExt::mode(0o600)` compiles only on unix — zero coverage on the
  Windows dev box.
- **Archive bound**: regenerate past `fah_certs::MAX_ARCHIVES` (8) and confirm
  the ninth answers `409` `archive_full` with the live pair intact.
- **Generate / import wall time** on the device — P6.

**8. 24 h soak in full mode** on the production container — a deploy, with its
own owner approval. ~~It cannot start before the 0.3.3 soak ends 2026-09-16~~ —
**moot twice over as of 2026-09-16.** This task is PARKED (the owner decided not
to use the interception code), and the container is free regardless: the soak
that occupied it became 0.3.4, and it was stopped on day 5 with a memory finding
rather than run to term (`ad795b0`,
`docs/code-review/phase2.6/soak-0.3.4/README.md` §day 5). Numbers against
the budget rows; RAM ≤ 128 MB steady (budget in decimal MB, readings in MiB —
compare like with like). Watch items:

- a. Peak concurrent DoH sessions against the shared 64-permit API ceiling —
  the recorded number decides whether the named escape hatch (const bump /
  separate semaphore) is ever built.
- b. `non_tls` vs `hello_timeouts` over the window — the ratio says whether
  silent browser preconnects dominate the port, which decides if the
  `hello_timeout_ms` default (10 s of permit per silent socket) needs
  revisiting. **No longer blocked** (verified 2026-09-08, review file
  §Telemetry prerequisite): these and `upstream_cert_failures` are counted in
  `fah_http::ProxyCounters` and the per-listener block already ships —
  `GET /api/v1/telemetry` returns `listeners.{http,https}`, documented in
  API.md with all twelve fields, so there is no API.md edit owed. p3-04 L5 is
  **refuted**: every `blocked` increment is preceded by a `requests` increment
  on the same path, so `blocked ≤ requests` holds per listener by
  construction, and the terminate leg counts per request — the D8 intercepted
  arm reports `connections=3061 requests=6122`, exactly one SNI verdict plus
  one inner request per connection.
- c. `GET /api/v1/certificates` `leaf_cache.unwarmed_misses` reads 0 after the
  browsing workload and at the end of the soak — **with a CA installed**.
  Attribute before filing: no CA means every SNI hello counts, which is
  expected; a non-zero value with a CA is the prewarm-then-evict window or a
  DoT mint failure.
- d. **Mint rate.** With a CA installed, every LAN client can drive one mint
  per DoT handshake for any SNI it names — 853 is default-on, whereas p3-04
  confined SNI-driven minting to listed clients. Record `minted_total` growth
  per hour, peak `inflight`, `superseded`/`evictions`. A per-client mint rate
  limit is the named escape hatch, built only if the soak shows the need.
- e. **Idle-cut sessions (p3-04 L4).** An intercepted h2 session cut by the
  idle watchdog leaves its in-flight stream tasks and the upstream connection
  task alive until the origin answers or `hello_timeout` fires, outside
  `max_connections`. RSS must not trend with the number of idle-cut sessions.
- f. **Shutdown drain (audit F4).** Record the stop time with live sessions;
  an idle spliced session holds the 5 s drain. Inside `stop-time=10s`, but the
  number belongs in the record.

Plus P8 and P9 proper on the soak deploy, against the 0.3.3 container as the
comparator.

### Step 5 — documentation sweep (all proposed, landed on approval)

- **PERFORMANCE.md §Budgets** — the new rows (SNI+splice, interception
  handshake, leaf-cache hit rate, DoT/DoH added latency, cold `prewarm` per
  first-sight host, CA generate / pair import wall time), each with its
  measured column, **the N it was measured at**, and a pointer to the review
  file. The splice-throughput row is **relative to P1-control**; the
  intercepted-h2-relay row keeps ≥ 50 MiB/s and gains a **P3 ÷ P1-LAN**
  column so a miss is attributable; campaign 1's absolute "≥ 100 MiB/s" is
  withdrawn as unmeasurable on this topology
  (`p3-06-testing-plan.md` §The 100 MiB/s row is withdrawn) — an owner
  decision to record, not a silent drop.
- **Audit F1** — owner decision, then the matching edit: either port the
  IP-literal fix to the HTTPS path, or state HTTP-only in CONFIGURATION.md and
  in the test that pins the current behaviour.
- **Audit F3** — one line recording that the per-domain `JoinSet` bound is
  `http.max_connections + https.max_connections` and that `HANDOFF_QUEUE` is
  shared by both lanes.
- **ADR-0006 revisit** — P10's result is the answer to the trigger. Record it:
  an amendment to the allocation-domain ADR if N changes for TLS, or a
  recorded confirmation that N=2 still holds. Owner decides which.
- **SECURITY.md** — no new promises; verify wording matches what shipped
  (present-tense sweep of §Later phases).
- **`docs/deploy-rb5009.md`** — new §HTTPS (dst-nat 443, CA install, Private
  DNS, rollback) mirroring §5b, including the operator warning that steering
  all :443 closes no-SNI/ECH connections (measured `ENOENT`, not forwardable;
  DNS-layer backstop).
- **Dashboard re-review** (ROADMAP.md: Phase 3 triggers one). p3-03 left
  `https-sni` out of `dashboard/frontend/src/pages/live-feed/filters.ts`
  (`KINDS = ['dns', 'http']`), and `detail.tsx` gates the HTTP detail block on
  `kind === 'http'`, so an SNI row renders through the DNS-shaped branch with
  empty method/path; p3-04 adds a third kind (`https`). Add both kinds and a
  detail branch for each, or record the owner's decision to defer to a
  dashboard task — an unfiltered kind is a finding, not a note.
- **README** operating-modes wording drift check.
- **CONFIGURATION.md / API.md** — the telemetry per-listener block (soak watch
  item b) and anything p3-02…p3-05 left approved-but-pending.
- **ROADMAP.md** — Phase 3 deliverables (SNI filtering, per-client
  interception, DoT/DoH listeners) to delivered wording at phase close.
- **`docs/project-state.md`** — rewritten, not appended, at phase close.

## Performance contract

This task *sets* the contract rather than consuming one. Classes:

- **Hard gates:** RAM ≤ 128 MB steady on-device in full mode; security suite
  green; verdict parity and splice-byte-identity properties.
- **Targets (numbers TBD, proposed to the owner):** SNI added latency,
  interception handshake overhead, leaf-cache hit rate, DoT/DoH added latency,
  splice throughput **relative to P1-control**.
- **Diagnostic:** mint cost, per-transport soak distribution, startup delta
  with the cert store present, CPU per relayed byte, the P10 curve.

Rule: every figure published in PERFORMANCE.md carries measured status and the
N it was taken at; a target that could not be measured on-device ships as
`TBD — must be measured during verification` or is withheld, never invented.
The >10 % hot-path regression rule applies to the phase's whole diff against
`main` `857865d` on the existing DNS/HTTP benches.

## Tests

**Unit** — none new; unit coverage belongs to p3-01…p3-05. Two existing tests
gain a line (F5's boot key, A4's pinned N).

**Integration** — the security suite, six named scenarios, re-run at the tip;
plus the F2 domain-splice test and the F3 hand-off saturation property.

**E2E** — `full_mode_blocks_at_every_layer`, with N pinned in the fixture.

**Regression** — full workspace suite green at the phase tip; existing DNS/HTTP
benches vs `main` `857865d` within the 10 % rule or justified; Phase 2 HTTP e2e
(`http_e2e.rs`) unchanged.

**Security** — the suite is the deliverable; on-device: pinned-app check,
CA-install verification, WAN-exposure re-check (nothing new listens WAN-side —
read-only router query).

**Performance** — step 1 benches + the step 4 campaign + the soak.

## Verification / Gates

- **Mandatory:** fmt / clippy / test workspace; security suite green; E2E
  scenario green; bench A/B recorded; review file complete with §Measurements
  (corpus / workload / device / N on every row).
- **Mandatory but owner-executed:** the probe preconditions (config, env list,
  tip images), P10 run and its N decision, dst-nat 443 applied, CA installed on
  the test device, Private DNS working, 24 h full-mode soak numbers recorded.
  If the owner defers the soak, the task stays `AWAITING SOAK` with the flip
  condition named — the phase is not finished while it stands.
- **Recommended:** harness sanity checks re-run before trusting probe numbers.
- **Diagnostic:** per-transport traffic split over the soak window.

## Non-goals

- Phase 4 HTML rewriting; performance tuning beyond meeting budgets (file
  follow-ups); ECH workarounds (measured transport limitation — no-SNI/ECH has
  no recoverable destination, the DNS layer catches those domains); iOS
  walkthrough (Android/Windows only, per the task); a line-rate LAN origin
  (needs a host on the far side of the router — recorded, not built).

## Acceptance criteria (from the task file)

- Every new budget row bench-backed and met on dev hardware; soak numbers on
  device; RAM ≤ 128 MB steady in full mode.
- Security suite green; deploy guide reproducible; pinned-app spot check
  unaffected.
- Gates green.
