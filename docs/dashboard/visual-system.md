# Visual System

How the FastAdHunter dashboard looks and is built. Pi-hole's interface is the
reference for *appearance and interaction*; none of its code is used.

## Stack

| Choice | Why |
| --- | --- |
| TypeScript | the API has a documented, stable shape — it should be typed once and checked everywhere |
| Preact | Pi-hole's chrome is a sidebar, cards and tables; that needs a renderer, not a framework. ~4 KB runtime against React's ~45 KB. |
| Vite | static build, no dev server in production, no runtime toolchain on the box |
| uPlot | time series is most of the charting. ~45 KB against Chart.js's ~200 KB, and it redraws a 1440-point series without dropping frames on an RB5009-class client. |
| hand-drawn SVG donuts | two donuts do not justify a chart library |
| own CSS | AdminLTE pulls Bootstrap 4 and jQuery. The look is reproducible in a few hundred lines of CSS with custom properties. |

No jQuery, no Bootstrap, no AdminLTE, no DataTables, no moment.js.

**Bundle budget: under 150 KB gzip, all in.** The dashboard is served to a
household from a box whose primary feature is performance; a megabyte of
JavaScript would contradict the product. Anything that pushes past the budget
has to earn it against this file.

## Output shape

A static bundle: `index.html`, one JS chunk, one CSS file, fonts and icons
inlined or self-hosted. No CDN — the box may be the only DNS resolver on the
network, and a dashboard that needs the internet to render is a dashboard that
fails exactly when it is needed.

## Layout

Pi-hole's AdminLTE arrangement, reproduced:

- fixed left sidebar, ~230 px, dark, four labelled sections, active item marked
- top navbar with the page title on the left and connection state on the right
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

- **Time series** — stacked area for allowed/blocked, line for rates and
  latency. Shared range selector: 24 h · 7 d · 30 d. Zoom and pan on the
  primary chart, as Pi-hole does.
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

## Accessibility

Keyboard-reachable navigation, visible focus rings, `aria-live` on the
connection-state indicator, and colour never carrying meaning alone.

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
