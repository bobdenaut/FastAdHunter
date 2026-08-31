# P3-06 — Phase 3 Verification

**Phase:** 3 · **Depends on:** p3-02, p3-04, p3-05 · **Model:** Fable

## Goal

Phase 3 proven: TLS budgets set and met, security properties re-verified
end-to-end, RB5009 running `dns+http+https` with real devices.

## Context

PERFORMANCE.md needs TLS rows; SECURITY.md promises need adversarial checks,
not just unit tests. On-device work needs the user: dst-nat 443, CA install
on a test device, Private DNS on a phone.

## Scope

- PERFORMANCE.md budget rows (doc update, bench-backed): SNI verdict +
  splice added latency, interception handshake overhead, minted-leaf cache
  hit rate under browsing load, DoT/DoH added latency vs UDP; RAM ceiling
  re-affirmed with all engines loaded.
- Security verification suite (`tests/`): CA key unreachable via every API
  route (walk them all), non-listed client cannot be intercepted, bad
  upstream cert never masked, exported artifacts contain no private material,
  interception disabled ⇒ byte-identical splice (sampled).
- End-to-end offline: full mode `dns+http+https` — DNS block, HTTP URL block,
  SNI block, intercepted HTTPS URL block, DoT query — one scripted scenario.
- RB5009 with the user: dst-nat 443 rule (+ rollback), CA install
  walkthrough on one Android/Windows test device (screenshots into
  docs/images/), Private DNS setup, browse; 24h soak in full mode; numbers
  vs budgets recorded; deploy-rb5009.md gains the HTTPS section.
- Update README operating-modes section if wording drifted (doc sweep).

## Acceptance criteria

- Every new budget row bench-backed and met on dev hardware; soak numbers
  recorded on-device, RAM ≤128MB steady in full mode.
- Security suite green; deploy guide reproducible; pinned-app (banking)
  spot-check unaffected on the test device.
- Gates green.

## Out of scope

Phase 4 HTML rewriting; performance tuning beyond budgets (file follow-ups).

## Suggested prompt

> Read plan/wip/phase3/p3-06-phase3-verification.md, PERFORMANCE.md,
> SECURITY.md. Add the TLS budget rows from bench data, build the security
> verification suite and the full-mode e2e scenario, then walk the RB5009
> setup, CA install and soak WITH the user, recording everything.
