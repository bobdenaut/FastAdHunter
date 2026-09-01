# Re-soak pre-declaration — 0.3.1 on RB5009

**Written and committed before the container start.** No evidence from this
soak has been read at the time the gates below are fixed — the image is built,
the container is not running, no pull exists, no window is open. `T0` is the
one field filled in after the start, from the container log, and filling it in
is not a gate change.

Supersedes [resoak-0.3.0-predeclaration.md](resoak-0.3.0-predeclaration.md),
which is left untouched. That soak was terminated at T0+~59 h for a
methodology change and produces **no verdict** — see
[phase2.6-audit.md](phase2.6-audit.md) §Re-soak termination — 0.3.0.

## Summary

| | |
| --- | --- |
| Subject | `0.3.1` — `0.3.0` plus `d420f38` (slim history rows) — replacing `0.3.0` as the serving container |
| Predecessors | L.3 on `0.2.20`, **terminated day ~5, FAIL**. Re-soak on `0.3.0`, **terminated T0+~59 h, no verdict** |
| What this soak carries | the `adaptive` upstream-strategy acceptance, and p5-10 Stage B. **Neither predecessor carries them** |
| Duration | 7 days from process start |
| Gates | G1 all-window drift · G2 floor plateau · G3 peak-step attribution · G4 shed counters · G5 304 in production · G6 residual slope |
| Verdict rule | **PASS only if all six pass.** One failing window, one unattributed step, or one non-zero shed counter is a FAIL |

## What changed relative to the 0.3.0 declaration

| Change | Why |
| --- | --- |
| **§Method: the pull passes an explicit 15-field `?fields=` set** — everything except `upstreams`. The `0.3.0` rule was "`?fields=` is never passed" | That rule forced [`PerfFields::ALL`](../../../crates/fah-api/src/wire.rs), whose `upstreams: true` drives the full-row parse `d420f38` addresses. Each pull injects an RSS excursion — measured +6.9 MiB on a gate pull, +15.3 MiB on a dashboard session — that **decays inside ~2 h**, distorting G1's hour-16–20 vs 20–24 means and able to push a G3 `peak_rss` step. It never sets a 24 h minimum, so G2 was not observer-contaminated ([audit](phase2.6-audit.md) §Re-soak termination) |
| **G5b's `+16` unavoidable-bodies allowance is removed** | `0.3.0` wrote `.validators` for all 16 lists by T0+27.4 h (`list_fetch.bodies` 3→17→19 across that batch). `/data` is the same mount, so every list starts `0.3.1` with a stored validator and a conditional first refresh |
| **§Method declares the pull cadence and the dashboard as operational variables** | Pulls and dashboard views have measurable RSS cost. On `0.3.0` they were ad-hoc and unlogged; G2's floor could not be separated from them |
| G1, G2, G3, G4, G5a, G5c, G6 | **unchanged, verbatim** from the `0.3.0` declaration |

## Declared gates

### G1 — half-to-half RSS drift, every window

- `T0` = the container process start, taken from the container log
  (`/log print where topics~"container"`), not from router uptime.
- Window `Wi` = `[T0 + (i−1)·24 h, T0 + i·24 h)`, `i = 1..7`.
- Metric: mean `rss_bytes` over hours 16–20 of the window against mean
  `rss_bytes` over hours 20–24. `drift = mean(20–24) − mean(16–20)`.
- **Gate: `drift < 2 MiB` (2 097 152 B), strict. A negative drift passes.**
- **All 7 windows must pass. Any one failing window fails the soak**, at day
  7, with no re-windowing and no appeal to the other six.
- W1 is included. A W7 short of the full hours 16–24 has **no figure**, and
  the soak is extended until it does. A missing window is never waived.

### G2 — floor plateau

- `floor(Wi)` = minimum `rss_bytes` over `Wi`.
- **Gate: `floor(W7) − floor(W4) < 2 MiB`.**
- Reported, not gated: `floor(W4) − floor(W1)` and each consecutive step.
- Why it is a gate: G1 only compares hours 16–24, so a ratchet that accrues
  under daytime traffic and plateaus by hour 16 passes G1 at any size. Both
  predecessors produced a rising floor series — L.3 41.9 → 51.9 → 54.8 →
  55.4, `0.3.0` 43.3 → 52.0 → 64.2 (partial) MiB — and neither was resolved
  by a G1 figure.
- **Declared expectation, recorded in advance: the `0.3.0` predecessor's
  floor climbed +5.21 MiB/day post-warm-up (six-hour minima, h ≥ 30, least
  squares — [phase2.6-audit.md](phase2.6-audit.md) §State at termination).
  If `0.3.1` reproduces that rate, G2 fails.** `d420f38` removes the
  history-parse churn, which is the component measured to decay inside ~2 h;
  it has no evident mechanism against the floor. G2 is therefore the gate
  expected to decide this soak, and this paragraph fixes that expectation
  before the container starts so the day-7 outcome cannot be explained
  afterwards.

