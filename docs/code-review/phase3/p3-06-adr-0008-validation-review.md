# ADR-0008 validation — live policy and client certificate rejection

Validation of [0008-live-policy-and-client-certificate-rejection.md](../../decisions/0008-live-policy-and-client-certificate-rejection.md)
against the tree at `f57f6a4` (branch `phase3-06`), 2026-09-10.
Scope: are the document's factual claims true, and does its internal logic
hold. The architecture itself is approved and was not re-litigated.

## Summary

The load-bearing claim survives measurement. A client's certificate-rejection
alert does reach `acceptor.accept()` as an `io::Error` of kind `InvalidData`
carrying `rustls::Error::AlertReceived(..)`, on TLS 1.3 and TLS 1.2 alike, so
the detection design stands and the ADR does not reopen.

Fifteen of the sixteen cited claims are true as written; one (R2) is wrong in
detail and misses a second runbook step. The substantive corrections are an
incomplete alert set, an understated swap cost, and a PSL paragraph that does
not know the project already decided this question once. None of them changes a
decision; all of them change wording or task scope.

The premise that the ADR is uncommitted is stale: it was committed as `f57f6a4`
and pushed to `origin` and `backup` earlier the same day. Corrections are a new
commit, not a free edit.

## Findings

| # | Severity | Finding |
| - | -------- | ------- |
| 1 | high | The classified alert set is incomplete: in the tested rustls 0.23.42 client stack, `ApplicationVerificationFailure` produced `AccessDenied` (0x31), which the ADR does not name |
| 2 | high | `certificate_error` must not be reused for detection — it matches `InvalidCertificate(_)`, never `AlertReceived(..)` |
| 3 | medium | R2 sets one policy key, not both; R8 sets one too and is not in the consequence list |
| 4 | medium | The atomic-swap paragraph understates the change: with `clients = []` there is no `Interception` to swap into |
| 5 | medium | PSL: a prior in-tree decision already declined one, and the existing `registrable()` is the exact hazard the ADR names |
| 6 | low | "seven other paths" emit `status 0` — it is six |
| 7 | low | Deleting `the_baseline_exclusions_ship_without_any_configuration` also deletes a lookup-cost assertion |
| 8 | low | "rules, lists and history live outside the TOML" is imprecise, and the imprecision weakens the sentence it supports |
| 9 | low | `/config` names the volume and the endpoint in adjacent sections |
| 10 | low | Line-number drift in four citations |

### 1 — the alert set is incomplete (§What counts as a rejection)

The ADR names `BadCertificate` (0x2a) and `CertificateUnknown` (0x2e) as the
pinning alerts, with `UnknownCA` (0x30) routed to the missing-CA diagnosis.
What was measured (table below): in the tested client stack — rustls 0.23.42
driven by a verifier returning a chosen verdict — `ApplicationVerificationFailure`
produced **`AccessDenied` (0x31)** on TLS 1.3 and TLS 1.2, and `NotValidForName`
produced `BadCertificate` (0x2a).

What that establishes is rustls's own verdict-to-alert mapping, which is the
observed evidence for classifying `AccessDenied`: a rejection that reaches the
accept side as 0x31 is real and the ADR's set drops it on `status 0`. What it
does not establish is anything about arbitrary real-world pinning clients —
which alert a given client sends is a property of its TLS stack, and no test
here touched one. `ApplicationVerificationFailure` is the verdict a
pinning-style verifier returns on rustls, not evidence that pinning implies
0x31 elsewhere. By the same token 0x2a is not exclusively a pinning signal.

The ADR already says the mapping is established by test, not by the document.
The fix is to stop naming the set as if it were closed: say which stack was
measured, add `AccessDenied` on that evidence, keep the default-to-`0` fallback
for everything unobserved.

### 2 — `certificate_error` is precedent, not machinery

`tls.rs:85` matches `err.kind() == InvalidData` **and**
`rustls::Error::InvalidCertificate(_)`. Every rejection measured downcasts to
`AlertReceived(..)`, a different variant, so reusing the helper would classify
nothing. The `InvalidData` half of the filter does hold on accept — that is the
part the precedent actually proves.

### 3 — the runbook consequence is wrong in detail (§Migration)

R2 sets three keys through `POST /api/v1/config`: `engine.mode`,
`egress.allow_destinations` and `https.interception.clients` (as `[]`). It does
not set `exclude_domains`, so "sets both keys" is wrong, and "that step moves to
`PUT /api/v1/policy`" is wrong for the two keys that stay on `/config`.

**R8 is the missed one.** It sets `https.interception.clients` to a real client,
and its table rows read *Takes effect: next container start* and *Restart
required: yes* — both false under this ADR, and R8 is the step whose whole point
is that listing a client used to need a restart. Its rollback row
(`re-POST with "clients": []` and restart) goes the same way.

### 4 — the swap is not a two-list swap (§Atomic swap)

