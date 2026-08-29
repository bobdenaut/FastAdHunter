# Re-soak pre-declaration — 0.3.0 on RB5009

**Written before the deploy, committed shortly after it — the weaker of the two,
and stated plainly rather than claimed away.** The gates below were fixed while
the image was still building; the deploy went ahead before the commit landed, so
the commit timestamp does **not** prove precedence over the container start.

What it does prove, and what F2 was actually about: **no soak evidence had been
read when these gates were fixed.** The container had been up 141 seconds at
first health check, all counters zero, no window closed, no pull taken. The
failure mode F2 named — choosing the rule after seeing the result — is closed by
that, not by the timestamp. Nothing below is added, relaxed, re-windowed or
re-interpreted after the first pull.

Commissioned by [plan/resoak-orchestration.md](../../../plan/resoak-orchestration.md)
§Stage 4.2.

## Summary

| | |
| --- | --- |
| Subject | `0.3.0` — phase 5 dashboard + conditional list refresh + attribution counters — replacing `0.2.20` as the serving container |
| Predecessor | L.3 soak on `0.2.20`, **terminated early at day ~5, verdict FAIL**, carries no acceptance ([phase2.6-audit.md](phase2.6-audit.md) §Soak termination) |
| What this soak carries | the `adaptive` upstream-strategy acceptance the terminated soak no longer can, and p5-10 Stage B |
| Duration | 7 days from process start |
| Gates | G1 all-window drift · G2 floor plateau · G3 peak-step attribution · G4 shed counters · G5 304 in production · G6 residual slope |
| Verdict rule | **PASS only if all six pass.** One failing window, one unattributed step, or one non-zero shed counter is a FAIL |

## What changed relative to the terminated soak's declaration

| Audit finding | Defect | Fixed here by |
| --- | --- | --- |
| F2 | no aggregation rule — a failed window had no declared consequence | G1's explicit all-must-pass |
| F2 | gate blind to a daytime ratchet that plateaus by hour 16 | G2, promoted from report to gate |
| F3 | p2.5-09's four soak criteria silently not carried | G3, G4, G6 — all four restored as gates (owner ruling, §F3 ruling) |
| §Excursion cause | the refresh download path exported no counter; the cause was found only from the router's bandwidth graph | G3's attribution runs on `list_fetch.*`, which `/history/perf` now carries |

## Declared gates

### G1 — half-to-half RSS drift, every window

- `T0` = the container process start, taken from the container log
  (`/log print where topics~"container"`), not from router uptime.
- Window `Wi` = `[T0 + (i−1)·24 h, T0 + i·24 h)`, `i = 1..7`.
- Metric, unchanged from L.3: mean `rss_bytes` over hours 16–20 of the window
  against mean `rss_bytes` over hours 20–24. `drift = mean(20–24) − mean(16–20)`.
- **Gate: `drift < 2 MiB` (2 097 152 B), strict. A negative drift passes.** The
  L.3 declaration wrote "< 2 MB" and reported MiB; the unit is fixed here as
  MiB and the comparison as one-sided.
- **Aggregation — the rule F2 found missing: all 7 windows must pass. Any one
  failing window fails the soak**, at day 7, with no re-windowing and no
  appeal to the other six.
- W1 is included. Its final third begins 16 h after boot, past the cold start.
- A W7 that is short of the full hours 16–24 has **no figure**, and the soak is
  extended until it does. A missing window is never waived.

### G2 — floor plateau (a gate, not a report)

- `floor(Wi)` = minimum `rss_bytes` over `Wi`.
- **Gate: `floor(W7) − floor(W4) < 2 MiB`.**
- Reported, not gated: `floor(W4) − floor(W1)` (cache and arena warm-up,
  expected positive) and each consecutive floor step.
- Why it is a gate: G1 only ever compares hours 16–24, so a ratchet that
  accrues under daytime traffic and plateaus by hour 16 passes G1 at any size
  (F2's sensitivity finding — a +5 MiB/day daytime ratchet, +35 MiB over the
  soak, is invisible to G1). The daily minimum is the discriminator for that
  shape, and the terminated soak's floor series (+9.94, +2.98, +0.53 MiB) is
  the reason it needs one.

### G3 — `peak_rss` step attribution (p2.5-09 criterion, restored)

- **Step** = `peak_rss[k] − peak_rss[k−1] ≥ 0.5 MiB` between consecutive
  `/history/perf` samples. Movements below that floor are not steps. The floor
  is declared here, in advance, because `peak_rss` is a lifetime high-water
  mark that page-granularity jitter nudges early in a process's life; the
  terminated soak's observed steps were 5.63, 0.03 and 7.78 MiB, so 0.5 MiB
  separates signal from noise without being tuned to an outcome.
