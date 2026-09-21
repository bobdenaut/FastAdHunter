# Soak 0.4.1 — RB5009, residual re-measure from 2026-09-21T07:14:50Z

## Summary

- **Question:** does the residual floor still climb day over day on the first
  build that carries `8941770` (idle upstream-pool reaper)? That commit is the
  mechanism [soak-0.3.4](../../phase2.6/soak-0.3.4/README.md) named for its
  doubling floor (19.0 → 38.6 MiB by h114, still accelerating; 23 → 42 MiB by
  daily minimum from 2026-09-12 to 2026-09-21).
- **Build:** `a437bec` = `v0.4.1` by build timestamp, container
  `fastadhunter-0.4.0`. **t0 is the second boot of this build**: the router
  itself was rebooted at 07:14Z on 2026-09-21 for a clean start, and the
  container came up on `start-on-boot` at `2026-09-21T07:14:50Z`. The first
  boot (05:37:31Z, 86 min, heating gateway and office laptop cut and exempted
  during it) is not part of this run; its samples stay in the router's
  `history/perf` between 05:37Z and 07:13Z.
- **Device:** RB5009, RouterOS 7.21.5, live household resolver, 799.7 MiB free
  right after the reboot.
- **Method:** the router's own `history/perf` series (360 s cadence, 30-day
  retention) plus [collect-soak.py](collect-soak.py) pulling hourly from the
  dev box into `pulls/` — every API read and every RouterOS figure as raw JSON
  (§Collection). Reads at h24, h48–h72, h168. Verdict from the slope of daily
  minimum `residual_bytes` and same-hour-of-day floors, not from the level.
- **Not comparable in level to 0.3.4:** this build carries every LAN port-443
  connection through the SNI proxy; its splice buffers and session state are
  not in `accounted_bytes` and land in the residual. Morning boot, daytime
  traffic in the first hours.
- **t0 read** by hand at h0.04 (uptime 157 s), figures in §t0 baseline. The
  raw t0 files are no longer in the tree; `pulls/` is the only raw store and it
  is untracked.

## Decisions

- Measure from the persisted series; retention (30 d) covers the seven days.
- The comparison is slope against the 0.3.4 series on the same device, which
  stays readable on the router until ~2026-10-11.
- No hand-made read folders. Every read is `reduce.py` over `pulls/`; the
  tables below gain a row per read.
- No knob changes during the run: `fah-env`, the TOML and the firewall stay as
  they are at t0, exemptions included.
- RouterOS is read over ssh as JSON (`:serialize to=json`, RouterOS ≥ 7.13),
  one session per pull, `print`/`get` only.

## t0 baseline — uptime 157 s, h0.04

| Memory | MiB |
| --- | --- |
| `process_rss` | 58.73 |
| `process_peak_rss` (boot transient: list load + compile) | 92.97 |
| `accounted_bytes` (ruleset 24.48 + cache 0.18 + aggregates 0.54 + clients 1.06) | 26.25 |
| **`residual_bytes`** | **32.48** |
| container `memory-current` by RouterOS (h0.16) | 115.2 |

Counters at t0: DNS pass 275 / block 36, HTTP 217 pass, HTTPS SNI 70
connections, `non_tls` 0, `hello_timeouts` 1, `major_page_faults` 15,
`tasks_died` 0. Four upstreams healthy, 0 failures.

## Collection

| What | Where |
| ---- | ----- |
| Collector | [collect-soak.py](collect-soak.py) — one directory per pull under `pulls/`, raw JSON only, `meta.json` with the headline numbers and the errors of that pull |
| Runner | [run-pull.cmd](run-pull.cmd) — reads the key from `%USERPROFILE%\.fah-soak\token.txt`, appends one line per pull to `collector.log` |
| Schedule | Task `FAH-soak-0.4.1`, hourly at :00 local, user `liviu`, **Interactive logon** (no run unless logged on — keep the session open), `StartWhenAvailable` on, 15 min limit, second instance ignored |
| API, every pull | `telemetry` (counters, listeners, upstreams, lists, ruleset), `debug/memory` (rss, peak, accounted, residual, faults, cpu ms), `health`, `cache`, `stats`, `history/summary`, `lists`, `history/perf` (1.2 h window, overlapping) |
| API, daily 00Z and `--tag t0` | `clients`, `history/top`, `config`, `policies`, `certificates`, `interception`, `rules/user` |
| RouterOS, every pull | `routeros-resource.json` (`cpu-load`, `free-memory`, uptime), `routeros-cpu.json` (per core), `routeros-health.json` (`cpu-temperature`), `routeros-container.json` (`memory-current` in bytes, `cpu-usage`), `routeros-log-problems.json` (error/critical/warning) |
| RouterOS, daily | `routeros-log-container.json` |
| API key | `kingston/fastadhunter/config/apikey` on the router; copy to `token.txt` after every rotation |
| Ignored by git | `pulls/`, `collector.log`, `__pycache__/` (root `.gitignore`, the `soak-*` patterns) |

Backfill or baseline by hand (every tier, `history/perf` since boot):

```powershell
python collect-soak.py --base https://fah-api.localbox.ro:8443 --token $key --out pulls --ssh-host bobdenaut --tag t0 --perf-hours 12
```

