# p3-06 — what changes after p3-07 / p3-08 / p3-09 landed

**Scope:** p3-06 stays the phase gate (`AWAITING SOAK`). This file lists what
p3-07…p3-09 broke in p3-06's own tooling and documents, and the device checks
they add. **No p3-10.** Everything here is a fix to p3-06's runbook, smoke plan,
testing plan or probe scripts, or a new runbook row. Written 2026-09-11 against
`phase3-06` at `d84383a`.

**Decision — no campaign re-run.** p3-07 adds one `ArcSwap::load` per accepted
connection (`fah-http/src/https.rs` `interception_for`); p3-08 touches only the
accept-failure arm of `intercept()`; p3-09 is dashboard-only. Nothing the
2026-09-08 sessions measured (D-arms, SNI, P4-LAN, P5, P6, P7, P10) changed by
mechanism. Recorded as **declaration delta 4** in
[p3-06-testing-plan.md](p3-06-testing-plan.md) §Declaration deltas: *survival
argument per arm, owner-approved; the domains merge forced a re-run because the
execution model changed, this change does not.* The owed arms (P1, P2, P3,
Runbook 1–4 and 7, the soak) run on the new build and need the fixes below.

## 1. Broken by p3-07 — fix before any owed arm runs

`GET /api/v1/config` no longer carries `https.interception`; the lists live in
`/config/interception.json`, read and replaced through
`GET`/`PUT /api/v1/interception`, applied on the next accepted connection. A
`[https.interception]` block in the TOML is migrated on the first boot that
finds no document and **ignored** (one `warn!`) on every boot after.

