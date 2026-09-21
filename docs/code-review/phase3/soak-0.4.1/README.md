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
  retention) plus the 0.3.4 collector reused unchanged, pulling hourly from the
  dev box into `pulls/` (§Collection). Reads at h24, h48–h72, h168. Verdict
  from the slope of daily minimum `residual_bytes` and same-hour-of-day floors,
  not from the level.
- **Not comparable in level to 0.3.4:** this build carries every LAN port-443
  connection through the SNI proxy; its splice buffers and session state are
  not in `accounted_bytes` and land in the residual. Morning boot, daytime
  traffic in the first hours.
- **t0 pulled** at h0.04 (uptime 157 s). Raw JSON in [t0/](t0/).

## Decisions

- Measure from the persisted series; retention (30 d) covers the seven days.
- The comparison is slope against the 0.3.4 series on the same device, which
  stays readable on the router until ~2026-10-11.
- Each later read gets its own subfolder (`h24/`, `h48/`, …) with the same five
  pulls as `t0/`; the tables below gain a row per read.
- No knob changes during the run: `fah-env`, the TOML and the firewall stay as
  they are at t0, exemptions included.

## Build, device, workload

| | |
| --- | --- |
| Image | `kingston/fastadhunter-arm64-0.4.1.tar`, 16.2 MiB, built 2026-09-19 14:58 local, `a437bec` |
| Container | `fastadhunter-0.4.0`, root-dir `/kingston/fastadhunter/root`, `veth1`, mounts `fah-config,fah-data`, `fah-env` (N=2, `MIMALLOC_ARENA_EAGER_COMMIT=0`, `MIMALLOC_PURGE_DECOMMITS=1`, `MIMALLOC_PURGE_DELAY=0`), `start-on-boot=yes` |
| Config | [docs/fastadhunter-0.4.0.toml](../../../fastadhunter-0.4.0.toml) (renamed from `fastadhunter.toml` in `1677f51`): `dns+http+https`, DoT 853, DoH `/dns-query`, `[https.listen]` 8444, `no_sni = "pass"`, `api.address = "::"`, `history.sample_interval_seconds = 360`, `retention_days = 30`, interception off |
| Ruleset | 766 499 rules, 431 587 duplicates removed, 24.48 MiB, compiled from cache in 2.92 s |
| Clients | 747 in the registry (729 IPv6, 6 named, 33 active in 24 h); cap 4 096 |
| Router | 443 steer + QUIC rejects (install guide ch. 7–8), all survived the reboot with counters reset. Exempt from the steer: heating gateway `192.168.10.15` and office laptop `192.168.10.14` (corporate VPN on 443, not a TLS hello). Ch. 12 and 13 not applied. The delegated IPv6 prefix changed with the reboot (`…5305:5f00::/56` → `…520d:9100::/56`) and `fah-lan6` re-populated itself |
| Boot log (container clock) | 07:14:50 starting; 07:14:53 ruleset compiled, DNS `[::]:53` udp+tcp, HTTP `[::]:8080`, HTTPS SNI `[::]:8444`, DoT serves the API certificate, API `[::]:8443` |

## t0 baseline — uptime 157 s, h0.04

| Memory | bytes | MiB |
| --- | --- | --- |
| `process_rss` | 61 587 456 | 58.73 |
| `process_peak_rss` | 97 488 896 | 92.97 |
| `process_rss_anon` | 50 380 800 | 48.05 |
| `process_rss_file` | 11 206 656 | 10.69 |
| `accounted_bytes` | 27 529 450 | 26.25 |
| `ruleset_bytes` | 25 666 316 | 24.48 |
| `cache_estimated_bytes` (157 entries) | 185 792 | 0.18 |
| `stats_aggregates_bytes` | 569 273 | 0.54 |
| `stats_clients_bytes` | 1 108 069 | 1.06 |
| **`residual_bytes`** | **34 058 006** | **32.48** |
| `minor_page_faults` / major | 37 248 / 15 | |
| `cpu_user_ms` / `cpu_system_ms` | 3 442 / 1 410 | |