Daily minimum of the residual, MiB, from any `history-perf.json`:

```sh
jq -r '.items | group_by(.ts[0:10])[] | "\(.[0].ts[0:10]) min \(map(.memory.residual_bytes)|min/1048576*10|floor/10) max \(map(.memory.residual_bytes)|max/1048576*10|floor/10)"' history-perf.json
```

| Read | When (Z) | Purpose |
| --- | --- | --- |
| h24 | 2026-09-22 07:15 | first same-hour floor; first quiet-night floor; `non_tls` still 0 |
| h48–h72 | 2026-09-23 / 09-24 07:15 | climbing or settled — 0.3.4 rose 1.3–8.2 MiB/day on same-hour floors and was still accelerating at h114 |
| h168 | 2026-09-28 07:15 | the seven-day figure p3-11 wants |

## Incidents

| When (Z) | What | Effect |
| --- | --- | --- |
| 08:38 | API key rotated on the device (`apikey` last-modified 11:38:13 local) | pulls 09:00–17:00 got `401` on every authenticated endpoint; `health` and the RouterOS tier still landed |
| 17:42 | `pulls/` emptied, collector rewritten (RouterOS as JSON, `stats` hourly, five more daily endpoints) | the hourly series restarts from the next pull; nothing is lost for the question — the router's `history/perf` holds every sample since 07:14 and the backfill command above pulls it |

## Traps

- **Level is not the comparison.** The SNI proxy's splice buffers (2 × 16 KiB
  per session plus hyper state) are unaccounted and sit in the residual; 0.3.4
  had no 443 traffic. Only the slope answers the question.
- **Cadence is 360 s, not 60.** `history/perf` returns ~240 rows per day; use
  daily minimum and 12 h floors, which both series support.
- **Two boots of 0.4.1 sit in the router's series on 2026-09-21.** Filter from
  07:14Z; a daily minimum over the whole day would pick the 05:37Z boot sample.
- **Clocks disagree by ~45 s.** `/health` uptime puts the start at 07:15:36Z;
  the container's own log says 07:14:50Z. `meta.json` `pull_utc` is dev-box
  time; offsets are computed from container timestamps.
- **`memory-current` is bytes now** (`print as-value`), not the `108.6MiB`
  string the text `print detail` gave; `reduce.py` from 0.3.4 parses the text
  form and needs adapting.
- **Any dev-box test expecting a port-443 connect to fail now succeeds** —
  the steer answers with the real FAH.
- See [docs/measurement-traps.md](../../../measurement-traps.md) and
  [soak-0.3.4 §Traps](../../phase2.6/soak-0.3.4/README.md#traps).

## Files

- [collect-soak.py](collect-soak.py), [run-pull.cmd](run-pull.cmd) — tracked
- `pulls/`, `collector.log` — hourly output, not tracked

## Remaining TODOs

- ~~Refresh `token.txt` and backfill~~ — done 18:01Z (`20260921T180133Z-t0`,
  21 files, 121 perf rows from 06:01Z, `errors=0`). The task was found
  disabled and re-enabled at 18:06Z; next pull 19:00Z.
- Copy `reduce.py` from soak-0.3.4 at h24 and adapt it to the JSON RouterOS
  files and to `debug-memory.json`.
- h24, h48–h72, h168 reads; a row per read in the tables above.
- At the end: `schtasks /delete /tn "FAH-soak-0.4.1" /f`.
- Verdict at h168 goes to [docs/project-state.md](../../../project-state.md)
  §0.3.4 soak, which this run supersedes.

## How to conclude at day 7

```powershell
cd E:\FastAdHunter\docs\code-review\phase3\soak-0.4.1
python reduce.py > h168.md
```

Then read the Verdict inputs table at the bottom. The question is answered by
one number: the slope of the daily minimum of `residual_bytes` in MiB per day,
with its R². The thresholds are printed next to each input, and the script
prints a suggested reading of CLIMBING, FLAT or UNCLEAR. The 0.3.4 reference
rose 1.3 to 8.2 MiB per day and doubled in nine days. You decide, the script
only proposes.

For an apples-to-apples slope against 0.3.4, pull that series from the router
while retention still holds it (until ~2026-10-11), then run:

```powershell
curl.exe -sk -H "Authorization: Bearer $key" "https://fah-api.localbox.ro:8443/api/v1/history/perf?from=2026-09-11T22:02:00Z&to=2026-09-21T05:30:00Z" -o ref-0.3.4-history-perf.json
python reduce.py residual --compare ref-0.3.4-history-perf.json --compare-boot 2026-09-11T22:02:48Z
```

What the report contains:

- Pulls and `history/perf` coverage, with any gaps.
- Residual per UTC day: min, p50, max, quiet-night floor (00 to 05Z),
  same-hour floor (07 to 08Z), day-over-day delta, plus three fits: daily
  minima, last three days, 12 h floors.
- Peak RSS per day and every time the peak moved.
- Accounted breakdown first vs last row, cache bounds.
- RouterOS view: container `memory-current`, router free memory, cpu-load,
  temperature, with its own slope.
- Upstreams, counters first vs last pull, traffic per day, RouterOS problem
  log.
- Options: `--days N` for the h24 and h72 reads, `--warmup-hours` to skip the
  boot sample.
