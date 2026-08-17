# Upstream failure alarm — making a total outage audible

## Why

Preparing the move to DoH-only upstreams surfaced a gap: **a total upstream
outage was silent.**

`UpstreamPool::forward` logged each failed attempt at `debug!`
(`crates/fah-dns/src/upstream/mod.rs`), which is below the shipped default level,
and then returned the error without saying anything else. Nothing at `warn!` or
`error!` existed anywhere on that path.

Under plain UDP that was tolerable. Under encrypted-only upstreams it is not:
there is no plaintext fallback behind DoH, so when every upstream fails, clients
are living entirely on what the cache can still serve (ADR-0005, serve-stale).
The household keeps working for a while and then quietly stops, with no trace in
`/log` to diagnose afterwards.

This change makes that condition audible without making it expensive.

## What changed

**New** — `crates/fah-dns/src/upstream/alarm.rs`

`FailureAlarm`: a rate limiter for the "every upstream failed" condition.

| Field | Purpose |
| --- | --- |
| `epoch: Instant` | Fixed reference for millisecond arithmetic |
| `interval_ms: u64` | Warn cadence (30 s in production; a test seam takes a shorter one) |
| `last_warn_ms: AtomicU64` | Millis since `epoch` at the last warning, or the `NEVER` sentinel |
| `suppressed: AtomicU64` | Failures swallowed since that warning |

Two operations:

- `claim() -> Option<u64>` — returns `Some(n)` when the caller should log,
  where `n` is the number of failures suppressed since the previous warning;
  `None` when another failure already warned inside this window. Concurrent
  failures race for the slot via `compare_exchange`, so a burst produces one
  line, not one per worker.
- `clear() -> bool` — returns `true` exactly once per outage, for the recovery
  message, and re-arms so the *next* outage warns immediately rather than
  waiting out a stale window.

**Modified** — `crates/fah-dns/src/upstream/mod.rs`

- `UpstreamPool` gains `alarm: Arc<FailureAlarm>`.
- On the all-servers-failed return: `warn!` with the upstream count, the
  suppressed tally and the last error — *"all upstreams failed — answers now
  depend on cached entries"*.
- On a successful forward: `info!` *"upstreams recovered"* if `clear()` says
  this success ended a warned outage.
- The empty-pool case now returns `NotFound` through its own branch and does
  **not** trip the alarm.

## Design decisions

**Why rate-limited rather than per-query.** A sustained outage at query rate
would bury `/log` and cost more than the outage itself. One line per 30 s stays
readable across a multi-hour failure while still saying the resolver is *still*
broken, not merely that it once was. The suppressed count means no information
is lost by the silence — the log states what the outage actually cost.

**Why atomics and not a mutex or a timestamp set.** CLAUDE.md rule 4: memory must
not grow with traffic or uptime. Remembering "we already complained" costs three
atomics and a fixed epoch, forever, regardless of how long the outage runs.
Rule 3 is also respected — the successful path adds a single relaxed load that
returns early, and the `warn!` allocation happens at most once per interval on a
path already dominated by network timeouts.

**Why `Arc<FailureAlarm>` rather than a plain field.** `UpstreamPool` is `Clone`
and handed out by cheap `Arc` clone (`crates/fastadhunter/src/adapters.rs`). A
per-clone alarm would let every holder warn once for the same outage, which is
precisely the flooding this exists to prevent.

**Why the empty pool does not alarm.** `fah_config` validation rejects an empty
server list, so that branch is degenerate configuration rather than an outage.
Alarming on it would report a network failure that never happened.

**Why no config key.** The interval is a named `const`. Adding a key would mean a
CONFIGURATION.md entry plus a boot/runtime classification in
`fah-api/src/config_store.rs`, for a value with no operational reason to vary.
If the circuit-breaker work later needs it tunable, it can promote it then.

## Tests

Five unit tests in `alarm.rs`, testing `FailureAlarm` directly — deterministic,
no tracing subscriber, and using a short injected interval so crossing a window
does not mean sleeping 30 s:

| Test | Asserts |
| --- | --- |
| `first_failure_warns_immediately` | An outage is visible at once, not one interval late |
| `failures_inside_the_window_are_counted_not_logged` | One line per window; the next warning reports the tally; the count clears once reported |
| `recovery_is_reported_once_then_rearms` | Recovery logs once, further successes are not news, and the next outage warns immediately |
| `a_pool_that_never_warned_reports_no_recovery` | A healthy resolver never announces a recovery from an outage that did not happen |
| `suppressed_failures_do_not_survive_a_recovery` | A new outage does not inherit the previous one's tally |

Existing behaviour is unchanged: `all_upstreams_dead_errors_and_counts_failures`,
`success_resets_consecutive_failures`, `empty_pool_errors_instead_of_hanging` and
the DoT reuse/reconnect tests all still pass.

## Gates

```text
cargo fmt --check                                     clean
cargo clippy --workspace --all-targets -- -D warnings clean
cargo test --workspace                                all pass
```

**No bench run.** The forward path gains one relaxed atomic load on success,
against a ~4.3 ms measured network round trip (`0.2.9-soak-24h.md`); the `warn!`
sits on the all-failed error path. Neither the block nor the cache-hit hot path
is touched.

## Scope note

This is Part A of the DoH adoption plan. Deliberately **not** included:

- The DoH upstream switch itself — `dns.upstreams` is a boot key, and the 0.2.9
  soak window is anchored at `2026-08-01T19:25:00Z`. No restart before that
  reading.
- Router-side probing and email alerting — ops, not code, and every router write
  needs an explicit go.
- The upstream circuit breaker — gated on the DoH measurement, and constrained to
  not violate the serve-stale guarantees.

The alarm is deliberately small enough that the circuit breaker can absorb or
replace it once per-upstream health state exists.