| Fact | Consequence |
| ---- | ----------- |
| `main.rs` returns `Ok(None)` when `clients.is_empty()` | with `clients = []` the cert store and both rustls configs are never built; a `PUT` adding the first client has nothing to swap into |
| `Server.interception: Option<Interception>` is set once by `with_interception` at bind | the field needs an indirection, not a value |
| `interception_for` returns `Option<&Interception>` borrowed from `&self` | no atomic swap can hand out that borrow; the signature must return an owned handle |
| `fah-http` does not depend on `arc-swap` | the workspace does (`fah-rules`, `fah-api`, `fah-metrics`), so the pattern is sanctioned and in-tree, but adopting it changes `fah-http`'s manifest and a hot-path signature |

The claim that hard rule 3 already sanctions the pattern is true. The claim that
the swap follows it is a task-scope statement the task will have to pay for.

### 5 — PSL is a decided question, not an open one (§The operator's path)

The ADR treats the public-suffix dependency as a cost to weigh in the task. The
project weighed it in p2-03 and declined:
`fah_rules::url_matcher::registrable()` is the last-two-labels approximation,
documented as *"not a Public Suffix List … carrying a PSL costs a dependency,
~200 KB of tables and a refresh story"*, accepted because a wrong answer there
only ever **narrows** a rule.

For the widening button the error direction inverts, and the existing helper is
precisely the hazard the ADR names: `registrable("api.foo.co.uk")` returns
`co.uk`. It must not be reused.

| Option | Cost | Verdict |
| ------ | ---- | ------- |
| Exact host only, widening withheld | none | the ADR's own fallback, and the recommendation |
| `psl` crate in `fah-api` | ~200 KB of compiled tables, MPL-2.0 list data, a refresh story that is a rebuild | reintroduces compiled-in data with a shelf life — the pattern this ADR exists to delete |
| JS public-suffix table in the dashboard | ~100 KB in a frontend whose entire runtime dependency set is `preact` + `uplot`; still a compiled-in copy | same shelf life, moved to another artefact |

Licensing is not neutral: the Mozilla PSL data is MPL-2.0, and the workspace
ships no `LICENSE` file and no third-party notice today, so an embedded list
would be the first artefact needing one.

### 6 — the `status 0` count

`intercept()` emits `status 0` on seven paths: unusable SNI, upstream connect
failure, no leaf, incomplete minting, the client not completing our handshake
(the rejection), our handshake deadline, upstream HTTP handshake failure. The
rejection is one of the seven, so it shares the code with **six** others — which
is also the number the ADR's own enumeration lists.

### 7 — a test deletion loses more than the claim (§Consequences)

`the_baseline_exclusions_ship_without_any_configuration` (interception.rs:1825)
also asserts 10 000 `contains` misses in under a second — the only lookup-cost
check in that file. The stated replacement, "an empty policy excludes nothing",
already exists as `the_empty_set_matches_nothing` (exclusions.rs). Deleting the
baseline also makes `ExclusionSet::empty()` and `ExclusionSet::new(&[])` the
same function; one of them should go with it.

### 8 — the precedent sentence

`rules.lists`, `schedule.timezone` and `policies` are TOML keys that are **not**
in `BOOT_KEYS` and are already applied live. What lives outside the TOML is
downloaded list content and history data. The tree's precedent is therefore that
a TOML key can have a live consumer — which does not block the split, but does
not support "these two lists were never configuration" either. The argument that
carries the decision is irrevocability, not precedent.

### 9 and 10 — wording and citations

`/config` is the container volume at line 64 and the API endpoint at lines 75
and 79. Drifted citations: `exclusions.rs:60` (seeding is at 67-68),
`config_store.rs:46` (`"https"` is at 47), `https.rs:169` (`judge` is at 170),
`intercept.rs:148` (the accept arm is 146-153). Exact: `config_store.rs:153`,
`tls.rs:85`, `intercept.rs:36`, `fah-config/src/lib.rs:110`. The rustls
citations (`error.rs:69`, `enums.rs:20-26`) could not be checked — registry
reads are denied here — but the values they assert are confirmed by test.

## Claims confirmed as written

