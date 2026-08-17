# Prometheus surface removed

## Summary

`GET /metrics` is gone. Every figure it published now has exactly one JSON
home. The `fah-metrics` registry, all counters and all five latency histograms
are untouched — only the text rendering was removed.

Driving rule, from the repo owner: **every endpoint must serve data no other
endpoint serves.** `/metrics` failed it — 31 of its 33 families duplicated
`/api/v1/telemetry` or `/api/v1/debug/memory`.

## Decisions

- **Histogram buckets dropped, not converted.** They were `/metrics`'s only
  unique content. Their purpose is computing percentiles, and
  `/api/v1/history/perf` already serves windowed p50/p99 per stage as JSON.
  Publishing both would reintroduce the duplication this change removes.
- **`[api] metrics_public` deleted, `/health` unconditionally public.** It
  returns status, version and uptime only, so there is nothing to gate.
- **Auth answers before routing.** An unknown path under `/api/v1/` is `401`
  without a key and `404` with one. Now asserted, because `requests/auth.http`
  depends on it.
- **`requests/` is one endpoint per file.** `auth.http` is the sole exception
  and probes a path no endpoint owns, rather than borrowing `lists.http`'s.
- **`benchmark.ps1` reads `/telemetry`** instead of scraping and regex-parsing
  Prometheus text.

## Endpoint coverage after the change

| Data | Home |
| ---- | ---- |
| ruleset, counters, latency totals, upstreams, cache, memory | `/api/v1/telemetry` |
| `allocator_committed_bytes`, `allocator_committed_peak_bytes` | `/api/v1/debug/memory` |
| latency percentiles, RSS series | `/api/v1/history/perf` |
| product statistics | `/api/v1/stats` |
| liveness | `/health` |

## Measurements

| Gate | Result |
| ---- | ------ |
| fmt / clippy / test | PASS · 834 tests, 40 binaries |
| Tests removed | 12 (`/metrics` + `metrics_public` cases) |

No hot-path change: nothing removed here ran per query or per request.

## Files changed

| File | Change |
| ---- | ------ |
| `crates/fah-metrics/src/encode.rs` | deleted |
| `crates/fah-metrics/src/lib.rs` | `mod encode` / re-export dropped |
| `crates/fah-api/src/routes.rs` | route + handler dropped |
| `crates/fah-api/src/auth.rs` | `PUBLIC_PATHS` = `["/health"]`, gate removed |
| `crates/fah-api/src/ports.rs` | `prometheus_text()` dropped |
| `crates/fah-api/src/state.rs` | `metrics_public()` dropped |
| `crates/fah-api/src/config_store.rs` | runtime-key list |
| `crates/fah-config/src/schema/api.rs` | `metrics_public` field dropped |
| `crates/fah-config/src/env.rs` | `FAH__API__METRICS_PUBLIC` dropped |
| `crates/fastadhunter/src/adapters.rs` | `prometheus_text()` impl dropped |
| `requests/metrics.http` | deleted |
| `requests/debug.http` | new |
| `requests/{auth,cache,health,stats,telemetry,settings}.http` | de-duplicated |
| `requests/benchmark.ps1` | reads `/telemetry` |
| API.md, CONFIGURATION.md, SECURITY.md, ARCHITECTURE.md, CONTEXT.md, ADR-0005, deploy-rb5009.md, requests/README.md | `/metrics` references retargeted |

## Remaining TODOs

- **Deploy order.** `metrics_public = true` must be deleted from
  `/config/fastadhunter.toml` **before** the next image is swapped in — root
  `ApiConfig` is `deny_unknown_fields`, so an inherited key is a hard boot
  failure. Same shape as the `[query_log]` removal.
- The deployed `0.2.11` image predates this change; a rebuild is required to
  put it on the router.
