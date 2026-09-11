# p3-06 — what changes after p3-07 / p3-08 / p3-09 landed

**Scope:** p3-06 stays the phase gate (`AWAITING SOAK`). This file lists what
p3-07…p3-09 changed in the p3-06 tooling and documents, and the additional
device checks required by the new interception contract. **No p3-10.**
Everything here is a declaration or tooling update to p3-06's runbook, smoke
plan, testing plan or probe scripts, or a new runbook row. Written 2026-09-11
against `phase3-06` at `d165724`. Planning only — nothing here is implemented
or run yet.

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
| B10 | smoke plan §Layer 0 boot-path table | no rows for the migration boot paths | add three rows, from a **dedicated migration fixture** (a TOML carrying `[https.interception]`, a fresh config dir): **(a) first boot, no document** — the lists are migrated into `interception.json`, the TOML is re-saved without the keys, one `info!` `migrated [https.interception] into interception.json`; an invalid legacy entry **fails this boot** naming list, index and entry (`clients[3]: "10.0.0.300" is not an IP address or CIDR block`); an over-cap legacy list fails naming list, len and cap (`clients: 300 entries exceed the cap of 256 by 44`) — exactly `DocumentError::InvalidEntry` / `OverCap`; in every `engine.mode`, document not written. **(b) document already exists, TOML still carries the block** — the document remains authoritative, boot continues, one `warn!` naming both files, **and the keys are stripped from the TOML again** (CONFIGURATION.md §Migration, "keys re-added by hand"); legacy entries are not validated on this path, so an invalid legacy entry is **not** a boot failure here. **(c) document unreadable, malformed, over cap or carrying an invalid entry** — boot fails naming the file; the file is never overwritten |

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
| N5 | **Runbook 6 (soak) watch list** | hourly: `listeners.https.client_cert_rejections` in the `/telemetry` read. Daily: `interception.json` date from `/file print` **and** the `GET /api/v1/interception` body compared to the previous day's (content comparison — RouterOS prints no hash). Invariant: the document does not change during the soak except after an explicit owner `PUT`, recorded with its time | flat, or every change matched to a recorded `PUT` |
| N6 | **Runbook 7 addendum** | `interception.json` is operator configuration, not CA private-key material: it does not inherit the CA-key-only permission expectations, and it is not a leak needle. After a `PUT`: `ca/export` in both formats and `/config` still answer without the CA key's payload; the traversal list still passes; the existing key-permission checks are unchanged | Runbook 7 green with the document present |

## 3. Additional dev-box validation

| # | Item | How | Status |
| --- | --- | --- | --- |
| D1 | 390 px / both themes, rejection view and Interception card | dev binary in `dns+http+https` with a CA, dashboard dev server, Playwright at 390 px in both themes; record in the p3-09 review file §Known limitations | open (p3-09) |
| D2 | migration first-boot path through the real binary | `crates/fastadhunter/tests/e2e_https.rs`: boot with a TOML carrying `[https.interception]` and no document; assert the file, the stripped TOML, `GET /interception` | **NON-BLOCKING FOLLOW-UP** — unit-covered in `interception_store.rs` (11 migration cases); the binary calls the same function |

## 4. Order

1. B1–B4 (scripts) → smoke re-run named in §1 → owner yes → commit.
2. B5–B10 document edits, owner yes per file.
3. N1 on the first tip-image boot of the probe (R0/R2 window).
4. N2–N4 with the device, after R7/R8.
5. N5–N6 inside the soak.
6. D1 any time on the dev box; D2 when convenient.

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
