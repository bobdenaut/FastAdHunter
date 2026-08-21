# P0-06 — Docker Image

**Phase:** 0 · **Depends on:** p0-05 · **Model:** Opus

## Goal

A distroless container image holding the static musl binary, buildable for
amd64 and arm64, that starts and passes its healthcheck.

## Context

ARCHITECTURE.md §Docker + SECURITY.md §Container hardening + PERFORMANCE.md
image budget (≤30MB). Deployment target is RouterOS/RB5009 (arm64), dev
machine is Windows/amd64.

## Scope

- Multi-stage `Dockerfile`: Rust musl builder → `gcr.io/distroless/static`
  (CA bundle included), non-root user, `/config` + `/data` volume mounts,
  ports 53/udp, 53/tcp, 8443/tcp declared.
- `HEALTHCHECK CMD ["/fastadhunter", "--healthcheck"]`.
- Targets: `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`
  (multi-arch via `docker buildx`; document the one-time builder setup).
- `.dockerignore`.
- Docs: a short `docs/deploy-rb5009.md` stub describing the RouterOS container
  steps (registry/tarball import, veth, mounts) — to be completed in Phase 1.

## Acceptance criteria

- `docker build` (amd64) succeeds locally; container starts with empty
  `/config` volume, generates default config, healthcheck reports healthy.
- arm64 target at minimum cross-compiles green
  (`cargo build --target aarch64-unknown-linux-musl --release`); buildx
  multi-arch documented even if not run locally.
- Image size ≤ 30MB (inspect and record actual size in the task completion
  note).
- Gates green workspace-wide.

## Out of scope

Registry publishing, on-device RB5009 validation (Phase 1 soak task),
docker-compose examples.

## Suggested prompt

> Read ARCHITECTURE.md §Docker, SECURITY.md §Container hardening,
> PERFORMANCE.md image budget, and plan/wip/phase0/p0-06-docker-image.md.
> Write the multi-stage Dockerfile and buildx notes; prove amd64 image runs
> healthy and arm64 cross-compiles.