- A step is **attributed** when, within samples `[k−1, k+1]`, any of:
  - `Δ list_fetch.bytes_fetched > 0` — a list body was downloaded;
  - `Δ list_fetch.bodies > 0` — a body was parsed and the ruleset recompiled;
  - the step's timestamp matches an operational event recorded in this file's
    §Operational log (deploy, restart, manual `POST /api/v1/lists/refresh`,
    config write).
- `allocator_committed_bytes` **corroborates but never attributes**: a rise at
  the step's tick is evidence the growth was in-heap allocation rather than
  file-backed page cache charged for reading `/data`. **A fall is not used as
  evidence of anything.** The counter is documented as monotone
  non-decreasing under mimalloc v3 ([memory.rs:96](../../../crates/fah-model/src/memory.rs))
  while the p2.6 audit observed it falling; this declaration is deliberately
  independent of which is right.
- **Gate: every step attributed. One unattributed step fails the soak.** p2.5's
  rule verbatim — it does not matter that RSS fell back afterwards.

### G4 — shed counters (p2.5-09 criteria, restored)

- **Gate: `counters.events_dropped == 0` and `counters.swr.dropped == 0`** at
  every pull. These are process-lifetime totals, so any non-zero reading at any
  pull fails the soak; there is no per-window budget.
- Read discipline: `counters.swr.*` and `upstreams[].attempts` are republished
  by the binary's telemetry poll and lag by up to 10 s
  ([routeros-traps.md](../../routeros-traps.md) §API access). Take the reading
  after a quiet interval, not while a load arm is running.

### G5 — the conditional GET proving itself in production

The Stage-2 fix is a production claim, not a unit-test claim, so it is gated on
device. A plain bytes threshold cannot be that gate: a list that genuinely
changed *must* download, and the origins' real change rates are not known
today. G5 therefore measures downloads against **independently observed
change**, not against a number picked in advance.

- **G5a: `counters.lists.not_modified > 0` within the first 48 h.** The
  conditional-GET path fires against real origins at all. All 16 origins were
  measured to serve validators and to answer 304 (§Pre-deploy origin probe), so
  a zero here is a defect, not an origin's fault.
- **Companion validator log**, dev box, no router contact: every hour for the
  soak's duration, one conditional `GET` per list URL carrying the previously
  seen validators, recording `(ts, id, status, etag, last_modified, bytes)`.
  Committed beside the pulls as
  `resoak-0.3.0/origin-log.tsv`. This is what makes G5b a gate rather than a
  guess.
- **G5b: no list downloads a body while its origin's content is unchanged.**
  From the origin log, `expected_bodies` = the number of (list, scheduled
  refresh) pairs whose origin validator differs from the validator at that
  list's previous refresh. **Gate:**
  - `counters.lists.bodies ≤ expected_bodies + 16`, the `+16` being the one
    unavoidable body per list at its first post-deploy refresh (see below);
  - `counters.lists.bytes_fetched ≤ 1.10 × Σ (sizes of those expected bodies)`,
    the 10 % covering list growth within the week.
  - Any excess is a failure of the fix, and is diagnosed per list from
    `/data/lists/*.validators` before it is called anything else.
- **The 16 first-post-deploy bodies are unavoidable and are declared now.**
  `0.2.20` wrote no `.validators` files
  ([cache.rs:15](../../../crates/fah-rules/src/lifecycle/cache.rs)), so each
  list's first refresh under `0.3.0` has no stored validator and downloads a
  body: 27.01 MiB across the 16, 16.8 % of `B`. They are not a G5 failure and
  are not counted as unattributed under G3 either — they attribute to
  `list_fetch` like any other body.
- **`bytes_fetched` counts decoded body bytes** (`text.len()` at
  [mod.rs:744](../../../crates/fah-rules/src/lifecycle/mod.rs)), not wire
  bytes, so the probe table's sizes are decoded sizes and the comparison is
  like-for-like.
- **G5c, sanity floor: total `bytes_fetched` < `B`.** The fix must save
  something against the no-conditional-GET baseline. Reported alongside:
  `bodies`, `not_modified`, and `bytes_fetched / (bodies + not_modified)`.

### G6 — residual slope (p2.5-09 criterion, restored)

- Split the soak into three equal spans. Fit a least-squares slope of
  `memory.residual_bytes` against time within each span.
- **Gate: the three slopes must not all share the same sign.**
- Recorded caveat, which does not weaken the gate: `residual_bytes` mixes
  allocator-held memory with file-backed pages charged for reading `/data`.
  The perf sample now separates those (`rss_anon_bytes`, `rss_file_bytes`,
  `allocator_committed_bytes`), so a G6 failure is diagnosed against those
  three before it is called a leak — but it is still a failure.

