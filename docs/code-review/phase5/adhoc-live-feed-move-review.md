# Review — ad-hoc: Live Feed moved to Overview, capacity banner removed

**Scope:** uncommitted working-tree diff, 9 files, +33/−66
(`dashboard/frontend` only). Change requested of Opus: place Live Feed in the
sidebar directly under Dashboard; delete the feed-notice banner.

**Verified:**

- `/live-feed` route declared second in `ROUTES`, `section: 'overview'`, no
  `group` — sidebar renders it as a plain item immediately below Dashboard.
- `query` event subscription, empty `endpoints`, `built`, `ownsHeader` and lazy
  `load` all carried over intact.
- Banner JSX, its two tests and the `.feed-notice` CSS rule removed together;
  `capacity` remains used (ring construction, `held / capacity` secondary), so
  no dead variable.
- No `/diagnostics/live-feed` reference left in `src/` (only stale `dist/`
  build artifacts). Topbar shows a plain "Live Feed" title — group prefix
  logic keys on `route.group`, now absent.
- Tests: 55 files / 950 pass. `tsc --noEmit` clean. No lint script exists.

## Findings

1. **Low — old deep link dies with no redirect.** `/diagnostics/live-feed`
   now resolves to the shell's not-found sentinel. Any bookmark, browser
   history entry or externally shared link from before this change breaks.
   Acceptable for a pre-release dashboard; a one-line redirect in the router
   would remove it.
2. **Low — stale doc comment in `sidebar.tsx` (lines 51–54).** The component
   doc still lists `LiveFeed` among the artboards drawn with the Diagnostics
   group expanded; the feed no longer lives in that group.
3. **Low — stale CSS section header.** `components.css:4397` still reads
   `── Diagnostics · Live Feed ──` above the feed styles.
4. **Info — icon collision.** `/live-feed` reuses the `diagnostics` sprite, so
   the Live Feed item and the Diagnostics group line show the same glyph in
   one sidebar. Cosmetic; a distinct sprite would need a new asset.
5. **Info — replacement test is weaker.** The two deleted banner tests also
   pinned the "live tail, not a query log" wording; the new
   `never calls itself a query log` test only asserts the absence of
   "Query Log". Coverage loss is consistent with the banner's removal.

**Verdict: PASS** — findings 1–3 fixed in the same working tree:

1. `MOVED` table in `routes.ts` (`/diagnostics/live-feed` → `/live-feed`);
   the shell replace-redirects and renders the new route on the first frame.
   Pinned by a new `routes.test.ts` case (951 tests green, `tsc` clean).
2. `sidebar.tsx` doc comment no longer lists `LiveFeed` as an expanded-group
   artboard.
3. CSS section header now reads `── Live Feed ──`.

Finding 4 fixed too: new `live-feed` symbol in `sprite.svg` (dot with
radiating arcs — "live signal"), sidebar keys `/live-feed` on it; the
Diagnostics heartbeat glyph is no longer duplicated. Finding 5 (accepted
coverage loss) stands as recorded.