`accounted` = ruleset + cache + aggregates + clients (27 529 450, checks);
`residual` = `process_rss` − `accounted` (34 058 006, checks). Peak RSS 92.97 MiB
is the boot transient: list cache load and ruleset compile, already behind us.

| Counters at t0 | |
| --- | --- |
| DNS | pass 275, block 36, cache hits 114, misses 161, stale 3 |
| HTTP (8080) | pass 217, 7 478 561 response bytes |
| HTTPS SNI (8444) | connections 70, requests 69, blocked 0, resolve_failures 0, upstream_failures 0, **non_tls 0, hello_timeouts 1** |
| DoT | active 0, peak 2 |
| DNS TCP | active 0, peak 0 |
| `tasks_died` / `events_dropped` | 0 / 0 |

| Upstream | protocol | state | attempts | failures |
| --- | --- | --- | --- | --- |
| 1.1.1.1 | udp | healthy | 161 | 0 |
| 9.9.9.9 | udp | healthy | 0 | 0 |
| 2606:4700:4700::1111 | udp | healthy | 0 | 0 |
| 2620:fe::fe | udp | healthy | 0 | 0 |

### Series since boot — every sample, MiB

| ts (Z) | RSS | residual | accounted | cache | cache entries | minor faults | alloc committed |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 07:14:53 | 43.29 | 17.21 | 26.07 | 0 | 2 | 35 615 | 141 |

One sample at t0 (3 s after start). The telemetry read 157 s later already shows
the residual at 32.48 after the first list refresh, the same first-minutes jump
both earlier boots showed (below).

### The same offsets after the 0.3.4 boot (2026-09-11T22:02:48Z), MiB

| ts | RSS | residual | accounted |
| --- | --- | --- | --- |
| 09-11T22:02 | 44.59 | 19.00 | 25.59 |
| 09-11T23:33 | 61.43 | 35.32 | 26.12 |
| 09-12T01:03 | 62.09 | 35.95 | 26.14 |
| 09-12T02:33 | 61.30 | 35.14 | 26.16 |
| 09-12T04:03 | 52.82 | 26.63 | 26.19 |
| 09-12T05:33 | 51.63 | 25.35 | 26.28 |
| 09-12T07:03 | 50.36 | 23.78 | 26.57 |
| 09-12T07:57 | 54.18 | 27.53 | 26.65 |

The first 0.4.1 boot (05:37Z) went 15.23 → 28.47 → 40.07 MiB in its first
twelve minutes and oscillated 30.8–40.1 for the next hour. On 0.3.4 the floor
came down to 23.8 in the night hours that followed its boot; this run's first
quiet window is 2026-09-22 ~00:00–05:00Z.

## Flags at t0

| Flag | Value | Reading |
| --- | --- | --- |
| `non_tls` | 0 | On the first boot it reached 693 in 86 min, ~10/min, and the office laptop's VPN was identified as the source by the connection table. The laptop is exempt since ~07:09Z; zero here is consistent with that, and h24 confirms it. The running build emits no event for this path (`c5732da` does, not deployed) |
| `hello_timeouts` | 1 | Connections with no ClientHello inside 10 s |
| `major_page_faults` | 15 | Boot only; the first boot had 0 after 86 min. Watch it stays flat |
| `allocator_committed_bytes` | 141 MiB | Monotone by construction — never subtract it from anything ([memory.rs](../../../../crates/fah-model/src/memory.rs) §commit) |

## Traps

- **Level is not the comparison.** The SNI proxy's splice buffers (2 × 16 KiB
  per session plus hyper state) are unaccounted and sit in the residual; 0.3.4
  had no 443 traffic. Only the slope answers the question.
- **Cadence is 360 s, not 60.** `history/perf` returns ~240 rows per day; the
  0.3.4 collector pulled hourly telemetry instead. Use daily minimum and 12 h
  floors, which both series support.
- **Two boots of 0.4.1 sit in the router's series on 2026-09-21.** Filter from
  07:14Z; a daily minimum over the whole day would pick the 05:37Z boot sample.
- **Clocks disagree by ~45 s.** The dev box's `date` and `/health`'s uptime put
  the start at 07:15:36Z; the container's own log says 07:14:50Z. `pulled-at.txt`
  is dev-box time; offsets are computed from container timestamps.