## F3 ruling — recorded here as the plan requires

p2.5-09's four soak criteria are **restored as gates** for this soak: peak-step
attribution (G3), `events_dropped` (G4), `swr.dropped` (G4), residual thirds
slope (G6). None is report-only. The silent non-carrying that F3 found is
closed by this paragraph, and any future soak that drops one of them must say
so in its own pre-declaration.

## Pre-deploy origin probe

Measured **2026-08-29, before the deploy**, from the dev box — no router
contact. List set read from the device's `/config/fastadhunter.toml` (16 lists,
`refresh_hours_default = 48`). Each URL fetched once with `--compressed` for
its decoded size, then re-requested carrying the validators it returned.

`n` = scheduled refreshes in 168 h, taken as `⌊168/h⌋ + 1` — the `+1` because
per-list schedules are seeded from cache-file mtimes, so an extra tick can fall
inside the window depending on phase. Baseline is deliberately the generous
reading; `B` is an upper bound, and G5c only requires beating it.

| List id | h | Decoded bytes | Validator | Conditional re-request | n | Baseline bytes |
| --- | --- | --- | --- | --- | --- | --- |
| `big.oisd.nl` | 48 | 6 186 383 | strong `ETag` + `Last-Modified` | **304** | 4 | 24 745 532 |
| `filter_1` | 48 | 4 275 637 | weak `ETag` + `Last-Modified` | **304** | 4 | 17 102 548 |
| `filter_2` | 48 | 243 130 | weak `ETag` + `Last-Modified` | **304** | 4 | 972 520 |
| `filter_3` | 48 | 69 150 | weak `ETag` + `Last-Modified` | **304** | 4 | 276 600 |
| `filter_11` | 48 | 136 299 | weak `ETag` + `Last-Modified` | **304** | 4 | 545 196 |
| `filter_18` | 48 | 3 268 301 | weak `ETag` + `Last-Modified` | **304** | 4 | 13 073 204 |
| `filter_30` | 48 | 1 184 229 | weak `ETag` + `Last-Modified` | **304** | 4 | 4 736 916 |
| `filter_43` | 48 | 3 404 | weak `ETag` + `Last-Modified` | **304** | 4 | 13 616 |
| `filter_48` | 48 | 4 990 916 | weak `ETag` + `Last-Modified` | **304** | 4 | 19 963 664 |
| `filter_50` | 48 | 62 810 | weak `ETag` + `Last-Modified` | **304** | 4 | 251 240 |
| `filter_59` | 48 | 58 197 | weak `ETag` + `Last-Modified` | **304** | 4 | 232 788 |
| `filter_63` | 48 | 13 650 | weak `ETag` + `Last-Modified` | **304** | 4 | 54 600 |
| `dyndns` | 48 | 24 930 | strong `ETag`, no `Last-Modified` | **304** | 4 | 99 720 |
| `doh-vpn-proxy-bypass` | 48 | 349 225 | strong `ETag`, no `Last-Modified` | **304** | 4 | 1 396 900 |
| `tif-mini` | 24 | 3 751 395 | strong `ETag`, no `Last-Modified` | **304** | 8 | 30 011 160 |
| `phishdestroy` | 12 | 3 702 570 | weak `ETag`, no `Last-Modified` | **304** | 15 | 55 538 550 |

- **All 16 origins serve a validator and all 16 answered 304.** No list is
  excluded from the 304 expectation, and the hash-compare fallback is not
  exercised by this list set — it is not what this soak measures.
- One full set = **28 320 226 B (27.01 MiB)** — also the unavoidable
  first-post-deploy download (G5b).
- `B` (total baseline, no conditional GET, 7 days) = **169 014 754 B
  (161.19 MiB / 169.01 MB)**. Independent corroboration: the terminated soak's
  router `veth1` inbound graph read ~20 MB/day ≈ 140 MB / 7 d against the same
  list set.
- **G5c threshold** = `bytes_fetched < B`. **G5b threshold** = computed at day 7
  from `origin-log.tsv`, per the rule declared above — not a number chosen here.
- Raw probe output: `resoak-0.3.0/origin-probe-20260829.tsv`, committed with
  this file.

## Environment fixed at soak start