| # | Claim | Evidence |
| - | ----- | -------- |
| 1 | 34 baseline entries, seeded on every `new`, user entries only `insert` | exclusions.rs:12-47, 66-76; the only production construction is main.rs:857. Irrevocable as claimed — `ExclusionSet::empty()` exists but is reachable only from tests and benches |
| 2 | `"https"` is a whole-section boot key | config_store.rs:47 plus `is_boot_key` prefix match, so `https.interception.exclude_domains` answers `restart_required: true` |
| 3 | `merge` recurses only on objects | config_store.rs:153; arrays replace wholesale, as its own doc comment states |
| 4 | The SNI verdict precedes interception | https.rs:170 judges and returns on `Block`; https.rs:183 then asks `interception_for`. `clients = []` means no MITM, not no filtering |
| 5 | The rejection branch exists and throws the observation away | intercept.rs:150-153 — `debug!` plus `emit_session(.., 0)` |
| 7 | `UPSTREAM_CERT_FAILURE = 526` | intercept.rs:36 |
| 8 | `certificate_error` downcasts `io::Error` to `rustls::Error` | tls.rs:85 — see finding 2 for the variant it matches |
| 9 | rustls 0.23.42 alert descriptions | asserted in the probe: 0x2a, 0x2e, 0x30 |
| 10 | `fah-http` cannot reach `fah-api` | manifest lists `fah-certs`, `fah-common`, `fah-config`, `fah-model`, `fah-rules` only; layering.rs checks `dev-dependencies` too, so even a test-only edge fails the build — stronger than the ADR claims |
| 11 | `InterceptionConfig` carries `deny_unknown_fields` | schema/https.rs:31 — deletion alone hard-fails an existing config |
| 12 | tmp-then-rename write path | fah-config/src/lib.rs:110-111 |
| 13 | `contains` walks the queried host's labels | exclusions.rs:90-102 — cost is independent of list length, so the cap is pathology detection, as argued |
| 14 | `clients` and `exclude_domains` are uncapped | no check in `validate`; `MAX_UPSTREAM_SERVERS = 8` (lib.rs:117), `MAX_POLICIES = 16` (lib.rs:314) |
| 15 | The four tests exist and say what the ADR claims | exclusions.rs:144, 182; interception.rs:783, 1825 — see finding 7 for what else one of them says |
| 16 | R2 sets policy through `POST /api/v1/config` | partially — see finding 3 |
| — | A listed client without the CA closes rather than splices | main.rs warns and still builds `Interception`, so the connection closes (p3-04 L2) |

## Measurements

Device: dev box, Windows 11, x86-64. Workload: loopback TLS handshakes against a
self-signed rcgen leaf, rustls 0.23.42 / tokio-rustls 0.26.4, aws-lc-rs. Probe
kept at `%TEMP%/fah-accept-alert-probe.rs`, not in the tree.

| Client behaviour | `io::ErrorKind` | Downcast | Alert |
| ---------------- | --------------- | -------- | ----- |
| TLS 1.3, empty root store | `InvalidData` | `AlertReceived` | `UnknownCA` 0x30 |
| TLS 1.3, verifier → `ApplicationVerificationFailure` | `InvalidData` | `AlertReceived` | `AccessDenied` 0x31 |
| TLS 1.3, verifier → `UnknownIssuer` | `InvalidData` | `AlertReceived` | `UnknownCA` 0x30 |
| TLS 1.3, verifier → `NotValidForName` | `InvalidData` | `AlertReceived` | `BadCertificate` 0x2a |
| TLS 1.2, verifier → `ApplicationVerificationFailure` | `InvalidData` | `AlertReceived` | `AccessDenied` 0x31 |
| TLS 1.2, verifier → `UnknownIssuer` | `InvalidData` | `AlertReceived` | `UnknownCA` 0x30 |
| TCP close, no hello | `UnexpectedEof` | none | — |

The last row is the ADR's unclassifiable case: no `rustls::Error` inside, so
nothing to classify. That a *real* pinning client closes with RST or FIN instead
of alerting remains unmeasured — the ADR asserts it from general knowledge, says
so, and states the limitation honestly ("not a complete census").

Dead-domain evidence, re-checked 2026-09-10 over DoH (`dns.google`; the LAN
blocks outbound 53):

| Name | Result |
| ---- | ------ |
| `otpbank.ro` A | `Status: 2` SERVFAIL — the authoritative servers answer REFUSED |
| `www.otpbank.ro` A | `Status: 2` SERVFAIL, same cause |
| `alphabank.ro` A | `Status: 0` with no answer section — NODATA, no A record |
| `www.alphabank.ro` A | `Status: 3` NXDOMAIN |

Exactly as the ADR states.

The 512 cap is a round number and the ADR does not pretend otherwise — it argues
the cap as pathology detection, which is the honest ground. Two gaps: no number
is proposed for `clients` while the error text promises to name which list is
over, and the memory bound goes unstated — `sni::normalize` caps an entry at 253
bytes (`MAX_NAME_LEN`), so 512 entries are ~128 KB worst case, which is what
satisfies hard rule 4.

## Internal contradictions

None found. No surviving text places `exclude_domains` under `config.toml`, no
compiled-in baseline survives the rewrite, and nothing defers this to Phase 4.
The weakened invariant is intact and correctly justified; it is not strengthened
back anywhere in the document.

## Files changed

None. The probe was written under `crates/fah-http/tests/`, run, and moved out
of the tree.

## Remaining TODOs

- Owner decision on the ten findings; 1-5 change ADR wording or task scope.
- Any ADR correction is a new commit on top of `f57f6a4`, pushed to both
  remotes.
- The probe deserves to become a real test in the detection task — it pins the
  accept-side contract the whole feature rests on.

**PASS WITH DEFERRED FINDINGS** — the decision holds; the document needs five
corrections before a task is written against it.
