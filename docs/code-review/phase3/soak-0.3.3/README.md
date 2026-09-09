# 0.3.3 production soak — restarted 2026-09-09

The 7-day production soak whose verdict is ADR-0006's plateau, and which gates
`phase3-06` → `main`. This folder holds its T0 artifacts.

| | |
| --- | --- |
| **T0** | `2026-09-09T11:15:15Z` (14:15:15 local) — container start in the router log |
| **Day 7** | `2026-09-16T11:15:15Z` |
| Build | `fastadhunter:0.3.3`, unchanged from the run it replaces |
| Device | RB5009, `veth1` / `172.17.0.2`, mounts `fah-config,fah-data` |
| Runtime | `FAH__RUNTIME__HTTP_RUNTIMES=2`, `mode=DnsHttp`, `TZ=Europe/Bucharest` |
| Allocator | `MIMALLOC_PURGE_DELAY=0`, `PURGE_DECOMMITS=1`, `ARENA_EAGER_COMMIT=0` |

## Why this run exists

Two reasons, both owner decisions:

1. **The previous run was invalidated.** The soak that started at the 2026-09-07
   owner-run swap was contaminated on 2026-09-08 by the p3-06 verification work,
   which drove a large volume of queries at the production container. A plateau
   measured across that window is not the household's steady state, and
   ADR-0006's verdict rests on it being exactly that.
2. **The API certificate needed to change.** Activating a new certificate
   requires a container restart, and restarting ends a soak regardless. Given
   (1) had already cost this run, doing both at once spent one restart instead
   of two.

Day 7 therefore moves from 2026-09-14 to **2026-09-16**, and everything
sequenced behind it moves with it — `project-state.md` §Now, the p3-06
verification plan, and the testing plan's full-mode soak gate.

## What changed at T0

### The API certificate

The self-signed pair generated at first boot was replaced with the Let's
Encrypt wildcard for `*.localbox.ro`.

| | Before | After |
| --- | --- | --- |
| Subject | `CN=FastAdHunter` | `CN=*.localbox.ro` |
| Issuer | itself | `Let's Encrypt YE2` → `Root YE` → `ISRG Root X2` |
| SANs | `fastadhunter`, `localhost`, `127.0.0.1`, `::1`, `172.17.0.2` | `*.localbox.ro`, `localbox.ro` |
| Valid to | 2027-09-30 | **2026-12-08** |
| Trusted by | only machines where it was installed | any public root store |

Copied onto `/config` as `api-cert.pem` (566 → 3233 bytes) and `api-key.pem`,
then loaded at the restart. **`POST /api/v1/certificates/import` was not used —
it does not exist in 0.3.3**, which predates Phase 3; replacing the files on
disk is the only route until a Phase 3 build is deployed. The replaced pair and
the rest of `/config` are backed up outside the repo.

Verified after the restart: `https://fah-api.localbox.ro:8443` passes strict
verification, and the served chain validates against `ISRG Root X2` **pinned as
the sole root** — the same root set `webpki-roots` gives the Rust clients. That
matters: Let's Encrypt's default chain now ends at `ISRG Root YE`, which no
released `webpki-roots` carries. See
[`../../../solutions/environment/lets-encrypt-gen-y-chain-vs-webpki-roots.md`](../../../solutions/environment/lets-encrypt-gen-y-chain-vs-webpki-roots.md).

Two names lost their clean padlock in the swap — `172.17.0.2` and
`fastadhunter` were SANs of the old certificate and are not on this one.

### Deleted history

Four files removed before the restart, to keep the contaminated window out of
the record and out of the dashboard graphs:

```text
data/history/perf/perf-2026-09-08.jsonl        500.9 KiB
data/history/perf/perf-2026-09-09.jsonl        233.4 KiB
data/history/rollups/rollup-2026-09-08.jsonl
data/history/rollups/top-2026-09-08.json
data/history/rollups/rollup-2026-09-09.jsonl
```

**A full `/data` clean was considered and rejected.** It would have destroyed
six weeks of history back to 2026-07-27 and changed what the soak measures — a
plateau from cold caches and empty history is not comparable to the 0.3.1
baseline or to the run this replaces. **28 `perf-` daily files survive.**

Consequence worth knowing when reading the graphs: the perf history has a
**hole from 2026-09-08 to T0**, so the RSS graph's 24-hour window held two
samples immediately after the restart and looked empty. It refills at the
sampling cadence; it is not a defect.

### Environment event

DIGI renumbered the IPv6 delegation during the restart:

```text
2a02:2f04:540c:9800::/56   →   2a02:2f04:5400:cc00::/56
```

The router's `fah-lan6` address-list is dynamic and tracked it at 14:15:14, one
second before the container started. No action was needed, but it is a change
inside the soak window and a prefix appearing in earlier captures will not match
later ones.

## Baseline at T0

| Reading | Value |
| --- | --- |
| `process_rss` (uptime 120 s) | 59,031,552 B = **56.3 MiB** |
| First perf sample, `rss_bytes` (11:15:19Z) | 43,200,512 B |
| Second perf sample (11:21:56Z) | 61,874,176 B |
| `peak_rss` at first sample | 97,271,808 B |
| `cache_estimated_bytes` | 233,344 |
| Ruleset | 756,492 rules, 12 lists, compiled from cache |
| Boot to serving | 4 s (14:15:15 → 14:15:19) |

## Reading the perf history

`history.sample_interval_seconds` is **360** on this deployment, not the 60 s
compiled-in default. So a day holds ~240 samples, not 1440.

That breaks an assumption in tui-monitor: `config.rs` sets the RSS graph to
`DAY_OF_SAMPLES = 1_440`, commented as "a day … at the appliance's 60 s
cadence". At 360 s that window is **six days, not one**. Nothing is wrong with
the values; the window is six times what its name claims, which for this soak
means the graph shows nearly the whole run.

## Artifacts

`soak-0.3.3-t0-*` — captured at T0 + ~2 minutes:

| File | Source |
| --- | --- |
| `timestamp.txt` | T0, from the router log |
| `container-log.txt`, `container-print-detail.txt` | `/log print`, `/container print detail` |
| `file-print-lists.txt`, `system-resource.txt` | `/file print`, `/system resource print` |
| `config.json`, `telemetry.json`, `stats.json`, `clients.json`, `lists.json` | `GET /api/v1/…` |
| `debug-memory.json` | `GET /api/v1/debug/memory` |
| `history-perf.json` | `GET /api/v1/history/perf` |
| `health.json` | `GET /health` — root, **not** under `/api/v1` |

## Scope

These readings apply to this device, this build and this household's traffic.
The soak measures a memory plateau; it says nothing about P3's per-session RSS
ceiling, which `p3-06-testing-plan.md` names as its own sole authority.
