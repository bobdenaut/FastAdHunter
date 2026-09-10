# Project risk inventory — 2026-09-10

**This is an inventory, not a backlog.** Nothing here is scheduled, and nothing
here is a finding against a task. It records where the code is hardest to be
sure about, so that a future bug has a place to be looked for first.

Captured on branch `phase3-06` at `75da6c6`.

## Method and limits

Static survey only: source read, `git` metadata, and the repo's own review
record under [docs/code-review/](.). No profiling, no fault injection, no
device run, nothing executed against production. Ranking is by blast radius ×
subtlety × how hard the failure is to catch in a test — not by line count.

## Inventory

### 1. `fah-certs/src/leaf.rs` — hand-rolled single-flight leaf cache

| | |
| --- | --- |
| Size | 645 lines, 14 inline tests |
| Machinery | `Mutex` + `Condvar` + a `Lease` guard whose `Drop` calls `notify_all` + an epoch counter for invalidation |
| Reached from | the TLS accept path, through `spawn_blocking` (`fah-http/src/intercept.rs`) |

Worst risk-per-line in the tree. A lost wakeup or a mint that panics mid-lease
is a hung handshake for a household device, not a failed request. Every waiter
parks a blocking-pool thread, so a burst of first-visit hosts is also a
pool-starvation path. The only place in the project that hand-rolls
synchronisation instead of taking a library primitive, and the only code here
whose correctness cannot be established by reading one function.

### 2. `fah-rules/src/lifecycle/mod.rs` — 3436 lines, ~30 public methods

Network fetch, disk cache, parse, compile, atomic swap, scheduler, policies and
user rules in one object. 18 lock sites mixing `tokio::sync::Mutex` (compile,
per-list refresh) with `std::sync::{Mutex, RwLock}`; 11 spawn/select sites; two
documented best-effort paths where a cache write fails and the new rules apply
anyway (lines 1020, 1036). 55 tests is a lot until the interleavings are
counted: scheduler refresh against an API refresh, remove-during-refresh,
compile-lock queued behind a slow download. Owns the data DNS blocking depends
on.

### 3. `fah-dns/src/upstream/` — ~3900 lines across mod/health/encrypted/plain

Adaptive selection and health scoring are statistical, so "correct" is a
distribution and every test is a timing test. 12 spawn/select in `mod.rs`
alone. Coverage is heavy (39 + 41 inline, plus `tests/adaptive_behaviour.rs`)
precisely because this is where heisenbugs live.

### 4. `fastadhunter/src/main.rs` — 1864 lines, 43 commits since June

Highest churn in the tree. Owns clocks, timers, wiring and shutdown ordering.
Teardown at `main.rs:707` is https → http → dns → api → abort; the integration
audit still lists 11b open, now also reaching idle spliced sessions. Process
teardown has no unit test that can fail, so regressions land silently and
surface as a container that will not stop cleanly.

### 5. The h2 relay — `fah-http/src/intercept.rs`, `proxy.rs`

Already bit once: `H2_CONNECTION_WINDOW` at 256 KiB against 64 streams × 64 KiB
stalled whole sessions until the idle watchdog cut them, reported to the client
as clean ends (p3-04 S2). The fix ties the constants together
(`H2_CONNECTION_WINDOW = H2_MAX_STREAMS × H2_STREAM_WINDOW`), but flow control
couples the two legs and the failure mode is invisible — bodies simply never
arrive. The container reproduction was never re-run.

### 6. `fah-dns/src/cache.rs` — 1832 lines, 10 inline tests

Listed for consequence, not quality: carefully bounded, two limits,
incremental byte accounting, sharded. It is the invariant that keeps a 1 GB
router alive, and its `Mutex<Shard>` sits on the per-query path, which reads
against hard rule 3's flat "no locks on the hot path". Thinnest direct
coverage of anything this critical.

### 7. `fah-config/src/tz.rs` — POSIX TZ and DST, 576 lines, 10 tests

Rules like `M2.5.0,M11.1.0` are where date code stays wrong for years. A DST
error fires schedules an hour off twice a year and nothing crashes.

### 8. `fah-http/src/sni.rs` — 525 lines, 13 inline tests plus `tests/sni.rs`

The only hand-rolled parser over attacker-controlled bytes reachable before any
auth. Lower risk than its category suggests — bounded by `MAX_HELLO_BYTES`,
`MAX_NAME_LEN`, `MAX_LABEL_LEN`, no `unsafe`. Listed because it is the one
place a malformed hello meets our code instead of rustls's.

## Better than expected

- `unsafe` is six sites, all in the binary (allocator, `libc` privilege checks,
  rusage), all localised.
- Not one `TODO`, `FIXME`, `HACK` or `XXX` in the source.
- 29 integration test files outside the unit tests.

## Remaining TODOs

- None scheduled. The `fah-certs` leaf-cache audit is recorded in
  [fah-certs-leaf-cache-audit.md](fah-certs-leaf-cache-audit.md) — no refactor
  recommended, no blocker found.
- Revisit this inventory when Phase 3 closes, not before.
