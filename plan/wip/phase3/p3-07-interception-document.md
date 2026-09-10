# P3-07 — Interception Document

**Phase:** 3 · **Depends on:** p3-04 (ADR-0008 step 1) · **Model:** Fable

## Goal

`clients` and `exclude_domains` leave the binary and the TOML. They live in
`/config/interception.json`, are read and replaced through
`GET`/`PUT /api/v1/interception`, and apply on the next connection — no
restart, no `restart_required`. `BASELINE_EXCLUSIONS` is deleted.

## Context

[ADR-0008](../../../docs/decisions/0008-live-interception-and-client-certificate-rejection.md)
step 1 of §Phasing. The ADR settles the what; this task and its approved plan
settle the how. Read §Two lifetimes, §The document and its API, §Atomic swap,
§`FAH__`, §Migration, §Detect instead of predict, §A long exclusion list,
§Consequences and §Acceptance before planning. The code lands on `phase3-06`;
nothing deploys until the 0.3.3 soak verdict (docs/project-state.md §Next).

## Scope

- **The document.** `interception.json` on the `/config` volume, two lists,
  unknown keys rejected, entries validated with the validators the TOML path
  uses today (`AllowedNet`, `sni::normalize`), duplicates after normalization
  rejected, caps `exclude_domains ≤ 512` and `clients ≤ 256`. Read at boot;
  written by migration (once, when absent) and by `PUT`; never rewritten at
  boot. Unreadable, unparsable or invalid ⇒ boot fails naming the file.
- **The endpoint.** `GET`/`PUT /api/v1/interception` in `fah-api`:
  whole-document replace, same auth as `POST /api/v1/config`, an accepted
  change logged at `info` with both counts, response shape without
  `restart_required`.
- **The swap.** Machinery built whenever the HTTPS listener runs and the
  certificate store opened, whatever the client list; live replacement of the
  value `interception_for` hands out by reference — one read per accepted
  connection, no lock, no allocation, handle chosen in the plan. A mode without
  the listener stores the document; a boot whose store did not open rejects a
  `PUT` that lists a client.
- **The apply path.** `fah-api` cannot build `fah-http` state (siblings): the
  binary owns validate → build → persist → publish behind a channel or
  callback, or the runtime types move to a layer both can see. The plan
  chooses; `fah-api` importing `fah-http` is not a choice.
- **Migration, release N.** `InterceptionConfig` fields become
  `Option<Vec<String>>`, serialised only when present. Boot: document absent ⇒
  write it from the TOML values (empty on a fresh install); document present
  and TOML carrying a key ⇒ warn naming both files, ignore the TOML value.
  Either way, clear both fields and re-save the TOML. `POST /api/v1/config`
  naming either key is rejected with an error naming `/api/v1/interception` —
  a check on the patch, not the schema.
- **Deletions.** `BASELINE_EXCLUSIONS`; one of `ExclusionSet::empty()` /
  `new(&[])`; the three baseline tests (deleted, not adapted), with the
  10 000-miss lookup-cost assertion re-pinned on a document-built set;
  `a_baseline_bank_is_never_intercepted_even_for_a_listed_client` rewritten to
  drive a document entry.
- **`FAH__`.** No arm added. A test pins that
  `FAH__HTTPS__INTERCEPTION__CLIENTS` fails boot with `UnknownEnvKey`.
- **Docs, same change.** CONFIGURATION.md (the TOML no longer owns the keys;
  `interception.json` is their source of truth; the sample rows go and
  `defaults_match_configuration_md_sample` follows), API.md (endpoint, the
  `POST /api/v1/config` rejection), SECURITY.md (opt-in description,
  "Exclusions always splice"), CONTEXT.md (§Exclusion; new term
  **Interception Document**), README's "ADRs 0001–0007" line, the p3-06
  runbook (`R2` drops the `https` part of its body, `R8` becomes a `PUT` with
  no restart or rollback rows) and the Runbook 4 precondition in
  `p3-06-measurement-audit.md`.

## Acceptance criteria

ADR-0008 §Acceptance, every line except the three that belong to detection
(`status 0` fallback, `UnknownCA`, no new `Event` kind). In particular:

- Two connections either side of a `PUT`, against a live origin, take
  different legs; boot with `clients: []`, `PUT` one client, its next
  connection is intercepted — no restart.
- After any boot, `fastadhunter.toml` carries neither key; a second boot
  leaves `interception.json` byte-identical.
- A rejected `PUT` (cap, duplicate, unknown key, invalid entry) leaves file
  and runtime unchanged and names the list and the entry or overage; a
  completed `PUT` leaves them equal.
- `layering.rs` green with no `fah-api ↔ fah-http` edge in either direction.
- Docs above updated in the same change. Gates green.

## Out of scope

Detection and status 525 (p3-08); the dashboard (p3-09); release N+1's
deletion of the two `Option` fields — a follow-up task in the phase that ships
the release after N; the missing-CA diagnosis; widening; auto-exclusion.

## Suggested prompt

> Read ADR-0008 in full, plan/wip/phase3/p3-07-interception-document.md,
> `fah-api/src/config_store.rs`, `interception()` in `fastadhunter/src/main.rs`,
> `interception_for` in `fah-http/src/https.rs` and
> `fah-http/src/exclusions.rs`. Write the plan first — the swap handle, the
> apply path and the migration order are its decisions — and wait for
> approval before implementing.
