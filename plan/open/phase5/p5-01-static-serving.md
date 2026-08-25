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
- **Serving from a directory** — `tower-http`'s `ServeDir`, rooted at the baked
  `/web`. **This is the decided default**: prefer the well-tested static-file
  service, and prefer correctness and a minimal attack surface over saving a
  small amount of code size. A hand-rolled handler is evaluated **only if**
  measurement shows `tower-http` materially regresses the ~30 MB image budget or
  brings dependency/runtime cost the appliance should not carry — and if it
  comes to that, the trade-off is recorded in the review file rather than
  decided silently.
- **The static root is fixed** to the baked `/web` directory. Not configurable,
  not derived from a request, not mountable. `/web` is image content: the UI and
  the API version and deploy as one artifact, and a volume over `/web` would
  pair a rolled-back binary with a newer UI.
- **Pre-compressed assets**: `ServeDir::precompressed_br()` and
  `precompressed_gzip()` select the `.br` / `.gz` sibling Vite emits, by
  `Accept-Encoding`. No runtime compression — the bytes never change.

### Caching and revalidation — match `ServeDir`'s real behaviour

Written out because the obvious specification is the wrong one. **`ServeDir`
does not generate `ETag`.** What it does provide is `Last-Modified`, and
`If-Modified-Since` revalidation answering `304`. Build to that.

- **Revalidation is `Last-Modified` / `If-Modified-Since`.** Do not write an
  acceptance criterion, a test, or a header layer that expects an `ETag` on a
  `ServeDir` response.
- **Do not add `ETag` generation.** Not as a nicety, not "for completeness" —
  only against a demonstrated requirement that `Last-Modified` cannot meet, and
  that requirement gets stated in the review file before any code exists for it.
  Hashed filenames already make content-addressed revalidation mostly moot.
- **Hashed `/assets/*` get an added response-header layer**:
  `Cache-Control: public, max-age=31536000, immutable`. The filename carries the
  content hash, so the URL changes whenever the bytes do and the response never
  needs revalidating.
- **`index.html` stays revalidatable, never `immutable`.** Its URL does not
  change when its contents do, so an immutable shell would outlive the image
  that replaced it — a new deployment would serve new hashed assets that the
  cached shell never references. It must be revalidated on each load, so a new
  image delivers a new shell.

That asymmetry is the whole caching design: **the shell is checked every time,
the assets it points at are never checked again.**

### Remaining scope

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

- `GET /` returns the placeholder page.
- `GET /assets/<hashed>` carries
  `Cache-Control: public, max-age=31536000, immutable`.
- `GET /` carries a revalidatable `Cache-Control`, never `immutable`, so a new
  image delivers a new shell.
- A conditional request with `If-Modified-Since` returns `304`. **Revalidation
  is `Last-Modified`-based; no test asserts an `ETag`,** because `ServeDir` does
  not emit one.
- **MIME types correct** for the types the bundle actually ships — `.html`,
  `.js`, `.css`, `.svg`, `.woff2`, `.json`, `.ico` — asserted by test. A wrong
  `Content-Type` on the module script is a page that silently does not run.
- **Pre-compressed selection** works: a client advertising `br` gets the `.br`
  sibling with the right `Content-Encoding`, one advertising nothing gets the
  plain file.
- `GET /api/v1/nonexistent` with a valid key returns the API's JSON `404` — not
  HTML. Asserted by test, not by inspection.
- An unknown non-API path returns `index.html` (SPA fallback), while an unknown
  path under `/assets/` returns a plain `404` rather than the shell — a missing
  asset must not resolve to HTML.
- `GET /health` unchanged, still unauthenticated.
- Static routes answer with no `Authorization` header.
- **Path traversal cannot escape `/web`** — `..`, percent-encoded variants,
  backslashes, absolute paths, and a symlink pointing outside the root. Tested,
  not assumed, even though `ServeDir` is expected to handle it: this is the
  reason the well-tested service was chosen, so the test is what proves the
  choice paid off.
- Missing `/web` logs the error and the DNS pipeline still serves.
- `crates/fastadhunter/tests/layering.rs` still passes.
- Image builds; final image contains no Node, no `node_modules`, no build
  toolchain.
- **Image and binary size recorded** against the 30 MB budget, measured against
  a pre-change checkout rather than a stored baseline: the delta `tower-http`
  and its transitive dependencies add, called out separately from the delta the
  bundle adds. If that delta is material against the budget, evaluate a small
  dedicated handler and record the trade-off; otherwise state the figure and
  keep `ServeDir`.
- Gates green.

## Out of scope

Authentication of any kind (`p5-02`). Any real frontend (`p5-03`). Serving
anything from `/config` or `/data`.

## Suggested prompt

> Read root CLAUDE.md hard rules, API.md, SECURITY.md,
> docs/dashboard/implementation-plan.md §Stage A, and
> plan/wip/phase5/p5-01-static-serving.md. Serve the baked `/web` directory from
> `fah-api` using `tower-http`'s `ServeDir` with pre-compressed variants, add the
> cache-header layer, get the route ordering and auth exemption right, add the
> boot check, wire the multi-stage Dockerfile, and update the listed docs in the
> same change. Measure the image-size delta the dependency adds against a
> pre-change checkout and record it.
