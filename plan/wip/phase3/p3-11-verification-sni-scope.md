# P3-11 — Phase 3 Verification, SNI Scope

**Phase:** 3 · **Depends on:** p3-03, p3-05 · **Model:** Fable

## Goal

Phase 3 proven for what is actually going to run: DNS, plain HTTP, HTTPS
filtered at the SNI with no decryption, and the DoT/DoH listeners — on the
RB5009, with a seven-day soak behind it.

## Context

p3-06 was written for a full-mode deployment and is `PARKED`: the owner decided
on 2026-09-13 not to use the interception code. Most of p3-06's scope was never
interception-specific, and that part moves here rather than being parked with
it — the 443 steering above all, without which no HTTPS reaches the container
and the SNI path never runs at all.

The Interception Document's `clients` list stays empty, so every HTTPS
connection takes the splice leg (CONFIGURATION.md §Interception Document:
"Empty = nobody is intercepted; every other client splices"). The interception
code still ships compiled and a later decision can switch it on; this task does
not verify that path, and one of its security arms proves the path is closed.

## Scope

Carried over from p3-06, unchanged in substance:

- **PERFORMANCE.md budget rows** (doc update, bench-backed): SNI verdict +
  splice added latency, DoT/DoH added latency against UDP, cache hit rate under
  browsing load, RAM ceiling with all engines loaded. The interception
  handshake and minted-leaf cache rows are **not** set here.
- **Security verification suite** (`tests/`): the CA key unreachable through
  every API route — walk them all; a bad upstream certificate never masked;
  exported artifacts carrying no private material; a non-listed client cannot
  be intercepted; and the arm that proves the decision is in force,
  **interception disabled ⇒ byte-identical splice** (sampled).
- **End-to-end offline**, one scripted scenario: DNS block, HTTP URL block, SNI
  block, DoT query, DoH query. No intercepted-URL leg.
- **RB5009 with the owner**: the **dst-nat 443 rule and its rollback**, Private
  DNS setup on a phone, a browsing pass, and `deploy-rb5009.md` gaining its
  HTTPS section. The owner runs every router command; this task proposes them.
- **Seven-day soak** on the deployed build, numbers recorded against the budget
  rows. Same shape as the 0.3.4 soak: hourly scheduled task, artefacts under
  `docs/code-review/phase3/soak-<version>/`.
- README operating-modes wording swept if it drifted.

Tooling is current and is not rewritten here: `p3-06-smoke-plan.md` layers 0–3
and `p3-06-testing-plan.md` had their scripts moved off the dead
`https.interception` key by p3-06b §3b, run green and committed (`223bf79`).
The interception arms inside those plans do not run.

## Acceptance criteria

- Every budget row listed above is bench-backed on dev hardware and then
  recorded on-device, each figure carrying its corpus, workload and device.
- The security suite is green, the byte-identical-splice arm included.
- The deploy guide is reproducible, and the dst-nat 443 rule is verified by a
  non-zero packet count with its rollback exercised.
- The seven-day soak completes: RAM steady and inside the ceiling its budget row
  sets, no unexplained restart, counters read at the end.
- Gates green.

## Out of scope

Everything that needs interception to exist — CA install on a client device,
intercepted HTTPS URL blocking, full-mode RAM figures, the pinned-app
spot-check. Those stay with p3-06, which is `PARKED`, and come back only with a
new decision. Phase 4 HTML rewriting. Performance characterization and the N
sweep, which are p3-10.

## Blocked on

The device half needs the merged build deployed to the RB5009, and that decision
has not been taken. The dev-box half — budget benches, security suite, e2e and
smoke layers 0–3 — runs today and does not wait for it.
