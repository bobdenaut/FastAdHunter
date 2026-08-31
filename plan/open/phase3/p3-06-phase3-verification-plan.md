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
| SNI verdict + splice added latency | `fah-http` bench (extend `benches/proxy.rs` or a new `sni.rs` per p3-03's actual module) | budget row |
| Interception handshake overhead (terminate + re-originate vs splice) | new `fah-http` bench | budget row |
| Minted-leaf cache hit rate under a browsing-like host distribution | `fah-certs` bench + a workload replay (host list from a real browsing session, corpus recorded) | budget row (hit-rate target) |
| Leaf mint cost | p3-01's `certs_mint` bench, re-run | diagnostic beside the cache row |
| DoT/DoH added latency vs UDP (in-engine) | p3-05's harness measurement, promoted to a repeatable bench | budget row |
| RAM with all engines loaded (`dns+http+https`, rules compiled, caches warm) | dev-box RSS reading + on-device soak (step 3) | re-affirmed RAM row |

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
   For every intercepted client, **verify the static-lease precondition**
   (p3-04's GAR §5.14 owner decision): confirm on the router (read-only) that
   the listed IP is a static lease/address before calling §5.14 closed.
5. **Measurements on-device (GAR §5.13):** TLS handshake cost, splice
   throughput, interception CPU+RSS under browsing, DoT/DoH latency vs UDP —
   via the probe-container procedure (`docs/routeros-traps.md`; the
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

### Step 5 — documentation sweep (all proposed, landed on approval)

- PERFORMANCE.md §Budgets: the new rows (SNI+splice, interception handshake,
  leaf-cache hit rate, DoT/DoH added latency), each with its measured column
  and a pointer to the review file.
- SECURITY.md: no new promises — verify wording matches what shipped
  (present-tense sweep of §Later phases).
- `docs/deploy-rb5009.md`: new §HTTPS (dst-nat 443, CA install, Private DNS,
  rollback) mirroring the existing §5b structure — including the operator
  warning that steering all :443 closes no-SNI/ECH connections (measured
  `ENOENT`, not forwardable; DNS-layer backstop), so the operator knows what
  full mode does to that slice of traffic before enabling it.
- README operating-modes wording drift check (the task's doc sweep).
- CONFIGURATION.md/API.md: only if p3-02…p3-05 left an approved edit pending.

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
