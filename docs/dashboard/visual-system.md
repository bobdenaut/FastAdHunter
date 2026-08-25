# Visual System

How the FastAdHunter dashboard looks and is built. Pi-hole's interface is the
reference for *appearance and interaction*; none of its code is used.

## Stack

| Choice | Why |
| --- | --- |
| TypeScript | the API has a documented, stable shape — it should be typed once and checked everywhere |
| Preact | Pi-hole's chrome is a sidebar, cards and tables; that needs a renderer, not a framework. ~4 KB gzip against React + ReactDOM's ~45 KB gzip. |
| Vite | static build, no dev server in production, no runtime toolchain on the box |
| uPlot | time series is most of the charting. ~16 KB gzip against Chart.js's ~60 KB gzip, and it redraws a 1440-point series without dropping frames on an RB5009-class client. |
| hand-drawn SVG donuts | two donuts do not justify a chart library |
| own CSS | AdminLTE pulls Bootstrap 4 and jQuery. The look is reproducible in a few hundred lines of CSS with custom properties. |

Sizes in this table are **gzip**, the same unit as the budget below. Comparing a
minified figure against a gzip budget is how a bundle quietly doubles.

No jQuery, no Bootstrap, no AdminLTE, no DataTables, no moment.js.

**Bundle budget: under 150 KB gzip, all in.** The dashboard is served to a
household from a box whose primary feature is performance; a megabyte of
JavaScript would contradict the product. Anything that pushes past the budget
has to earn it against this file.

**Brotli is measured and reported alongside gzip.** Gzip gates because it is the
stricter bound; `.br` is what the handler actually serves, so the figure that
travels is never left unmeasured.

## Output shape

A static bundle: `index.html`, **three to four JS chunks**, CSS, and a small
self-hosted SVG sprite. No CDN — the box may be the only DNS resolver on the
network, and a dashboard that needs the internet to render is a dashboard that
fails exactly when it is needed.

**Not one chunk, and not dozens.** One chunk puts uPlot on the login page and
makes any edit invalidate the whole immutable asset; unbounded splitting spends
the cache and the request budget for nothing. The split is roughly shell and
login · charts · infrequently-visited pages, and Vite's module-preload behaviour
is configured explicitly — the default injects preload links that eagerly fetch
the very chunks that were split out, which pays the cost of splitting and
collects none of the benefit.

On a LAN the transfer saving is small and is not the argument. The arguments are
cache granularity, keeping the chart library off the login path, and parse cost
on an old phone.

**No bundled web font.** A subset WOFF2 is 15–40 KB already-compressed — brotli
takes nothing further off it — plus a request on the critical path, for a
dashboard whose entire budget is 150 KB. A system font stack costs nothing and
looks native on every household device. If a named visual requirement ever
justifies one, it is stated here first.

## Layout

Pi-hole's AdminLTE arrangement, reproduced:

- fixed left sidebar, ~230 px, dark, four labelled sections, active item marked
- top navbar with the page title on the left and connection state on the right —
  **three states**: `live` · `not needed here` · `reconnecting`. Subscriptions are
  route-scoped, so a closed socket is the correct steady state on nine of the
  thirteen screens; only `reconnecting` is styled as a problem, and
  `not needed here` must not read as an error
- content area with a `content-header` (page name plus a short line of context)
  then a 12-column card grid
- cards are white (dark-mode: raised surface) with a title bar, an optional
  tool button, and a body

Grid columns follow Pi-hole's: full width for a primary time series, halves for
paired charts and tables, quarters for the tile row.

## Tiles

Pi-hole's `small-box`: a large number, a label beneath, a translucent glyph in
the corner, a solid accent background, and a footer strip that links deeper.

Accent roles are fixed and never reused for another meaning:

| Colour | Means |
| --- | --- |
| aqua | volume — total queries, HTTP requests |
| red | blocked |
| yellow | ratio — blocked %, cache hit % |
| green | healthy state — uptime, compiled rules |
| grey | a figure that is zero and expected to be |