- **Any dev-box test expecting a port-443 connect to fail now succeeds** —
  the steer answers with the real FAH. Irrelevant to the device figures,
  relevant to anything run from this LAN.
- See [docs/measurement-traps.md](../../../measurement-traps.md) and
  [soak-0.3.4 §Traps](../../phase2.6/soak-0.3.4/README.md#traps).

## Collection

| What | Where |
| ---- | ----- |
| Collector | [collect-soak.py](collect-soak.py) — the 0.3.4 script, logic unchanged, docstring updated; one directory per pull under `pulls/`, raw JSON/text only |
| Runner | [run-pull.cmd](run-pull.cmd) — reads the key from `%USERPROFILE%\.fah-soak\token.txt`, appends one line per pull to `collector.log` |
| Schedule | Task `FAH-soak-0.4.1`, hourly at :00 local from 2026-09-21 11:00 (08:00Z), user `liviu`, **Interactive logon** (no run unless logged on — keep the session open), `StartWhenAvailable` on, battery allowed, 15 min limit, second instance ignored. Created with `Register-ScheduledTask`, not `schtasks`, so the two wrong defaults (§project-state 2026-09-17) are set right |
| Every pull | telemetry, debug/memory, health, cache, history/summary, lists, history/perf (1.2 h window), RouterOS resource + container detail + warning/error log |
| Daily (00 UTC) and the `t0` tag | clients, stats, history/top, config, policies, full container log |
| First pulls | `20260921T072434Z` (runner test, every tier), `20260921T072456Z-t0` (all tiers), both `errors=0`, container RSS 108.6 → 115.2 MiB by RouterOS |
| Ignored by git | `pulls/`, `collector.log`, `__pycache__/` (root `.gitignore`, the `soak-*` patterns) |

Manual read between pulls, `$key` is `kingston/fastadhunter/config/apikey`:

```powershell
$H = "https://fah-api.localbox.ro:8443"
curl.exe -sk -H "Authorization: Bearer $key" "$H/api/v1/history/perf?from=2026-09-21T07:14:00Z&stride=1" -o history-perf-since-boot.json
curl.exe -sk -H "Authorization: Bearer $key" "$H/api/v1/telemetry" -o telemetry.json
```

Daily minimum of the residual, MiB:

```sh
jq -r '.items | group_by(.ts[0:10])[] | "\(.[0].ts[0:10]) min \(map(.memory.residual_bytes)|min/1048576*10|floor/10) max \(map(.memory.residual_bytes)|max/1048576*10|floor/10)"' history-perf-since-boot.json
```

| Read | When (Z) | Purpose |
| --- | --- | --- |
| h24 | 2026-09-22 07:15 | first same-hour floor; first quiet-night floor; `non_tls` still 0 |
| h48–h72 | 2026-09-23 / 09-24 07:15 | climbing or settled — 0.3.4 rose 1.3–8.2 MiB/day on same-hour floors and was still accelerating at h114 |
| h168 | 2026-09-28 07:15 | the seven-day figure p3-11 wants |

## Files

- [t0/pulled-at.txt](t0/pulled-at.txt), [t0/health.json](t0/health.json),
  [t0/telemetry.json](t0/telemetry.json),
  [t0/history-perf-since-boot.json](t0/history-perf-since-boot.json),
  [t0/config.json](t0/config.json),
  [t0/clients-summary.json](t0/clients-summary.json) — the hand-pulled t0 at
  uptime 157 s, tables above
- [collect-soak.py](collect-soak.py), [run-pull.cmd](run-pull.cmd) — tracked
- `pulls/`, `collector.log` — hourly output, not tracked

## Remaining TODOs

- h24, h48–h72, h168 reads; a row per read in §t0 baseline's tables and the
  daily-minimum table. `reduce.py` from soak-0.3.4 is not copied yet; copy and
  adapt it at h24 when there is something to reduce.
- At the end: `schtasks /delete /tn "FAH-soak-0.4.1" /f`.
- Verdict at h168 goes to [docs/project-state.md](../../../project-state.md)
  §0.3.4 soak, which this run supersedes.
