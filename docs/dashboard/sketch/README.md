# Dashboard sketch

**These files are a design sketch, not source code.** Nothing here is imported,
built, or shipped. The dashboard is implemented in TypeScript + Preact per
[../implementation-plan.md](../implementation-plan.md), from scratch — this
directory is what the screens should look like, not how they are made.

## What is in here

Thirteen `.dc.html` files, one per screen, plus `canvas.json` describing how
they sit on a single pan-and-zoom canvas.

| Row | Artboards |
| --- | --- |
| Overview & Filtering | `Main` (Dashboard) · `Lists` · `CustomRules` · `Policies` · `Clients` · `RuleTester` |
| Runtime & System | `Cache` · `Performance` · `Upstreams` · `Settings` · `Health` · `Memory` · `LiveFeed` |

Each file is self-contained HTML with inline styles and hand-drawn SVG charts.
They open in a browser directly. There is no build step, no dependency, and no
shared stylesheet — the chrome is duplicated across files on purpose, because
each artboard has to render alone.

## What they are worth

They settle **layout and interaction**, and they carry the decisions that were
argued for rather than assumed:

- Every panel maps to a route in [../capability-matrix.md](../capability-matrix.md).
  Nothing is drawn without an endpoint behind it, and nothing Pi-hole has but
  FastAdHunter cannot back appears at all.
- Live Feed states on the page that it is an ephemeral tail, because there is
  no per-query store to search.
- Upstreams charts endpoint health, never a share-of-traffic pie: per-query
  upstream attribution is deliberately not carried.
- Memory puts peak on a budget rail rather than in the composition donut — a
  lifetime high-water mark is not a slice of current RSS.
- Budgets are drawn as markers, never as walls. Nothing enforces memory at
  runtime; the container runs `memory-high=unlimited`.
- Custom Rules is a validated document, not a per-rule CRUD table, because
  `/rules/user` has no per-rule identity.

## About the figures

Where real readings existed they were used — 752,585 rules, RSS 54.9 MiB, peak
133.7 MiB, residual 28.2 MiB, 89.7 % hit rate over 11,171 lookups, 1,108 of
50,000 cache entries, series min 42 / max 97 MiB. The rest are representative
values chosen to exercise a layout.

**None of it is live data**, and the sketch must never be read as a measurement.
Real figures belong in `docs/code-review/`, with the corpus, workload and device
that produced them.

## Changing them

Edit the `.dc.html` file and, if the layout moves, `canvas.json`. They are
plain files — any editor works.

The published canvas these were seeded into is regenerated from this directory;
the generated bundle itself is roughly 2 MB and is deliberately **not** checked
in, since it is reproducible from these fourteen files.

## When to delete this directory

When the real dashboard exists and these stop matching it. A sketch that has
drifted from the implementation is worse than no sketch — it becomes a second
source of truth that quietly disagrees with the first.