### G3 — `peak_rss` step attribution

- **Step** = `peak_rss[k] − peak_rss[k−1] ≥ 0.5 MiB` between consecutive
  `/history/perf` samples. Movements below that floor are not steps.
- A step is **attributed** when, within samples `[k−1, k+1]`, any of:
  - `Δ list_fetch.bytes_fetched > 0` — a list body was downloaded;
  - `Δ list_fetch.bodies > 0` — a body was parsed and the ruleset recompiled;
  - the step's timestamp matches an event in §Operational log (deploy,
    restart, manual `POST /api/v1/lists/refresh`, config write, **gate pull,
    dashboard session**).
- `allocator_committed_bytes` **corroborates but never attributes**. A rise at
  the step's tick is evidence the growth was in-heap allocation rather than
  file-backed page cache. **A fall is not used as evidence of anything.**
- **Gate: every step attributed. One unattributed step fails the soak.** It
  does not matter that RSS fell back afterwards.

### G4 — shed counters

- **Gate: `counters.events_dropped == 0` and `counters.swr.dropped == 0`** at
  every pull. Process-lifetime totals, so any non-zero reading at any pull
  fails the soak; there is no per-window budget.
- Read discipline: `counters.swr.*` and `upstreams[].attempts` lag by up to
  10 s ([routeros-traps.md](../../routeros-traps.md) §API access). Take the
  reading after a quiet interval, not while a load arm is running.

### G5 — the conditional GET proving itself in production

- **G5a: `counters.lists.not_modified > 0` within the first 48 h.** All 16
  origins were measured to serve validators and answer 304
  ([0.3.0 §Pre-deploy origin probe](resoak-0.3.0-predeclaration.md)), and
  every list now carries a stored validator at `T0`, so a zero here is a
  defect, not an origin's fault and not a construction artefact.
  - **Declared expectation, recorded in advance:** the 14 lists on
    `refresh_hours_default = 48` last fetched at 2026-08-30T22:36Z, so their
    next scheduled refresh falls at **~2026-09-01T22:36Z** — the first
    fully validator-armed wave, and the event that decides G5a. Schedules
    reseed from cache mtimes at boot, so a start before that time leaves the
    wave in place.
- **Companion validator log**, dev box, no router contact: hourly conditional
  `GET` per list URL carrying the previously seen validators, recording
  `(ts, id, status, size, etag, last_modified)` in
  `resoak-0.3.0/origin-log.tsv`. It has run since 2026-08-29 and so already
  predates this `T0` by four days, satisfying "starts before the container
  start" without a restart.
- **G5b: no list downloads a body while its origin's content is unchanged.**
  From the origin log, `expected_bodies` = the number of (list, scheduled
  refresh) pairs whose origin validator differs from the validator at that
  list's previous refresh. **Gate:**
  - `counters.lists.bodies ≤ expected_bodies`. **No `+16` allowance** — every
    list starts with a stored validator, so there is no unavoidable
    first-refresh body;
  - `counters.lists.bytes_fetched ≤ 1.10 × Σ (sizes of those expected
    bodies)`, the 10 % covering list growth within the week.
  - Any excess is a failure of the fix, and is diagnosed per list from
    `/data/lists/*.validators` before it is called anything else.
- **`bytes_fetched` counts decoded body bytes** (`text.len()` at
  [mod.rs:744](../../../crates/fah-rules/src/lifecycle/mod.rs)), not wire
  bytes, so the probe table's sizes are decoded sizes and the comparison is
  like-for-like.
- **G5c, sanity floor: total `bytes_fetched` < `B` = 169 014 754 B**
  (161.19 MiB), carried unchanged from the 2026-08-29 probe. Re-measuring
  would only raise `B` as lists grow, so the stale figure is the stricter
  choice. Reported alongside: `bodies`, `not_modified`, and
  `bytes_fetched / (bodies + not_modified)`.

### G6 — residual slope

- Split the soak into three equal spans. Fit a least-squares slope of
  `memory.residual_bytes` against time within each span.
- **Gate: the three slopes must not all share the same sign.**
- Recorded caveat, which does not weaken the gate: `residual_bytes` mixes
  allocator-held memory with file-backed pages charged for reading `/data`.
  The perf sample separates those (`rss_anon_bytes`, `rss_file_bytes`,
  `allocator_committed_bytes`), so a G6 failure is diagnosed against those
  three before it is called a leak — but it is still a failure.

## Method — pulls and evidence

