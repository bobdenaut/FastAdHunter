# P5-08 — Phase 5 Verification

**Phase:** 5 · **Depends on:** p5-07 · **Model:** Opus

## Goal

Prove the dashboard on the real device against the real budgets, and record the
evidence. Nothing new is built here — this task measures, tests end to end, and
closes the phase.

## Context

Budgets that apply, from PERFORMANCE.md and this phase's own constraints:
container image at most 30 MB, measured at 13.0 MiB before this phase; RAM
steady-state at most 128 MB; the frontend bundle at most 150 KB gzip. Budgets
are compared against a pre-change checkout, never against a stored baseline —
see [docs/measurement-traps.md](../../../docs/measurement-traps.md).

## Scope

- **End-to-end tests** against a running `fah-api`: sign in, load every page,
  perform one mutation per mutating page, sign out. A page that renders but
  whose data never arrives must fail the test, not pass it silently.
- **Route-ordering regression test** kept from p5-01, re-asserted: unknown API
  paths still return the API's JSON error, never the SPA shell.
- **Bundle measurement**: total gzip size, the largest contributors, and the
  cost of uPlot and the fonts stated separately.
- **Image measurement**: final image size against the 30 MB budget, and the
  delta this phase added. Confirm the runtime image contains no Node, no
  `node_modules`, no source maps, and no build toolchain.
- **RSS measurement on the RB5009**: steady-state RSS with the dashboard served
  and a browser connected, against the same box without it. Static serving
  should be close to free; if it is not, say by how much and why.
- **Auth cost on the device**: login latency and peak RSS during Argon2id
  verification, re-measured on the final build, with the rate limiter engaged.
- **Socket load**: one browser on the Live Feed for a sustained period — event
  rate, client memory, and confirmation that the ring stays bounded and the
  engine sheds nothing it would not otherwise shed.
- **Mobile pass**: every page opened on a real phone over the LAN, checked
  against the phone artboards in [docs/dashboard/sketch/](../../../docs/dashboard/sketch/)
  — `MobileNav`, `MobileDashboard`, `MobileLiveFeed`, `MobileClients` — which are
  the source of truth for these criteria:
  - no horizontal body scroll on any page at 390 px; wide content scrolls only
    inside its own container;
  - every interactive control at least 44 px on its smallest axis;
  - the sidebar is an overlay drawer below 768 px, dismissible by scrim and by
    close control;
  - no table-heavy view renders as a horizontal table — Lists, Clients and the
    Live Feed are one card per row;
  - the Live Feed holds its narrow-viewport ring and stops rendering while the
    page is hidden. Verify by backgrounding the tab and confirming no work
    continues.
- **Certificate experience on the phone**, recorded as a deployment finding —
  see the scope note below.
- **A page-by-page trace**: for every figure rendered anywhere in the UI, the
  API field it comes from. Anything that cannot be traced is a defect, not a
  footnote.
- **Doc updates in the same change**: PERFORMANCE.md gains the bundle and image
  figures if they belong there; docs/project-state.md is rewritten to say where
  the work now stands.
- All measurements go to `docs/code-review/phase5/` with corpus, workload and
  device, per root CLAUDE.md rule 19.

## Acceptance criteria

- Every sidebar entry resolves to a working page against a live API.
- Bundle at most 150 KB gzip, recorded.
- Image at most 30 MB, recorded, with the phase's delta stated.
- Runtime image proven free of Node and build tooling.
- Steady-state RSS on the device recorded with and without the dashboard.
- Argon2id parameters confirmed acceptable on the device under rate limiting.
- Live Feed proven bounded over a sustained run.
- Every rendered figure traced to an API field.
- Mobile pass completed on a real device, with the certificate experience
  recorded.
- Gates green, cargo and frontend.

## HTTPS and the certificate — a deployment concern, not a dashboard feature

The dashboard is served over the API's existing TLS listener and inherits
whatever certificate that listener presents. On a household box that is the
box's own certificate, so the first visit shows a browser warning — and phone
browsers make that warning harder to get past than desktop ones do.

**This is recorded, not designed around.** The task measures and documents the
experience: what each household browser shows on first visit, what it takes to
proceed, and what the options are (accepting it once, installing the CA on
household devices, or a name and certificate that browsers already trust). The
recommendation goes to SECURITY.md and the deployment notes — not into the UI.

**Explicitly out of bounds:** an HTTP fallback to dodge the warning. The session
cookie is `Secure` and `__Host-`-prefixed, so it does not exist over plain HTTP;
serving the dashboard unencrypted would send a session credential and the whole
config surface across the LAN in clear. If the warning is unacceptable, the
answer is a trusted certificate, never a downgrade.

## Out of scope

New features. Anything the capability matrix marks as cut. Any change to the
dashboard intended to make a certificate warning less visible.

## Suggested prompt

> Read PERFORMANCE.md section Budgets, docs/measurement-traps.md,
> docs/dashboard/capability-matrix.md, and
> plan/wip/phase5/p5-08-phase5-verification.md. Run the end-to-end tests,
> measure bundle, image, device RSS and login cost against a pre-change
> checkout, complete the mobile pass, trace every rendered figure to its API
> field, and write the evidence to docs/code-review/phase5/.
