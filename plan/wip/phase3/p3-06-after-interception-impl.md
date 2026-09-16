# p3-06 — what changes after p3-07 / p3-08 / p3-09 landed

**Scope:** p3-06 stays the phase gate (`AWAITING SOAK`). This file lists what
p3-07…p3-09 changed in the p3-06 tooling and documents, and the additional
device checks required by the new interception contract. **No p3-10.**
Everything here is a declaration or tooling update to p3-06's runbook, smoke
plan, testing plan or probe scripts, or a new runbook row. Written 2026-09-11
against `phase3-06` at `b3237ef`. §3b (scripts, e2e suite, smoke driver) is
implemented, run green and committed (`223bf79`); B5–B10 applied 2026-09-11
(four files, owner yes per file). **N1, N2, N4, D1 and D2 ran 2026-09-11** —
N1/N2/N4/D1 on the tip image built from `4365932` and deployed to the probe,
D1 finding three dashboard CSS defects, all fixed. **N3 FAILED**, a design
finding against p3-08/p3-09 — **closed the same day as a technical experiment,
not promoted** ([p3-06-n3-alert-ab.md](../../../docs/code-review/phase3/p3-06-n3-alert-ab.md)):
the alert names the TLS stack, not the cause; per-alert counters,
`handshakes_completed` and a per-client `intercepted` account landed; HTTP/3
must be refused for intercepted clients. N5 and N6 remain planning; they live inside the
soak. §4 carries the per-item status.

**Decision — no campaign re-run.** p3-07 adds one `ArcSwap::load` per accepted
connection (`fah-http/src/https.rs` `interception_for`); p3-08 touches only the
accept-failure arm of `intercept()`; p3-09 is dashboard-only. Nothing the
2026-09-08 sessions measured (D-arms, SNI, P4-LAN, P5, P6, P7, P10) changed by
mechanism. Recorded as **declaration delta 4** in
[p3-06-testing-plan.md](p3-06-testing-plan.md) §Declaration deltas: *survival
argument per arm, owner-approved; the domains merge forced a re-run because the
execution model changed, this change does not.* The owed arms (P1, P2, P3,
Runbook 1–4 and 7, the soak) run on the new build and need the updates below.

## The contract these updates follow (p3-07…p3-09, as shipped)

| Fact | Source |
| --- | --- |
| `clients` and `exclude_domains` live in `/config/interception.json`, the **Interception Document**; read and replaced whole through `GET`/`PUT /api/v1/interception`; a `PUT` applies on the **next accepted connection**, no restart, no `restart_required` | API.md §Interception, CONFIGURATION.md §Interception Document |
| `GET /api/v1/config` omits `https.interception`; `POST /api/v1/config` carrying it answers 422; a `FAH__HTTPS__INTERCEPTION__*` variable is refused at load | same |
| `[https.interception]` in the TOML is **legacy migration input only** (§B10) | CONFIGURATION.md §Migration from 0.3.x |
| There is no compiled-in baseline; `BASELINE_EXCLUSIONS` is deleted; an empty `exclude_domains` excludes nothing | SECURITY.md, CONTEXT.md §Exclusion |
| A client answering our leaf with `bad_certificate`, `certificate_unknown` or `access_denied` is an `https` event with **status 525** (`ClientCertRejected`) and one tick of `listeners.https.client_cert_rejections`; `unknown_ca` and unclassified accept failures (any other alert, transport error, EOF, our handshake deadline) stay **status 0**, uncounted | API.md §Events, CONTEXT.md §Client Certificate Rejection |
| The dashboard's Live Feed has a "Certificate rejected by client" view grouped by client and host with one action — exclude the exact host, after confirmation; Settings has the Interception card. Neither writes except on click + confirm | `docs/dashboard/information-architecture.md` |

## 1. Tooling and declaration updates — apply before any owed arm runs