- Read-only pulls only. **No router write for the duration of the soak.**
- Per pull: `GET /health`, `GET /api/v1/telemetry`,
  `GET /api/v1/debug/memory`, `GET /api/v1/history/perf` with `from=T0`,
  `to=<now>`, `max_points=5000` and **exactly this `fields` set**:

  ```text
  fields=rss_bytes,peak_rss,qps,queries_delta,blocked_delta,allowed_delta,
         cache,latency,memory,minor_page_faults,rss_anon_bytes,
         rss_file_bytes,answers_delta,allocator_committed_bytes,list_fetch
  ```

  15 of the 16 names in [`PerfFields::NAMES`](../../../crates/fah-api/src/wire.rs);
  `upstreams` is the only omission. Verified on device against `0.3.0`
  before this file was written: the response drops `upstreams` and nothing
  else, keeps all six `memory` subkeys, `list_fetch`,
  `allocator_committed_bytes`, `rss_anon_bytes` and `rss_file_bytes`, and
  returns a byte-identical `rss_bytes`/`peak_rss` series at `stride 1`
  (21-row window: 41 192 B full, 18 869 B slim).
- **Declared pull cadence: one pull per 24 h, at `T0 + 24k h ± 1 h`, plus a
  final pull at day 7.** Any additional pull is permitted but **must be
  appended to §Operational log at the time it is taken**, and is available
  to G3 as an attribution. An unlogged pull is a method violation, not an
  attribution.
- **The dashboard is an operational variable.** Any dashboard session during
  the soak is logged in §Operational log with its start and end. A `0.3.0`
  dashboard session was measured to retain +15.3 MiB.
- **A response reporting `stride > 1` is discarded and re-fetched
  paginated** — decimated rows cannot support G3, which differences
  consecutive samples. At 360 s, 7 days = 1 680 samples, inside
  `max_points=5000` in one request; pagination is the contingency, not the
  plan.
- Every pull's raw JSON is committed as it lands, under
  `docs/code-review/phase2.6/resoak-0.3.1/pull<N>-{health,telemetry,memory,perf}.json`.
- All figures in the day-7 write-up must be recomputable from those files
  alone.

### Reported, not gated: the observer A/B

Once, within the first 24 h and logged as an operational event: one slim
pull and one full pull (`?fields=` omitted) over the same range, back to
back, with `/api/v1/debug/memory` read before and after each. Reported:
`rss_bytes` and `residual_bytes` delta retained by each. This measures
`d420f38` on device. **It is a measurement, not a gate** — it cannot pass or
fail the soak, and it is declared here so that its two pulls are attributable
under G3.

## Environment fixed at soak start

| Field | Value |
| --- | --- |
| Version | `0.3.1` |
| Image id | *filled at start from `/container/print detail`* |
| Image tar | *filled at start: name, size, `sha256`* |
| Container | comment exactly `fastadhunter`; `0.3.0` removed at deploy |
| `root-dir` | `/kingston/fastadhunter/root` |
| Mount lists | `fah-config`, `fah-data` → `/kingston/fastadhunter/{config,data}`, unchanged |
| Interface | `veth1`, `172.17.0.2` |
| Opt-in | **`strategy = "adaptive"` in `/config/fastadhunter.toml`**, on the `/config` mount and therefore carried across the swap untouched. Confirmed after start via `GET /api/v1/config`, never by editing the TOML |
| `T0` | *filled at start* — `fastadhunter starting` in the container log. Day 7 closes `T0 + 168 h` |
| RouterOS | 7.21.5 (long-term), RB5009UG+S+, 4× ARM64, 1024 MiB |
| History sample interval | **360 s** (device `fastadhunter.toml`) |
| List set | 16 lists, `refresh_hours_default = 48`; unchanged from the `0.3.0` probe |
| Cache carried over | `/data` is the same mount. Cached `.raw` files **and** `.validators` written by `0.3.0` survive — no boot-time refetch storm, and no unavoidable first body (G5b) |
| History carried over | `/data/history/perf-YYYY-MM-DD.jsonl` day files from `0.3.0` remain. Only the deploy-day file mixes both builds' rows; pulls use `from=T0`, so mixed rows fall outside every window |

## Operational log

Every event that G3 may cite as an attribution. Appended as it happens, never
retrospectively.

| UTC | Event |
| --- | --- |
| *pending* | final `0.3.0` pull, immediately before the stop |
| *pending* | `0.3.0` container stopped and removed |
| *pending* | `0.3.1` container start — `T0` |

## What a PASS does not claim

- The gates bind this build, this list set, this device and this traffic. A
  list set growing to 30 MB moves the recompile transient proportionally and
  is outside what this soak measures.
- `peak_rss` exceeding RSS is by design and is not itself a finding; G3 gates
  the *attribution* of its steps, not their size.
- A PASS is evidence for `adaptive`, for the conditional-GET fix and for
  `d420f38` on the RB5009. It is not a general statement about any of them.
- A PASS says nothing about the full-row history path. This soak's pulls
  exclude `upstreams` by declaration; the dashboard does not, and its cost is
  measured only by the §Reported A/B.

## Open items

| Item | Owner |
| --- | --- |
| API key for the read-only pulls | owner-held, never committed |
| The single router intervention (final `0.3.0` pull → stop → deploy → start) | proposed separately, owner-run |
| p5-10 Stage B measurements at soak start | scoped in [p5-10-phase5-verification.md](../../../plan/wip/phase5/p5-10-phase5-verification.md) §Execution split |
