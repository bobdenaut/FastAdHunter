# P2-06 — Per-Client Enforcement and Policy API

**Phase:** 2 · **Depends on:** p2-04, p2-05 · **Model:** Opus

## Goal

Both pipelines resolve the client's active policy per request/query; policies
manageable over the API; stats become policy-aware.

## Context

The verdict call gains a policy dimension: client IP → assignment → (schedule
now?) → policy ruleset → verdict. Lookup must stay hot-path cheap: the
client→policy resolution is a read on swapped state, not a computation.
API.md grows a §policies section (doc update in the same change).

## Scope

- Enforcement: fah-dns and fah-http resolve policy before verdict; schedule
  state precomputed on a coarse tick (e.g. per-minute atomic swap of the
  effective client→ruleset map — no time math per query).
- API endpoints (API.md updated): `GET/POST /api/v1/policies`,
  `PATCH/DELETE /api/v1/policies/{id}`, assignment endpoints under
  `/api/v1/clients/{ip}` (set policy/schedule), `rules/test` gains optional
  `policy` context in response (which policy decided).
- Config write-back for policies/assignments (runtime-mutable, atomic swap).
- Stats: per-policy counters; query-log entries record the policy id;
  `/api/v1/queries` filter by policy (API.md updated).
- Tests: two clients, two policies — same domain, different verdicts;
  schedule flip changes verdict at the boundary without restart; API
  round-trip creates policy → assignment → verdict changes live.

## Acceptance criteria

- Per-query policy resolution adds no measurable latency vs Phase 1 baseline
  (re-run pipeline benches, compare, record).
- Kids-policy scenario green end-to-end (the DoD scenario, as a test).
- API.md consistent with implementation (golden-file response tests).
- Gates green.

## Out of scope

Dashboard UI, safe-search implementations (hook stays no-op, backlog).

## Suggested prompt

> Read plan/wip/phase2/p2-06-per-client-enforcement.md, API.md, and the
> p2-05 policy model. Wire policy resolution into both pipelines with the
> precomputed schedule map, add the policy API + stats dimensions, update
> API.md, and prove the two-client scenario end-to-end.