| # | Where | What is wrong | Fix |
| --- | --- | --- | --- |
| B1 | `docs/code-review/phase3/p3-06-probe/lib.mjs` `Run.init` / `snapshotConfig` | snapshots `/api/v1/config` only; a client-posture change no longer shows in `config.json`, so the `degraded: probe config changed` label misses it | add `snapshotInterception()` — `GET /api/v1/interception` → `interception.json` beside `config.json`, same changed-since-opened rule; expose `run.interception.clients` |
| B2 | `p2-handshake.mjs:140-143` | `run.config?.https?.interception?.clients ?? []` is always `[]` → identity precondition `INVALID` unconditionally | read `run.interception.clients`; message names `/api/v1/interception` |
| B3 | `p3-h2stall.mjs:66-68` | same read, same outcome: `INVALID: this host … is not in https.interception.clients` on every run | same fix |
| B4 | `p2-handshake.mjs:7,59`, `p3-h2stall.mjs:5` header comments and `--listed` help | name the dead TOML key | reword to the document |
| B5 | [p3-06-smoke-plan.md](p3-06-smoke-plan.md) §1.1 lines 83–84, 118–131 | postures A/B by editing `[https.interception] clients` and rebooting. After the first boot the TOML key is migrated and then ignored, so posture B never applies | drop the block from the smoke TOML; posture = one `PUT /api/v1/interception`, **no restart**; `--out` still per posture (`layer1-a`, `layer1-b`) because B1 labels the run |
| B6 | smoke plan §2 rows "both addresses unlisted (`p2`)", "this host not listed (`p3`)" | force by `clients = []` in TOML | force by `PUT {"clients":[]}`; expected text per B2/B3 |
| B7 | smoke plan §Report item 2 | "the probe's config fixed — … `https.interception.clients`" | remove that key; the probe boots with an empty document and R8 lists the client |
| B8 | [p3-06-testing-plan.md:545](p3-06-testing-plan.md#L545) §Environment row `https.interception.clients` | boot key set via `POST /config` — now 422 | row becomes `interception document` · `PUT /api/v1/interception` · live, not a boot key |
| B9 | [p3-06-phase3-verification-review.md:730-737](../../../docs/code-review/phase3/p3-06-phase3-verification-review.md#L730-L737) §Runbook 4 item 1 | "`BASELINE_EXCLUSIONS` settled … the excluded arm exists as shipped" — false since p3-07 | item 1 becomes: `PUT` `exclude_domains: ["unicredit.ro"]` before the excluded arm (already said in `p3-06-measurement-audit.md`); baseline gone |
| B10 | smoke plan §Layer 0 boot-path table | no row for the new boot path | add: TOML carrying `[https.interception]` + no document → boots, writes the document, re-saves TOML without the keys, `info!` line; invalid entry in that block → **boot fails naming the list, in every `engine.mode`** |

Smoke after B1–B4: re-run only Layer 1 `p2` (Windows negative path) and `p3`,
plus Layer 2 rows B6 and the two B10 rows. Nothing else re-smokes.

## 2. New device checks — rows added to the p3-06 runbook

Dev box cannot produce any of these; all read-only or owner-run per root
CLAUDE.md.

| # | Runbook row | Check | Pass |
| --- | --- | --- | --- |
| N1 | **R2 addendum — migration on the probe** | the probe's stored TOML (campaign-1 era) carries `[https.interception]`. First boot of the tip image: `/file print` shows `interception.json` under the probe's config mount; `GET /api/v1/interception` returns the migrated lists; `GET /config` has no `https.interception`; container log has `migrated [https.interception] into interception.json`. The file is written **after** `drop_to_service_user` on a reowned `/config` — a failure here is a boot refusal (`InterceptionStoreError::Write`), new since p3-07 | all four reads; second boot logs no migration line |
| N2 | **R4 (pinned-app) rewritten as the ADR-0008 path** | device listed, CA installed, `exclude_domains: []`; open the banking app → Live Feed → "Certificate rejected by client" view shows the app's hosts with `status 525`, count rising on retry; **Exclude** the exact host → next connection `https-sni pass`, app works; `GET /api/v1/interception` shows the host; `/telemetry` `listeners.https.client_cert_rejections` moved | 525 rows appear, exclusion applies without restart, app functional. Record app, hosts, count, alert (from the `debug` log if enabled) |
| N3 | **R4 negative — `UnknownCA` is not a 525** | same device, CA **removed**, listed: connections close, feed shows `https` `status 0`, rejection view empty, counter flat | no 525, no row |
| N4 | **R8** | unchanged since p3-07 (`PUT`, no restart) — run as written; add: `PUT` with a bad entry answers 422 with `details`, nothing changes | 200 then 422 |
| N5 | **R6 (soak) watch list** | add `listeners.https.client_cert_rejections` to the hourly `/telemetry` read and `interception.json` mtime to the daily `/file print` — the document must not change during the soak unless the owner `PUT`s | flat, or explained by a recorded `PUT` |
| N6 | **Runbook 7 addendum** | `interception.json` is not key material; confirm it is **not** `0600`-required and that `ca/export` and `/config` still answer without the CA key needle after a `PUT` | as before |

## 3. Dev-box items left open by p3-09

| # | Item | How |
| --- | --- | --- |
| D1 | 390 px / both themes, rejection view and Interception card | dev binary in `dns+http+https` with a CA, dashboard dev server, Playwright at 390 px in both themes; record in the p3-09 review file §Known limitations |
| D2 | e2e for the migration `NotFound` branch through the real binary | `crates/fastadhunter/tests/e2e_https.rs`: boot with a TOML carrying `[https.interception]` and no document; assert the file, the stripped TOML, `GET /interception`. Unit-covered only today (p3-07 review) |

## 4. Order

1. B1–B4 (scripts) → smoke re-run named above → owner yes → commit.
2. B5–B10 doc edits (owner yes per file).
3. N1 on the first tip-image boot of the probe (R0/R2 window).
4. N2–N4 with the device, after R7/R8.
5. N5–N6 inside the soak.
6. D1–D2 any time on the dev box.

## 5. Not owed

- A bench for p3-07's hot-path delta — none (p3-07 plan §11, review
  §Measurements: `https_handshake` cannot resolve it; h2 and prewarm arms flat).
- P2 / P3 re-declaration — their quantities are unchanged; only their
  precondition read (B2/B3) moves.
- A dashboard listener-counter surface for `client_cert_rejections` (p3-08 F8).