| # | Where | What changed | Update |
| --- | --- | --- | --- |
| B1 | `docs/code-review/phase3/p3-06-probe/lib.mjs` `Run.init` / `snapshotConfig` | the run directory snapshots `/api/v1/config` only; a client-posture change no longer shows in `config.json`, so the `degraded: probe config changed` label cannot see it | add `snapshotInterception()`: `GET /api/v1/interception` → `interception.json` beside `config.json`; taken when the run directory is opened and on every later run, with the same changed-since-opened comparison and `degraded` label; parsed document exposed as `run.interception` (`clients`, `exclude_domains`). **This snapshot is the source of truth for posture and `degraded` detection after p3-07.** `config.json` keeps `engine.mode` and `runtime.http_runtimes` |
| B2 | `p2-handshake.mjs:140-143` | the identity precondition reads `run.config?.https?.interception?.clients ?? []`, always `[]` now → `INVALID` unconditionally | read `run.interception.clients`; the interception precondition never reads `/api/v1/config` (`engine.mode` and N still come from it); `INVALID` text names `/api/v1/interception`; no reference to the removed TOML key remains |
| B3 | `p3-h2stall.mjs:66-68` | same read, same outcome | same update |
| B4 | `p2-handshake.mjs:7,59`, `p3-h2stall.mjs:5` | header comments and `--listed` help describe `[https.interception] clients` | describe the Interception Document and the live endpoint |
| B5 | [p3-06-smoke-plan.md](p3-06-smoke-plan.md) §1.1 lines 83–84, 118–131 | postures A/B by editing `[https.interception] clients` and rebooting. After the first boot that block is migration input and is ignored for policy, so posture B never applies | **normal postures:** remove the block from the smoke TOML fixture; posture A = `PUT {"clients":[],"exclude_domains":[]}`, posture B = `PUT {"clients":["127.0.0.1"],"exclude_domains":[]}`; **no restart**; keep `--out` per posture (`layer1-a`, `layer1-b`) because B1's snapshot is part of the run comparison. **Migration testing is separate** — the legacy block lives only in the dedicated B10 fixture |
| B6 | smoke plan §2 rows "both addresses unlisted (`p2`)", "this host not listed (`p3`)" | forced by `clients = []` in TOML | forced by `PUT {"clients":[],"exclude_domains":[]}`; expected text names the Interception Document / `/api/v1/interception` |
| B7 | smoke plan §Report item 2 | "the probe's config fixed — … `https.interception.clients`" | drop that key from the list; the probe boots with an empty document and R8 lists the client by `PUT` |
| B8 | [p3-06-testing-plan.md:545](p3-06-testing-plan.md#L545) §Environment | row `https.interception.clients` as a boot key set via `POST /config` | row becomes **Interception Document** · `PUT /api/v1/interception` · live, next connection · not a boot key, not a config key. Remove the statement that `clients` is a boot key |
| B9 | [p3-06-phase3-verification-review.md:730-737](../../../docs/code-review/phase3/p3-06-phase3-verification-review.md#L730-L737) §Runbook 4 item 1 | "`BASELINE_EXCLUSIONS` settled … the excluded arm exists as shipped" — the baseline is deleted | item 1 becomes: `PUT` the document with `exclude_domains: ["unicredit.ro"]` (clients unchanged), **then** run the excluded arm. State that this is **operator policy**, not a compiled-in baseline; `p3-06-measurement-audit.md` already says so |
| B10 | smoke plan §Layer 0 boot-path table | no rows for the migration boot paths | add three rows, from a **dedicated migration fixture** (a TOML carrying `[https.interception]`, a fresh config dir): **(a) first boot, no document** — the lists are migrated into `interception.json`, the TOML is re-saved without the keys, one `info!` `migrated [https.interception] into interception.json`; an invalid legacy entry **fails this boot** naming list, index and entry (`clients[3]: "10.0.0.300" is not an IP address or CIDR block`); an over-cap legacy list fails naming list, len and cap (`clients: 300 entries exceed the cap of 256 by 44`) — exactly `DocumentError::InvalidEntry` / `OverCap`; in every `engine.mode`, document not written. **(b) document already exists, TOML still carries the block** — the document remains authoritative, boot continues, one `warn!` naming both files, **and the keys are stripped from the TOML again** (CONFIGURATION.md §Migration, "keys re-added by hand"); legacy entries are not validated on this path, so an invalid legacy entry is **not** a boot failure here. **(c) document unreadable, malformed, over cap or carrying an invalid entry** — boot fails naming the file (unreadable, malformed: `InterceptionStoreError::Read` / `Parse`) or the list, index and entry (invalid entry, over cap: `Invalid(DocumentError)`, whose message carries no path); the file is never overwritten |

Smoke after B1–B4: re-run Layer 1 `p2` (Windows negative path) and `p3`, Layer 2
rows B6, and the three B10 rows. Nothing else re-smokes.

## 2. New device checks — rows added to the p3-06 runbook

Dev box cannot produce any of these. Read-only or owner-run per root CLAUDE.md;
the agent proposes the exact commands and stops.

| # | Runbook row | Check | Pass |
| --- | --- | --- | --- |
| N1 | **R2 addendum — migration on the probe** | the probe's stored TOML (campaign-1 era) carries `[https.interception]`, and no document exists. First boot of the tip image, proof: (1) `/file print` shows `interception.json` under the probe's config mount; (2) `GET /api/v1/interception` returns the migrated values; (3) `GET /api/v1/config` carries no `https.interception`; (4) the container log has the migration `info!` line. Second boot: no migration line, and the TOML on disk carries no `[https.interception]`. **Privilege-drop path:** the write runs after `drop_to_service_user` on the re-owned `/config` mount; ownership is not observable from RouterOS, so the proof is behavioural — the first boot succeeds past the drop, a later `PUT` rewrites the file (200, `GET` reflects it), and the second boot reads it. A failure here is a boot refusal (`InterceptionStoreError::Write`), a path new since p3-07 | all four first-boot reads; second boot clean; `PUT` after migration succeeds |
| N2 | **Runbook 4 rewritten — the ADR-0008 path, end to end** | client listed, CA installed on the device, `exclude_domains: []`. Open the pinned banking app: (1) its hosts appear in the Live Feed "Certificate rejected by client" view with **status 525**, count rising on retry; (2) the operator excludes the **exact observed host** through the view's confirmation; (3) the device's next connection to that host is admitted to the SNI/splice path, not intercepted — proof is that the certificate served to the device is **no longer issued by `FastAdHunter CA`** (no MITM — the upstream chain, whatever CDN or proxy it comes from), with the feed's `https-sni` row as supporting evidence; (4) `GET /api/v1/interception` contains the exact host; (5) no restart, no `restart_required`; (6) `listeners.https.client_cert_rejections` moved by the observed count. Record app, hosts, counts | 525 rows appear; the exclusion applies on the next connection; app functional; document contains the host |
| N3 | **Runbook 4 negative — `UnknownCA` is not a rejection** | same device, listed, **CA uninstalled from the device's trust store, probe store unchanged**: the app's connections fail; the feed shows those sessions as `https` **status 0**; the rejection view stays **empty**; `client_cert_rejections` flat; no 525. The rest of the Live Feed is not required to be empty — only the rejection view | no 525, no row, counter flat |
| N4 | **R8 — negative `PUT`, atomicity** | after the valid `PUT` (200): a `PUT` carrying one invalid entry answers **422** with the structured `details` object; then `GET /api/v1/interception` returns the previous document unchanged — **the primary proof: no mutation**; `/file print` showing no timestamp change is corroborating only (filesystem timestamp granularity); a new connection from the listed device is still intercepted under the previous document | 200, then 422 with `details`, then `GET` equals the previous document and a new connection reflects it |
| N5 | **Runbook 6 (soak) watch list** | **First snapshot, before any other pull:** the `GET /api/v1/interception` body verbatim and the R7 steer state — both `nat/print stats` filters on `comment~"p3-06 https steer"` plus `address-list/print where list=p3-06-probe-client`. Without it the daily comparison has no baseline, and a steer removed or re-scoped mid-soak silently changes which traffic the figures describe (owner decision 2026-09-11). Then hourly: `listeners.https.client_cert_rejections` in the `/telemetry` read. Daily: `interception.json` date from `/file print` **and** the `GET /api/v1/interception` body compared to the previous day's (content comparison — RouterOS prints no hash). Invariant: the document does not change during the soak except after an explicit owner `PUT`, recorded with its time | first snapshot present; then flat, or every change matched to a recorded `PUT` |
| N6 | **Runbook 7 addendum** | `interception.json` is operator configuration, not CA private-key material: it does not inherit the CA-key-only permission expectations, and it is not a leak needle. After a `PUT`: `ca/export` in both formats and `/config` still answer without the CA key's payload; the traversal list still passes; the existing key-permission checks are unchanged | Runbook 7 green with the document present |

## 3. Additional dev-box validation

| # | Item | How | Status |
| --- | --- | --- | --- |
| D1 | 390 px / both themes, rejection view and Interception card | Ran against **the p3-06 probe** (real dashboard + API on the RB5009), not a dev server, with Playwright at 390 px and 1280 px in both themes | **run 2026-09-11 — three defects found and fixed**: the line editor never grew past `cols=20` (161 px at every width, truncating an IPv6 CIDR); `Exclude` was a 34 px touch target against 44 px elsewhere; the focused textarea painted over the sticky save bar while scrolling. `components.css` +29/−0, 1043 frontend tests green. Recorded in the p3-09 review §Known limitations and [p3-06-testing-results-2.md](../../../docs/code-review/phase3/p3-06-testing-results-2.md) §Session 3 |
| D2 | migration first-boot path through the real binary | `crates/fastadhunter/tests/interception_migration.rs`: boot with a TOML carrying `[https.interception]` and no document; assert the file, the stripped TOML, `GET /interception` | **run 2026-09-11, §3b** — 5/5 green, 1.1 s; was a non-blocking follow-up (unit-covered in `interception_store.rs`, 11 migration cases) |

## 3b. Implemented 2026-09-11 — B1–B4, D2, the smoke driver (written and run)

Committed at `223bf79`. Run 2026-09-11 on the dev box after a review
pass fixed four defects: `cargo fmt --check`, `clippy -p fastadhunter
--all-features --tests -D warnings` and `node --check` on all four `.mjs` files
clean; the e2e suite 5/5 (1.1 s); the driver 12/12, exit 0, ~8 s wall against
the release build. Report:
`docs/code-review/phase3/p3-06-probe/smoke-20260911T0700Z/after-interception.json`
(untracked; its `work/` subtree is the store — CA and API keys, session
secret, engine logs — hidden by the existing `smoke-*/work/` ignore rule; the
report, `run.log` and the probe output directories beside it are the
evidence). Everything the driver persists — `run.log`,
`after-interception.json`, the probe scripts' stdout — passes through one
`redact()` that blanks the engine's first-boot `api_key=` /
`dashboard_password=` fields by name and every fixture API key by value, so a
FAIL row's engine-log tail cannot carry a secret.

| # | Defect found by the review | Fix |
| --- | --- | --- |
| 1 | driver `POST /certificates/ca/generate` sent no body; the handler requires `{"confirm": true}`, so the CA row answered 400 and seven rows never ran | body `{ confirm: true }` |
| 2 | driver `stop()` slept 300 ms instead of awaiting the child's exit — the next fixture rebinds the same five ports | awaits the `exit` event |
| 3 | driver B10b threw on a missing carried document instead of reporting | `existsSync` guard, FAIL row |
| 4 | `refused_boot` spawned `CARGO_BIN_EXE_fastadhunter` directly, bypassing `FAH_E2E_BINARY` | `common::binary_under_test` is `pub` and used |

| Item | File | What it holds |
| --- | --- | --- |
| D2 + B10 a/b/c | `crates/fastadhunter/tests/interception_migration.rs` (new) | five e2e tests through the real binary: legacy block migrated on the first boot (`GET /interception`, `GET /config` without the key, TOML stripped, `info!` line) and ignored on the second (`warn!`, stripped again, no migration line); legacy block beside an existing document — document wins, its bytes on disk stay identical (seeded with odd whitespace so a re-serialisation would show), legacy entries not validated; invalid legacy entry refuses the first boot naming `clients[1]: "10.0.0.300" …`, no document, TOML untouched; over-cap legacy list refuses naming `clients: 257 entries exceed the cap of 256 by 1`; unreadable document refuses naming the file, file byte-identical |
| B1 | `docs/code-review/phase3/p3-06-probe/lib.mjs` | `snapshotInterception()` — `GET /api/v1/interception` → `interception.json` beside `config.json`, same changed-since-opened comparison and `degraded` label; `run.interception = { clients, exclude_domains }`; `snapshotConfig` shares one `snapshot()` helper; a build without the endpoint is `INVALID` |
| B2 / B3 / B4 | `p2-handshake.mjs`, `p3-h2stall.mjs` | preconditions read `run.interception.clients`; `INVALID` text and `--listed` help name `/api/v1/interception`; header comments describe the document; no probe script references the removed key (grep clean) |
| §1 smoke rows | `docs/code-review/phase3/p3-06-probe/smoke/after-interception.mjs` (new) | unattended driver against the release binary, fresh fixture per row under `--out/work/` (the store, gitignored): B10 (a) migration, (b) existing document, (a) invalid entry and over-cap refusals, (c) unreadable document; B5 postures A/B by `PUT`, no restart; B6 `p3` negative on `clients=[]`; B1 `degraded` label on a posture change; `p2` Windows negative path (skipped elsewhere); an N4 rehearsal — invalid `PUT` → 422 with `details`, `GET` unchanged, **and the runtime policy unchanged**: one TLS connection from the listed client before and one after the rejected `PUT` must behave identically — the terminate leg refuses a self-signed origin the driver runs on `127.0.0.1:443` with the probe's own `api-cert.pem`/`api-key.pem` (`listeners.https.upstream_cert_failures` +1 each, socket closed before our handshake, the 526 path), with an unlisted-posture control spliced through to a completed handshake and no counter move. The driver runs its own UDP upstream answering `127.0.0.1` so any SNI resolves inside `egress.allow_destinations` without the LAN resolver. Port 443 is unprivileged on Windows; on Linux the driver needs `CAP_NET_BIND_SERVICE` and reports the bind failure as a row. Writes `after-interception.json` and a table; exit 0 only when every row passes; refuses to start if the API port already answers |

Run by the next agent, no intervention needed (ports default to the smoke
plan's 5300 / 8853 / 8080 / 8444 / 8443):

```sh
cargo test -p fastadhunter --all-features --test interception_migration
cargo build --release --locked -p fastadhunter
node docs/code-review/phase3/p3-06-probe/smoke/after-interception.mjs --out docs/code-review/phase3/p3-06-probe/smoke-<ts>
```

D2 is therefore no longer a follow-up: §3 row D2 is delivered by this file.
B5–B10 applied 2026-09-11, owner yes per file: smoke plan (B5, B6, B7, B10),
testing plan (B8 + the P2 precondition sentence), verification review (B9, the
§5 P2 sentence, §Runbook 1 item 6, N2/N3/N5/N6 in its Runbook 4/6/7), runbook
(N1 under R2, N4 under R8).

## 4. Order

1. B1–B4 (scripts) → smoke re-run named in §1 → owner yes → commit. **Written, run green (§3b) and committed (`223bf79`).**
2. B5–B10 document edits, owner yes per file. **Applied 2026-09-11.**
3. N1 on the first tip-image boot of the probe (R0/R2 window). **Run
   2026-09-11 — PASS**, against a *constructed* legacy fixture; R0 proved no
   campaign-1 probe config exists, so the block was placed before the first
   start. See the runbook's R2 addendum correction and
   [p3-06-testing-results-2.md](../../../docs/code-review/phase3/p3-06-testing-results-2.md)
   §Session 3. Resolved the same day:
   [p3-06-n3-alert-ab.md](../../../docs/code-review/phase3/p3-06-n3-alert-ab.md).
4. N2–N4 with the device, after R7/R8. **All run 2026-09-11** on the owner's
   OnePlus 15 (192.168.10.11 and `2a02:2f04:5400:cc00::/64`, R7 scoped to it
   alone): **N2 PASS**, **N4 PASS** (API half and device leg), **N3 FAIL** — a
   design finding filed against p3-08 and p3-09, not a defect in the run. See
   [p3-06-testing-results-2.md](../../../docs/code-review/phase3/p3-06-testing-results-2.md)
   §Session 3.
5. N5–N6 inside the soak.
6. D1 and D2 both **run 2026-09-11** — see §3. D1 found three dashboard CSS
   defects, all fixed; D2 is delivered by §3b's e2e suite.

## 5. Decision

No full Phase 3 campaign re-run is owed solely because p3-07…p3-09 landed.
The affected checks are declaration/tooling updates plus the targeted device
validations listed here. Existing p3-06 arms whose execution mechanism and
measured invariant did not change remain covered by the approved declaration
delta.

## 6. Not owed

- A new p3-10 task or test plan.
- A bench for p3-07's hot-path delta — none (p3-07 plan §11, review
  §Measurements: `https_handshake` cannot resolve it; h2 and prewarm arms flat).
- A full P2 / P3 re-declaration — their quantities are unchanged; only the
  precondition read (B2/B3) moves.
- A dashboard listener-counter surface for `client_cert_rejections` (p3-08 F8).

## 7. Prompt for the next agent — re-run what §3b wrote

First run 2026-09-11, green (§3b); a re-run reproduces it. Copy verbatim into
a fresh session.

> Read `plan/CLAUDE.md`, then `plan/wip/phase3/p3-06-after-interception-impl.md`
> in full. Root `CLAUDE.md` is loaded; its working agreement applies: no `.md`
> edit, no commit, no push, no RB5009 command without an explicit yes. Dev box
> only. Branch `phase3-06`; the tip must contain the §3b files
> (`crates/fastadhunter/tests/interception_migration.rs`,
> `docs/code-review/phase3/p3-06-probe/smoke/after-interception.mjs`, the B1–B4
> edits to `lib.mjs`, `p2-handshake.mjs`, `p3-h2stall.mjs`), in the working
> tree or committed. Record the current `HEAD` and `git status --short` before
> running anything and put both at the top of the report.
>
> **Task.** Execute §3b's three commands, in this order, and report per row.
> Do not change any test, script or plan to make a row pass: a failing row is
> a finding, recorded and reported, not worked around (root CLAUDE.md
> §Engineering principles, memory "no workaround code").
>
> **1 — the e2e migration tests (D2 + B10 a/b/c), Rust.**
>
> ```sh
> cargo test -p fastadhunter --all-features --test interception_migration
> ```
>
> Five tests, all must pass — the summary line reads `6 passed`; the sixth is
> the harness's own `common::free_udp_port_is_bindable_on_both_protocols`,
> compiled into every test binary that includes `mod common`. The five:
> migrated once then ignored; existing document
> byte-identical; invalid legacy entry refuses naming `clients[1]`; over-cap
> refuses naming `clients: 257 … cap of 256 by 1`; unreadable document refuses
> naming the file. Each test boots the real binary; a port-conflict retry is
> the harness's own, not a failure. Report how many real-binary boots each
> test performed if the harness exposes that information; do not infer it. On
> a failure quote the assertion line and the engine log tail it prints.
>
> **2 — the release binary the smoke driver boots.**
>
> ```sh
> cargo build --release --locked -p fastadhunter
> ```
>
> **3 — the smoke driver (§1 rows B10, B5, B6, B1, p2 negative, N4 rehearsal).**
>
> ```sh
> node docs/code-review/phase3/p3-06-probe/smoke/after-interception.mjs --out docs/code-review/phase3/p3-06-probe/smoke-<YYYYMMDDTHHMMZ>
> ```
>
> Preconditions the driver checks itself and refuses on: nothing answering on
> `127.0.0.1:8443` (pass other `--*-port` values if the smoke plan's
> 5300 / 8853 / 8080 / 8444 / 8443 collide), the release binary present. It
> binds `127.0.0.1:443` for the N4 origin — unprivileged on Windows. It passes
> `--skip-host-checks` to the probe scripts, so the idle check never runs and an
> open browser does not matter. It runs unattended, seconds (~8 s observed
> 2026-09-11), prints one table and writes `<out>/after-interception.json`,
> `<out>/run.log` and the probe output directories; the throwaway stores go
> under `<out>/work/`, which is gitignored. Exit 0 only when every row passes.
> Twelve rows expected: B10a migration, B10b existing
> document, B10a invalid legacy entry, B10a over-cap, B10c unreadable
> document, B5 posture A, B6 p3 negative, B5 posture B, B1 posture-change
> label, p2 Windows negative path, N4 rehearsal, N4 control.
>
> **Report**, in chat, nothing else: the `cargo test` summary line; the
> driver's table verbatim; for every FAIL the `reason` field whole and the
> matching `run.log` lines; the `<out>` directory path. **Do not diagnose,
> modify, or propose fixes; only report the observed results** — this session
> is an executor, not a reviewer. Then stop. Do not commit the results
> directory, do not touch the B5–B10 files, do not start N1–N6.

## 8. Prompt for the next agent — the device path (§2, owner-run)

**Superseded for N1–N4 and D1 — those ran 2026-09-11 (§4).** What remains of
this prompt is step 2 (the owed p3-06 load arms) and step 4 (N5/N6 inside the
soak); step 1, step 3 and step 5 are done. The standing device state the next
session inherits — probe up, R7 steer **removed**, document and address list
retained and inert — is in
[p3-06-testing-results-2.md](../../../docs/code-review/phase3/p3-06-testing-results-2.md)
§Standing state, together with the two `add` pairs needed to put the steer back.

§7 covers the dev box. The rows below need the tip image on the probe and the
owner's device; the agent proposes and verifies, the owner runs. Gate: B5–B10
landed. ~~The 0.3.3 soak verdict gates the final phase decision, not N1–N4.~~
**Superseded 2026-09-16.** That soak became 0.3.4 and was stopped on day 5 with
its verdict already in hand — the residual floor doubled, 19.0 → 38.6 MiB, while
`accounted_bytes` held at 28.00 MiB (`ad795b0`,
`docs/code-review/phase2.6/soak-0.3.4/README.md` §day 5). **No production soak is
running on the RB5009**, so the load arms no longer wait on one: the p3-06 query
flood of 2026-09-08 invalidated the soak then running, and that constraint has
lapsed with it. P1–P3 and the measurement arms of Runbook 1–4 are unblocked on
that ground alone — this task is PARKED, so nothing here is picked up without a
new decision. Copy verbatim into a fresh session.

> Read `plan/CLAUDE.md`, then `plan/wip/phase3/p3-06-after-interception-impl.md`
> in full, then `docs/routeros-traps.md`. Root `CLAUDE.md` is loaded and its
> working agreement applies without exception: **you change nothing on the
> RB5009** — no `/container`, `/ip`, `/system`, `/disk` or config command, not
> even a reversible one. You propose the exact command, say what it does and
> when it takes effect, and stop; the owner runs it. Read-only queries
> (`/container/print`, `/log print`, `/file print`, GETs on the FAH API) need
> no asking. No `.md` edit, commit or push without an explicit yes per file.
> Ask once at the start whether the owner authorizes updates to
> `docs/code-review/phase3/p3-06-testing-results-2.md` (confirm from its header
> that it is the campaign-2 results file). If not explicitly authorized, report
> results in chat only and do not edit it.
>
> **State to confirm before anything runs, and put at the top of the report:**
> `HEAD` and `git status --short` on `phase3-06`; B5–B10 applied — the runbook
> `p3-06-phase3-verification-runbook.md` carries the N1–N6 rows, the R2
> addendum, the rewritten Runbook 4 and R8; the tip image built from this
> `HEAD` and loaded on the probe container by the owner; the probe's stored
> TOML carries `[https.interception]` and no `interception.json` exists — and
> if it does not, that this is expected: no campaign-1 probe config has ever
> existed on this device, so the legacy block is **placed** before the first
> start rather than found. N1 was run this way on 2026-09-11 and passed; if it
> has already run, its precondition is spent and re-running it needs a wiped
> config directory. No production soak is running on the RB5009 — 0.3.4 was
> stopped on day 5, 2026-09-16 (`ad795b0`) — so the load arms in step 2 are not
> blocked on one; confirm that is still true before running them, because the
> 2026-09-08 flood invalidated the soak then running. If any of these is
> missing, say which and stop.
>
> **Task.** Execute §2 in §4's order and report per row against the row's
> Pass column. Nothing is skipped, reordered or worked around; a failing row is
> a finding, recorded with the evidence read, not fixed in this session.
>
> 1. **N1** at the first boot of the tip image (the R0/R2 window): the four
>    first-boot reads, the second-boot reads, then one `PUT` and its `GET`.
> 2. The owed p3-06 arms on the new build — P1, P2, P3, Runbook 1–4 and 7 —
>    with the B1–B4 scripts and the B5–B9 declaration changes. Use the existing
>    p3-06 row definitions and pass criteria from `p3-06-testing-plan.md` and
>    the runbook; do not invent replacement tests. Declaration delta 4 says why
>    nothing else re-runs.
> 3. **N2, N3, N4** after R7/R8, with the owner's device: record app, hosts,
>    counts, the certificate issuer the device sees before and after the
>    exclusion, and the `client_cert_rejections` delta.
> 4. **N5, N6** inside the 24 h full-mode soak.
> 5. **D1** may be run independently on the dev box; it is not a prerequisite
>    for N1–N6 or the Phase 3 gate.
>
> Every result carries the corpus, workload and device it applies to; claims
> stay scoped to what was observed. Report: per row PASS/FAIL with the evidence
> (GET body, log line, counter delta, `/file print` line), the commands the
> owner ran, and the results-file sections written or, without authorization,
> the same content in chat. Do not commit.
