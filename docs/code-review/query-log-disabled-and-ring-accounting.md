# `query_log` disabled — scheduler and ring accounting

**Date:** 2026-08-06 · **Trigger:** `[query_log] enabled = false` set on the
device (RouterOS reported ~700 MB of disk in use). Not a plan task.

---

## 1. Summary

Turning off `query_log` left a flush task ticking every 5 s to hit an early
return, and exposed a pre-existing accounting bug: the ring reported its
*configured* capacity as heap it did not own.

Two fixes, no behaviour change to either pipeline. The scheduler is now `None`
when disabled; `Ring::heap_bytes` reports the deque's actual buffer.

The p2-07 memory instrument was wrong before this change too — see §3.

## 2. Decisions

- **`spawn_query_log_scheduler` returns `Option<JoinHandle<()>>`**, matching
  `Pipeline::spawn_cache_cleanup`. Safe because `query_log_enabled` is a plain
  `bool` captured at construction, not an `AtomicBool` like `history_enabled` —
  it cannot change under a running process.
- **`enabled` keeps controlling both the ring and the segments.** Splitting it
  (ring on `ring_entries`, segments on `enabled`) was considered and rejected:
  the live view in use is `WS /api/v1/events`, which is fed from the binary's
  event fan-out and never touches `Stats::log`.
- **`Ring::heap_bytes` uses `entries.capacity()`, not `self.capacity`.**
- **p2-09 stays blocked, deliberately.** Its recorded fallback — "scope the
  on-device check to the in-memory ring" — no longer exists (§3). Revisit only
  to validate persistence itself.

## 3. Bugs found

### `enabled = false` also empties the in-memory ring

`Stats::log` returns before `ring.push`, so the flag governs two things that
read as independent. `GET /api/v1/queries` returns an empty page on the
deployed device. `WS /api/v1/events` is unaffected.

Not a regression — the flag has always worked this way. Recorded because the
config key names only the log, and the API doc does not say the page depends
on it.

### `Ring::heap_bytes` reported a buffer that was never allocated

`Ring::new` builds `VecDeque::new()` — no preallocation — but `heap_bytes`
returned `vecdeque_bytes(self.capacity)`, i.e. `ring_entries`. Its own comment
asserted "the allocation is what occupies RAM, not the fill", which was never
true of this constructor.

Wrong in **both** directions, not one:

| Ring state | Reported (old) | Actual |
| --- | ---: | ---: |
| empty (`enabled = false`) | 10 000 slots | 0 |
| partially filled | 10 000 slots | next power of two ≥ len |
| saturated at `ring_entries` | 10 000 slots | 16 384 slots |

`VecDeque::new()` grows by doubling, so 10 000 pushes land on a 16 384-slot
buffer. `with_capacity(10_000)` would allocate exactly 10 000; the ring does
not use it.

Effect on the p2-07 residual: `stats` over-counted while the ring was
under-filled and under-counted by ~39 % once saturated. The p2-07 soak windows
ran saturated, so those residuals carry a known constant offset rather than
being invalid — see [p2-07-review.md](p2-07-review.md) §12.

### A test had encoded the bug as its contract

`heap_accounting_tracks_recorded_traffic_and_stays_bounded` asserted
`empty.ring > 0` with the message *"the ring allocates its buffer up front"*.
It passed because the same wrong assumption sat on both sides. Now asserts
`empty.ring == 0` plus `loaded.ring > empty.ring`, so the ring is actually
checked to move with traffic — which is what the test's doc comment claims it
is for.

### Adjacent, unfixed

`heap::string_bytes` is documented *"A `String`'s buffer. Capacity, not
length"* but takes `&str` and returns `value.len()`, so it cannot see capacity.
Same under-report class as the ring. Left alone.

`ring.rs` said "16 384 entries by default"; `default_ring_entries()` is 10 000.
Corrected — the figure was almost certainly read off a live `VecDeque`
capacity, which is the coincidence above, not a typo.

## 4. Measurements

`VecDeque` growth vs. bytes actually allocated, counting global allocator,
64-byte `T`:

| pushes | `capacity()` | measured bytes | `capacity()*size` |
| ---: | ---: | ---: | ---: |
| 0 | 0 | 0 | 0 |
| 1 | 4 | 256 | 256 |
| 1 000 | 1 024 | 65 536 | 65 536 |
| 10 000 | 16 384 | 1 048 576 | 1 048 576 |
| `with_capacity(10 000)` | 10 000 | 640 000 | 640 000 |

**Scope: rustc 1.96.0, x86-64 Windows, `System` allocator, one `T` size.**
`capacity()`'s documented contract is only "elements holdable without
reallocating" — the byte equality is a `RawVec` implementation detail and is
not a guarantee. The fix does not depend on it: it needs only that an unwritten
deque reports 0 and that `capacity()` tracks the buffer upward, both of which
follow from the documented semantics.

Before rustc 1.67 this would have read differently — `VecDeque` kept a
power-of-two buffer with one slot permanently empty and `capacity()` returned
`buf − 1`.

## 5. Files changed

| File | Change |
| --- | --- |
| `crates/fah-stats/src/stats.rs` | `spawn_query_log_scheduler` → `Option`; test assertion corrected |
| `crates/fah-stats/src/query_log/ring.rs` | `heap_bytes` → `entries.capacity()`; stale default in doc comment |
| `crates/fastadhunter/src/main.rs` | scheduler wired via `if let Some(...)`, out of the `vec![]` |

Gates: `test` PASS (89 in `fah-stats`, workspace green). `fmt` and `clippy`
fail only in `tui-monitor/`, which is off-plan and untouched here.

## 6. Remaining TODOs

- `heap::string_bytes` doc/behaviour mismatch.
- Re-read the p2-07 residual once `query_log` is re-enabled: the `stats`
  component shifts by the ring delta above.
- No test covers "`enabled = false` ⇒ empty `GET /api/v1/queries`". The
  behaviour is now deliberate, so it should be pinned.