| Field | Value |
| --- | --- |
| Version | `0.3.0` |
| Image id | `6b39353dcba8243f4357113133c5796d875638ef030bc10bac67f9c11de68c08` — confirmed identical in `/container/print detail` |
| Image tar | `fastadhunter-rosready-0.3.0.tar`, legacy docker-archive (skopeo-converted from the OCI buildx output), 14.2 MB, `sha256:6aba7af82c78dedda5ff85f199a80b4545394675cbd1225157cd842384bec0db` |
| Container | `fastadhunter-0.3.0`, comment exactly `fastadhunter`; `fah-next` (0.2.20) and the stopped 0.2.19 both removed at deploy |
| `root-dir` | `/kingston/fastadhunter/root` |
| Mount lists | `fah-config`, `fah-data` → `/kingston/fastadhunter/{config,data}`, unchanged |
| Interface | `veth1`, `172.17.0.2` |
| Opt-in | **`strategy = "adaptive"` in `/config/fastadhunter.toml`**, not an env override — the `fah-optin` envlist is gone and the container carries `fah-env` only. Deliberate owner change from the terminated soak's arrangement; the opt-in is now visible in `GET /api/v1/config` rather than only in the container start line |
| `T0` | **2026-08-29T19:18:00Z** — `fastadhunter starting` in the container log. Day 7 closes 2026-09-05T19:18Z |
| Certificate | regenerated at first boot with the p5-02 SAN set — `dns=["fastadhunter","localhost"] ip=[127.0.0.1, ::1, 172.17.0.2]`, `not_after=2027-09-30`. The pre-p5-02 pair was removed before start; it carried `127.0.0.1` only and would have name-mismatched the dashboard |
| RouterOS | 7.21.5 (long-term), build-time 2026-07-03, RB5009UG+S+, 4× ARM64, 1024 MiB |
| History sample interval | **360 s** (device `fastadhunter.toml`) — 7 days = 1 680 samples, inside `max_points=5000` in one request |
| List set | 16 lists, `refresh_hours_default = 48`; see §Pre-deploy origin probe |
| Cache carried over | `/data` is the same mount, so cached `.raw` files survive the swap and there is **no boot-time refetch storm** — but no `.validators` file exists yet, hence G5b's `+16` |

## Method — pulls and evidence

- Read-only pulls only. **No router write for the duration of the soak.**
- One pull per day, plus a final pull at day 7:
  `GET /health`, `GET /api/v1/telemetry`, `GET /api/v1/debug/memory`,
  `GET /api/v1/history/perf?from=<T0>&to=<now>&max_points=5000`.
- **`?fields=` is never passed.** Omitting it serves the whole sample
  ([`PerfFields::ALL`](../../../crates/fah-api/src/wire.rs)); any narrowing
  would drop `list_fetch`, `allocator_committed_bytes` or `memory`, which G3,
  G5 and G6 read.
- `max_points` defaults to 1 000 and caps at 5 000; paginate on `from` until
  the full series is retrieved. A response reporting `stride > 1` is
  **discarded and re-fetched paginated** — decimated rows cannot support G3,
  which differences consecutive samples.
- Every pull's raw JSON is committed as it lands, under
  `docs/code-review/phase2.6/resoak-0.3.0/pull<N>-{health,telemetry,memory,perf}.json`.
  F7's "recomputable in principle" is not repeated: the raws land with the
  figures, not at the end.
- All figures in the day-7 write-up must be recomputable from those files
  alone.
- **The companion origin log starts before the container start**, so the first
  post-deploy refresh of every list already has a prior validator observation
  to compare against. It runs on the dev box against the 16 list URLs, hourly,
  and never touches the router.
- `stride 1` at 360 s gives 1 680 samples across 7 days, so one request covers
  the series; pagination is the contingency, not the plan.

## Operational log

Every event that G3 may cite as an attribution. Appended as it happens, never
retrospectively.

| UTC | Event |
| --- | --- |
| 2026-08-29T19:18:00Z | container start — `T0`. Ruleset compiled from cache (753 270 rules), refresh schedule restored from the 16 cached copies, no boot download |
| 2026-08-29T19:18:04Z | self-signed API certificate regenerated (the removed pair predated p5-02) |
| 2026-08-29T19:20:21Z | first read-only check: `version 0.3.0`, `uptime_seconds 141`, `lists {bodies 0, not_modified 0, bytes_fetched 0}`, `events_dropped 0`, all `swr.*` 0 |

## What a PASS does not claim

- The gates bind this build, this list set, this device and this traffic. A
  list set growing to 30 MB moves the recompile transient proportionally
  (audit §Excursion cause) and is outside what this soak measures.
- `peak_rss` exceeding RSS is by design and is not itself a finding; G3 gates
  the *attribution* of its steps, not their size.
- A PASS is evidence for `adaptive` and for the conditional-GET fix on the
  RB5009. It is not a general statement about either.

## Open items

| Item | Owner |
| --- | --- |
| `fah-next` API key for the read-only pulls | owner-held, never committed |
| The single router intervention (final pull → deploy → p2.6 cleanup → start) | proposed separately, owner-run |
| p5-10 Stage B measurements at soak start | scoped in [p5-10-phase5-verification.md](../../../plan/wip/phase5/p5-10-phase5-verification.md) §Execution split |
