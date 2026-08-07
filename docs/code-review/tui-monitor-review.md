# tui-monitor review

Full review of `tui-monitor/` as an unfamiliar PR. 32 findings; 14 fixed, the
rest listed below. Tests 838 → 861.

## Decisions

- **The client reduces, not the server.** `/history/perf` decimation drops whole
  rows (API.md §History), so a spike in a dropped row is gone for good. The
  monitor asks for the window undecimated and reduces by peak when it draws.
- `max_points` is a wire constant, **not** `rss_points`. Tying them made a
  24 h window flip between stride 1 and 2 between polls, because the half-open
  window holds 1440 or 1441 rows depending on the sampler's phase.
- Braille is an **area** chart, so min/max banding was rejected — a min is drawn
  under the fill and never seen. Peak-per-column is the whole fix.
- Events liveness is an **idle budget**, not client pings: same detection of a
  half-open TCP without a write half and a select loop.
- `Flow` → `bool` was raised and **withdrawn**: it trades six lines of
  declaration for ambiguity at the one call site that matters.

## Bugs found

| # | Bug | Status |
| - | --- | ------ |
| 1 | API token committed in `tui-monitor/config.toml`; pre-rotation one still in history from `046d68f` | **open — owner** |
| 2 | Nearest-neighbour resample dropped spikes from both the drawn height *and* the colour band; `Braille.columns` was the peak of two resampled points, not of the range | fixed |
| 3 | `/history/perf` `stride` never deserialized — the one series that is always decimated charted as if dense | fixed |
| 4 | `rss_points = 288` documented against a 5-min cadence; appliance samples at 60 s, so the graph covered ~9.6 h, not 24 h | fixed |
| 5 | "Today" was a rolling 24 h; "Last 7 days" spanned 8 buckets; `coverage()` printed "dayly" and "1 buckets" | fixed |
| 6 | Popup branched on `qtype`, not the documented `kind` — an HTTP event with a qtype hid every HTTP field | fixed |
| 7 | No liveness on the events socket: a half-open TCP froze the feed at `ONLINE` forever | fixed |
| 8 | `decode()` swallowed every parse failure — a renamed server field empties the feed silently | fixed |
| 9 | Unbounded `LinkStatus::Down` reason clipped version/uptime/clock off the header | fixed |
| 10 | `App::run` was `async` with zero awaits; worked only because `rt-multi-thread` leaked in from the workspace | fixed |
| 11 | history/perf/routeros workers swallowed errors — could fail forever with nothing on screen | fixed |
| 12 | `run_summary` sequential where `routeros::run` used `join!` | fixed |
| 13 | `paths::HISTORY_PERF` carried a query string; a second param would produce `??` | fixed |
| 14 | `requests/history.http`: four requests filed under "should be 400" that the server answers happily, incl. `?fields=rss_bytes` — the monitor's own call | fixed |
| 15 | `requests/README.md` claimed coverage of every endpoint; `/api/v1/policies*` and `/clients/{ip}/policy` had no file | fixed |
| 16 | Divider said "Now" for the newest *persisted* sample, contradicting the live gauge 2.1 MB away | fixed |

Simplifications applied: `rss_history` `VecDeque`→`Vec` (was copying the series
out every frame), `truncate`/`client_label` → `Cow`, redundant `drop`.

## Measurements

| | Before | After |
| - | ------ | ----- |
| Perf rows fetched / poll | ~720 (stride 2) | 1440 (stride 1) |
| Graph time span | ~9.6 h | 24 h |
| Response size / poll (300 s) | ~40 KB | ~75 KB |
| Series retained | 288 × f64 | 1440 × f64 (11.5 KB) |
| `rss_series()` per frame | alloc + copy | borrow |

## Files changed

26 files, +935 −209. New: `requests/policies.http`,
`crates/fah-api/tests/request_coverage.rs`.

Guards worth knowing about:

- `UNDECIMATED > DAY_OF_SAMPLES` is a **compile-time** assert — the wire budget
  cannot drop below what the graph retains.
- `request_coverage.rs` enumerates the router against `requests/`. Verified by
  removing `policies.http` and confirming it named the three missing routes.
- `the_shipped_config_file_parses_against_this_build` — `config.toml` is what
  the owner runs; a renamed key otherwise fails into a cleared terminal.

## Remaining TODOs

| Item | Note |
| ---- | ---- |
| Finding 1 — token | Rotate + untrack; history rewrite is the owner's call |
| `fah-monitor.ps1` | Hard-codes `E:\`; `$PSScriptRoot` fix proposed, not applied |
| `fah-monitor.lnk` | Binary in git, 3 absolute paths, unfixable by editing. Kept deliberately |
| `/history/perf` stalling after success | Graph keeps its series; failure named only when empty |
| `config.toml` from CWD | Root cause of needing a launcher script at all |
| tui-monitor undocumented | In no architecture doc; invisible to `layering.rs` |
