# S1-G4 run-length window — segment captures

Input to **Phase 2.6**, not a Phase 2.5 artifact. Gate definition:
[adaptive-upstream-selection.md](../../design/adaptive-upstream-selection.md)
§S1-G4. Source field: `upstreams[].failure_runs` (p2.5-06).

**`failure_runs` resets on every container restart.** The window is read as
the **sum of per-process deltas**, so a segment survives only if a capture was
taken before the process ended. Capture immediately before any deploy, or that
segment's runs are lost.

## Capturing a segment

Owner says **"capture S1-G4"**. The API key is not in the repo (p2.5-08);
the owner supplies it.

```sh
curl -sk -H "Authorization: Bearer <key>" https://172.17.0.2:8443/api/v1/telemetry
curl -sk https://172.17.0.2:8443/health
```

`upstreams[].failure_runs` is the only required field. `/health` decides
which file to write, because **counters are cumulative within one process** —
only a segment's *latest* capture carries information, so an older capture of
the same process is superseded, not accumulated.

| `/health` says | Meaning | Action |
| --- | --- | --- |
| `uptime_seconds` higher than at the previous capture, same `version` | same process | **replace** the current segment's file |
| `uptime_seconds` reset, or `version` changed | new process after a restart | **new segment**: `seg03`, `seg04`, … |

Write to this folder as
`seg<NN>-<version>-<ISO8601 basic UTC>.json`, then update the Segments and
Running total tables below. One file per segment keeps the folder bounded.

Optional alongside it, useful only at a build boundary:
`/api/v1/debug/memory`, `/api/v1/stats`,
`/api/v1/history/perf?from=<T0>&to=<T1>&fields=rss_bytes,peak_rss,memory`.

## Segments

| # | Build | Captured | Window covered | `1.1.1.1` attempts / failures | `failure_runs` |
| --- | --- | --- | --- | --- | --- |
| 01 | 0.2.18 | 2026-08-23T14:54:35Z | 2026-08-22T22:19:31Z → capture (16.6 h) | 18,780 / 1 | `[1,0,0,0]` |
| 02 | 0.2.19 | 2026-08-24T08:28:51Z | process start 14:59:52Z → capture (17.5 h), **open** | 12,287 / 1 | `[1,0,0,0]` |

Segment 02 is live and has no end capture yet.

`9.9.9.9` and both v6 endpoints: 0–1 attempts across both segments. Under
`fallback` the primary answers essentially everything, so three of four
endpoints are unmeasured — Stage 1 cannot penalize or probe what never runs.

## Running total

| Metric | Value |
| --- | --- |
| Window opened | 2026-08-22T22:19:31Z (p2.5-09 V5d), 0.2.18 deploy |
| Closed runs, all segments | **2**, both of length 1 |
| Runs of length ≥ 2 | **0** |
| Primary failure rate | 2 / 31,067 attempts = 0.0064 % |

Earlier suite T sample 1 put the base rate at 0.072 % and found 27 partial
failure events over 45.7 h. This window is an order of magnitude quieter. The
discrepancy is unexplained and matters: it is the difference between a gate
with data and a gate without.

## Memory at the segment boundary

Recorded because the swap crossed a build, not because a soak is running.
0.2.19 = 0.2.18 + p2.5-10 + p2.5-11; `Cargo.lock` moved by version strings
only, no dependency changed.

| Metric | 0.2.18 at 14:54:35Z (15.8 h uptime) | 0.2.19 at 15:07:57Z (8 min uptime) |
| --- | --- | --- |
| `process_rss` | 54.4 MB | 47.9 MB |
| `process_peak_rss` | 143.9 MB | 92.9 MB |
| `ruleset_bytes` | 25.2 MB | 25.2 MB |
| `residual_bytes` | 23.5 MB | 21.4 MB |
| `allocator_committed_bytes` | 304.0 MB | 162.9 MB |
| `cache_entries` | 3,259 | 128 |

Not comparable as a memory result — the two columns are 15.8 h against 8 min,
with a cold cache and no list refresh yet on 0.2.19. 0.2.18's 143.9 MB peak
includes the p2.5-09 V4 refresh-all drill, which 0.2.19 has not performed.
The one honest reading: boot peak is 92.9 MB against 0.2.18's 95.0 MB at the
same point, so the new build starts no heavier.

RSS history for segment 02, 6 min cadence:

| Sample | `rss_bytes` | `peak_rss` | `residual_bytes` |
| --- | --- | --- | --- |
| 14:59:54Z | 43.6 MB | 92.9 MB | 17.2 MB |
| 15:06:28Z | 47.6 MB | 92.9 MB | 21.1 MB |

## What this decides

`penalty_failures = 2` is provisional and cannot be frozen from this data.
Spec §S1-G5's narrow rejection route: if the window shows only isolated single
losses and no run of ≥ 2, Stage 1 at `penalty_failures = 2` never engages on
this deployment, and not shipping is the correct outcome.

Two closed runs, both of length 1, are consistent with that route and equally
consistent with far too small a sample. The spec is explicit — too few closed
runs means **extend the window, do not guess the constant**.