## Charts

- **Time series** — stacked area for permitted/blocked, line for rates and
  latency. Shared range selector: 24 h · 7 d · 30 d. Zoom and pan on the
  primary chart, as Pi-hole does. `permitted` is `queries − blocked` and is never
  labelled "allowed": `allow` is the explicit exception verdict and a different,
  far smaller number (capability-matrix.md §Vocabulary).
- **Decimation is visible.** When the API returns `stride > 1`, the chart
  footnote states the series is decimated and that every plotted point is a
  real reading, never an average.
- **Donuts** — query types only. Legend beside the ring, not inside it.
- **Bars** — upstream attempts with failures overlaid; cache stages stacked;
  per-list rule partition stacked.
- **Latency** is drawn as p50 and p99 lines per class, never as an average.
  Averages hide the tail the budget cares about.
- Axes carry units. Bytes are binary-prefixed and labelled. Durations are ms
  under a second, s above.

## Tables

Pi-hole's density and alignment: numbers right-aligned and tabular-figure, a
translucent frequency bar behind the count in top-N tables, zebra striping off,
row hover on.

Sorting and filtering are client-side over what a response already holds. No
pagination controls where the API returns a bounded top-N — the bound is the
page.

## Live Feed

Pi-hole's query-log row rhythm over an ephemeral source. Verdict is a coloured
pill: `pass` neutral, `allow` green, `block` red. `cached` is a small marker on
the row, not a verdict. HTTP rows carry method and resource type in the same
column band DNS rows use for qtype, so the two pipelines read as one table.

The panel header states plainly that the feed starts empty, holds a bounded
number of rows, and retains nothing.

## Theme

Light and dark, both first-class, switched by an explicit control and defaulting
to `prefers-color-scheme`. Every colour is a custom property on `:root`; the
dark theme redefines the tokens and nothing else. Verdict colours keep their
contrast in both themes and are never the only signal — every coloured pill
also carries its word.

The stored choice is applied **before first paint**, by a tiny inline script in
`index.html`. Applied from the bundle instead, every load flashes the wrong
theme.

## Responsive

| Width | Behaviour |
| --- | --- |
| ≥ 1200 px | full grid, sidebar expanded |
| 768–1199 px | halves become full width, sidebar collapses to icons |
| < 768 px | single column, sidebar is an overlay drawer, tables scroll inside their own container |

The page body never scrolls horizontally. Wide tables and charts scroll inside
themselves.

## Icons

One small self-hosted SVG sprite of the glyphs actually used. No icon font, no
Font Awesome.

## Typography

A system font stack. See §Output shape — no web font ships.

## Accessibility

Keyboard-reachable navigation, visible focus rings, `aria-live` on the
connection-state indicator across all three of its states, and colour never
carrying meaning alone.

## Design inspiration

The FastAdHunter web interface is inspired by the Pi-hole Web Interface
(<https://github.com/pi-hole/web>), particularly its dashboard composition,
navigation chrome, card layout, statistics presentation, tables, charts,
responsive behavior, and overall administrative UX.

FastAdHunter does not import Pi-hole's web frontend code, AdminLTE, Bootstrap,
jQuery, or other Pi-hole web dependencies. The interface is implemented
independently using TypeScript, Preact, Vite, and FastAdHunter's own CSS and
components.

Pi-hole-specific branding, terminology, functionality, and product identity are
not used in the FastAdHunter interface.

Pi-hole Web Interface: <https://github.com/pi-hole/web> — License: EUPL-1.2
(verified 2026-08-25 against the repository's `LICENSE`).

## Branding

FastAdHunter name, logo and favicon throughout. Page title is
`FastAdHunter — <page>`. No Pi-hole name, mark, wording or link survives
anywhere in the shipped bundle, including comments, alt text and page titles.
