# P5-01 — Static Serving from fah-api

**Phase:** 5 · **Depends on:** — · **Model:** Opus

## Goal

`fah-api` serves a static web bundle from `/web` on the same origin as the API,
and the runtime image carries that bundle. No frontend code exists yet: this
task builds and proves the delivery path with a placeholder page, so every later
task ships into a shape that already works.

## Context

Verified 2026-08-25: `crates/fah-api/src/routes.rs` has no static serving at
all — the router is `/health`, `.nest("/api/v1", …)` and `.fallback(not_found)`,
wrapped in `require_api_key`. There is no `ServeDir`, no `ServeFile`, and
`tower-http` is not a dependency of the crate.

One origin is the point. It removes CORS entirely and lets the session cookie
from `p5-02` be same-origin, which is why this is not a second container.

## Scope

- **Static routes**, alongside the existing surface:
  - `/` → `index.html`
  - `/assets/*` → hashed assets
  - SPA fallback for unknown non-API paths → `index.html`
  - `/api/v1/*` and `/health` unchanged in behaviour
- **Route ordering**: `/api/v1` answers first. An unknown path under `/api/v1/`
  must still produce the API's own JSON error, never the SPA shell. This is the
  single highest-risk detail in the task.
- **Auth exemption**: static routes sit outside `require_api_key`, as `/health`
  does. The bundle holds no secrets, and gating it would make the login page
  unreachable in `p5-02`.
- **Serving from a directory** (`/web`), not embedded in the binary. Decide
  `tower-http`'s `ServeDir` versus a small hand-rolled handler by weighing the
  dependency against the image budget, and record the reasoning in the review
  file. Either way: no path traversal above the root, correct `Content-Type`,
  `ETag`, `304` on revalidation.
- **Pre-compressed assets**: serve `.br` / `.gz` siblings by `Accept-Encoding`
  when present. No runtime compression — the bytes never change.
- **Cache headers**: hashed assets `immutable` with a long max-age;
  `index.html` `no-cache`.
- **Boot check**: if the static root is missing or holds no `index.html`, log at
  `error` naming the path and the likely cause (a volume mounted over `/web`).
  Do not fail the process — DNS filtering must keep running without a UI.
- **Multi-stage Dockerfile**: a `frontend` stage running the Vite build, and
  `COPY --from=frontend … /web` in the runtime stage. Until `p5-03` exists, that
  stage emits a placeholder `index.html` naming the version — enough to prove
  the path end to end.
- **Config**: only if a path genuinely needs to be configurable. Default first;
  a compiled-in default that never needs editing beats a knob.
- **Doc updates in the same change**: API.md gains the static routes and states
  they are unauthenticated; ARCHITECTURE.md notes `fah-api` serves the UI;
  SECURITY.md notes the static surface is exempt from the API key.

## Acceptance criteria

- `GET /` returns the placeholder page; `GET /assets/<hashed>` returns the asset
  with `immutable` caching and a working `304`.
- `GET /api/v1/nonexistent` with a valid key returns the API's JSON `404` — not
  HTML. Asserted by test, not by inspection.
- `GET /health` unchanged, still unauthenticated.
- Static routes answer with no `Authorization` header.
- A path-traversal attempt (`/../`, encoded variants) cannot escape the root —
  test.
- Missing `/web` logs the error and the DNS pipeline still serves.
- `crates/fastadhunter/tests/layering.rs` still passes.
- Image builds; final image contains no Node, no `node_modules`, no build
  toolchain. Image size recorded against the 30 MB budget.
- Gates green.

## Out of scope

Authentication of any kind (`p5-02`). Any real frontend (`p5-03`). Serving
anything from `/config` or `/data`.

## Suggested prompt

> Read root CLAUDE.md hard rules, API.md, SECURITY.md,
> docs/dashboard/implementation-plan.md §Stage A, and
> plan/wip/phase5/p5-01-static-serving.md. Add static serving of `/web` to
> `fah-api` with correct route ordering, auth exemption, pre-compressed asset
> selection and a boot check, wire the multi-stage Dockerfile, and update the
> listed docs in the same change.
