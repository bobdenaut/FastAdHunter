# P1-11 — RB5009 Deployment and Soak

**Phase:** 1 · **Depends on:** p1-10 · **Model:** Sonnet

## Goal

FastAdHunter runs on the real RB5009: documented deployment, real household
traffic, budgets verified on-device.

## Context

Target environment: RouterOS container, veth 172.17.0.2/24, LAN
192.168.10.1/24, external SSD mounted for `/config` + `/data`. This task needs
the user's router access — coordinate each on-device step with them.

## Scope

- Complete `docs/deploy-rb5009.md`: buildx arm64 image → tarball/registry →
  RouterOS `/container` setup (veth, mounts to the SSD, env vars, start),
  pointing LAN DHCP DNS at the container IP, rollback procedure.
- On-device checklist with the user: container starts, first-boot generates
  config/key/cert on the SSD, phone/laptop resolve through it, known ad
  domain blocked, dashboard-less verification via `curl` to the API.
- Soak: ≥24h of real household traffic; collect `/metrics` + `/api/v1/stats`;
  record RSS, p99 latencies, QPS peaks against PERFORMANCE.md budgets.
- File issues (or ADRs if design-level) for anything the device disproves.

## Acceptance criteria

- Deployment doc reproducible start-to-finish by the user without improvising.
- 24h soak: RAM ≤128MB steady, no crashes, no watchdog restarts; numbers
  recorded in the completion note.
- Gates green (no code expected, but if fixes land they pass the gates).

## Out of scope

Performance tuning beyond budget compliance (file follow-ups), Phase 2 scope.

## Suggested prompt

> Read plan/wip/phase1/p1-11-rb5009-deploy.md and docs/deploy-rb5009.md stub.
> Finish the deployment guide, then walk the on-device checklist WITH the user
> step by step (they run the RouterOS commands), and run the 24h soak.
