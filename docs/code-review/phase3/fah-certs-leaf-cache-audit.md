# Audit record — `fah-certs/src/leaf.rs` single-flight leaf cache

Read-only audit, 2026-09-10, branch `phase3-06` at `75da6c6`.

**This is an audit record, not a task and not a backlog item.** It exists so
that a future stall, hung handshake or blocking-pool symptom starts from what
was already checked rather than from scratch.

| | |
| --- | --- |
| Verdict | **No refactor recommended. No production blocker identified.** |
| Deadlock / lost-wakeup | none found |
| Validation performed | code and caller reading only |
| Validation *not* performed | no `loom` model, no stress run, no fault injection |
| Changed by this audit | nothing |

## Scope

`crates/fah-certs/src/leaf.rs` (645 lines) and its callers:
`CertStore::prewarm` / `cached_leaf` (`fah-certs/src/store.rs`),
`fah-http/src/intercept.rs`, `fah-dns/src/dot.rs`, and the runtime
configuration in `fastadhunter/src/main.rs`.

## 1. Core correctness — holds

| Property | Evidence |
| -------- | -------- |
| Single-flight is airtight | `lease()` holds the `Inner` guard continuously from the predicate check through `inflight.insert` (leaf.rs:187-196); no window admits a second minter for one host |
| No lost wakeup | one `wait` site (leaf.rs:191) inside a loop that re-tests the predicate under the mutex; the notifier mutates `inflight` under the lock and notifies after releasing it |
| Spurious wakeups absorbed | same loop |
| Every mint exit releases the marker | `InflightGuard::drop` runs on success, on `?` from `mint`, and on unwind; `mint` holds no lock, so an unwind cannot poison `Inner` |
| Panic path does not self-deadlock | in `store_minted` the guard is a parameter and `inner` a local, so unwinding drops the lock before the guard's `Drop` re-locks. Correct today, and load-bearing on drop order that nothing states |
| `spawn_blocking` cancellation is not reachable | a started blocking task runs to completion; dropping the `JoinHandle` only detaches. Both call sites discard the returned key and serve through `cached_leaf` afterwards |
| Epoch invalidation is safe | `clear()` bumps the epoch and drops entries but deliberately leaves `inflight` alone — clearing it would admit a second minter whose marker the first guard's `Drop` would then remove |
| Superseded mints converge | `store_minted` discards a stale-epoch leaf and `CertStore::prewarm` retries with a fresh CA and epoch (store.rs:434-456). A CA replacement racing K waiters costs up to 2K mints and still hands every caller a valid leaf |
| Failed mint does not herd | the guard drops, waiters wake, the next becomes the minter — K serial attempts, not K parallel ones |
| `notify_all` herd is bounded | woken threads queue on the mutex; the first re-check finds the fresh entry and the rest return `Fresh` as `coalesced` |
| Poisoning | swallowed on purpose — `lock()` and `wait()` both `clear_poison` and continue (leaf.rs:247-259). Defensible for a cache; it means a panic mid-mutation is recovered silently |

## 2. Follow-up observations — real, currently latent

Recorded, not scheduled.

1. **Refresh `now` after a blocking wait.** `lease()` captures `now` before
   parking and reuses it afterwards (leaf.rs:173-192), so a leaf that expired
   during the wait can be returned as fresh.
   *Current impact:* latent — the serving path `cached_leaf` re-reads the
   clock, and both callers discard `prewarm`'s return value.
   *Becomes active if:* `prewarm()`'s result is consumed as the authority.

2. **Bound or yield the `prewarm()` retry loop.** `CertStore::prewarm`
   (store.rs:434-456) has no iteration cap and no yield; each turn pays a full
   keygen.
   *Current impact:* operational and resource risk during repeated CA
   replacement, which is operator-driven rather than adversarial.

## 3. Resource risk — by design, worth knowing

One waiter parks one blocking-pool thread. The runtime is built with defaults
(`main.rs:243`, no `max_blocking_threads`), so the pool is 512 and is shared
with argon2 password hashing, history reads, cache cleaning and the
certificates API.

Deadlock is impossible: a waiter can only exist once the minter for that host
is already running and holding a thread. Crowding is possible: a large
same-host burst can starve the other users of the pool for the duration of a
keygen. The only thing bounding it is that minting is fast, which nothing
enforces.

## Alternatives considered, and rejected

| Alternative | Why not |
| ----------- | ------- |
| `OnceLock` per host | cannot express eviction or epoch invalidation |
| `tokio::sync::Notify` / semaphore | requires an async path and moving minting off the blocking pool — a behaviour change, not a simplification |
| A caching crate with built-in single-flight | a new dependency to replace code this audit found correct |
| `Condvar::wait_while` | the predicate also mutates the LRU clock, so it does not fit cleanly |

The code is complex because the problem is. That is not a reason to touch it.

## Files changed

None.

## Remaining TODOs

- Nothing scheduled. The two observations above are recorded, not tasks.
- The upgrade path from argument to proof is a `loom` harness over `lease`,
  `store_minted` and `clear` — worth it only if a stall is ever observed.
